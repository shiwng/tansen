use crate::StateHandle;
use persistence::models::{PlaylistMeta, TrackRef};
use tauri::command;

use super::err;
use super::spotify::SPOTIFY_IMPORT_MAX;

// Playlists

#[command]
pub fn playlist_create(state: StateHandle<'_>, name: String) -> Result<i64, String> {
    state.playlist_create(&name).map_err(err)
}

#[command]
pub fn playlists_get(state: StateHandle<'_>) -> Result<Vec<PlaylistMeta>, String> {
    state.playlists().map_err(err)
}

#[command]
pub fn playlist_set_favourite(
    state: StateHandle<'_>,
    id: i64,
    favourite: bool,
) -> Result<(), String> {
    state.playlist_set_favourite(id, favourite).map_err(err)
}

#[command]
pub fn playlist_tracks_get(state: StateHandle<'_>, id: i64) -> Result<Vec<TrackRef>, String> {
    state.playlist_tracks(id).map_err(err)
}

#[command]
pub fn playlist_tracks_set(state: StateHandle<'_>, id: i64, ids: Vec<i64>) -> Result<(), String> {
    state.playlist_set_tracks(id, &ids).map_err(err)
}

#[command]
pub fn playlist_add_tracks(state: StateHandle<'_>, id: i64, ids: Vec<i64>) -> Result<(), String> {
    state.playlist_add_tracks(id, &ids).map_err(err)
}

#[command]
pub fn playlist_rename(state: StateHandle<'_>, id: i64, name: String) -> Result<(), String> {
    state.playlist_rename(id, &name).map_err(err)
}

#[command]
pub fn playlist_delete(state: StateHandle<'_>, id: i64) -> Result<(), String> {
    state.playlist_delete(id).map_err(err)
}

/// Queue display rows by entry-id lists. The frontend calls this
/// when the engine publishes a new `queue_rev`, so the Up Next mirror
/// re-reads authoritative metadata for BOTH kinds: `ids` are local library
/// ids, `remote_ids` are remote-track row ids. The combined cap mirrors
/// the engine's enqueue bound.
#[command]
pub async fn queue_tracks_get(
    state: StateHandle<'_>,
    ids: Vec<i64>,
    remote_ids: Vec<i64>,
) -> Result<Vec<persistence::repo::QueueDisplayRow>, String> {
    const MAX_IDS: usize = 10_000;
    if ids.len() + remote_ids.len() > MAX_IDS {
        return Err(format!(
            "too many ids: {} (cap {MAX_IDS})",
            ids.len() + remote_ids.len()
        ));
    }
    let state = state.inner().clone();
    // Remote ids arrive as engine-side ids (offset applied); the DB
    // stores raw rowids.
    let remote_ids: Vec<i64> = remote_ids
        .into_iter()
        .map(|id| id - app_core::playback_types::REMOTE_ID_OFFSET as i64)
        .collect();
    tauri::async_runtime::spawn_blocking(move || {
        persistence::repo::queue_rows_by_ids(&state.db_ro, &ids, &remote_ids).map_err(err)
    })
    .await
    .map_err(err)?
}

/// Upserts remote track metadata for a queue entry via the single
/// writer (`state.db`), BEFORE the frontend sends the Enqueue/Play command.
/// Returns the `remote_tracks` row id — the EntryId the engine and the
/// queue mirror use for this track. Text is length-bounded (clipped, never
/// rejected); hostile inputs cannot grow the schema unboundedly.
#[command]
pub fn remote_track_upsert(
    state: StateHandle<'_>,
    key_source: String,
    key_id: String,
    title: String,
    artist: String,
    album: Option<String>,
    duration_sec: f64,
    artwork_url: Option<String>,
) -> Result<i64, String> {
    use persistence::repo::{REMOTE_MAX_KEY, REMOTE_MAX_STR};
    if !matches!(
        key_source.as_str(),
        "youtube" | "addon" | "subsonic" | "radio"
    ) {
        return Err(format!("unknown source kind: {key_source}"));
    }
    if key_id.trim().is_empty() || key_id.len() > REMOTE_MAX_KEY {
        return Err(format!("track id out of bounds (cap {REMOTE_MAX_KEY})"));
    }
    if !duration_sec.is_finite() || !(0.0..=86_400.0).contains(&duration_sec) {
        return Err("duration out of range".into());
    }
    if let Some(ref a) = artwork_url {
        if !a.starts_with("https://") || a.len() > REMOTE_MAX_STR {
            return Err("artwork url invalid".into());
        }
    }
    #[cfg(debug_assertions)]
    tracing::info!(target: "play-path", source = %key_source, id = %key_id, "remote track upsert");
    let row_id = persistence::repo::upsert_remote_track(
        &state.db,
        &key_source,
        &key_id,
        &title,
        &artist,
        album.as_deref(),
        duration_sec,
        artwork_url.as_deref(),
    )
    .map_err(err)?;
    // Mint into the remote EntryId space (see REMOTE_ID_OFFSET).
    Ok(row_id + app_core::playback_types::REMOTE_ID_OFFSET as i64)
}

/// One stored-playlist row handed to `playlist_add_remote_tracks`.
#[derive(serde::Deserialize)]
pub struct RemotePlaylistRowDto {
    pub key_source: String,
    pub key_id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_sec: f64,
    pub artwork_url: Option<String>,
}

/// Appends matched/unmatched import rows to a stored playlist. Same trust
/// boundary shape as `remote_track_upsert`: kinds allowlisted, ids and
/// text bounded, durations ranged, artwork https-only.
#[command]
pub fn playlist_add_remote_tracks(
    state: StateHandle<'_>,
    id: i64,
    tracks: Vec<RemotePlaylistRowDto>,
) -> Result<usize, String> {
    use persistence::repo::{REMOTE_MAX_KEY, REMOTE_MAX_STR};
    if tracks.len() > SPOTIFY_IMPORT_MAX {
        return Err(format!(
            "too many tracks: {} (cap {SPOTIFY_IMPORT_MAX})",
            tracks.len()
        ));
    }
    for t in &tracks {
        if !matches!(t.key_source.as_str(), "youtube" | "spotify") {
            return Err(format!("unknown source kind: {}", t.key_source));
        }
        if t.key_id.trim().is_empty() || t.key_id.len() > REMOTE_MAX_KEY {
            return Err(format!("track id out of bounds (cap {REMOTE_MAX_KEY})"));
        }
        if !t.duration_sec.is_finite() || !(0.0..=86_400.0).contains(&t.duration_sec) {
            return Err("duration out of range".into());
        }
        if let Some(ref a) = t.artwork_url {
            if !a.starts_with("https://") || a.len() > REMOTE_MAX_STR {
                return Err("artwork url invalid".into());
            }
        }
    }
    let rows: Vec<persistence::repo::RemotePlaylistRow> = tracks
        .into_iter()
        .map(|t| persistence::repo::RemotePlaylistRow {
            key_source: t.key_source,
            key_id: t.key_id,
            title: t.title,
            artist: t.artist,
            album: t.album,
            duration_sec: t.duration_sec,
            artwork_url: t.artwork_url,
        })
        .collect();
    persistence::repo::append_remote_playlist_tracks(&state.db, id, &rows).map_err(err)
}
