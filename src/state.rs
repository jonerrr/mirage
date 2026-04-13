use std::sync::Arc;

use reqwest::Client;
use tokio::sync::Semaphore;

use crate::cache::AppCache;
use crate::config::MirageLimits;
use crate::head_metadata::HeadMetadataCache;
use crate::tv_catalog::TvCatalogHandle;
use crate::xtream::XtreamClient;

#[derive(Clone)]
pub struct AppState {
    pub xtream: XtreamClient,
    pub http: Client,
    pub cache: AppCache,
    pub limits: MirageLimits,
    pub head_cache: HeadMetadataCache,
    pub tv_catalog: TvCatalogHandle,
    pub stream_probe_use_upstream_head: bool,
    pub stream_inflight: Arc<Semaphore>,
}
