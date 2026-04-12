use reqwest::Client;

use crate::cache::AppCache;
use crate::head_metadata::HeadMetadataCache;
use crate::limits::MirageLimits;
use crate::xtream::XtreamClient;

#[derive(Clone)]
pub struct AppState {
    pub xtream: XtreamClient,
    pub http: Client,
    pub cache: AppCache,
    pub limits: MirageLimits,
    pub head_cache: HeadMetadataCache,
}
