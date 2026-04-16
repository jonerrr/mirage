use std::sync::Arc;

use reqwest::Client;

use crate::cache::AppCache;
use crate::catalog::{MovieCatalogHandle, TvCatalogHandle};
use crate::config::MirageLimits;
use crate::head_metadata::HeadMetadataCache;
use crate::pace::UpstreamPacer;
use crate::xtream::XtreamClient;

#[derive(Clone)]
pub struct AppState {
    pub xtream: XtreamClient,
    pub http: Client,
    pub cache: AppCache,
    pub limits: MirageLimits,
    pub head_cache: HeadMetadataCache,
    pub tv_catalog: TvCatalogHandle,
    pub movie_catalog: MovieCatalogHandle,
    pub stream_probe_use_upstream_head: bool,
    pub pacer: Arc<UpstreamPacer>,
}
