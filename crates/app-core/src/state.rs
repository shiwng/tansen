//! Application core: owns the database, the library scanner and the OS media
//! integration; exposes the operations the UI drives. No Tauri dependency —
//! the desktop shell translates these into commands and events.
//!
//! Playback state left this module. The task-based engine
//! (`crate::playback_engine`) is owned by the shell; the queue, transport and
//! restore concerns moved to the frontend (localStorage mirror) and the
//! engine itself. The legacy `playback::Engine` path is gone.

use library::Scanner;
use persistence::models::{PlaylistMeta, Track, TrackRef};
use persistence::{repo, Db};
use std::path::PathBuf;
use std::sync::Arc;

/// Persisted local-files source switch. Default on: the library behaved this
/// way before the switch existed, so an upgrade never drops a user's source.
pub const LOCAL_FILES_SETTING: &str = "local_files_enabled";

pub fn local_files_enabled(db: &Db) -> bool {
    repo::get_setting(db, LOCAL_FILES_SETTING)
        .ok()
        .flatten()
        .map(|v| v != "false")
        .unwrap_or(true)
}

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("db: {0}")]
    Db(#[from] persistence::DbError),
    #[error("track {0} not found")]
    TrackMissing(i64),
    #[error("{0}")]
    Catalogue(String),
}

pub struct AppState {
    pub db: Arc<Db>,
    /// Read-only WAL connection for query-heavy reads. The writer stays
    /// `db` alone; the engine never touches either.
    pub db_ro: Arc<Db>,
    pub scanner: Scanner,
    /// Completed-scan notifications (watcher scans surface here too).
    pub scan_events: tokio::sync::broadcast::Sender<library::ScanSummary>,
    /// Serializes scans; the watcher skips when the command path holds it.
    scan_lock: std::sync::Mutex<()>,
    /// The sidecar-backed YouTube source, launched once on first use and kept
    /// for the process lifetime. `None` until the catalogue is first used.
    yt: std::sync::Mutex<Option<Arc<crate::yt_handle::YtHandle>>>,
    /// Jar path found by the shell (Tauri resource dir). Checked first.
    sidecar_hint: std::sync::Mutex<Option<PathBuf>>,
    /// Bundled-JRE java binary found by the shell. Checked before
    /// JAVA_HOME/PATH discovery so a clean Mac works without system Java.
    java_hint: std::sync::Mutex<Option<String>>,
    /// Disk cache for streamed tracks, read by the playback pipeline.
    stream_cache: Arc<playback::StreamCache>,
    /// Root data directory (for caches the shell owns, like lyrics).
    pub data_dir: PathBuf,
}

impl AppState {
    pub fn open(data_dir: &std::path::Path) -> Result<Self, CoreError> {
        let db = Arc::new(Db::open(&data_dir.join("tansen.db"))?);
        let db_ro = Arc::new(Db::open_read_only(&data_dir.join("tansen.db"))?);
        let stream_cache = Arc::new(playback::StreamCache::new(
            data_dir.join("stream-cache"),
            512 * 1024 * 1024,
        ));
        let scanner = Scanner::new(data_dir.join("art"));
        let (scan_events, _) = tokio::sync::broadcast::channel(16);
        crate::watcher::spawn_watcher(db.clone(), scanner.clone(), scan_events.clone());
        Ok(Self {
            db,
            db_ro,
            scanner,
            scan_events,
            scan_lock: std::sync::Mutex::new(()),
            yt: std::sync::Mutex::new(None),
            sidecar_hint: std::sync::Mutex::new(None),
            java_hint: std::sync::Mutex::new(None),
            stream_cache,
            data_dir: data_dir.to_path_buf(),
        })
    }

    // Library

    pub fn add_folder(&self, path: &str) -> Result<i64, CoreError> {
        let canonical = std::fs::canonicalize(path)
            .map_err(persistence::DbError::Io)?
            .to_string_lossy()
            .into_owned();
        Ok(repo::add_folder(
            &self.db,
            &canonical,
            library::scanner::now_secs(),
        )?)
    }

    pub fn remove_folder(&self, id: i64) -> Result<(), CoreError> {
        repo::remove_folder(&self.db, id)?;
        Ok(())
    }

    pub fn folders(&self) -> Result<Vec<persistence::models::Folder>, CoreError> {
        Ok(repo::list_folders(&self.db)?)
    }

    pub fn tracks(&self) -> Result<Vec<Track>, CoreError> {
        Ok(repo::list_tracks(&self.db)?)
    }

    /// Runs one scan pass over every registered folder, on the caller's
    /// blocking thread (Tauri wraps this in spawn_blocking).
    pub fn scan(
        &self,
        on_progress: impl Fn(library::ScanProgress) + Send + Sync,
    ) -> Result<library::ScanSummary, CoreError> {
        let _guard = self.scan_lock.lock().unwrap();
        let folders = self.scan_folders()?;
        Ok(self.scanner.scan(&self.db, &folders, on_progress)?)
    }

    fn scan_folders(&self) -> Result<Vec<(i64, String)>, CoreError> {
        Ok(repo::list_folders(&self.db)?
            .into_iter()
            .map(|f| (f.id, f.path))
            .collect())
    }

    /// Watcher entry: scans only when both the watch and local-files settings
    /// are on and no scan is running, then reports the summary on
    /// `scan_events`.
    pub fn scan_if_free(&self) {
        if !local_files_enabled(&self.db) {
            return;
        }
        if repo::get_setting(&self.db, "watch_folders")
            .ok()
            .flatten()
            .map(|v| v == "false")
            .unwrap_or(false)
        {
            return;
        }
        let Ok(_guard) = self.scan_lock.try_lock() else {
            return; // a user-triggered scan is already running
        };
        let folders = match self.scan_folders() {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!(error = %e, "watcher scan listing failed");
                return;
            }
        };
        match self.scanner.scan(&self.db, &folders, |_| {}) {
            Ok(summary) => {
                let _ = self.scan_events.send(summary);
            }
            Err(e) => tracing::warn!(error = %e, "watcher scan failed"),
        }
    }

    pub fn playlist_add_tracks(&self, id: i64, ids: &[i64]) -> Result<(), CoreError> {
        repo::append_playlist_tracks(&self.db, id, ids)?;
        Ok(())
    }

    // Remote (YouTube via sidecar)

    /// Shell-provided jar location (the packaged app's resource directory).
    /// Checked before env/settings/discovery.
    pub fn set_sidecar_hint(&self, jar: PathBuf) {
        *self.sidecar_hint.lock().unwrap() = Some(jar);
    }

    /// The shell points at the bundled JRE's java binary (resource dir).
    pub fn set_java_hint(&self, java: PathBuf) {
        *self.java_hint.lock().unwrap() = Some(java.to_string_lossy().into_owned());
    }

    /// The stream disk cache, for services that prefetch outside app-core.
    pub fn stream_cache(&self) -> Arc<playback::StreamCache> {
        self.stream_cache.clone()
    }

    /// Directory for the lyrics response cache (created on demand).
    pub fn lyrics_cache_dir(&self) -> PathBuf {
        let dir = self.data_dir.join("lyrics-cache");
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    /// The catalogue flag plus a launched-once sidecar handle. Errors name the
    /// fix (flag off, jar missing, no Java) — remote playback never pretends.
    pub fn yt(&self) -> Result<Arc<crate::yt_handle::YtHandle>, CoreError> {
        let enabled = repo::get_setting(&self.db, "catalogue_sources")
            .ok()
            .flatten()
            .map(|v| v == "true")
            .unwrap_or(false);
        if !enabled {
            return Err(CoreError::Catalogue(
                "catalogue sources are disabled (enable them in Settings → Sources)".into(),
            ));
        }
        let mut guard = self.yt.lock().unwrap();
        if let Some(handle) = &*guard {
            return Ok(handle.clone());
        }
        // Settings survive upgrades and can point at a removed checkout. Prefer
        // them when valid, but fall back to the resources injected by the
        // desktop shell; otherwise a stale setting permanently disables the
        // bundled sidecar/JRE.
        let jar = existing_or_hint(
            repo::get_setting(&self.db, "sidecar_jar")
                .ok()
                .flatten()
                .map(PathBuf::from),
            self.sidecar_hint.lock().unwrap().clone(),
        );
        let java = existing_or_hint(
            repo::get_setting(&self.db, "java_path")
                .ok()
                .flatten()
                .map(PathBuf::from),
            self.java_hint.lock().unwrap().clone().map(PathBuf::from),
        )
        .map(|p| p.to_string_lossy().into_owned());
        let source = sources::yt::YtMusicSource::launch(jar, java)
            .map_err(|e| CoreError::Catalogue(format!("YouTube sidecar unavailable: {e}")))?;
        let handle = Arc::new(crate::yt_handle::YtHandle {
            source: Arc::new(source),
        });
        *guard = Some(handle.clone());
        Ok(handle)
    }

    /// Kill the sidecar JVM. The shell calls this on exit so a relaunch never
    /// inherits an orphaned listener.
    pub fn shutdown_sidecar(&self) {
        if let Some(handle) = self.yt.lock().unwrap().take() {
            handle.source.shutdown();
        }
    }

    // Settings

    pub fn settings(&self) -> Result<Vec<(String, String)>, CoreError> {
        Ok(repo::list_settings(&self.db)?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), CoreError> {
        repo::set_setting(&self.db, key, value)?;
        Ok(())
    }

    // Playlists

    pub fn playlist_create(&self, name: &str) -> Result<i64, CoreError> {
        Ok(repo::create_playlist(
            &self.db,
            name,
            library::scanner::now_secs(),
        )?)
    }

    pub fn playlists(&self) -> Result<Vec<PlaylistMeta>, CoreError> {
        Ok(repo::list_playlists(&self.db)?)
    }

    pub fn playlist_set_favourite(&self, id: i64, favourite: bool) -> Result<(), CoreError> {
        Ok(repo::set_playlist_favourite(&self.db, id, favourite)?)
    }

    pub fn playlist_tracks(&self, id: i64) -> Result<Vec<TrackRef>, CoreError> {
        Ok(repo::playlist_tracks(&self.db, id)?)
    }

    pub fn playlist_set_tracks(&self, id: i64, ids: &[i64]) -> Result<(), CoreError> {
        repo::set_playlist_tracks(&self.db, id, ids)?;
        Ok(())
    }

    pub fn playlist_rename(&self, id: i64, name: &str) -> Result<(), CoreError> {
        repo::rename_playlist(&self.db, id, name)?;
        Ok(())
    }

    pub fn playlist_delete(&self, id: i64) -> Result<(), CoreError> {
        repo::delete_playlist(&self.db, id)?;
        Ok(())
    }
}

pub fn data_dir_default() -> PathBuf {
    dirs_data_root().join("tansen")
}

/// One-time migration from the pre-rename data directory
/// (`bitchord-desktop` → `tansen`): renames the directory and its database
/// files. Runs before `AppState::open`; a no-op when either side is absent.
pub fn migrate_legacy_data_dir() {
    let new = data_dir_default();
    if new.exists() {
        return;
    }
    let old = dirs_data_root().join("bitchord-desktop");
    if !old.is_dir() {
        return;
    }
    if std::fs::rename(&old, &new).is_err() {
        tracing::warn!("could not migrate legacy data dir {}", old.display());
        return;
    }
    for (from, to) in [
        ("bitchord.db", "tansen.db"),
        ("bitchord.db-wal", "tansen.db-wal"),
        ("bitchord.db-shm", "tansen.db-shm"),
    ] {
        let _ = std::fs::rename(new.join(from), new.join(to));
    }
    tracing::info!("migrated data dir to {}", new.display());
}

#[cfg(target_os = "macos")]
fn dirs_data_root() -> PathBuf {
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join("Library/Application Support"))
        .unwrap_or_else(std::env::temp_dir)
}

#[cfg(target_os = "windows")]
fn dirs_data_root() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

/// A persisted path is used only while it still exists: settings survive
/// upgrades, so a path into a since-trashed app must fall back to the path the
/// current bundle injected.
fn existing_or_hint(setting: Option<PathBuf>, hint: Option<PathBuf>) -> Option<PathBuf> {
    setting.filter(|p| p.is_file()).or(hint)
}

#[cfg(test)]
mod state_path_tests {
    use super::existing_or_hint;

    #[test]
    fn stale_setting_falls_back_to_the_current_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let bundled = dir.path().join("sidecar-all.jar");
        std::fs::write(&bundled, b"jar").unwrap();
        let trashed = dir
            .path()
            .join("Tansen-old.app/Contents/Resources/resources/sidecar-all.jar");
        assert_eq!(
            existing_or_hint(Some(trashed), Some(bundled.clone())),
            Some(bundled)
        );
        assert_eq!(
            existing_or_hint(None, Some(dir.path().join("jre/bin/java"))),
            Some(dir.path().join("jre/bin/java"))
        );
        assert_eq!(existing_or_hint(None, None), None);
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn dirs_data_root() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(std::env::temp_dir)
}
