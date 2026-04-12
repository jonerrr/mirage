use std::env;

use crate::limits::MirageLimits;

#[derive(Debug, Clone)]
pub struct Config {
    pub xtream_base_url: String,
    pub xtream_username: String,
    pub xtream_password: String,
    pub listen: String,
    pub limits: MirageLimits,
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

        Ok(Self {
            xtream_base_url: base,
            xtream_username,
            xtream_password,
            listen,
            limits,
        })
    }
}
