mod client;
mod error;
mod types;
mod url;

pub use client::XtreamClient;
pub use error::XtreamError;
pub use types::{
    SeriesDetail, SeriesEpisode, SeriesInfoMeta, SeriesListing, VodCategory, VodStream,
};
