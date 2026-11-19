//! Embedded artwork extraction with a content-hash cache.
//!
//! Cache key = sha256(path + mtime). A rescan of unchanged files therefore
//! never rewrites art files, and stale entries for deleted tracks are garbage
//! collected by `prune_orphans` after a full scan.

use lofty::prelude::*;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Extracts the primary picture to `art_dir`. Returns the cached file path,
/// `Ok(None)` when the file carries no picture, and an error only on I/O
/// trouble — a broken image must not fail the scan.
pub fn extract(
    path: &Path,
    mtime_secs: i64,
    art_dir: &Path,
) -> Result<Option<PathBuf>, std::io::Error> {
    std::fs::create_dir_all(art_dir)?;
    let mut hasher = Sha256::new();
    hasher.update(path.to_string_lossy().as_bytes());
    hasher.update(mtime_secs.to_le_bytes());
    let hash = format!("{:x}", hasher.finalize());

    let cached = art_dir.join(&hash);
    if cached.exists() {
        return Ok(Some(cached));
    }

    let tagged = match lofty::read_from_path(path) {
        Ok(t) => t,
        // Untaggable or corrupt file: no art, not an error.
        Err(_) => return Ok(None),
    };
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
    let Some(pic) = tag.and_then(|t| t.pictures().first()) else {
        return Ok(None);
    };

    let ext = match pic.mime_type() {
        Some(lofty::picture::MimeType::Png) => "png",
        Some(lofty::picture::MimeType::Jpeg) => "jpg",
        Some(lofty::picture::MimeType::Bmp) => "bmp",
        Some(lofty::picture::MimeType::Gif) => "gif",
        _ => "img",
    };
    let out = art_dir.join(format!("{hash}.{ext}"));
    let tmp = art_dir.join(format!("{hash}.tmp"));
    std::fs::write(&tmp, pic.data())?;
    std::fs::rename(&tmp, &out)?;
    Ok(Some(out))
}

/// Deletes cache entries not in `keep`. Cheap safety net after rescans.
pub fn prune_orphans(art_dir: &Path, keep: &[PathBuf]) -> std::io::Result<()> {
    let keep: std::collections::HashSet<_> = keep.iter().cloned().collect();
    for entry in std::fs::read_dir(art_dir)?.flatten() {
        let p = entry.path();
        if p.is_file() && !keep.contains(&p) {
            let _ = std::fs::remove_file(&p);
        }
    }
    Ok(())
}
