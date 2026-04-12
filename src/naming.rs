use serde_json::Value;

use crate::xtream::VodStream;

/// Primary display title for a stream.
pub fn display_title(listing: &VodStream) -> String {
    let t = listing
        .title
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(listing.name.as_str());
    t.trim().to_string()
}

fn year_from_json_value(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => {
            let s = s.trim();
            if s.is_empty() {
                return None;
            }
            // "1974-01-01" or "1974"
            if s.len() >= 4 && s[..4].chars().all(|c| c.is_ascii_digit()) {
                let y: u32 = s[..4].parse().ok()?;
                if (1900..=2100).contains(&y) {
                    return Some(s[..4].to_string());
                }
            }
            if s.len() == 4 && s.chars().all(|c| c.is_ascii_digit()) {
                let y: u32 = s.parse().ok()?;
                if (1900..=2100).contains(&y) {
                    return Some(s.to_string());
                }
            }
            None
        }
        Value::Number(n) => {
            let y = n.as_u64().or_else(|| n.as_i64().map(|i| i as u64))? as u32;
            if (1900..=2100).contains(&y) {
                Some(y.to_string())
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Scan `text` for `(YYYY)` year windows; returns the **last** plausible movie year.
fn last_paren_year(text: &str) -> Option<String> {
    let mut best: Option<String> = None;
    for (open_idx, _) in text.match_indices('(') {
        let after = &text[open_idx + 1..];
        if after.len() < 5 || !after.is_char_boundary(4) {
            continue;
        }
        if !after[4..].starts_with(')') {
            continue;
        }
        let inner = after[..4].trim();
        if inner.len() == 4 && inner.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(y) = inner.parse::<u32>() {
                if (1900..=2100).contains(&y) {
                    best = Some(inner.to_string());
                }
            }
        }
    }
    best
}

/// Remove a single trailing ` (19xx|20xx)` so we do not duplicate year in the filename.
fn strip_trailing_paren_year(s: &str) -> String {
    let s = s.trim_end();
    if s.len() < 6 {
        return s.to_string();
    }
    if !s.ends_with(')') {
        return s.to_string();
    }
    if let Some(open) = s[..s.len() - 1].rfind('(') {
        let inner = s[open + 1..s.len() - 1].trim();
        if inner.len() == 4 && inner.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(y) = inner.parse::<u32>() {
                if (1900..=2100).contains(&y) {
                    return s[..open].trim_end().to_string();
                }
            }
        }
    }
    s.to_string()
}

/// Year string for naming; falls back to release date, title/name `(YYYY)`, then `"Unknown"`.
pub fn display_year(listing: &VodStream) -> String {
    if let Some(ref v) = listing.year {
        if let Some(y) = year_from_json_value(v) {
            return y;
        }
    }
    if let Some(rd) = listing
        .release_date
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if let Some(y) = year_from_json_value(&Value::String(rd.to_string())) {
            return y;
        }
    }
    if let Some(y) = last_paren_year(&listing.name) {
        return y;
    }
    if let Some(ref t) = listing.title {
        if let Some(y) = last_paren_year(t) {
            return y;
        }
    }
    "Unknown".to_string()
}

fn tmdb_from_value(v: &Value) -> Option<u64> {
    match v {
        Value::Number(n) => n
            .as_u64()
            .or_else(|| n.as_i64().filter(|&i| i >= 0).map(|i| i as u64))
            .or_else(|| {
                n.as_f64()
                    .filter(|f| f.is_finite() && *f >= 0.0)
                    .map(|f| f as u64)
            }),
        Value::String(s) => {
            let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
            if digits.is_empty() {
                None
            } else {
                digits.parse().ok()
            }
        }
        Value::Object(map) => {
            for key in ["id", "tmdb_id", "tmdbId", "tmdb"] {
                if let Some(inner) = map.get(key) {
                    if let Some(t) = tmdb_from_value(inner) {
                        return Some(t);
                    }
                }
            }
            None
        }
        Value::Array(arr) => arr.iter().find_map(tmdb_from_value),
        _ => None,
    }
}

/// Normalize `tmdb_id` JSON value to a numeric id for tags.
pub fn tmdb_number(listing: &VodStream) -> Option<u64> {
    listing.tmdb_id.as_ref().and_then(tmdb_from_value)
}

/// Characters unsafe for common filesystems / paths (Xtream titles sometimes include `/`).
pub fn sanitize_title(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\n' | '\r' => out.push(' '),
            _ => out.push(ch),
        }
    }
    let s = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.is_empty() {
        "Untitled".to_string()
    } else {
        s
    }
}

/// `movie_dir` == `movie_base` (no extension): title (year) tags… {vodid-n}
pub fn movie_base_name(listing: &VodStream) -> String {
    let raw_title = display_title(listing);
    let year = display_year(listing);
    let title_core = strip_trailing_paren_year(&raw_title);
    let title = sanitize_title(&title_core);
    let mut base = format!("{title} ({year})");
    if let Some(t) = tmdb_number(listing) {
        base.push_str(&format!(" {{tmdb-{t}}} [tmdbid-{t}]"));
    }
    base.push_str(&format!(" {{vodid-{}}}", listing.stream_id));
    base
}

pub fn video_extension(listing: &VodStream) -> String {
    listing
        .container_extension
        .as_deref()
        .map(|s| s.trim().trim_start_matches('.'))
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_else(|| "mp4".to_string())
}

pub fn video_filename(listing: &VodStream) -> String {
    format!("{}.{}", movie_base_name(listing), video_extension(listing))
}

/// Extract `{vodid-123}` from a basename or filename stem.
pub fn parse_vodid(name: &str) -> Option<i64> {
    const KEY: &str = "{vodid-";
    let start = name.find(KEY)? + KEY.len();
    let rest = &name[start..];
    let end = rest.find('}')?;
    rest[..end].parse().ok()
}

/// Known video extensions for splitting `path.ext` from the last segment.
pub fn split_video_ext(filename: &str) -> Option<(&str, &str)> {
    let lower = filename.to_ascii_lowercase();
    const EXTS: &[&str] = &[
        "mp4", "mkv", "avi", "ts", "m3u8", "webm", "mov", "wmv", "flv", "m4v",
    ];
    for ext in EXTS {
        let suffix = format!(".{ext}");
        if lower.ends_with(&suffix) {
            let cut = filename.len() - suffix.len();
            if cut > 0 {
                return Some((&filename[..cut], ext));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vodid_roundtrip() {
        let listing = VodStream {
            stream_id: 42,
            name: "Test / Movie".into(),
            title: None,
            year: Some(serde_json::json!(2021)),
            release_date: None,
            category_id: None,
            container_extension: Some("mp4".into()),
            tmdb_id: Some(serde_json::json!(99)),
        };
        let base = movie_base_name(&listing);
        assert!(base.contains("{tmdb-99}"));
        assert!(base.contains("[tmdbid-99]"));
        assert!(base.contains("{vodid-42}"));
        assert_eq!(parse_vodid(&base), Some(42));
    }

    #[test]
    fn deserialize_xtream_style_json() {
        let j = r#"{
            "stream_id": 12345,
            "name": "The Savage Is Loose (1974)",
            "year": "",
            "tmdbId": "73180",
            "container_extension": "mkv"
        }"#;
        let v: VodStream = serde_json::from_str(j).unwrap();
        assert_eq!(tmdb_number(&v), Some(73180));
        assert_eq!(display_year(&v), "1974");
        let base = movie_base_name(&v);
        assert!(base.contains("{tmdb-73180}"));
        assert!(!base.contains("(Unknown)"), "{}", base);
    }

    #[test]
    fn year_numeric_json() {
        let j = r#"{"stream_id":1,"name":"X","year":1999,"tmdb":"550"}"#;
        let v: VodStream = serde_json::from_str(j).unwrap();
        assert_eq!(display_year(&v), "1999");
        assert_eq!(tmdb_number(&v), Some(550));
    }

    #[test]
    fn tmdb_nested_object() {
        let j = r#"{"stream_id":1,"name":"X","year":"2000","tmdb_id":{"id":999}}"#;
        let v: VodStream = serde_json::from_str(j).unwrap();
        assert_eq!(tmdb_number(&v), Some(999));
    }

    #[test]
    fn strip_duplicate_year_in_title() {
        let listing = VodStream {
            stream_id: 1,
            name: "The Savage Is Loose (1974)".into(),
            title: None,
            year: Some(Value::String(String::new())),
            release_date: None,
            category_id: None,
            container_extension: Some("mp4".into()),
            tmdb_id: Some(serde_json::json!(73180)),
        };
        let base = movie_base_name(&listing);
        assert!(!base.contains("(Unknown)"), "{base}");
        assert_eq!(base.matches("(1974)").count(), 1);
        assert!(base.contains("{tmdb-73180}"));
        assert!(base.contains("[tmdbid-73180]"));
    }
}
