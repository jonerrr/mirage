use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use indicatif::{ProgressBar, ProgressStyle};

use crate::naming::{display_title, movie_base_name, show_base_name_listing, video_extension};
use crate::state::AppState;
use crate::xtream::XtreamError;

use super::snapshot::{
    MovieCatalogCategory, MovieCatalogSnapshot, MovieCatalogStream, TvCatalogSeries,
    TvCatalogSnapshot,
};
use super::{
    load_movie_catalog_from_path, load_tv_catalog_from_path, movie_catalog_format_ok,
    tv_catalog_format_ok, write_movie_catalog_atomic, write_tv_catalog_atomic,
};

pub async fn tv_catalog_worker_loop(state: AppState, catalog_path: PathBuf, refresh: Duration) {
    let built_secs = state.tv_catalog.get().await.map(|l| l.built_at_unix_secs());
    defer_if_snapshot_still_fresh("TV", built_secs, refresh).await;

    let mut interval = tokio::time::interval(refresh);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        interval.tick().await;
        match run_tv_catalog_rebuild(&state, &catalog_path).await {
            Ok(()) => tracing::info!("TV catalog snapshot rebuilt"),
            Err(e) => tracing::error!(error = %e, "TV catalog rebuild failed"),
        }
    }
}

pub async fn movie_catalog_worker_loop(state: AppState, catalog_path: PathBuf, refresh: Duration) {
    let built_secs = state
        .movie_catalog
        .get()
        .await
        .map(|l| l.built_at_unix_secs());
    defer_if_snapshot_still_fresh("Movie", built_secs, refresh).await;

    let mut interval = tokio::time::interval(refresh);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        interval.tick().await;
        match run_movie_catalog_rebuild(&state, &catalog_path).await {
            Ok(()) => tracing::info!("Movie catalog snapshot rebuilt"),
            Err(e) => tracing::error!(error = %e, "Movie catalog rebuild failed"),
        }
    }
}

async fn defer_if_snapshot_still_fresh(name: &str, built_secs: Option<u64>, refresh: Duration) {
    let Some(built_secs) = built_secs else {
        return;
    };

    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    if built_secs > now_secs {
        tracing::warn!(
            built_secs,
            now_secs,
            "{} catalog built_at is in the future; not deferring first rebuild",
            name
        );
        return;
    }

    let built_time = UNIX_EPOCH + Duration::from_secs(built_secs);
    let stale_time = built_time + refresh;
    let now = SystemTime::now();
    let Ok(wait) = stale_time.duration_since(now) else {
        return;
    };
    if wait.is_zero() {
        return;
    }

    tracing::info!(
        wait_secs = wait.as_secs(),
        "{} catalog rebuild deferred (snapshot still within refresh interval)",
        name
    );
    tokio::time::sleep(wait).await;
}

pub async fn run_tv_catalog_rebuild(
    state: &AppState,
    catalog_path: &std::path::Path,
) -> Result<(), String> {
    let snapshot = build_tv_catalog_snapshot(state)
        .await
        .map_err(|e| e.to_string())?;
    write_tv_catalog_atomic(catalog_path, &snapshot).map_err(|e| e.to_string())?;
    let loaded = load_tv_catalog_from_path(catalog_path).map_err(|e| e.to_string())?;
    if !tv_catalog_format_ok(&loaded) {
        return Err("reloaded TV catalog failed format check".into());
    }
    state.tv_catalog.set(Some(loaded)).await;
    Ok(())
}

async fn build_tv_catalog_snapshot(state: &AppState) -> Result<TvCatalogSnapshot, XtreamError> {
    let client = &state.xtream;
    let limits = state.limits;

    let mut categories = client.get_series_categories().await?;
    if limits.test_mode {
        categories.truncate(limits.max_categories);
    }

    let pb = ProgressBar::new(categories.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} categories - {msg} ({eta})")
            .unwrap()
            .progress_chars("#>-")
    );

    let mut seen = HashSet::new();
    let mut merged = Vec::new();
    for cat in categories {
        pb.set_message(format!("Fetching TV category: {}", cat.category_name));
        let rows = client.get_series(&cat.category_id).await?;
        for s in rows {
            if seen.insert(s.series_id) {
                merged.push(s);
            }
        }
        pb.inc(1);
    }
    pb.finish_with_message("TV catalog categories fetched");

    if limits.test_mode {
        merged.truncate(limits.max_series_per_category);
    }

    let mut rows: Vec<TvCatalogSeries> = merged
        .into_iter()
        .map(|s| TvCatalogSeries {
            series_id: s.series_id,
            folder_name: show_base_name_listing(&s),
            list_label: s.name.trim().to_string(),
        })
        .collect();
    rows.sort_by(|a, b| a.folder_name.cmp(&b.folder_name));

    Ok(TvCatalogSnapshot::new(rows))
}

pub async fn run_movie_catalog_rebuild(
    state: &AppState,
    catalog_path: &std::path::Path,
) -> Result<(), String> {
    let snapshot = build_movie_catalog_snapshot(state)
        .await
        .map_err(|e| e.to_string())?;
    write_movie_catalog_atomic(catalog_path, &snapshot).map_err(|e| e.to_string())?;
    let loaded = load_movie_catalog_from_path(catalog_path).map_err(|e| e.to_string())?;
    if !movie_catalog_format_ok(&loaded) {
        return Err("reloaded Movie catalog failed format check".into());
    }
    state.movie_catalog.set(Some(loaded)).await;
    Ok(())
}

async fn build_movie_catalog_snapshot(
    state: &AppState,
) -> Result<MovieCatalogSnapshot, XtreamError> {
    let client = &state.xtream;
    let limits = state.limits;

    let mut categories = client.get_vod_categories().await?;
    if limits.test_mode {
        categories.truncate(limits.max_categories);
    }

    let pb = ProgressBar::new(categories.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} categories - {msg} ({eta})")
            .unwrap()
            .progress_chars("#>-")
    );

    let mut out_categories = Vec::with_capacity(categories.len());

    for cat in categories {
        pb.set_message(format!("Fetching Movie category: {}", cat.category_name));
        let mut rows = client.get_vod_streams(&cat.category_id).await?;
        pb.inc(1);

        if limits.test_mode {
            rows.truncate(limits.max_vod_per_category);
        }

        let mut streams: Vec<MovieCatalogStream> = rows
            .into_iter()
            .map(|v| MovieCatalogStream {
                stream_id: v.stream_id,
                folder_name: movie_base_name(&v),
                list_label: display_title(&v).to_string(),
                extension: video_extension(&v).to_string(),
            })
            .collect();
        streams.sort_by(|a, b| a.folder_name.cmp(&b.folder_name));

        out_categories.push(MovieCatalogCategory {
            category_id: cat.category_id,
            category_name: cat.category_name,
            streams,
        });
    }
    pb.finish_with_message("Movie catalog categories fetched");

    Ok(MovieCatalogSnapshot::new(out_categories))
}
