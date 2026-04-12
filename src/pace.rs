use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{Mutex, Semaphore};

/// Global minimum spacing between Xtream JSON API calls, plus optional max concurrency.
#[derive(Debug)]
pub struct UpstreamPacer {
    min_interval: Duration,
    sem: Semaphore,
    /// `None` until the first request so the first call is not delayed by `min_interval`.
    last_start: Mutex<Option<Instant>>,
}

impl UpstreamPacer {
    pub fn new(min_interval: Duration, max_inflight: u32) -> Arc<Self> {
        Arc::new(Self {
            min_interval,
            sem: Semaphore::new(max_inflight.max(1) as usize),
            last_start: Mutex::new(None),
        })
    }

    /// Runs `body` after taking a permit and honoring `min_interval` between request starts.
    pub async fn throttle<F, Fut, T>(&self, body: F) -> T
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        let _permit = self
            .sem
            .acquire()
            .await
            .expect("UpstreamPacer semaphore closed");

        {
            let mut last = self.last_start.lock().await;
            loop {
                let now = Instant::now();
                let earliest = last.map(|t| t + self.min_interval).unwrap_or(now);
                if now >= earliest {
                    *last = Some(now);
                    break;
                }
                let wait = earliest - now;
                tokio::time::sleep(wait).await;
            }
        }

        body().await
    }
}
