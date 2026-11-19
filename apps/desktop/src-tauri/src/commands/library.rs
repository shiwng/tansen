use crate::StateHandle;
use persistence::models::{Folder, Track, TrackRef};
use tauri::{command, Emitter};

use super::err;

// Library

#[command]
pub fn folder_add(state: StateHandle<'_>, path: String) -> Result<i64, String> {
    if !app_core::state::local_files_enabled(&state.db) {
        return Err("Local files are off — enable them in Settings → Downloads.".into());
    }
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
    if !app_core::state::local_files_enabled(&state.db) {
        return Err("Local files are off — no scan started.".into());
    }
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

// Grouped views + history

#[command]
pub fn albums_get(state: StateHandle<'_>) -> Result<Vec<persistence::views::AlbumGroup>, String> {
    persistence::views::albums(&state.db).map_err(err)
}

#[command]
pub fn artists_get(state: StateHandle<'_>) -> Result<Vec<persistence::views::ArtistGroup>, String> {
    persistence::views::artists(&state.db).map_err(err)
}

#[command]
pub fn album_tracks_get(state: StateHandle<'_>, key: String) -> Result<Vec<TrackRef>, String> {
    persistence::views::album_tracks(&state.db, &key).map_err(err)
}

#[command]
pub fn artist_tracks_get(state: StateHandle<'_>, name: String) -> Result<Vec<TrackRef>, String> {
    persistence::views::artist_tracks(&state.db, &name).map_err(err)
}

#[command]
pub fn history_get(
    state: StateHandle<'_>,
    limit: i64,
) -> Result<Vec<persistence::views::HistoryEntry>, String> {
    persistence::views::history(&state.db, limit).map_err(err)
}

/// Library additions by `added_at`, newest first — not playback history.
#[command]
pub fn recently_added_get(
    state: StateHandle<'_>,
    limit: i64,
) -> Result<Vec<persistence::models::Track>, String> {
    persistence::repo::recently_added(&state.db, limit).map_err(err)
}

// Home recommendations + listening feedback

#[command]
pub async fn home_feed(
    state: StateHandle<'_>,
    refresh: Option<bool>,
) -> Result<crate::home::HomeFeed, String> {
    crate::home::home_feed(&state, refresh.unwrap_or(false)).await
}

#[command]
pub fn liked_tracks_get(state: StateHandle<'_>) -> Result<Vec<persistence::repo::KeyRef>, String> {
    persistence::repo::liked_tracks(&state.db).map_err(err)
}

#[command]
pub fn recommendation_dismissed_get(
    state: StateHandle<'_>,
) -> Result<Vec<persistence::repo::KeyRef>, String> {
    persistence::repo::dismissed_recommendations(&state.db).map_err(err)
}

#[command]
pub fn liked_tracks_set(
    state: StateHandle<'_>,
    key_source: String,
    key_id: String,
    liked: bool,
) -> Result<(), String> {
    persistence::repo::set_liked_track(
        &state.db,
        &key_source,
        &key_id,
        liked,
        library::scanner::now_secs(),
    )
    .map_err(err)
}

#[command]
pub fn liked_albums_get(state: StateHandle<'_>) -> Result<Vec<persistence::repo::KeyRef>, String> {
    persistence::repo::liked_albums(&state.db).map_err(err)
}

#[command]
pub fn liked_albums_set(
    state: StateHandle<'_>,
    key_source: String,
    key_id: String,
    liked: bool,
) -> Result<(), String> {
    persistence::repo::set_liked_album(
        &state.db,
        &key_source,
        &key_id,
        liked,
        library::scanner::now_secs(),
    )
    .map_err(err)
}

#[command]
pub fn followed_artists_get(
    state: StateHandle<'_>,
) -> Result<Vec<(persistence::repo::KeyRef, String)>, String> {
    persistence::repo::followed_artists(&state.db).map_err(err)
}

#[command]
pub fn followed_artist_set(
    state: StateHandle<'_>,
    key_source: String,
    key_id: String,
    name: String,
    followed: bool,
) -> Result<(), String> {
    persistence::repo::set_followed_artist(
        &state.db,
        &key_source,
        &key_id,
        &name,
        followed,
        library::scanner::now_secs(),
    )
    .map_err(err)
}

#[command]
pub fn recommendation_dismiss(
    state: StateHandle<'_>,
    key_source: String,
    key_id: String,
    dismissed: bool,
) -> Result<(), String> {
    persistence::repo::set_recommendation_dismissed(
        &state.db,
        &key_source,
        &key_id,
        dismissed,
        library::scanner::now_secs(),
    )
    .map_err(err)
}
