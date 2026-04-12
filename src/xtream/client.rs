//! Thin async client over `reqwest` for VOD JSON endpoints.

use std::sync::Arc;

use reqwest::StatusCode;
use serde::de::DeserializeOwned;

use crate::pace::UpstreamPacer;

use super::error::XtreamError;
use super::types::{SeriesDetail, SeriesListing, VodCategory, VodStream, normalize_series_detail};
use super::url::build_api_url_with_params;

#[derive(Clone)]
pub struct XtreamClient {
    http: Arc<reqwest::Client>,
    pacer: Arc<UpstreamPacer>,
    base_url: String,
    username: String,
    password: String,
}

impl XtreamClient {
    pub fn new(
        http: reqwest::Client,
        pacer: Arc<UpstreamPacer>,
        base_url: String,
        username: String,
        password: String,
    ) -> Self {
        Self {
            http: Arc::new(http),
            pacer,
            base_url,
            username,
            password,
        }
    }

    pub async fn get_vod_categories(&self) -> Result<Vec<VodCategory>, XtreamError> {
        let url = build_api_url_with_params(
            &self.base_url,
            &self.username,
            &self.password,
            "get_vod_categories",
            &[],
        )
        .map_err(|e| XtreamError::Network(e.to_string()))?;
        self.get_json(&url).await
    }

    pub async fn get_vod_streams(&self, category_id: &str) -> Result<Vec<VodStream>, XtreamError> {
        let url = build_api_url_with_params(
            &self.base_url,
            &self.username,
            &self.password,
            "get_vod_streams",
            &[("category_id", category_id)],
        )
        .map_err(|e| XtreamError::Network(e.to_string()))?;
        self.get_json(&url).await
    }

    pub fn movie_stream_url(&self, stream_id: i64, extension: &str) -> String {
        super::url::build_movie_stream_url(
            &self.base_url,
            &self.username,
            &self.password,
            stream_id,
            extension,
        )
    }

    pub fn series_stream_url(&self, stream_id: i64, extension: &str) -> String {
        super::url::build_series_stream_url(
            &self.base_url,
            &self.username,
            &self.password,
            stream_id,
            extension,
        )
    }

    pub async fn get_series_categories(&self) -> Result<Vec<VodCategory>, XtreamError> {
        let url = build_api_url_with_params(
            &self.base_url,
            &self.username,
            &self.password,
            "get_series_categories",
            &[],
        )
        .map_err(|e| XtreamError::Network(e.to_string()))?;
        self.get_json(&url).await
    }

    pub async fn get_series(&self, category_id: &str) -> Result<Vec<SeriesListing>, XtreamError> {
        let url = build_api_url_with_params(
            &self.base_url,
            &self.username,
            &self.password,
            "get_series",
            &[("category_id", category_id)],
        )
        .map_err(|e| XtreamError::Network(e.to_string()))?;
        self.get_json(&url).await
    }

    pub async fn get_series_info(&self, series_id: i64) -> Result<SeriesDetail, XtreamError> {
        let series_id_string = series_id.to_string();
        let url = build_api_url_with_params(
            &self.base_url,
            &self.username,
            &self.password,
            "get_series_info",
            &[("series_id", series_id_string.as_str())],
        )
        .map_err(|e| XtreamError::Network(e.to_string()))?;
        let mut detail: SeriesDetail = self.get_json(&url).await?;
        normalize_series_detail(&mut detail);

        let has_episodes = detail.episodes.values().flatten().next().is_some();
        let has_named_info = detail
            .info
            .as_ref()
            .and_then(|i| i.name.as_ref())
            .map(|n| !n.trim().is_empty())
            .unwrap_or(false);

        if !has_episodes && !has_named_info {
            return Err(XtreamError::UnexpectedResponse(format!(
                "series {series_id} not found or empty info"
            )));
        }
        Ok(detail)
    }

    async fn get_json<T: DeserializeOwned>(&self, url: &str) -> Result<T, XtreamError> {
        let http = self.http.clone();
        let url = url.to_string();
        let pacer = self.pacer.clone();
        pacer
            .throttle(|| {
                let http = http.clone();
                let url = url.clone();
                async move { Self::execute_get_json::<T>(&http, &url).await }
            })
            .await
    }

    async fn execute_get_json<T: DeserializeOwned>(
        http: &reqwest::Client,
        url: &str,
    ) -> Result<T, XtreamError> {
        let response = http
            .get(url)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|e| XtreamError::Network(e.to_string()))?;

        let status = response.status();
        match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                return Err(XtreamError::Auth(format!("server returned {status}")));
            }
            _ => {}
        }

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(XtreamError::UnexpectedResponse(format!(
                "HTTP {status}: {body}"
            )));
        }

        let text = response
            .text()
            .await
            .map_err(|e| XtreamError::Network(e.to_string()))?;

        if text.is_empty() {
            return Err(XtreamError::UnexpectedResponse(
                "empty response body".into(),
            ));
        }

        serde_json::from_str(&text).map_err(|e| {
            XtreamError::UnexpectedResponse(format!("json parse error: {e}; body: {text}"))
        })
    }
}
