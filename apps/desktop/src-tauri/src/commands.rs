//! Tauri command surface. Thin: each command calls an app-core service and
//! maps errors to strings for the JS bridge.

use crate::StateHandle;
use playback::QueueSnapshot;
use persistence::models::{Folder, Track, TrackRef};
use tauri::{command, Emitter};

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

// ---- Settings -------------------------------------------------------------

#[command]
pub fn settings_get(state: StateHandle<'_>) -> Result<Vec<(String, String)>, String> {
    state.settings().map_err(err)
}

#[command]
pub fn settings_set(state: StateHandle<'_>, key: String, value: String) -> Result<(), String> {
    state.set_setting(&key, &value).map_err(err)
}

// ---- Library --------------------------------------------------------------

#[command]
pub fn folder_add(state: StateHandle<'_>, path: String) -> Result<i64, String> {
    state.add_folder(&path).map_err(err)
}

#[command]
pub fn folder_remove(state: StateHandle<'_>, id: i64) -> Result<(), String> {
    state.remove_folder(id).map_err(err)
}

#[command]
pub fn folders_get(state: StateHandle<'_>) -> Result<Vec<Folder>, String> {
    state.folders().map_err(err)
}

#[command]
pub fn tracks_get(state: StateHandle<'_>) -> Result<Vec<Track>, String> {
    state.tracks().map_err(err)
}

/// Blocking scan run off the async runtime; progress is emitted as
/// `scan://progress` events while the scan runs.
#[command]
pub async fn scan_start(
    app: tauri::AppHandle,
    state: StateHandle<'_>,
) -> Result<library::ScanSummary, String> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<library::ScanProgress>();
    let emitter = app.clone();
    let forwarder = tauri::async_runtime::spawn(async move {
        while let Some(p) = rx.recv().await {
            let _ = emitter.emit("scan://progress", p);
        }
    });

    let state2 = state.inner().clone();
    let res = tauri::async_runtime::spawn_blocking(move || {
        state2.scan(move |p| {
            let _ = tx.send(p);
        })
    })
    .await
    .map_err(err)?
    .map_err(err)?;

    let _ = forwarder.await;
    let _ = app.emit("scan://done", res);
    Ok(res)
}

// ---- Playback -------------------------------------------------------------

#[command]
pub fn player_load(state: StateHandle<'_>, ids: Vec<i64>, start: usize, play: bool) -> Result<(), String> {
    state.play(&ids, start, play).map_err(err)
}

#[command]
pub fn player_toggle(state: StateHandle<'_>) -> Result<(), String> {
    state.toggle_play();
    Ok(())
}

#[command]
pub fn player_pause(state: StateHandle<'_>) -> Result<(), String> {
    state.pause();
    Ok(())
}

#[command]
pub fn player_resume(state: StateHandle<'_>) -> Result<(), String> {
    state.resume();
    Ok(())
}

#[command]
pub fn player_stop(state: StateHandle<'_>) -> Result<(), String> {
    state.stop();
    Ok(())
}

#[command]
pub fn player_next(state: StateHandle<'_>) -> Result<(), String> {
    state.next();
    Ok(())
}

#[command]
pub fn player_previous(state: StateHandle<'_>) -> Result<(), String> {
    state.previous();
    Ok(())
}

#[command]
pub fn player_seek(state: StateHandle<'_>, sec: f64) -> Result<(), String> {
    state.seek(sec);
    Ok(())
}

#[command]
pub fn player_volume(state: StateHandle<'_>, level: f64) -> Result<(), String> {
    state.set_volume(level);
    Ok(())
}

#[command]
pub fn player_repeat(state: StateHandle<'_>, mode: String) -> Result<(), String> {
    let r = match mode.as_str() {
        "all" => playback::Repeat::All,
        "one" => playback::Repeat::One,
        _ => playback::Repeat::Off,
    };
    state.set_repeat(r);
    Ok(())
}

#[command]
pub fn queue_snapshot(state: StateHandle<'_>) -> QueueSnapshot {
    state.queue_snapshot()
}

#[command]
pub fn queue_insert(state: StateHandle<'_>, ids: Vec<i64>, at: Option<usize>) -> Result<(), String> {
    state.queue_insert(&ids, at).map_err(err)
}

#[command]
pub fn queue_remove(state: StateHandle<'_>, index: usize) -> Result<(), String> {
    state.queue_remove(index);
    Ok(())
}

#[command]
pub fn queue_jump(state: StateHandle<'_>, index: usize) -> Result<(), String> {
    state.queue_jump(index);
    Ok(())
}

#[command]
pub fn queue_clear(state: StateHandle<'_>) -> Result<(), String> {
    state.queue_clear();
    Ok(())
}

// ---- Playlists ------------------------------------------------------------

#[command]
pub fn playlist_create(state: StateHandle<'_>, name: String) -> Result<i64, String> {
    state.playlist_create(&name).map_err(err)
}

#[command]
pub fn playlists_get(state: StateHandle<'_>) -> Result<Vec<(i64, String)>, String> {
    state.playlists().map_err(err)
}

#[command]
pub fn playlist_tracks_get(state: StateHandle<'_>, id: i64) -> Result<Vec<TrackRef>, String> {
    state.playlist_tracks(id).map_err(err)
}

#[command]
pub fn playlist_tracks_set(state: StateHandle<'_>, id: i64, ids: Vec<i64>) -> Result<(), String> {
    state.playlist_set_tracks(id, &ids).map_err(err)
}
