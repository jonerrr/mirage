use url::Url;

pub fn build_api_url_with_params(
    base_url: &str,
    username: &str,
    password: &str,
    action: &str,
    extra_params: &[(&str, &str)],
) -> Result<String, url::ParseError> {
    let base = base_url.trim_end_matches('/');
    let mut url = Url::parse(&format!("{base}/player_api.php"))?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("username", username);
        q.append_pair("password", password);
        q.append_pair("action", action);
        for (k, v) in extra_params {
            q.append_pair(k, v);
        }
    }
    Ok(url.into())
}

/// `{base}/movie/{username}/{password}/{stream_id}.{extension}`
pub fn build_movie_stream_url(
    base_url: &str,
    username: &str,
    password: &str,
    stream_id: i64,
    extension: &str,
) -> String {
    let base = base_url.trim_end_matches('/');
    format!("{base}/movie/{username}/{password}/{stream_id}.{extension}")
}

/// `{base}/series/{username}/{password}/{stream_id}.{extension}`
pub fn build_series_stream_url(
    base_url: &str,
    username: &str,
    password: &str,
    stream_id: i64,
    extension: &str,
) -> String {
    let base = base_url.trim_end_matches('/');
    format!("{base}/series/{username}/{password}/{stream_id}.{extension}")
}
