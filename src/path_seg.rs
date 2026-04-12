use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};

pub fn encode_path_segment(s: &str) -> String {
    utf8_percent_encode(s, NON_ALPHANUMERIC).to_string()
}
