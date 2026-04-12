//! Thin async client over `reqwest` for VOD JSON endpoints.

use std::sync::Arc;

use reqwest::StatusCode;
use serde::de::DeserializeOwned;

use super::error::XtreamError;
use super::types::{VodCategory, VodStream};
use super::url::build_api_url_with_params;

#[derive(Clone)]
pub struct XtreamClient {
    http: Arc<reqwest::Client>,
    base_url: String,
    username: String,
    password: String,
}

impl XtreamClient {
    pub fn new(
        http: reqwest::Client,
        base_url: String,
        username: String,
        password: String,
    ) -> Self {
        Self {
            http: Arc::new(http),
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

    pub async fn get_vod_streams(
        &self,
        category_id: &str,
    ) -> Result<Vec<VodStream>, XtreamError> {
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

    async fn get_json<T: DeserializeOwned>(&self, url: &str) -> Result<T, XtreamError> {
        let response = self
            .http
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
