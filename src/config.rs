use std::env;
use std::path::PathBuf;
use std::time::Duration;

use crate::limits::MirageLimits;

#[derive(Debug, Clone)]
pub struct TvCatalogConfig {
    pub catalog_path: PathBuf,
    pub refresh: Duration,
}

#[derive(Debug, Clone)]
pub struct UpstreamPaceConfig {
    pub min_interval: Duration,
    pub max_inflight: u32,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub xtream_base_url: String,
    pub xtream_username: String,
    pub xtream_password: String,
    pub listen: String,
    pub limits: MirageLimits,
    pub tv_catalog: TvCatalogConfig,
    pub upstream_pace: UpstreamPaceConfig,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let xtream_base_url = env::var("XTREAM_BASE_URL")
            .map_err(|_| anyhow::anyhow!("XTREAM_BASE_URL is required"))?;
        let xtream_username = env::var("XTREAM_USERNAME")
            .map_err(|_| anyhow::anyhow!("XTREAM_USERNAME is required"))?;
        let xtream_password = env::var("XTREAM_PASSWORD")
            .map_err(|_| anyhow::anyhow!("XTREAM_PASSWORD is required"))?;

        let listen = env::var("LISTEN").unwrap_or_else(|_| "127.0.0.1:8080".to_string());

        let mut base = xtream_base_url.trim().to_string();
        while base.ends_with('/') {
            base.pop();
        }

        let limits = MirageLimits::from_env();

        let catalog_path = env::var("MIRAGE_TV_CATALOG_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("data/tv_catalog.rkyv"));

        let refresh_secs = env_positive("MIRAGE_TV_REFRESH_SECS", 12 * 60 * 60);
        let tv_catalog = TvCatalogConfig {
            catalog_path,
            refresh: Duration::from_secs(refresh_secs),
        };

        let min_interval_ms = env_positive("MIRAGE_UPSTREAM_MIN_INTERVAL_MS", 300);
        let max_inflight = env_positive("MIRAGE_UPSTREAM_MAX_INFLIGHT", 1);
        let upstream_pace = UpstreamPaceConfig {
            min_interval: Duration::from_millis(min_interval_ms),
            max_inflight,
        };

        Ok(Self {
            xtream_base_url: base,
            xtream_username,
            xtream_password,
            listen,
            limits,
            tv_catalog,
            upstream_pace,
        })
    }
}

fn env_positive<T>(key: &str, default: T) -> T
where
    T: std::cmp::PartialOrd + Copy + std::str::FromStr + From<u8>,
{
    env::var(key)
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&n| n >= T::from(1))
        .unwrap_or(default)
}
