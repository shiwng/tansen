//! Application core: owns the database, the audio engine and the library
//! scanner; exposes the operations the UI drives. No Tauri dependency — the
//! desktop shell translates these into commands and events.

use library::Scanner;
use persistence::models::{Track, TrackRef};
use persistence::{repo, Db};
use playback::{Command, Engine, EngineEvent, QueueOp, Repeat};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("db: {0}")]
    Db(#[from] persistence::DbError),
    #[error("audio: {0}")]
    Audio(#[from] playback::EngineError),
    #[error("track {0} not found")]
    TrackMissing(i64),
    #[error("{0}")]
    Catalogue(String),
}

pub struct AppState {
    pub db: Arc<Db>,
    pub engine: Engine,
    pub scanner: Scanner,
    pub media: crate::media::Media,
    /// Completed-scan notifications (watcher scans surface here too).
    pub scan_events: tokio::sync::broadcast::Sender<library::ScanSummary>,
    /// Serializes scans; the watcher skips when the command path holds it.
    scan_lock: std::sync::Mutex<()>,
    /// The sidecar-backed YouTube source, launched once on first use and kept
    /// for the process lifetime. `None` until the catalogue is first used.
    yt: std::sync::Mutex<Option<Arc<crate::yt_handle::YtHandle>>>,
    /// Jar path found by the shell (Tauri resource dir). Checked first.
    sidecar_hint: std::sync::Mutex<Option<PathBuf>>,
}

impl AppState {
    pub fn open(data_dir: &std::path::Path) -> Result<Self, CoreError> {
        let db = Arc::new(Db::open(&data_dir.join("tansen.db"))?);
        let engine = Engine::spawn()?;
        let scanner = Scanner::new(data_dir.join("art"));
        let (scan_events, _) = tokio::sync::broadcast::channel(16);
        crate::history::spawn_recorder(db.clone(), engine.events.clone());
        crate::persist_state::spawn_persister(db.clone(), engine.events.clone());
        crate::watcher::spawn_watcher(db.clone(), scanner.clone(), scan_events.clone());
        Ok(Self {
            db,
            engine,
            scanner,
            media: crate::media::Media::new(),
            scan_events,
            scan_lock: std::sync::Mutex::new(()),
            yt: std::sync::Mutex::new(None),
            sidecar_hint: std::sync::Mutex::new(None),
        })
    }

    /// Restores the last queue (paused) from SQLite, skipping tracks whose
    /// files are gone. Called once after open, before the UI loads.
    pub fn restore_playback(&self) -> Result<bool, CoreError> {
        let Some(state) = persistence::views::load_playback_state(&self.db)? else {
            return Ok(false);
        };
        let alive: Vec<TrackRef> = state
            .tracks
            .into_iter()
            .filter(|t| {
                crate::remote::is_placeholder(&t.path)
                    || t.path.starts_with("http://")
                    || t.path.starts_with("https://")
                    || std::path::Path::new(&t.path).exists()
            })
            .map(|mut t| {
                // Stream URLs expire (~6 h). A restored remote row goes back to
                // its placeholder so the resolve-ahead loop re-resolves it.
                if t.video_id.is_some() && t.path.starts_with("http") {
                    t.path = crate::remote::placeholder_for(t.video_id.as_deref().unwrap_or(""));
                }
                t
            })
            .collect();
        if alive.is_empty() {
            persistence::views::clear_playback_state(&self.db)?;
            return Ok(false);
        }
        let index = state.index.filter(|i| *i < alive.len()).unwrap_or(0);
        let position = state.position_sec;
        let repeat = match state.repeat.as_str() {
            "all" => Repeat::All,
            "one" => Repeat::One,
            _ => Repeat::Off,
        };
        self.engine.send(Command::Apply(QueueOp::Load {
            tracks: alive,
            index,
            play: false,
        }));
        self.engine.send(Command::Apply(QueueOp::SetRepeat(repeat)));
        if position > 0.5 {
            self.engine.send(Command::Seek(position));
        }
        Ok(true)
    }

    // ---- Library -----------------------------------------------------------

    pub fn add_folder(&self, path: &str) -> Result<i64, CoreError> {
        let canonical = std::fs::canonicalize(path)
            .map_err(persistence::DbError::Io)?
            .to_string_lossy()
            .into_owned();
        Ok(repo::add_folder(&self.db, &canonical, library::scanner::now_secs())?)
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

    pub fn track_refs(&self, ids: &[i64]) -> Result<Vec<TrackRef>, CoreError> {
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let t = repo::get_track(&self.db, *id)?
                .ok_or(CoreError::TrackMissing(*id))?;
            if !t.missing {
                out.push(t.into());
            }
        }
        Ok(out)
    }

    /// Runs one scan pass over every registered folder, on the caller's
    /// blocking thread (Tauri wraps this in spawn_blocking).
    pub fn scan(&self, on_progress: impl Fn(library::ScanProgress) + Send + Sync) -> Result<library::ScanSummary, CoreError> {
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

    /// Watcher entry: scans only when the setting is on and no scan is running,
    /// then reports the summary on `scan_events`.
    pub fn scan_if_free(&self) {
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

    // ---- Remote (YouTube via sidecar) --------------------------------------

    /// Shell-provided jar location (the packaged app's resource directory).
    /// Checked before env/settings/discovery.
    pub fn set_sidecar_hint(&self, jar: PathBuf) {
        *self.sidecar_hint.lock().unwrap() = Some(jar);
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
            return Err(CoreError::Catalogue("catalogue sources are disabled (enable them in Settings → Sources)".into()));
        }
        let mut guard = self.yt.lock().unwrap();
        if let Some(handle) = &*guard {
            return Ok(handle.clone());
        }
        let jar = repo::get_setting(&self.db, "sidecar_jar")
            .ok()
            .flatten()
            .map(PathBuf::from)
            .or_else(|| self.sidecar_hint.lock().unwrap().clone());
        let java = repo::get_setting(&self.db, "java_path").ok().flatten();
        let source = sources::yt::YtMusicSource::launch(jar, java)
            .map_err(|e| CoreError::Catalogue(format!("YouTube sidecar unavailable: {e}")))?;
        let handle = Arc::new(crate::yt_handle::YtHandle { source: Arc::new(source) });
        *guard = Some(handle.clone());
        Ok(handle)
    }

    /// Queues catalogue tracks. The start track is resolved immediately so
    /// sound starts without waiting for the resolve-ahead loop; the rest ride
    /// placeholders until the loop fills them.
    pub fn play_remote(
        &self,
        tracks: &[crate::yt_handle::RemoteTrack],
        start: usize,
        mode: crate::yt_handle::PlayMode,
    ) -> Result<(), CoreError> {
        use crate::yt_handle::PlayMode;
        if tracks.is_empty() {
            return Ok(());
        }
        let yt = self.yt()?;
        let mut refs: Vec<TrackRef> = tracks
            .iter()
            .map(|t| TrackRef {
                id: 0,
                title: t.title.clone(),
                artist: t.artist.clone(),
                album: t.album.clone().unwrap_or_default(),
                duration_sec: t.duration_sec,
                art_path: t.thumbnail_url.clone(),
                path: crate::remote::placeholder_for(&t.video_id),
                video_id: Some(t.video_id.clone()),
            })
            .collect();

        match mode {
            PlayMode::Load => {
                let start = start.min(refs.len() - 1);
                let stream = yt
                    .stream_blocking(&tracks[start].video_id)
                    .map_err(|e| CoreError::Catalogue(format!("stream resolve failed: {e}")))?;
                refs[start].path = stream.url;
                self.media.update_track(refs.get(start));
                self.engine.send(Command::Apply(playback::QueueOp::Load {
                    tracks: refs,
                    index: start,
                    play: true,
                }));
            }
            PlayMode::InsertNext => {
                self.engine
                    .send(Command::Apply(playback::QueueOp::Insert { tracks: refs, at: Some(self.engine.snapshot().index.map(|i| i + 1).unwrap_or(0)) }));
            }
            PlayMode::Append => {
                self.engine.send(Command::Apply(playback::QueueOp::Insert { tracks: refs, at: None }));
            }
        }
        Ok(())
    }

    // ---- Playback ----------------------------------------------------------

    /// Replaces the queue with `ids` starting at `start`.
    pub fn play(&self, ids: &[i64], start: usize, play: bool) -> Result<(), CoreError> {
        let tracks = self.track_refs(ids)?;
        if tracks.is_empty() {
            return Ok(());
        }
        self.media.update_track(tracks.get(start));
        self.engine.send(Command::Apply(QueueOp::Load { tracks, index: start, play }));
        Ok(())
    }

    pub fn toggle_play(&self) {
        self.engine.send(Command::TogglePlay);
        self.sync_media_playback();
    }

    pub fn pause(&self) {
        self.engine.send(Command::Pause);
        self.sync_media_playback();
    }

    pub fn resume(&self) {
        self.engine.send(Command::Resume);
        self.sync_media_playback();
    }

    pub fn stop(&self) {
        self.engine.send(Command::Stop);
        self.media.update_track(None);
        self.sync_media_playback();
    }

    pub fn next(&self) {
        self.engine.send(Command::Apply(QueueOp::Next));
        self.sync_media_playback();
    }

    pub fn previous(&self) {
        self.engine.send(Command::Apply(QueueOp::Prev));
        self.sync_media_playback();
    }

    pub fn seek(&self, sec: f64) {
        self.engine.send(Command::Seek(sec));
        self.sync_media_playback();
    }

    pub fn set_volume(&self, v: f64) {
        self.engine.send(Command::SetVolume(v));
    }

    pub fn set_repeat(&self, r: Repeat) {
        self.engine.send(Command::Apply(QueueOp::SetRepeat(r)));
    }

    pub fn queue_insert(&self, ids: &[i64], at: Option<usize>) -> Result<(), CoreError> {
        let tracks = self.track_refs(ids)?;
        self.engine.send(Command::Apply(QueueOp::Insert { tracks, at }));
        Ok(())
    }

    pub fn queue_remove(&self, index: usize) {
        self.engine.send(Command::Apply(QueueOp::RemoveAt(index)));
    }

    pub fn queue_jump(&self, index: usize) {
        self.engine.send(Command::Apply(QueueOp::Jump(index)));
    }

    pub fn queue_clear(&self) {
        self.engine.send(Command::Apply(QueueOp::Clear));
    }

    pub fn queue_move(&self, from: usize, to: usize) {
        self.engine.send(Command::Apply(QueueOp::Move { from, to }));
    }

    pub fn queue_snapshot(&self) -> playback::QueueSnapshot {
        self.engine.snapshot()
    }

    /// Subscribe to engine state changes (UI + OS media controls feed).
    pub fn engine_events(&self) -> tokio::sync::broadcast::Sender<EngineEvent> {
        self.engine.events.clone()
    }

    /// Command channel for services that push into the engine from their own
    /// threads (the resolve-ahead loop).
    pub fn engine_tx(&self) -> std::sync::mpsc::Sender<Command> {
        self.engine.command_tx()
    }

    fn sync_media_playback(&self) {
        let snap = self.engine.snapshot();
        self.media.update_playback(snap.playing, None);
    }

    // ---- Settings ----------------------------------------------------------

    pub fn settings(&self) -> Result<Vec<(String, String)>, CoreError> {
        Ok(repo::list_settings(&self.db)?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), CoreError> {
        repo::set_setting(&self.db, key, value)?;
        Ok(())
    }

    // ---- Playlists ---------------------------------------------------------

    pub fn playlist_create(&self, name: &str) -> Result<i64, CoreError> {
        Ok(repo::create_playlist(&self.db, name, library::scanner::now_secs())?)
    }

    pub fn playlists(&self) -> Result<Vec<(i64, String)>, CoreError> {
        Ok(repo::list_playlists(&self.db)?)
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

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn dirs_data_root() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(std::env::temp_dir)
}
