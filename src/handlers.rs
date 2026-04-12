use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::header::RANGE;
use axum::http::{HeaderMap, HeaderValue, Method, Request, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use futures_util::StreamExt;
use reqwest::header::HeaderMap as ReqwestHeaderMap;

use crate::error::AppError;
use crate::head_metadata::{
    head_response_from_meta, merge_upstream_headers, resolve_stream_head_metadata,
};
use crate::html::directory_page;
use crate::naming::{movie_base_name, parse_vodid, split_video_ext, video_extension, video_filename};
use crate::path_seg::encode_path_segment;
use crate::range_expand::upstream_range_header_value;
use crate::state::AppState;

pub async fn index(State(state): State<AppState>) -> Html<String> {
    let title = if state.limits.test_mode {
        "Mirage (test mode)"
    } else {
        "Mirage"
    };
    let entries = if state.limits.test_mode {
        vec![("movies/".into(), "Movies (limited catalog)".into())]
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

pub async fn tv_stub() -> Html<String> {
    Html(directory_page("TV Shows — not implemented yet", &[]))
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

    let (stem, ext) = split_video_ext(&file).ok_or_else(|| {
        AppError::bad_request("file must end with a known video extension")
    })?;

    let stream_id = parse_vodid(stem).ok_or_else(|| {
        AppError::bad_request("filename must contain {vodid-<id>}")
    })?;

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
        return Err(AppError::not_found("filename does not match expected video name"));
    }

    let ext_upstream = video_extension(listing);
    if ext != ext_upstream {
        return Err(AppError::bad_request("extension does not match stream metadata"));
    }

    let upstream_url = state.xtream.movie_stream_url(stream_id, ext);

    let mut headers = ReqwestHeaderMap::new();
    if method == Method::GET {
        if let Some(range) = req.headers().get(RANGE) {
            if let Ok(range_str) = range.to_str() {
                let known_len = state
                    .head_cache
                    .get(&upstream_url)
                    .await
                    .and_then(|h| h.content_length());
                if let Some(upstream_range) =
                    upstream_range_header_value(Some(range_str), known_len)
                {
                    if upstream_range != range_str {
                        tracing::debug!(
                            client_range = %range_str,
                            upstream_range = %upstream_range,
                            "expanded Range for upstream (ffprobe/FUSE tiny reads)"
                        );
                    }
                    if let Ok(hv) = HeaderValue::from_str(&upstream_range) {
                        headers.insert(reqwest::header::RANGE, hv);
                    }
                }
            }
        }
    }

    if method == Method::HEAD {
        if let Some(meta) = state.head_cache.get(&upstream_url).await {
            return Ok(head_response_from_meta(&meta));
        }
        let meta =
            resolve_stream_head_metadata(&state.http, &upstream_url, ext_upstream.as_str()).await;
        state
            .head_cache
            .insert(upstream_url.clone(), meta.clone())
            .await;
        return Ok(head_response_from_meta(&meta));
    }

    let resp = state
        .http
        .get(&upstream_url)
        .headers(headers)
        .send()
        .await
        .map_err(|e| AppError::internal(e.to_string()))?;

    Ok(upstream_to_axum_response(resp).await)
}

async fn upstream_to_axum_response(resp: reqwest::Response) -> Response {
    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut out = HeaderMap::new();

    let hdr = resp.headers();
    merge_upstream_headers(&mut out, hdr);

    if !resp.status().is_success() {
        let msg = resp.text().await.unwrap_or_default();
        return Response::builder()
            .status(status)
            .body(Body::from(msg))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
    }

    let stream = resp
        .bytes_stream()
        .map(|res| res.map_err(std::io::Error::other));
    let body = Body::from_stream(stream);
    (status, out, body).into_response()
}
