use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use indicatif::{ProgressBar, ProgressStyle};

use crate::naming::show_base_name_listing;
use crate::state::AppState;
use crate::xtream::XtreamError;

use super::snapshot::{TvCatalogSeries, TvCatalogSnapshot};
use super::{catalog_format_ok, load_catalog_from_path, write_catalog_atomic};

pub async fn tv_catalog_worker_loop(state: AppState, catalog_path: PathBuf, refresh: Duration) {
    // If we already loaded a valid snapshot at startup, wait until built_at + refresh before the
    // first API rebuild so restarts do not always hammer Xtream.
    defer_if_snapshot_still_fresh(&state, refresh).await;

    // First `tick()` completes immediately; subsequent ticks wait `refresh`. Placing `tick`
    // before work avoids the bug where `work` + `tick` ran two rebuilds back-to-back on startup.
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

/// When memory already holds a catalog from disk, sleep until `built_at + refresh` if that time is
/// still in the future.
async fn defer_if_snapshot_still_fresh(state: &AppState, refresh: Duration) {
    let Some(loaded) = state.tv_catalog.get().await else {
        return;
    };

    let built_secs = loaded.built_at_unix_secs();
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    if built_secs > now_secs {
        tracing::warn!(
            built_secs,
            now_secs,
            "TV catalog built_at is in the future; not deferring first rebuild"
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
        "TV catalog rebuild deferred (snapshot still within refresh interval)"
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
    write_catalog_atomic(catalog_path, &snapshot).map_err(|e| e.to_string())?;
    let loaded = load_catalog_from_path(catalog_path).map_err(|e| e.to_string())?;
    if !catalog_format_ok(&loaded) {
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
        pb.set_message(format!("Fetching category: {}", cat.category_name));
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
