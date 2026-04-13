mod cache;
mod config;
mod error;
mod handlers;
mod head_metadata;
mod html;
mod naming;
mod pace;
mod path_seg;
mod range_expand;
mod state;
mod tv_catalog;
mod xtream;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::routing::get;
use tokio::net::TcpListener;
use tokio::signal;
use tokio::sync::Semaphore;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::cache::AppCache;
use crate::config::Config;
use crate::head_metadata::HeadMetadataCache;
use crate::pace::UpstreamPacer;
use crate::state::AppState;
use crate::tv_catalog::{TvCatalogHandle, tv_catalog_worker_loop};
use crate::xtream::XtreamClient;

fn init_tracing() {
    let default_filter = format!(
        "{}=debug,tower_http=debug,axum=trace",
        env!("CARGO_CRATE_NAME")
    );
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
        pacer,
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
    if let Ok(loaded) = crate::tv_catalog::load_catalog_from_path(&config.tv_catalog.catalog_path) {
        if crate::tv_catalog::catalog_format_ok(&loaded) {
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

    let cache = AppCache::new();
    let head_cache = HeadMetadataCache::new();
    let stream_inflight = Arc::new(Semaphore::new(config.stream.max_inflight as usize));

    let worker_state = AppState {
        xtream: xtream.clone(),
        http: http.clone(),
        cache: cache.clone(),
        limits: config.limits,
        head_cache: head_cache.clone(),
        tv_catalog: tv_catalog.clone(),
        stream_probe_use_upstream_head: config.stream.probe_use_upstream_head,
        stream_inflight: stream_inflight.clone(),
    };
    let catalog_path = config.tv_catalog.catalog_path.clone();
    let refresh = config.tv_catalog.refresh;
    tokio::spawn(async move {
        tv_catalog_worker_loop(worker_state, catalog_path, refresh).await;
    });

    let state = AppState {
        xtream,
        http,
        cache,
        limits: config.limits,
        head_cache,
        tv_catalog,
        stream_probe_use_upstream_head: config.stream.probe_use_upstream_head,
        stream_inflight,
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
            get(handlers::proxy_video).head(handlers::proxy_video),
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
            get(handlers::proxy_episode).head(handlers::proxy_episode),
        )
        .with_state(state)
        .layer(TraceLayer::new_for_http());

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
