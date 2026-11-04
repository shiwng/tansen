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
    /// Folder id — the path stays inside the backend (TAN-11).
    pub folder_id: i64,
}

#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct ScanSummary {
    pub added: usize,
    /// M12: files whose stored mtime+size matched and were skipped without
    /// parsing (the field was never written before).
    pub unchanged: usize,
    pub marked_missing: usize,
    pub errors: usize,
}

#[derive(Clone)]
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

        // M12, phase 1 (parallel parse): stat-compare against the stored
        // mtime+size and read tags only for changed or missing rows. Nothing
        // touches the database here.
        let writes: Vec<(repo::TrackWrite, bool)> = jobs
            .par_iter()
            .filter_map(|(folder_id, path)| {
                let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                if n % 25 == 0 || n == total {
                    on_progress(ScanProgress { scanned: n, total, folder_id: *folder_id });
                }
                match self.parse_job(db, *folder_id, path) {
                    Ok(Some(pair)) => Some(pair),
                    Ok(None) => None,
                    Err(e) => {
                        tracing::warn!(path = %path.display(), error = %e, "scan parse failed");
                        errors.fetch_add(1, Ordering::Relaxed);
                        None
                    }
                }
            })
            .collect();
        summary.unchanged = total - writes.len();

        // M12, phase 2 (batched write): every insert of a chunk commits
        // inside one transaction instead of two autocommits per file.
        let mut added = 0usize;
        for chunk in writes.chunks(200) {
            let items: Vec<repo::TrackWrite> =
                chunk.iter().map(|(w, _)| repo::TrackWrite { track: w.track.clone(), folder_id: w.folder_id, mtime: w.mtime, size: w.size, art_path: w.art_path.clone() }).collect();
            match repo::upsert_batch(db, &items, now_secs()) {
                Ok(_) => {
                    added += chunk.iter().filter(|(_, existed)| !existed).count();
                }
                Err(e) => {
                    tracing::warn!(error = %e, "scan batch write failed");
                    errors.fetch_add(chunk.len(), Ordering::Relaxed);
                }
            }
        }

        summary.added = added;
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

    /// Stats + parses one file. `Ok(None)` = unchanged row skipped (M12):
    /// the stored mtime+size match the file and the row is not missing, so
    /// no metadata is parsed and nothing is written. `existed` distinguishes
    /// inserts from updates for the summary.
    fn parse_job(
        &self,
        db: &Db,
        folder_id: i64,
        path: &Path,
    ) -> std::result::Result<Option<(repo::TrackWrite, bool)>, String> {
        let stat = repo::stat_by_path(db, &path.to_string_lossy()).map_err(|e| e.to_string())?;
        let mtime = file_mtime(path);
        let size = std::fs::metadata(path).map(|m| m.len() as i64).unwrap_or(0);
        if let Some((st_mtime, st_size, missing)) = stat {
            if !missing && st_mtime == mtime && st_size == size {
                return Ok(None);
            }
        }
        let existed = stat.is_some();
        let track = metadata::read(path, folder_id).map_err(|e| e.to_string())?;
        // Art failures never fail the file (old behavior): the row just gets
        // no artwork path. The hash includes mtime, so unchanged files hit
        // the cache for free.
        let art_path = art::extract(path, mtime, &self.art_dir)
            .ok()
            .flatten()
            .and_then(|p| p.to_str().map(str::to_owned));
        Ok(Some((
            repo::TrackWrite { track, folder_id, mtime, size, art_path },
            existed,
        )))
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

pub(crate) fn file_mtime(path: &Path) -> i64 {
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
