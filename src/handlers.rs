use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{Method, Request, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};

use crate::error::AppError;
use crate::head_metadata::{head_response_from_meta, resolve_stream_head_metadata};
use crate::html::directory_page;
use crate::naming::{
    episode_extension, episode_filename, episode_season_number, episodes_in_season,
    find_episode_by_stream_id, movie_base_name, parse_epid, parse_season_dir, parse_seriesid,
    parse_vodid, season_dir_name, season_numbers_for_series, split_video_ext, video_extension,
    video_filename,
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

pub async fn list_vod_categories(State(state): State<AppState>) -> Result<Html<String>, AppError> {
    let cats = state
        .cache
        .vod_categories(&state.xtream, state.limits)
        .await
        .map_err(AppError::from)?;

    let mut entries = Vec::new();
    for c in cats.iter() {
        let href = format!("{}/", encode_path_segment(&c.category_id));
        let label = if c.category_name.trim().is_empty() {
            c.category_id.clone()
        } else {
            c.category_name.clone()
        };
        entries.push((href, label));
    }

    Ok(Html(directory_page("Movies — categories", &entries)))
}

pub async fn list_movies_in_category(
    State(state): State<AppState>,
    Path(category_id): Path<String>,
) -> Result<Html<String>, AppError> {
    let streams = state
        .cache
        .vod_streams_for_category(&state.xtream, &category_id, state.limits)
        .await
        .map_err(AppError::from)?;

    let mut entries = Vec::new();
    for v in streams.iter() {
        let base = movie_base_name(v);
        let href = format!("{}/", encode_path_segment(&base));
        entries.push((href, crate::naming::display_title(v)));
    }

    let title = format!("Movies — category {category_id}");
    Ok(Html(directory_page(&title, &entries)))
}

pub async fn list_movie_folder(
    State(state): State<AppState>,
    Path((category_id, movie_dir)): Path<(String, String)>,
) -> Result<Html<String>, AppError> {
    let streams = state
        .cache
        .vod_streams_for_category(&state.xtream, &category_id, state.limits)
        .await
        .map_err(AppError::from)?;

    let listing = streams
        .iter()
        .find(|v| movie_base_name(v) == movie_dir)
        .ok_or_else(|| AppError::not_found("unknown movie folder"))?;

    let vf = video_filename(listing);
    let href = encode_path_segment(&vf);
    let entries = vec![(href, vf.clone())];
    let title = format!("{movie_dir} /");
    Ok(Html(directory_page(&title, &entries)))
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

pub async fn proxy_video(
    State(state): State<AppState>,
    Path((category_id, movie_dir, file)): Path<(String, String, String)>,
    req: Request<Body>,
) -> Result<Response, AppError> {
    let method = req.method().clone();
    if method != Method::GET && method != Method::HEAD {
        return Err(AppError::bad_request("only GET and HEAD supported"));
    }

    let (stem, ext) = split_video_ext(&file)
        .ok_or_else(|| AppError::bad_request("file must end with a known video extension"))?;

    let stream_id = parse_vodid(stem)
        .ok_or_else(|| AppError::bad_request("filename must contain {vodid-<id>}"))?;

    let streams = state
        .cache
        .vod_streams_for_category(&state.xtream, &category_id, state.limits)
        .await
        .map_err(AppError::from)?;

    let listing = streams
        .iter()
        .find(|v| v.stream_id == stream_id)
        .ok_or_else(|| AppError::not_found("stream not in category"))?;

    if movie_base_name(listing) != movie_dir {
        return Err(AppError::not_found("movie folder does not match stream"));
    }

    let expected = video_filename(listing);
    if file != expected {
        return Err(AppError::not_found(
            "filename does not match expected video name",
        ));
    }

    let ext_upstream = video_extension(listing);
    if ext != ext_upstream {
        return Err(AppError::bad_request(
            "extension does not match stream metadata",
        ));
    }

    let upstream_url = state.xtream.movie_stream_url(stream_id, ext);
    proxy_upstream_stream(&state, method, upstream_url).await
}

pub async fn proxy_episode(
    State(state): State<AppState>,
    Path((show_dir, season_dir, file)): Path<(String, String, String)>,
    req: Request<Body>,
) -> Result<Response, AppError> {
    let method = req.method().clone();
    if method != Method::GET && method != Method::HEAD {
        return Err(AppError::bad_request("only GET and HEAD supported"));
    }

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

    let upstream_url = state.xtream.series_stream_url(stream_id, ext);

    proxy_upstream_stream(&state, method, upstream_url).await
}

async fn proxy_upstream_stream(
    state: &AppState,
    method: Method,
    upstream_url: String,
) -> Result<Response, AppError> {
    match method {
        Method::HEAD if state.stream_probe_use_upstream_head => {
            Ok(Redirect::temporary(upstream_url.as_str()).into_response())
        }
        Method::HEAD => {
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
        Method::GET => Ok(Redirect::temporary(upstream_url.as_str()).into_response()),
        _ => Err(AppError::bad_request(
            "only GET and HEAD are supported for this resource",
        )),
    }
}
