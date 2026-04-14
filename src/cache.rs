use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

use crate::config::MirageLimits;
use crate::xtream::{SeriesDetail, XtreamClient, XtreamError};

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
                series_info: HashMap::new(),
            })),
        }
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
            if let Some(e) = g.series_info.get(&key)
                && now < e.expires_at
            {
                return Ok(e.value.clone());
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
