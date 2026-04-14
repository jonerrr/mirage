use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};

use crate::error::AppError;
use crate::head_metadata::{head_response_from_meta, resolve_stream_head_metadata};
use crate::html::directory_page;
use crate::naming::{
    episode_extension, episode_filename, episode_season_number, episodes_in_season,
    find_episode_by_stream_id, parse_epid, parse_season_dir, parse_seriesid, season_dir_name,
    season_numbers_for_series, split_video_ext,
};
use crate::path_seg::encode_path_segment;
use crate::state::AppState;

pub async fn index(State(state): State<AppState>) -> Html<String> {
    let title = if state.limits.test_mode {
        "Mirage (test mode)"
    } else {
        "Mirage"
    };
    let entries = if state.limits.test_mode {
        vec![
            ("movies/".into(), "Movies (limited catalog)".into()),
            ("tv/".into(), "TV Shows (limited catalog)".into()),
        ]
    } else {
        vec![
            ("movies/".into(), "Movies".into()),
            ("tv/".into(), "TV Shows".into()),
        ]
    };
    Html(directory_page(title, &entries))
}

pub async fn redirect_movies() -> Redirect {
    Redirect::permanent("/movies/")
}

pub async fn redirect_tv() -> Redirect {
    Redirect::permanent("/tv/")
}

pub async fn list_vod_categories(State(state): State<AppState>) -> impl IntoResponse {
    let Some(loaded) = state.movie_catalog.get().await else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Html(directory_page("Movies — catalog not ready yet", &[])),
        )
            .into_response();
    };

    let archived = loaded.archived();
    let mut entries = Vec::new();
    for c in archived.categories.iter() {
        let href = format!("{}/", encode_path_segment(&c.category_id));
        let label = if c.category_name.trim().is_empty() {
            c.category_id.to_string()
        } else {
            c.category_name.to_string()
        };
        entries.push((href, label));
    }

    Html(directory_page("Movies — categories", &entries)).into_response()
}

pub async fn list_movies_in_category(
    State(state): State<AppState>,
    Path(category_id): Path<String>,
) -> impl IntoResponse {
    let Some(loaded) = state.movie_catalog.get().await else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Html(directory_page("Movies — catalog not ready yet", &[])),
        )
            .into_response();
    };

    let archived = loaded.archived();
    let Some(cat) = archived
        .categories
        .iter()
        .find(|c| c.category_id == category_id)
    else {
        return (
            StatusCode::NOT_FOUND,
            Html(directory_page("Movies — category not found", &[])),
        )
            .into_response();
    };

    let mut entries = Vec::new();
    for v in cat.streams.iter() {
        let href = format!("{}/", encode_path_segment(v.folder_name.as_str()));
        entries.push((href, v.list_label.as_str().to_string()));
    }

    let title = format!("Movies — category {}", cat.category_name);
    Html(directory_page(&title, &entries)).into_response()
}

pub async fn list_movie_folder(
    State(state): State<AppState>,
    Path((category_id, movie_dir)): Path<(String, String)>,
) -> impl IntoResponse {
    let Some(loaded) = state.movie_catalog.get().await else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Html(directory_page("Movies — catalog not ready yet", &[])),
        )
            .into_response();
    };

    let archived = loaded.archived();
    let Some(cat) = archived
        .categories
        .iter()
        .find(|c| c.category_id == category_id)
    else {
        return (StatusCode::NOT_FOUND, Html(String::new())).into_response();
    };

    let Some(listing) = cat.streams.iter().find(|v| v.folder_name == movie_dir) else {
        return (StatusCode::NOT_FOUND, Html(String::new())).into_response();
    };

    let vf = format!("{}.{}", listing.folder_name, listing.extension);
    let href = encode_path_segment(&vf);
    let entries = vec![(href, vf.clone())];
    let title = format!("{} /", listing.folder_name);
    Html(directory_page(&title, &entries)).into_response()
}

pub async fn list_all_tv_series(State(state): State<AppState>) -> impl IntoResponse {
    let Some(loaded) = state.tv_catalog.get().await else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Html(directory_page("TV Shows — catalog not ready yet", &[])),
        )
            .into_response();
    };

    let archived = loaded.archived();
    let mut entries = Vec::new();
    for row in archived.series.iter() {
        let href = format!("{}/", encode_path_segment(row.folder_name.as_str()));
        entries.push((href, row.list_label.as_str().to_string()));
    }

    Html(directory_page("TV Shows", &entries)).into_response()
}

pub async fn list_seasons(
    State(state): State<AppState>,
    Path(show_dir): Path<String>,
) -> Result<Html<String>, AppError> {
    let series_id = parse_seriesid(&show_dir)
        .ok_or_else(|| AppError::not_found("show folder must contain {seriesid-<id>}"))?;

    let detail = state
        .cache
        .series_detail(&state.xtream, series_id, state.limits)
        .await
        .map_err(AppError::from)?;

    detail
        .info
        .as_ref()
        .ok_or_else(|| AppError::not_found("series has no info"))?;

    let seasons = season_numbers_for_series(&detail);
    let mut entries = Vec::new();
    for n in seasons {
        let name = season_dir_name(n);
        let href = format!("{}/", encode_path_segment(&name));
        entries.push((href, name));
    }

    let title = format!("{show_dir} /");
    Ok(Html(directory_page(&title, &entries)))
}

pub async fn list_episodes_in_season(
    State(state): State<AppState>,
    Path((show_dir, season_dir)): Path<(String, String)>,
) -> Result<Html<String>, AppError> {
    let series_id = parse_seriesid(&show_dir)
        .ok_or_else(|| AppError::not_found("show folder must contain {seriesid-<id>}"))?;

    let season = parse_season_dir(&season_dir)
        .ok_or_else(|| AppError::not_found("season folder must be like Season 01"))?;

    let detail = state
        .cache
        .series_detail(&state.xtream, series_id, state.limits)
        .await
        .map_err(AppError::from)?;

    detail
        .info
        .as_ref()
        .ok_or_else(|| AppError::not_found("series has no info"))?;

    let eps = episodes_in_season(&detail, season);
    let mut entries = Vec::new();
    for ep in eps {
        let Some(name) = episode_filename(ep) else {
            continue;
        };
        let href = encode_path_segment(&name);
        entries.push((href, name));
    }

    let title = format!("{show_dir} / {season_dir} /");
    Ok(Html(directory_page(&title, &entries)))
}

async fn resolve_movie_url(
    state: &AppState,
    category_id: &str,
    movie_dir: &str,
    file: &str,
) -> Result<String, AppError> {
    let Some(loaded) = state.movie_catalog.get().await else {
        return Err(AppError::internal("catalog not ready yet"));
    };

    let archived = loaded.archived();
    let Some(cat) = archived
        .categories
        .iter()
        .find(|c| c.category_id == category_id)
    else {
        return Err(AppError::not_found("category not found"));
    };

    let Some(listing) = cat.streams.iter().find(|v| v.folder_name == movie_dir) else {
        return Err(AppError::not_found("movie folder not found"));
    };

    let expected = format!("{}.{}", listing.folder_name, listing.extension);
    if file != expected {
        return Err(AppError::not_found(
            "filename does not match expected video name",
        ));
    }

    Ok(state
        .xtream
        .movie_stream_url(listing.stream_id.into(), listing.extension.as_str()))
}

async fn resolve_episode_url(
    state: &AppState,
    show_dir: &str,
    season_dir: &str,
    file: &str,
) -> Result<String, AppError> {
    let (stem, ext) = split_video_ext(&file)
        .ok_or_else(|| AppError::bad_request("file must end with a known video extension"))?;

    let stream_id = parse_epid(stem)
        .ok_or_else(|| AppError::bad_request("filename must contain {epid-<id>}"))?;

    let series_id = parse_seriesid(&show_dir)
        .ok_or_else(|| AppError::not_found("show folder must contain {seriesid-<id>}"))?;

    let season = parse_season_dir(&season_dir)
        .ok_or_else(|| AppError::not_found("season folder must be like Season 01"))?;

    let detail = state
        .cache
        .series_detail(&state.xtream, series_id, state.limits)
        .await
        .map_err(AppError::from)?;

    detail
        .info
        .as_ref()
        .ok_or_else(|| AppError::not_found("series has no info"))?;

    let ep = find_episode_by_stream_id(&detail, stream_id)
        .ok_or_else(|| AppError::not_found("episode not in series"))?;

    if episode_season_number(ep) != season {
        return Err(AppError::not_found("episode not in this season folder"));
    }

    let expected =
        episode_filename(ep).ok_or_else(|| AppError::not_found("episode has no playable id"))?;
    if file != expected {
        return Err(AppError::not_found(
            "filename does not match expected episode name",
        ));
    }

    let ext_upstream = episode_extension(ep);
    if ext != ext_upstream {
        return Err(AppError::bad_request(
            "extension does not match stream metadata",
        ));
    }

    Ok(state.xtream.series_stream_url(stream_id, ext))
}

pub async fn proxy_video_get(
    State(state): State<AppState>,
    Path((category_id, movie_dir, file)): Path<(String, String, String)>,
) -> Result<Redirect, AppError> {
    let url = resolve_movie_url(&state, &category_id, &movie_dir, &file).await?;
    Ok(Redirect::temporary(&url))
}

pub async fn proxy_video_head(
    State(state): State<AppState>,
    Path((category_id, movie_dir, file)): Path<(String, String, String)>,
) -> Result<Response, AppError> {
    let url = resolve_movie_url(&state, &category_id, &movie_dir, &file).await?;
    handle_head_request(&state, url).await
}

pub async fn proxy_episode_get(
    State(state): State<AppState>,
    Path((show_dir, season_dir, file)): Path<(String, String, String)>,
) -> Result<Redirect, AppError> {
    let url = resolve_episode_url(&state, &show_dir, &season_dir, &file).await?;
    Ok(Redirect::temporary(&url))
}

pub async fn proxy_episode_head(
    State(state): State<AppState>,
    Path((show_dir, season_dir, file)): Path<(String, String, String)>,
) -> Result<Response, AppError> {
    let url = resolve_episode_url(&state, &show_dir, &season_dir, &file).await?;
    handle_head_request(&state, url).await
}

async fn handle_head_request(state: &AppState, upstream_url: String) -> Result<Response, AppError> {
    if state.stream_probe_use_upstream_head {
        return Ok(Redirect::temporary(upstream_url.as_str()).into_response());
    }

    if let Some(meta) = state.head_cache.get(&upstream_url).await {
        return Ok(head_response_from_meta(&meta));
    }

    let _permit = state
        .stream_inflight
        .acquire()
        .await
        .map_err(|_| AppError::internal("stream concurrency limiter closed"))?;

    let meta = resolve_stream_head_metadata(&state.http, &upstream_url)
        .await
        .map_err(AppError::bad_gateway)?;

    state
        .head_cache
        .insert(upstream_url.clone(), meta.clone())
        .await;

    Ok(head_response_from_meta(&meta))
}
