use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use rkyv::util::AlignedVec;

use super::TvCatalogLoaded;
use super::snapshot::TV_CATALOG_FORMAT_VERSION;

pub fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("rkyv.tmp");
    let mut f = File::create(&tmp)?;
    f.write_all(data)?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn load_from_path(path: &Path) -> std::io::Result<TvCatalogLoaded> {
    let mut f = File::open(path)?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    let mut aligned = AlignedVec::new();
    aligned.extend_from_slice(&buf);
    TvCatalogLoaded::from_aligned(aligned)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

pub fn snapshot_format_ok(loaded: &TvCatalogLoaded) -> bool {
    let root = loaded.archived();
    root.format_version == TV_CATALOG_FORMAT_VERSION
}
