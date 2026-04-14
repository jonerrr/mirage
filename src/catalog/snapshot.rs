use std::time::{SystemTime, UNIX_EPOCH};

use rkyv::util::AlignedVec;
use rkyv::{Archive, Deserialize, Serialize};

/// Bump when the TV archived layout changes.
pub const TV_CATALOG_FORMAT_VERSION: u32 = 1;

/// Bump when the Movie archived layout changes.
pub const MOVIE_CATALOG_FORMAT_VERSION: u32 = 1;

#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
#[rkyv(derive(Debug))]
pub struct TvCatalogSeries {
    pub series_id: i64,
    pub folder_name: String,
    pub list_label: String,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
#[rkyv(derive(Debug))]
pub struct TvCatalogSnapshot {
    pub format_version: u32,
    pub built_at_unix_secs: u64,
    pub series: Vec<TvCatalogSeries>,
}

impl TvCatalogSnapshot {
    pub fn new(series: Vec<TvCatalogSeries>) -> Self {
        let built_at_unix_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self {
            format_version: TV_CATALOG_FORMAT_VERSION,
            built_at_unix_secs,
            series,
        }
    }

    pub fn to_bytes_rkyv(&self) -> Result<AlignedVec, rkyv::rancor::Error> {
        rkyv::to_bytes::<rkyv::rancor::Error>(self)
    }
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
#[rkyv(derive(Debug))]
pub struct MovieCatalogStream {
    pub stream_id: i64,
    pub folder_name: String,
    pub list_label: String,
    pub extension: String,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
#[rkyv(derive(Debug))]
pub struct MovieCatalogCategory {
    pub category_id: String,
    pub category_name: String,
    pub streams: Vec<MovieCatalogStream>,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
#[rkyv(derive(Debug))]
pub struct MovieCatalogSnapshot {
    pub format_version: u32,
    pub built_at_unix_secs: u64,
    pub categories: Vec<MovieCatalogCategory>,
}

impl MovieCatalogSnapshot {
    pub fn new(categories: Vec<MovieCatalogCategory>) -> Self {
        let built_at_unix_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self {
            format_version: MOVIE_CATALOG_FORMAT_VERSION,
            built_at_unix_secs,
            categories,
        }
    }

    pub fn to_bytes_rkyv(&self) -> Result<AlignedVec, rkyv::rancor::Error> {
        rkyv::to_bytes::<rkyv::rancor::Error>(self)
    }
}
