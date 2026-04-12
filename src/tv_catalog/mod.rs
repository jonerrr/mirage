mod snapshot;
mod store;
mod worker;

pub use worker::tv_catalog_worker_loop;

use rkyv::rancor::Error;
use rkyv::util::AlignedVec;

use snapshot::{ArchivedTvCatalogSnapshot, TvCatalogSnapshot};

/// In-memory catalog validated once at load; bytes are never mutated afterward.
pub struct TvCatalogLoaded {
    bytes: AlignedVec,
}

impl TvCatalogLoaded {
    pub fn from_aligned(bytes: AlignedVec) -> Result<Self, Error> {
        rkyv::access::<<TvCatalogSnapshot as rkyv::Archive>::Archived, Error>(bytes.as_slice())?;
        Ok(Self { bytes })
    }

    /// # Safety contract
    /// Validated in [`Self::from_aligned`]; `bytes` are not mutated afterward.
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

pub fn write_catalog_atomic(
    path: &std::path::Path,
    snapshot: &TvCatalogSnapshot,
) -> std::io::Result<()> {
    let bytes = snapshot
        .to_bytes_rkyv()
        .map_err(|e: Error| std::io::Error::other(e.to_string()))?;
    store::write_atomic(path, bytes.as_slice())
}

pub fn load_catalog_from_path(path: &std::path::Path) -> std::io::Result<TvCatalogLoaded> {
    store::load_from_path(path)
}

pub fn catalog_format_ok(loaded: &TvCatalogLoaded) -> bool {
    store::snapshot_format_ok(loaded)
}
