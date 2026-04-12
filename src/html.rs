use html_escape::encode_text;

/// One directory listing page: `<ul>` of `<a href="...">` entries.
pub fn directory_page(title: &str, entries: &[(String, String)]) -> String {
    let mut out = String::new();
    out.push_str("<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>");
    out.push_str(&encode_text(title));
    out.push_str("</title></head><body><h1>");
    out.push_str(&encode_text(title));
    out.push_str("</h1><ul>");
    for (href, label) in entries {
        out.push_str("<li><a href=\"");
        out.push_str(&encode_text(href));
        out.push_str("\">");
        out.push_str(&encode_text(label));
        out.push_str("</a></li>");
    }
    out.push_str("</ul></body></html>");
    out
}
