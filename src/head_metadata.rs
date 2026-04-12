use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::header::{
    ACCEPT_RANGES, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, LAST_MODIFIED,
};
use axum::http::{HeaderMap, HeaderValue, Response};
use axum::response::IntoResponse;
use reqwest::StatusCode as ReqwestStatus;
use reqwest::header::HeaderMap as ReqwestHeaderMap;
use tokio::sync::RwLock;

// TODO: maybe increase
/// How long we reuse HEAD / probe metadata for one URL (reduces duplicate upstream calls when
/// file managers stat many entries).
const DEFAULT_HEAD_CACHE_TTL: Duration = Duration::from_secs(15 * 60);

#[derive(Clone)]
pub struct HeadMetadataCache {
    ttl: Duration,
    inner: Arc<RwLock<HashMap<String, CachedHead>>>,
}

#[derive(Clone)]
struct CachedHead {
    expires_at: Instant,
    headers: HeadHeaders,
}

#[derive(Clone, Default)]
pub struct HeadHeaders {
    content_length: Option<u64>,
    content_type: Option<String>,
    accept_ranges: Option<String>,
    last_modified: Option<String>,
}

impl HeadHeaders {
    pub fn content_length(&self) -> Option<u64> {
        self.content_length
    }
}

impl HeadMetadataCache {
    pub fn new() -> Self {
        Self::with_ttl(DEFAULT_HEAD_CACHE_TTL)
    }

    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            ttl,
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn get(&self, key: &str) -> Option<HeadHeaders> {
        let now = Instant::now();
        let mut g = self.inner.write().await;
        g.retain(|_, v| v.expires_at > now);
        g.get(key).map(|e| e.headers.clone())
    }

    pub async fn insert(&self, key: String, headers: HeadHeaders) {
        let now = Instant::now();
        let mut g = self.inner.write().await;
        g.retain(|_, v| v.expires_at > now);
        if g.len() > 10_000 {
            g.clear();
        }
        g.insert(
            key,
            CachedHead {
                expires_at: now + self.ttl,
                headers,
            },
        );
    }
}

/// Parse `Content-Range: bytes 0-0/123456789` (or `bytes */len`) for total length.
fn total_from_content_range(value: &str) -> Option<u64> {
    let value = value.trim();
    let slash = value.rfind('/')?;
    let tail = value[slash + 1..].trim();
    if tail == "*" {
        return None;
    }
    tail.parse().ok()
}

fn header_first(headers: &ReqwestHeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

/// Best-effort MIME from file extension when upstream omits `Content-Type`.
pub fn guess_video_mime(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "mkv" => "video/x-matroska",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "avi" => "video/x-msvideo",
        "wmv" => "video/x-ms-wmv",
        "flv" => "video/x-flv",
        "m4v" => "video/x-m4v",
        "ts" => "video/mp2t",
        "m3u8" => "application/vnd.apple.mpegurl",
        _ => "application/octet-stream",
    }
}

/// Try HEAD, then a tiny ranged GET, then fall back to extension-only headers.
pub async fn resolve_stream_head_metadata(
    http: &reqwest::Client,
    upstream_url: &str,
    ext: &str,
) -> HeadHeaders {
    if let Ok(resp) = http.head(upstream_url).send().await {
        let status = resp.status();
        let headers = resp.headers().clone();
        let _ = resp.bytes().await;
        if status.is_success() {
            return headers_from_reqwest(&headers, ext);
        }
    }

    if let Ok(resp) = http
        .get(upstream_url)
        .header(reqwest::header::RANGE, "bytes=0-0")
        .send()
        .await
    {
        let status = resp.status();
        let headers = resp.headers().clone();
        let _ = resp.bytes().await;
        if status == ReqwestStatus::PARTIAL_CONTENT || status.is_success() {
            let mut meta = headers_from_reqwest(&headers, ext);
            if meta.content_length.is_none() {
                if let Some(cr) = header_first(&headers, "content-range") {
                    meta.content_length = total_from_content_range(&cr);
                }
            }
            if meta.content_type.is_some() || meta.content_length.is_some() {
                return meta;
            }
        }
    }

    HeadHeaders {
        content_length: None,
        content_type: Some(guess_video_mime(ext).to_string()),
        accept_ranges: Some("bytes".to_string()),
        last_modified: None,
    }
}

fn headers_from_reqwest(headers: &ReqwestHeaderMap, ext: &str) -> HeadHeaders {
    let content_type =
        header_first(headers, "content-type").or_else(|| Some(guess_video_mime(ext).to_string()));
    let content_length = header_first(headers, "content-length").and_then(|s| s.parse().ok());
    HeadHeaders {
        content_length,
        content_type,
        accept_ranges: header_first(headers, "accept-ranges"),
        last_modified: header_first(headers, "last-modified"),
    }
}

pub fn head_response_from_meta(meta: &HeadHeaders) -> Response<Body> {
    let mut out = HeaderMap::new();
    if let Some(ref ct) = meta.content_type {
        if let Ok(v) = HeaderValue::from_str(ct) {
            let _ = out.insert(CONTENT_TYPE, v);
        }
    }
    if let Some(len) = meta.content_length {
        if let Ok(v) = HeaderValue::from_str(&len.to_string()) {
            let _ = out.insert(CONTENT_LENGTH, v);
        }
    }
    if let Some(ref ar) = meta.accept_ranges {
        if let Ok(v) = HeaderValue::from_str(ar) {
            let _ = out.insert(ACCEPT_RANGES, v);
        }
    } else {
        let _ = out.insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    }
    if let Some(ref lm) = meta.last_modified {
        if let Ok(v) = HeaderValue::from_str(lm) {
            let _ = out.insert(LAST_MODIFIED, v);
        }
    }
    (axum::http::StatusCode::OK, out).into_response()
}

/// Copy selected headers from a successful upstream GET into an Axum response (streaming GET).
pub fn merge_upstream_headers(out: &mut HeaderMap, hdr: &ReqwestHeaderMap) {
    macro_rules! copy_if_present {
        ($name:expr) => {
            if let Some(v) = hdr.get($name) {
                let _ = out.insert($name, v.clone());
            }
        };
    }
    copy_if_present!(CONTENT_TYPE);
    copy_if_present!(CONTENT_LENGTH);
    copy_if_present!(CONTENT_RANGE);
    copy_if_present!(ACCEPT_RANGES);
    copy_if_present!(LAST_MODIFIED);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_range_total() {
        assert_eq!(
            total_from_content_range("bytes 0-0/12345678"),
            Some(12_345_678)
        );
        assert_eq!(total_from_content_range("bytes 0-999/*"), None);
    }
}
