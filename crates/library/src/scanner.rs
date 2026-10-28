//! Folder scanner: walk → read metadata → upsert → mark missing → extract art.
//!
//! Bounded concurrency via rayon's default pool; progress reported through a
//! callback so app-core can forward it as UI events without the scanner
//! knowing anything about Tauri.

use crate::{art, metadata};
use persistence::repo;
use persistence::{Db, Result as DbResult};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ScanProgress {
    pub scanned: usize,
    pub total: usize,
    pub folder: String,
}

#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct ScanSummary {
    pub added: usize,
    pub updated: usize,
    pub marked_missing: usize,
    pub errors: usize,
}

pub struct Scanner {
    pub art_dir: PathBuf,
}

impl Scanner {
    pub fn new(art_dir: PathBuf) -> Self {
        Self { art_dir }
    }

    /// Scans `folders`, upserting into `db`. Idempotent: re-running a scan of
    /// unchanged folders updates nothing but timestamps.
    pub fn scan(
        &self,
        db: &Db,
        folders: &[(i64, String)],
        on_progress: impl Fn(ScanProgress) + Send + Sync,
    ) -> DbResult<ScanSummary> {
        let mut summary = ScanSummary::default();

        // Collect files first so the progress denominator is real.
        let mut jobs: Vec<(i64, PathBuf)> = Vec::new();
        for (folder_id, path) in folders {
            for entry in walkdir::WalkDir::new(path)
                .follow_links(false)
                .into_iter()
                .filter_map(std::result::Result::ok)
                .filter(|e| e.file_type().is_file() && metadata::is_supported(e.path()))
            {
                jobs.push((*folder_id, entry.path().to_path_buf()));
            }
        }
        let total = jobs.len();
        let done = Arc::new(AtomicUsize::new(0));
        let errors = Arc::new(AtomicUsize::new(0));
        let added = Arc::new(AtomicUsize::new(0));

        // New tracks are recorded so the summary can count them without a
        // second query.
        jobs.par_iter().for_each(|(folder_id, path)| {
            match self.record(db, *folder_id, path) {
                Ok(true) => {
                    added.fetch_add(1, Ordering::Relaxed);
                }
                Ok(false) => {}
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "scan record failed");
                    errors.fetch_add(1, Ordering::Relaxed);
                }
            }
            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
            if n % 25 == 0 || n == total {
                let folder = path.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
                on_progress(ScanProgress { scanned: n, total, folder });
            }
        });

        summary.added = added.load(Ordering::Relaxed);
        summary.errors = errors.load(Ordering::Relaxed);

        // Missing-file detection: every previously known track under a scanned
        // folder that was not seen in this walk is marked missing (kept, not
        // deleted — a temporarily unmounted drive must not wipe the library).
        for (folder_id, _path) in folders {
            let known = self.known_paths(db, *folder_id)?;
            let seen: std::collections::HashSet<String> = jobs
                .iter()
                .filter(|(fid, _)| fid == folder_id)
                .map(|(_, p)| p.to_string_lossy().into_owned())
                .collect();
            let gone: Vec<i64> = known
                .into_iter()
                .filter(|(_, p, exists)| {
                    !seen.contains(p) && !Path::new(p).exists() && *exists
                })
                .map(|(id, _, _)| id)
                .collect();
            summary.marked_missing += repo::mark_missing(db, &gone, true)?;
            // Files that came back are un-marked by upsert (missing = 0).
        }

        Ok(summary)
    }

    /// Reads and upserts one file. Returns `Ok(true)` when the track is new.
    fn record(&self, db: &Db, folder_id: i64, path: &Path) -> std::result::Result<bool, String> {
        let existed = repo::get_track_by_path(db, &path.to_string_lossy())
            .map_err(|e| e.to_string())?
            .is_some();
        let mtime = file_mtime(path);
        let size = std::fs::metadata(path).map(|m| m.len() as i64).unwrap_or(0);

        match metadata::read(path, folder_id) {
            Ok(mut track) => {
                let id = repo::upsert_track(db, &track, folder_id, mtime, size, now_secs())
                    .map_err(|e| e.to_string())?;
                track.id = id;
                // Artwork only when absent or file changed (hash includes mtime,
                // so an unchanged file hits the cache and costs nothing).
                if let Ok(Some(art_path)) = art::extract(path, mtime, &self.art_dir) {
                    repo::set_art_path(db, id, art_path.to_str()).map_err(|e| e.to_string())?;
                } else {
                    repo::set_art_path(db, id, None).map_err(|e| e.to_string())?;
                }
                Ok(!existed)
            }
            Err(e) => Err(e.to_string()),
        }
    }

    fn known_paths(&self, db: &Db, folder_id: i64) -> DbResult<Vec<(i64, String, bool)>> {
        Ok(repo::track_paths_in_folder(db, folder_id)?
            .into_iter()
            .map(|(id, p)| {
                let exists = Path::new(&p).exists();
                (id, p, exists)
            })
            .collect())
    }
}

fn file_mtime(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
