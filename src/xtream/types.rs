use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct VodCategory {
    #[serde(default)]
    pub category_id: String,
    #[serde(default)]
    pub category_name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VodStream {
    #[serde(default)]
    pub stream_id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub title: Option<String>,
    /// Year may be sent as JSON string or number.
    #[serde(default)]
    pub year: Option<serde_json::Value>,
    #[serde(default, alias = "releaseDate", alias = "releasedate")]
    pub release_date: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub category_id: Option<String>,
    #[serde(default)]
    pub container_extension: Option<String>,
    #[serde(default, alias = "tmdbId", alias = "tmdb")]
    pub tmdb_id: Option<serde_json::Value>,
}
