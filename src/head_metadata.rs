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

/// How long we reuse probe metadata for one URL (reduces duplicate upstream calls when
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

fn normalized_accept_ranges(raw: Option<String>) -> Option<String> {
    let raw = raw?.trim().to_ascii_lowercase();
    match raw.as_str() {
        "bytes" => Some("bytes".to_string()),
        "none" => Some("none".to_string()),
        _ => None,
    }
}

/// Reject only explicit `Accept-Ranges: none`. RFC 7233 allows `bytes` or `none`; many IPTV/CDN
/// stacks send non-standard values (e.g. `0-3918810744`) while still returning valid `206` +
/// `Content-Range`, so we treat anything other than `none` as acceptable when range support is
/// already proven by the response (206 + `Content-Range`, or successful HEAD with size).
fn reject_accept_ranges_none(headers: &ReqwestHeaderMap) -> Result<(), String> {
    if matches!(
        normalized_accept_ranges(header_first(headers, "accept-ranges")).as_deref(),
        Some("none")
    ) {
        Err("upstream Accept-Ranges is 'none' (byte ranges not supported)".to_string())
    } else {
        Ok(())
    }
}

fn parse_strict_head_ok(
    headers: &ReqwestHeaderMap,
    status: ReqwestStatus,
) -> Result<HeadHeaders, String> {
    if !status.is_success() {
        return Err(format!("HEAD status {status}"));
    }
    let content_type = header_first(headers, "content-type")
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "missing Content-Type on upstream HEAD response".to_string())?;
    reject_accept_ranges_none(headers)?;
    let cl = header_first(headers, "content-length")
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|&n| n > 0)
        .ok_or_else(|| "missing or zero Content-Length on upstream HEAD response".to_string())?;
    Ok(HeadHeaders {
        content_length: Some(cl),
        content_type: Some(content_type),
        accept_ranges: Some("bytes".to_string()),
        last_modified: header_first(headers, "last-modified"),
    })
}

fn parse_strict_ranged_get_probe(
    headers: &ReqwestHeaderMap,
    status: ReqwestStatus,
) -> Result<HeadHeaders, String> {
    if status != ReqwestStatus::PARTIAL_CONTENT {
        return Err(format!(
            "ranged GET probe expected 206 Partial Content, got {status} (upstream must support byte ranges with Content-Range)"
        ));
    }
    let content_type = header_first(headers, "content-type")
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "missing Content-Type on ranged GET probe".to_string())?;
    reject_accept_ranges_none(headers)?;
    let cr = header_first(headers, "content-range")
        .ok_or_else(|| "missing Content-Range on 206 ranged GET probe".to_string())?;
    let total = total_from_content_range(&cr).ok_or_else(|| {
        "invalid Content-Range on ranged GET probe (could not parse total size)".to_string()
    })?;
    if total == 0 {
        return Err("invalid Content-Range total (zero) on ranged GET probe".to_string());
    }
    Ok(HeadHeaders {
        content_length: Some(total),
        content_type: Some(content_type),
        accept_ranges: Some("bytes".to_string()),
        last_modified: header_first(headers, "last-modified"),
    })
}

fn probe_failure_message(detail: String) -> String {
    format!(
        "{detail} If rclone listing/stat fails with this error, you may try rclone's --http-no-head (client-side only; it does not fix a broken upstream)."
    )
}

/// Probe upstream metadata with `GET` + `Range: bytes=0-0` (strict: 206 + Content-Range + headers).
/// If `probe_use_upstream_head`, tries upstream `HEAD` first when it returns a strictly valid response.
pub async fn resolve_stream_head_metadata(
    http: &reqwest::Client,
    upstream_url: &str,
    probe_use_upstream_head: bool,
) -> Result<HeadHeaders, String> {
    if probe_use_upstream_head && let Ok(resp) = http.head(upstream_url).send().await {
        let status = resp.status();
        let headers = resp.headers().clone();
        let _ = resp.bytes().await;
        if status.is_success()
            && let Ok(meta) = parse_strict_head_ok(&headers, status)
        {
            return Ok(meta);
        }
    }

    let resp = http
        .get(upstream_url)
        .header(reqwest::header::RANGE, "bytes=0-0")
        .send()
        .await
        .map_err(|e| probe_failure_message(format!("upstream ranged GET probe failed: {e}")))?;

    let status = resp.status();
    let headers = resp.headers().clone();
    let _ = resp.bytes().await;

    parse_strict_ranged_get_probe(&headers, status).map_err(probe_failure_message)
}

pub fn head_response_from_meta(meta: &HeadHeaders) -> Response<Body> {
    let mut out = HeaderMap::new();
    if let Some(ref ct) = meta.content_type
        && let Ok(v) = HeaderValue::from_str(ct)
    {
        let _ = out.insert(CONTENT_TYPE, v);
    }
    if let Some(len) = meta.content_length
        && let Ok(v) = HeaderValue::from_str(&len.to_string())
    {
        let _ = out.insert(CONTENT_LENGTH, v);
    }
    if let Some(ref ar) = meta.accept_ranges {
        if let Ok(v) = HeaderValue::from_str(ar) {
            let _ = out.insert(ACCEPT_RANGES, v);
        }
    } else {
        let _ = out.insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    }
    if let Some(ref lm) = meta.last_modified
        && let Ok(v) = HeaderValue::from_str(lm)
    {
        let _ = out.insert(LAST_MODIFIED, v);
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
    if let Some(ar) = normalized_accept_ranges(header_first(hdr, "accept-ranges")) {
        if let Ok(v) = HeaderValue::from_str(&ar) {
            let _ = out.insert(ACCEPT_RANGES, v);
        }
    } else {
        let _ = out.insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    }
    copy_if_present!(LAST_MODIFIED);
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;

    #[test]
    fn content_range_total() {
        assert_eq!(
            total_from_content_range("bytes 0-0/12345678"),
            Some(12_345_678)
        );
        assert_eq!(total_from_content_range("bytes 0-999/*"), None);
    }

    #[test]
    fn strict_ranged_probe_ok() {
        let mut headers = ReqwestHeaderMap::new();
        headers.insert("content-type", HeaderValue::from_static("video/x-matroska"));
        headers.insert("accept-ranges", HeaderValue::from_static("bytes"));
        headers.insert(
            "content-range",
            HeaderValue::from_static("bytes 0-0/6783696380"),
        );

        let meta = parse_strict_ranged_get_probe(&headers, ReqwestStatus::PARTIAL_CONTENT).unwrap();
        assert_eq!(meta.content_length, Some(6_783_696_380));
        assert_eq!(meta.content_type.as_deref(), Some("video/x-matroska"));
        assert_eq!(meta.accept_ranges.as_deref(), Some("bytes"));
    }

    #[test]
    fn strict_ranged_probe_rejects_non_206() {
        let mut headers = ReqwestHeaderMap::new();
        headers.insert("content-type", HeaderValue::from_static("video/x-matroska"));
        assert!(parse_strict_ranged_get_probe(&headers, ReqwestStatus::OK).is_err());
    }

    /// IPTV/CDN quirk: `Accept-Ranges` is not RFC-compliant but `206` + `Content-Range` proves ranges.
    #[test]
    fn nonstandard_accept_ranges_allowed_with_valid_content_range() {
        let mut headers = ReqwestHeaderMap::new();
        headers.insert("content-type", HeaderValue::from_static("video/x-matroska"));
        headers.insert("accept-ranges", HeaderValue::from_static("0-6783696380"));
        headers.insert(
            "content-range",
            HeaderValue::from_static("bytes 0-0/6783696380"),
        );

        let meta = parse_strict_ranged_get_probe(&headers, ReqwestStatus::PARTIAL_CONTENT).unwrap();
        assert_eq!(meta.content_length, Some(6_783_696_380));
        assert_eq!(meta.accept_ranges.as_deref(), Some("bytes"));
    }

    #[test]
    fn strict_ranged_probe_rejects_accept_ranges_none() {
        let mut headers = ReqwestHeaderMap::new();
        headers.insert("content-type", HeaderValue::from_static("video/x-matroska"));
        headers.insert("accept-ranges", HeaderValue::from_static("none"));
        headers.insert(
            "content-range",
            HeaderValue::from_static("bytes 0-0/6783696380"),
        );

        assert!(parse_strict_ranged_get_probe(&headers, ReqwestStatus::PARTIAL_CONTENT).is_err());
    }

    #[test]
    fn strict_head_ok() {
        let mut headers = ReqwestHeaderMap::new();
        headers.insert("content-type", HeaderValue::from_static("video/mp4"));
        headers.insert("accept-ranges", HeaderValue::from_static("bytes"));
        headers.insert("content-length", HeaderValue::from_static("999"));

        let meta = parse_strict_head_ok(&headers, ReqwestStatus::OK).unwrap();
        assert_eq!(meta.content_length, Some(999));
    }
}
