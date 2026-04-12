use std::env;

/// Caps applied after each Xtream API response (truncation only; the HTTP body is still fetched once).
#[derive(Debug, Clone, Copy)]
pub struct MirageLimits {
    pub test_mode: bool,
    pub max_categories: usize,
    pub max_vod_per_category: usize,
    pub max_series_per_category: usize,
    pub max_episodes_per_series: usize,
}

impl MirageLimits {
    pub fn from_env() -> Self {
        let test_mode = env_truthy("MIRAGE_TEST_MODE");
        let max_categories = env_usize_positive("MIRAGE_TEST_MAX_CATEGORIES", 1);
        let max_vod_per_category = env_usize_positive("MIRAGE_TEST_MAX_VOD", 10);
        let max_series_per_category = env_usize_positive("MIRAGE_TEST_MAX_SERIES", 10);
        let max_episodes_per_series = env_usize_positive("MIRAGE_TEST_MAX_EPISODES", 10);
        Self {
            test_mode,
            max_categories,
            max_vod_per_category,
            max_series_per_category,
            max_episodes_per_series,
        }
    }
}

fn env_truthy(key: &str) -> bool {
    matches!(
        env::var(key).map(|v| v.to_ascii_lowercase()).as_deref(),
        Ok("1" | "true" | "yes" | "on")
    )
}

fn env_usize_positive(key: &str, default: usize) -> usize {
    env::var(key)
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&n| n >= 1)
        .unwrap_or(default)
}
