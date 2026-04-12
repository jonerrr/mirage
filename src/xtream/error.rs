use std::fmt;

#[derive(Debug)]
pub enum XtreamError {
    Auth(String),
    Network(String),
    UnexpectedResponse(String),
}

impl fmt::Display for XtreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auth(s) => write!(f, "auth: {s}"),
            Self::Network(s) => write!(f, "network: {s}"),
            Self::UnexpectedResponse(s) => write!(f, "unexpected response: {s}"),
        }
    }
}

impl std::error::Error for XtreamError {}
