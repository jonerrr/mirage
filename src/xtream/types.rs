use std::collections::HashMap;

use serde::{Deserialize, Deserializer};

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
    /// Snake case only; `releaseDate` / `releasedate` are separate fields (see below).
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default, rename = "releaseDate")]
    pub release_date_alt: Option<String>,
    #[serde(default, rename = "releasedate")]
    pub release_date_lower: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub category_id: Option<String>,
    #[serde(default)]
    pub container_extension: Option<String>,
    #[serde(default, alias = "tmdbId", alias = "tmdb")]
    pub tmdb_id: Option<serde_json::Value>,
}

/// One series row from `get_series`.
#[derive(Debug, Clone, Deserialize)]
pub struct SeriesListing {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub series_id: i64,
    #[serde(default, alias = "tmdbId", alias = "tmdb")]
    pub tmdb: Option<serde_json::Value>,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default, alias = "releaseDate")]
    pub release_date_alt: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct SeriesInfoMeta {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, alias = "tmdbId", alias = "tmdb")]
    pub tmdb: Option<serde_json::Value>,
    #[serde(default, deserialize_with = "deserialize_optional_string_or_number")]
    pub category_id: Option<String>,
    #[serde(default)]
    pub category_ids: Vec<serde_json::Value>,
    /// JSON key `release_date` only (do not alias `releaseDate` here — both keys often appear).
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default, rename = "releaseDate")]
    pub release_date_alt: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SeriesEpisode {
    #[serde(default)]
    pub id: Option<serde_json::Value>,
    #[serde(default)]
    pub episode_num: Option<serde_json::Value>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub container_extension: Option<String>,
    #[serde(default)]
    pub season: Option<serde_json::Value>,
}

fn deserialize_optional_string_or_number<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let v: Option<serde_json::Value> = Option::deserialize(deserializer)?;
    Ok(match v {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(s)) => Some(s),
        Some(serde_json::Value::Number(n)) => n.as_i64().map(|i| i.to_string()),
        _ => None,
    })
}

fn deserialize_series_info<'de, D>(deserializer: D) -> Result<Option<SeriesInfoMeta>, D::Error>
where
    D: Deserializer<'de>,
{
    let v = serde_json::Value::deserialize(deserializer)?;
    match v {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::Array(a) if a.is_empty() => Ok(None),
        serde_json::Value::Array(_) => Ok(None),
        serde_json::Value::Object(_) => serde_json::from_value(v)
            .map(Some)
            .map_err(serde::de::Error::custom),
        _ => Ok(None),
    }
}

fn deserialize_episodes_map<'de, D>(
    deserializer: D,
) -> Result<HashMap<String, Vec<SeriesEpisode>>, D::Error>
where
    D: Deserializer<'de>,
{
    let v = serde_json::Value::deserialize(deserializer)?;
    match v {
        serde_json::Value::Null => Ok(HashMap::new()),
        serde_json::Value::Array(_) => Ok(HashMap::new()),
        serde_json::Value::Object(_) => serde_json::from_value(v).map_err(serde::de::Error::custom),
        _ => Ok(HashMap::new()),
    }
}

/// Full series payload from `get_series_info`.
#[derive(Debug, Clone, Deserialize)]
pub struct SeriesDetail {
    #[serde(default)]
    #[allow(dead_code)]
    pub seasons: Vec<serde_json::Value>,
    #[serde(default, deserialize_with = "deserialize_series_info")]
    pub info: Option<SeriesInfoMeta>,
    #[serde(default, deserialize_with = "deserialize_episodes_map")]
    pub episodes: HashMap<String, Vec<SeriesEpisode>>,
}

/// Fill missing `info` and names when the panel returns `info: []` but episode lists exist.
pub fn normalize_series_detail(detail: &mut SeriesDetail) {
    let has_episodes = detail.episodes.values().flatten().next().is_some();
    if let Some(ref mut info) = detail.info {
        if info
            .name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .is_none()
            && has_episodes
        {
            info.name = Some("Unknown".into());
        }
    } else if has_episodes {
        detail.info = Some(SeriesInfoMeta {
            name: Some("Unknown".into()),
            tmdb: None,
            category_id: None,
            category_ids: vec![],
            release_date: None,
            release_date_alt: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn series_detail_info_empty_array_deserializes() {
        let j = r#"{"seasons":[],"info":[],"episodes":{"1":[{"id":"1","title":"Pilot","container_extension":"mkv","season":1}]}}"#;
        let mut d: SeriesDetail = serde_json::from_str(j).expect("parse");
        assert!(d.info.is_none());
        normalize_series_detail(&mut d);
        assert!(d.info.is_some());
        assert_eq!(
            d.info.as_ref().and_then(|i| i.name.as_deref()),
            Some("Unknown")
        );
    }

    #[test]
    fn series_info_meta_duplicate_release_keys_deserializes() {
        let j = r#"{"name":"Test","releaseDate":"","release_date":"","category_ids":[]}"#;
        let m: SeriesInfoMeta = serde_json::from_str(j).expect("parse");
        assert_eq!(m.release_date.as_deref(), Some(""));
        assert_eq!(m.release_date_alt.as_deref(), Some(""));
    }
}
