mod cache;
mod config;
mod error;
mod handlers;
mod head_metadata;
mod html;
mod limits;
mod naming;
mod path_seg;
mod range_expand;
mod state;
mod xtream;

use std::net::SocketAddr;

use axum::Router;
use axum::routing::get;
use tokio::net::TcpListener;
use tokio::signal;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::cache::AppCache;
use crate::config::Config;
use crate::head_metadata::HeadMetadataCache;
use crate::state::AppState;
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

    let xtream = XtreamClient::new(
        http.clone(),
        config.xtream_base_url.clone(),
        config.xtream_username.clone(),
        config.xtream_password.clone(),
    );

    if config.limits.test_mode {
        tracing::warn!(
            max_categories = config.limits.max_categories,
            max_vod_per_category = config.limits.max_vod_per_category,
            "MIRAGE_TEST_MODE is on: truncated catalog after each API response (payload is still downloaded once per request)"
        );
    }

    let state = AppState {
        xtream,
        http,
        cache: AppCache::new(),
        limits: config.limits,
        head_cache: HeadMetadataCache::new(),
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
        .route("/tv/", get(handlers::tv_stub))
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
