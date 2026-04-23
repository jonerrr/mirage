mod cache;
mod catalog;
mod config;
mod error;
mod handlers;
mod head_metadata;
mod html;
mod naming;
mod pace;
mod path_seg;
mod state;
mod xtream;

use std::net::SocketAddr;
use std::time::Instant;

use axum::Router;
use axum::http::Request;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::get;
use tokio::net::TcpListener;
use tokio::signal;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::cache::AppCache;
use crate::catalog::{
    MovieCatalogHandle, TvCatalogHandle, movie_catalog_worker_loop, tv_catalog_worker_loop,
};
use crate::config::Config;
use crate::head_metadata::HeadMetadataCache;
use crate::pace::UpstreamPacer;
use crate::state::AppState;
use crate::xtream::XtreamClient;

fn init_tracing() {
    let default_filter = format!("{}=info", env!("CARGO_CRATE_NAME"));
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer().without_time())
        .init();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let config = Config::from_env()?;
    let http = reqwest::Client::builder()
        .user_agent("mirage/0.1")
        .build()?;

    let pacer = UpstreamPacer::new(
        config.upstream_pace.min_interval,
        config.upstream_pace.max_inflight,
    );

    let xtream = XtreamClient::new(
        http.clone(),
        pacer.clone(),
        config.xtream_base_url.clone(),
        config.xtream_username.clone(),
        config.xtream_password.clone(),
    );

    if config.limits.test_mode {
        tracing::warn!(
            max_categories = config.limits.max_categories,
            max_vod_per_category = config.limits.max_vod_per_category,
            max_series_per_category = config.limits.max_series_per_category,
            max_episodes_per_series = config.limits.max_episodes_per_series,
            "MIRAGE_TEST_MODE is on: truncated catalog after each API response (payload is still downloaded once per request)"
        );
    }

    let tv_catalog = TvCatalogHandle::new();
    if let Ok(loaded) = crate::catalog::load_tv_catalog_from_path(&config.tv_catalog.catalog_path) {
        if crate::catalog::tv_catalog_format_ok(&loaded) {
            tv_catalog.set(Some(loaded)).await;
            tracing::info!(
                path = %config.tv_catalog.catalog_path.display(),
                "loaded TV catalog snapshot from disk"
            );
        } else {
            tracing::warn!(
                path = %config.tv_catalog.catalog_path.display(),
                "TV catalog file has wrong format_version; ignoring"
            );
        }
    }

    let movie_catalog = MovieCatalogHandle::new();
    if let Ok(loaded) =
        crate::catalog::load_movie_catalog_from_path(&config.movie_catalog.catalog_path)
    {
        if crate::catalog::movie_catalog_format_ok(&loaded) {
            movie_catalog.set(Some(loaded)).await;
            tracing::info!(
                path = %config.movie_catalog.catalog_path.display(),
                "loaded Movie catalog snapshot from disk"
            );
        } else {
            tracing::warn!(
                path = %config.movie_catalog.catalog_path.display(),
                "Movie catalog file has wrong format_version; ignoring"
            );
        }
    }

    let cache = AppCache::new();
    let head_cache = HeadMetadataCache::new();

    let worker_state = AppState {
        xtream: xtream.clone(),
        pacer: pacer.clone(),
        http: http.clone(),
        cache: cache.clone(),
        limits: config.limits,
        head_cache: head_cache.clone(),
        tv_catalog: tv_catalog.clone(),
        movie_catalog: movie_catalog.clone(),
        stream_probe_use_upstream_head: config.stream.probe_use_upstream_head,
    };
    let catalog_path = config.tv_catalog.catalog_path.clone();
    let refresh = config.tv_catalog.refresh;
    let movie_path = config.movie_catalog.catalog_path.clone();
    let movie_refresh = config.movie_catalog.refresh;

    let worker_state_tv = worker_state.clone();
    tokio::spawn(async move {
        tv_catalog_worker_loop(worker_state_tv, catalog_path, refresh).await;
    });

    let worker_state_movie = worker_state.clone();
    tokio::spawn(async move {
        movie_catalog_worker_loop(worker_state_movie, movie_path, movie_refresh).await;
    });

    let state = AppState {
        xtream,
        pacer,
        http,
        cache,
        limits: config.limits,
        head_cache,
        tv_catalog,
        movie_catalog,
        stream_probe_use_upstream_head: config.stream.probe_use_upstream_head,
    };

    let app = Router::new()
        .route("/", get(handlers::index))
        .route("/movies", get(handlers::redirect_movies))
        .route("/movies/", get(handlers::list_vod_categories))
        .route(
            "/movies/{category_id}/",
            get(handlers::list_movies_in_category),
        )
        .route(
            "/movies/{category_id}/{movie_dir}/",
            get(handlers::list_movie_folder),
        )
        .route(
            "/movies/{category_id}/{movie_dir}/{file}",
            get(handlers::proxy_video_get).head(handlers::proxy_video_head),
        )
        .route("/tv", get(handlers::redirect_tv))
        .route("/tv/", get(handlers::list_all_tv_series))
        .route("/tv/{show_dir}/", get(handlers::list_seasons))
        .route(
            "/tv/{show_dir}/{season_dir}/",
            get(handlers::list_episodes_in_season),
        )
        .route(
            "/tv/{show_dir}/{season_dir}/{file}",
            get(handlers::proxy_episode_get).head(handlers::proxy_episode_head),
        )
        .with_state(state)
        .layer(middleware::from_fn(log_request));

    let addr: SocketAddr = config.listen.parse()?;
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(%addr, "listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("shutdown complete");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("shutdown signal received");
}

async fn log_request(req: Request<axum::body::Body>, next: Next) -> Response {
    let started_at = Instant::now();
    let method = req.method().clone();
    let uri = req.uri().clone();

    let response = next.run(req).await;
    let status = response.status();
    tracing::info!(
        %method,
        uri = %uri.path(),
        status = status.as_u16(),
        latency_ms = started_at.elapsed().as_millis(),
        "request completed"
    );

    response
}
