// Expand tiny `Range: bytes=0-…` requests before sending them upstream.
//
// FUSE/rclone/Jellyfin/ffprobe often issue small ranges at the start of a file (e.g. `bytes=0-0`
// or a few KB). Many CDNs respond with **206** and a **1-byte** body for `bytes=0-0`, which is
// not enough for MKV EBML / MP4 `ftyp` detection. We widen the upstream range to at least
// [`MIN_PROBE_BYTES`] so the first chunk is enough for container probing.
// TODO: is this needed?
/// Minimum bytes to request from the start of the file when the client asks for a tiny range.
const MIN_PROBE_BYTES: u64 = 256 * 1024;

fn parse_first_range(s: &str) -> Option<(u64, Option<u64>)> {
    let s = s.trim();
    if s.len() < 6 || !s[..6].eq_ignore_ascii_case("bytes=") {
        return None;
    }
    let rest = &s[6..];
    if rest.contains(',') {
        return None;
    }
    let rest = rest.trim();
    let (start_s, end_s) = rest.split_once('-')?;
    let start: u64 = start_s.parse().ok()?;
    if end_s.is_empty() {
        return Some((start, None));
    }
    let end: u64 = end_s.parse().ok()?;
    Some((start, Some(end)))
}

/// If the client `Range` is a small read from offset 0, return a wider `Range` header value for
/// the upstream request. Otherwise return `None` (caller should forward the original range).
pub fn expand_upstream_range_for_probe(
    range_header: &str,
    known_content_length: Option<u64>,
) -> Option<String> {
    let (start, end_opt) = parse_first_range(range_header)?;
    if start != 0 {
        return None;
    }
    // bytes=0- means "rest of file"; do not expand.
    let end = end_opt?;
    let requested_len = end.saturating_sub(start).saturating_add(1);
    if requested_len >= MIN_PROBE_BYTES {
        return None;
    }
    let mut last_byte = start + MIN_PROBE_BYTES - 1;
    if let Some(total) = known_content_length
        && total > 0
    {
        last_byte = last_byte.min(total - 1);
    }
    // After capping to file size, widening must still cover what the client asked for.
    if last_byte < end {
        return None;
    }
    Some(format!("bytes={start}-{last_byte}"))
}

/// Value to send upstream for a GET: expanded range when applicable, else the original.
pub fn upstream_range_header_value(
    client_range: Option<&str>,
    known_content_length: Option<u64>,
) -> Option<String> {
    let r = client_range?;
    let expanded = expand_upstream_range_for_probe(r, known_content_length);
    Some(expanded.unwrap_or_else(|| r.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_tiny_zero_start() {
        let e = expand_upstream_range_for_probe("bytes=0-0", None).unwrap();
        assert_eq!(e, "bytes=0-262143");
    }

    #[test]
    fn no_expand_when_large_enough() {
        assert!(expand_upstream_range_for_probe("bytes=0-262143", None).is_none());
    }

    #[test]
    fn caps_by_content_length() {
        let e = expand_upstream_range_for_probe("bytes=0-0", Some(100)).unwrap();
        assert_eq!(e, "bytes=0-99");
    }

    #[test]
    fn no_expand_open_ended() {
        assert!(expand_upstream_range_for_probe("bytes=0-", None).is_none());
    }

    #[test]
    fn no_expand_non_start() {
        assert!(expand_upstream_range_for_probe("bytes=1000-2000", None).is_none());
    }

    #[test]
    fn case_insensitive_bytes_prefix() {
        let e = expand_upstream_range_for_probe("Bytes=0-0", None).unwrap();
        assert_eq!(e, "bytes=0-262143");
    }
}
