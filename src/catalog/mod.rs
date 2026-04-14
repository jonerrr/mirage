mod snapshot;
mod store;
mod worker;

pub use worker::{movie_catalog_worker_loop, tv_catalog_worker_loop};

use rkyv::rancor::Error;
use rkyv::util::AlignedVec;

use snapshot::{
    ArchivedMovieCatalogSnapshot, ArchivedTvCatalogSnapshot, MovieCatalogSnapshot,
    TvCatalogSnapshot,
};

// ------------------------------------------------------------------
// TV
// ------------------------------------------------------------------

pub struct TvCatalogLoaded {
    bytes: AlignedVec,
}

impl TvCatalogLoaded {
    pub fn from_aligned(bytes: AlignedVec) -> Result<Self, Error> {
        rkyv::access::<<TvCatalogSnapshot as rkyv::Archive>::Archived, Error>(bytes.as_slice())?;
        Ok(Self { bytes })
    }

    pub fn archived(&self) -> &ArchivedTvCatalogSnapshot {
        unsafe {
            rkyv::access_unchecked::<<TvCatalogSnapshot as rkyv::Archive>::Archived>(
                self.bytes.as_slice(),
            )
        }
    }

    pub fn built_at_unix_secs(&self) -> u64 {
        self.archived().built_at_unix_secs.into()
    }
}

#[derive(Clone, Default)]
pub struct TvCatalogHandle {
    inner: std::sync::Arc<tokio::sync::RwLock<Option<std::sync::Arc<TvCatalogLoaded>>>>,
}

impl TvCatalogHandle {
    pub fn new() -> Self {
        Self {
            inner: std::sync::Arc::new(tokio::sync::RwLock::new(None)),
        }
    }

    pub async fn set(&self, loaded: Option<TvCatalogLoaded>) {
        let mut g = self.inner.write().await;
        *g = loaded.map(std::sync::Arc::new);
    }

    pub async fn get(&self) -> Option<std::sync::Arc<TvCatalogLoaded>> {
        self.inner.read().await.clone()
    }
}

// ------------------------------------------------------------------
// Movie
// ------------------------------------------------------------------

pub struct MovieCatalogLoaded {
    bytes: AlignedVec,
}

impl MovieCatalogLoaded {
    pub fn from_aligned(bytes: AlignedVec) -> Result<Self, Error> {
        rkyv::access::<<MovieCatalogSnapshot as rkyv::Archive>::Archived, Error>(bytes.as_slice())?;
        Ok(Self { bytes })
    }

    pub fn archived(&self) -> &ArchivedMovieCatalogSnapshot {
        unsafe {
            rkyv::access_unchecked::<<MovieCatalogSnapshot as rkyv::Archive>::Archived>(
                self.bytes.as_slice(),
            )
        }
    }

    pub fn built_at_unix_secs(&self) -> u64 {
        self.archived().built_at_unix_secs.into()
    }
}

#[derive(Clone, Default)]
pub struct MovieCatalogHandle {
    inner: std::sync::Arc<tokio::sync::RwLock<Option<std::sync::Arc<MovieCatalogLoaded>>>>,
}

impl MovieCatalogHandle {
    pub fn new() -> Self {
        Self {
            inner: std::sync::Arc::new(tokio::sync::RwLock::new(None)),
        }
    }

    pub async fn set(&self, loaded: Option<MovieCatalogLoaded>) {
        let mut g = self.inner.write().await;
        *g = loaded.map(std::sync::Arc::new);
    }

    pub async fn get(&self) -> Option<std::sync::Arc<MovieCatalogLoaded>> {
        self.inner.read().await.clone()
    }
}

// ------------------------------------------------------------------
// Writer / Loaders
// ------------------------------------------------------------------

pub fn write_tv_catalog_atomic(
    path: &std::path::Path,
    snapshot: &TvCatalogSnapshot,
) -> std::io::Result<()> {
    let bytes = snapshot
        .to_bytes_rkyv()
        .map_err(|e: Error| std::io::Error::other(e.to_string()))?;
    store::write_atomic(path, bytes.as_slice())
}

pub fn load_tv_catalog_from_path(path: &std::path::Path) -> std::io::Result<TvCatalogLoaded> {
    store::load_tv_from_path(path)
}

pub fn tv_catalog_format_ok(loaded: &TvCatalogLoaded) -> bool {
    store::tv_snapshot_format_ok(loaded)
}

pub fn write_movie_catalog_atomic(
    path: &std::path::Path,
    snapshot: &MovieCatalogSnapshot,
) -> std::io::Result<()> {
    let bytes = snapshot
        .to_bytes_rkyv()
        .map_err(|e: Error| std::io::Error::other(e.to_string()))?;
    store::write_atomic(path, bytes.as_slice())
}

pub fn load_movie_catalog_from_path(path: &std::path::Path) -> std::io::Result<MovieCatalogLoaded> {
    store::load_movie_from_path(path)
}

pub fn movie_catalog_format_ok(loaded: &MovieCatalogLoaded) -> bool {
    store::movie_snapshot_format_ok(loaded)
}
