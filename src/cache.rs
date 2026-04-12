use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

use crate::limits::MirageLimits;
use crate::xtream::{SeriesDetail, VodCategory, VodStream, XtreamClient, XtreamError};

const DEFAULT_TTL: Duration = Duration::from_secs(12 * 60 * 60);

#[derive(Clone)]
pub struct CacheEntry<T: Clone> {
    pub value: Arc<T>,
    pub expires_at: Instant,
}

#[derive(Clone)]
pub struct AppCache {
    ttl: Duration,
    inner: Arc<RwLock<Inner>>,
}

struct Inner {
    vod_categories: Option<CacheEntry<Vec<VodCategory>>>,
    vod_streams: HashMap<String, CacheEntry<Vec<VodStream>>>,
    series_info: HashMap<String, CacheEntry<SeriesDetail>>,
}

impl AppCache {
    pub fn new() -> Self {
        Self::with_ttl(DEFAULT_TTL)
    }

    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            ttl,
            inner: Arc::new(RwLock::new(Inner {
                vod_categories: None,
                vod_streams: HashMap::new(),
                series_info: HashMap::new(),
            })),
        }
    }

    pub async fn vod_categories(
        &self,
        client: &XtreamClient,
        limits: MirageLimits,
    ) -> Result<Arc<Vec<VodCategory>>, XtreamError> {
        let now = Instant::now();
        {
            let g = self.inner.read().await;
            if let Some(ref e) = g.vod_categories {
                if now < e.expires_at {
                    return Ok(e.value.clone());
                }
            }
        }

        let mut data = client.get_vod_categories().await?;
        if limits.test_mode {
            data.truncate(limits.max_categories);
        }
        let entry = CacheEntry {
            value: Arc::new(data),
            expires_at: now + self.ttl,
        };

        let mut g = self.inner.write().await;
        g.vod_categories = Some(entry.clone());
        Ok(entry.value)
    }

    pub async fn vod_streams_for_category(
        &self,
        client: &XtreamClient,
        category_id: &str,
        limits: MirageLimits,
    ) -> Result<Arc<Vec<VodStream>>, XtreamError> {
        let now = Instant::now();
        let key = category_id.to_string();

        {
            let g = self.inner.read().await;
            if let Some(e) = g.vod_streams.get(&key) {
                if now < e.expires_at {
                    return Ok(e.value.clone());
                }
            }
        }

        let mut data = client.get_vod_streams(category_id).await?;
        if limits.test_mode {
            data.truncate(limits.max_vod_per_category);
        }
        let entry = CacheEntry {
            value: Arc::new(data),
            expires_at: now + self.ttl,
        };

        let mut g = self.inner.write().await;
        g.vod_streams.insert(key, entry.clone());
        Ok(entry.value)
    }

    pub async fn series_detail(
        &self,
        client: &XtreamClient,
        series_id: i64,
        limits: MirageLimits,
    ) -> Result<Arc<SeriesDetail>, XtreamError> {
        let now = Instant::now();
        let key = series_id.to_string();

        {
            let g = self.inner.read().await;
            if let Some(e) = g.series_info.get(&key) {
                if now < e.expires_at {
                    return Ok(e.value.clone());
                }
            }
        }

        let mut data = client.get_series_info(series_id).await?;
        if limits.test_mode {
            for (_, eps) in data.episodes.iter_mut() {
                eps.truncate(limits.max_episodes_per_series);
            }
        }
        let entry = CacheEntry {
            value: Arc::new(data),
            expires_at: now + self.ttl,
        };

        let mut g = self.inner.write().await;
        g.series_info.insert(key, entry.clone());
        Ok(entry.value)
    }
}
