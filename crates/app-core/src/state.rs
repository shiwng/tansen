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
}

pub struct AppState {
    pub db: Arc<Db>,
    pub engine: Engine,
    pub scanner: Scanner,
    pub media: crate::media::Media,
}

impl AppState {
    pub fn open(data_dir: &std::path::Path) -> Result<Self, CoreError> {
        let db = Arc::new(Db::open(&data_dir.join("bitchord.db"))?);
        let engine = Engine::spawn()?;
        let scanner = Scanner::new(data_dir.join("art"));
        crate::history::spawn_recorder(db.clone(), engine.events.clone());
        crate::persist_state::spawn_persister(db.clone(), engine.events.clone());
        Ok(Self {
            db,
            engine,
            scanner,
            media: crate::media::Media::new(),
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
            .filter(|t| std::path::Path::new(&t.path).exists())
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
        let folders: Vec<(i64, String)> = repo::list_folders(&self.db)?
            .into_iter()
            .map(|f| (f.id, f.path))
            .collect();
        Ok(self.scanner.scan(&self.db, &folders, on_progress)?)
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
    dirs_data_root().join("bitchord-desktop")
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
