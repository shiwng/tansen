use super::tracks::tracks_by_ids;
use super::{clip, REMOTE_MAX_KEY, REMOTE_MAX_STR};
use crate::models::{PlaylistMeta, TrackRef};
use crate::{Db, Result};
use rusqlite::{params, OptionalExtension};

/// Clears and rewrites one playlist's ordered contents.
pub fn set_playlist_tracks(db: &Db, playlist_id: i64, track_ids: &[i64]) -> Result<()> {
    let mut conn = db.lock()?;
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM playlist_tracks WHERE playlist_id = ?1",
        params![playlist_id],
    )?;
    for (pos, id) in track_ids.iter().enumerate() {
        tx.execute(
            "INSERT INTO playlist_tracks(playlist_id, position, track_id) VALUES(?1, ?2, ?3)",
            params![playlist_id, pos as i64, id],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Appends tracks to a playlist's end. Returns the new length. The next
/// position seeds after the max of BOTH membership tables, so local appends
/// to an imported (remote-member) playlist stay after the imported rows.
pub fn append_playlist_tracks(db: &Db, playlist_id: i64, track_ids: &[i64]) -> Result<i64> {
    let mut conn = db.lock()?;
    let tx = conn.transaction()?;
    let next: i64 = tx
        .query_row(
            "SELECT COALESCE(MAX(pos) + 1, 0) FROM (
                 SELECT MAX(position) AS pos FROM playlist_tracks WHERE playlist_id = ?1
                 UNION ALL
                 SELECT MAX(position) AS pos FROM playlist_remote_tracks WHERE playlist_id = ?1
             )",
            params![playlist_id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    for (offset, id) in track_ids.iter().enumerate() {
        tx.execute(
            "INSERT INTO playlist_tracks(playlist_id, position, track_id) VALUES(?1, ?2, ?3)",
            params![playlist_id, next + offset as i64, id],
        )?;
    }
    tx.commit()?;
    Ok(next + track_ids.len() as i64)
}

/// One playlist's ordered contents across BOTH membership tables: local
/// library tracks (`playlist_tracks`) and remote rows (Spotify import,
/// `playlist_remote_tracks`). The tables share one position sequence per
/// playlist — every append seeds after the max of both — so a single
/// ORDER BY position merges them. Missing local files are skipped.
pub fn playlist_tracks(db: &Db, playlist_id: i64) -> Result<Vec<TrackRef>> {
    let conn = db.lock()?;
    let mut stmt = conn.prepare(
        "SELECT pt.position, t.id, t.title, t.artist, t.album, t.duration_sec, t.art_path, t.path,
                NULL, NULL, NULL
         FROM playlist_tracks pt JOIN tracks t ON t.id = pt.track_id
         WHERE pt.playlist_id = ?1 AND t.missing = 0
         UNION ALL
         SELECT r.position, 0, r.title, r.artist, COALESCE(r.album, ''), r.duration_sec, NULL, '',
                r.key_id, r.key_source, r.artwork_url
         FROM playlist_remote_tracks r
         WHERE r.playlist_id = ?1
         ORDER BY position",
    )?;
    let rows = stmt.query_map(params![playlist_id], |r| {
        Ok(TrackRef {
            id: r.get(1)?,
            title: r.get(2)?,
            artist: r.get(3)?,
            album: r.get(4)?,
            duration_sec: r.get(5)?,
            art_path: r.get(6)?,
            path: r.get(7)?,
            video_id: r.get(8)?,
            key_source: r.get(9)?,
            artwork_url: r.get(10)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Creates a playlist; returns its id. Empty name falls back to "New Playlist".
pub fn create_playlist(db: &Db, name: &str, now: i64) -> Result<i64> {
    let conn = db.lock()?;
    let name = if name.trim().is_empty() {
        "New Playlist"
    } else {
        name
    };
    conn.execute(
        "INSERT INTO playlists(name, created_at) VALUES(?1, ?2)",
        params![name, now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn rename_playlist(db: &Db, playlist_id: i64, name: &str) -> Result<()> {
    let conn = db.lock()?;
    conn.execute(
        "UPDATE playlists SET name = ?2 WHERE id = ?1",
        params![playlist_id, name],
    )?;
    Ok(())
}

pub fn delete_playlist(db: &Db, playlist_id: i64) -> Result<()> {
    let conn = db.lock()?;
    conn.execute("DELETE FROM playlists WHERE id = ?1", params![playlist_id])?;
    Ok(())
}

pub fn list_playlists(db: &Db) -> Result<Vec<PlaylistMeta>> {
    let conn = db.lock()?;
    let mut stmt = conn.prepare(
        "SELECT p.id, p.name, p.favourite, p.created_at,
                (SELECT COUNT(*) FROM playlist_tracks WHERE playlist_id = p.id)
              + (SELECT COUNT(*) FROM playlist_remote_tracks WHERE playlist_id = p.id)
         FROM playlists p
         ORDER BY p.id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(PlaylistMeta {
            id: r.get(0)?,
            name: r.get(1)?,
            favourite: r.get(2)?,
            created_at: r.get(3)?,
            track_count: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Marks a playlist as a favourite (the All Playlists "Only Favourites" filter).
pub fn set_playlist_favourite(db: &Db, playlist_id: i64, favourite: bool) -> Result<()> {
    let conn = db.lock()?;
    conn.execute(
        "UPDATE playlists SET favourite = ?1 WHERE id = ?2",
        params![favourite, playlist_id],
    )?;
    Ok(())
}

/// One remote-track row (display + identity).
#[derive(Debug, Clone)]
pub struct RemoteTrack {
    pub id: i64,
    pub key_source: String,
    pub key_id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_sec: f64,
    pub artwork_url: Option<String>,
}

/// Upserts a remote track's metadata; returns its row id. Single writer
/// path (the app layer calls this before sending the Command).
pub fn upsert_remote_track(
    db: &Db,
    key_source: &str,
    key_id: &str,
    title: &str,
    artist: &str,
    album: Option<&str>,
    duration_sec: f64,
    artwork_url: Option<&str>,
) -> Result<i64> {
    let conn = db.lock()?;
    conn.execute(
        r#"INSERT INTO remote_tracks(
               key_source, key_id, title, artist, album, duration_sec, artwork_url)
           VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
           ON CONFLICT(key_source, key_id) DO UPDATE SET
               title = excluded.title,
               artist = excluded.artist,
               album = excluded.album,
               duration_sec = excluded.duration_sec,
               artwork_url = excluded.artwork_url"#,
        params![
            clip(key_source, REMOTE_MAX_KEY),
            clip(key_id, REMOTE_MAX_KEY),
            clip(title, REMOTE_MAX_STR),
            clip(artist, REMOTE_MAX_STR),
            album.map(|a| clip(a, REMOTE_MAX_STR)),
            duration_sec,
            artwork_url.map(|a| clip(a, REMOTE_MAX_STR)),
        ],
    )?;
    conn.query_row(
        "SELECT id FROM remote_tracks WHERE key_source = ?1 AND key_id = ?2",
        params![
            clip(key_source, REMOTE_MAX_KEY),
            clip(key_id, REMOTE_MAX_KEY)
        ],
        |r| r.get(0),
    )
    .map_err(Into::into)
}

/// Display rows for remote entries by their row ids. Chunked like
/// `tracks_by_ids` so the IN list stays bounded; input order preserved,
/// duplicates collapsed, unknown ids skipped.
pub fn remote_tracks_by_ids(db: &Db, ids: &[i64]) -> Result<Vec<RemoteTrack>> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let conn = db.lock()?;
    let mut by_id: std::collections::HashMap<i64, RemoteTrack> = std::collections::HashMap::new();
    for chunk in ids.chunks(500) {
        let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, key_source, key_id, title, artist, album, duration_sec, artwork_url \
             FROM remote_tracks WHERE id IN ({placeholders})"
        );
        let mut stmt = conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::ToSql> =
            chunk.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
        let rows = stmt.query_map(params.as_slice(), |r| {
            Ok(RemoteTrack {
                id: r.get(0)?,
                key_source: r.get(1)?,
                key_id: r.get(2)?,
                title: r.get(3)?,
                artist: r.get(4)?,
                album: r.get(5)?,
                duration_sec: r.get(6)?,
                artwork_url: r.get(7)?,
            })
        })?;
        for row in rows {
            let t = row?;
            by_id.insert(t.id, t);
        }
    }
    let mut seen = std::collections::HashSet::new();
    Ok(ids
        .iter()
        .filter(|id| seen.insert(**id))
        .filter_map(|id| by_id.get(id).cloned())
        .collect())
}

/// One remote-track row by its (source kind, id) key — the resolver's
/// target-metadata lookup for fallback matching.
pub fn get_remote_track_by_key(
    db: &Db,
    key_source: &str,
    key_id: &str,
) -> Result<Option<RemoteTrack>> {
    let conn = db.lock()?;
    conn.query_row(
        "SELECT id, key_source, key_id, title, artist, album, duration_sec, artwork_url \
         FROM remote_tracks WHERE key_source = ?1 AND key_id = ?2",
        params![key_source, key_id],
        |r| {
            Ok(RemoteTrack {
                id: r.get(0)?,
                key_source: r.get(1)?,
                key_id: r.get(2)?,
                title: r.get(3)?,
                artist: r.get(4)?,
                album: r.get(5)?,
                duration_sec: r.get(6)?,
                artwork_url: r.get(7)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

/// One display row for a queue entry — local library track or remote
/// (source-keyed) track. `kind` tells the frontend which identity
/// space the row belongs to (`local` / `remote`).
#[derive(Debug, Clone, serde::Serialize)]
pub struct QueueDisplayRow {
    pub kind: &'static str,
    /// Local: `tracks.id`; remote: `remote_tracks.id` — the EntryId the
    /// engine holds and the frontend mirrors.
    pub id: i64,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_sec: f64,
    /// Local rows: art cache path (rendered via `art://`).
    pub art_path: Option<String>,
    /// Remote rows: catalogue artwork URL (never fetched directly; the app
    /// routes it through `fetch_artwork`'s host allowlist).
    pub artwork_url: Option<String>,
    /// Remote rows: the source's own track id (the TrackKey id).
    pub video_id: Option<String>,
    /// Remote rows: the source kind (`youtube` / `addon`).
    pub key_source: Option<String>,
}

/// The queue display query: rows for BOTH kinds in one call. Local ids
/// resolve through `tracks_by_ids` (missing rows dropped), remote ids through
/// `remote_tracks_by_ids`; each group keeps its input order.
pub fn queue_rows_by_ids(db: &Db, ids: &[i64], remote_ids: &[i64]) -> Result<Vec<QueueDisplayRow>> {
    let mut rows = Vec::with_capacity(ids.len() + remote_ids.len());
    for t in tracks_by_ids(db, ids)? {
        if t.missing {
            continue;
        }
        rows.push(QueueDisplayRow {
            kind: "local",
            id: t.id,
            title: t.title,
            artist: t.artist,
            album: if t.album.is_empty() {
                None
            } else {
                Some(t.album)
            },
            duration_sec: t.duration_sec,
            art_path: t.art_path,
            artwork_url: None,
            video_id: None,
            key_source: None,
        });
    }
    for r in remote_tracks_by_ids(db, remote_ids)? {
        rows.push(QueueDisplayRow {
            kind: "remote",
            id: r.id,
            title: r.title,
            artist: r.artist,
            album: r.album,
            duration_sec: r.duration_sec,
            art_path: None,
            artwork_url: r.artwork_url,
            video_id: Some(r.key_id),
            key_source: Some(r.key_source),
        });
    }
    Ok(rows)
}

/// Stored-playlist remote membership (Spotify import).

/// One remote row of a stored playlist: either a matched catalogue track
/// (`key_source = "youtube"`, `key_id` = video id — playable) or an imported
/// Spotify metadata row (`key_source = "spotify"`, `key_id` = Spotify track
/// id — displayed as unavailable, never played).
pub struct RemotePlaylistRow {
    pub key_source: String,
    pub key_id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_sec: f64,
    pub artwork_url: Option<String>,
}

/// Appends remote rows to a playlist, seeded after the max position of both
/// membership tables. Text is length-bounded (clipped, never rejected).
/// Returns the number of rows written.
pub fn append_remote_playlist_tracks(
    db: &Db,
    playlist_id: i64,
    rows: &[RemotePlaylistRow],
) -> Result<usize> {
    let mut conn = db.lock()?;
    let tx = conn.transaction()?;
    let next: i64 = tx
        .query_row(
            "SELECT COALESCE(MAX(pos) + 1, 0) FROM (
                 SELECT MAX(position) AS pos FROM playlist_tracks WHERE playlist_id = ?1
                 UNION ALL
                 SELECT MAX(position) AS pos FROM playlist_remote_tracks WHERE playlist_id = ?1
             )",
            params![playlist_id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    for (offset, row) in rows.iter().enumerate() {
        tx.execute(
            "INSERT INTO playlist_remote_tracks(
                 playlist_id, position, key_source, key_id, title, artist, album,
                 duration_sec, artwork_url)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                playlist_id,
                next + offset as i64,
                clip(&row.key_source, REMOTE_MAX_KEY),
                clip(&row.key_id, REMOTE_MAX_KEY),
                clip(&row.title, REMOTE_MAX_STR),
                clip(&row.artist, REMOTE_MAX_STR),
                row.album.as_deref().map(|a| clip(a, REMOTE_MAX_STR)),
                row.duration_sec,
                row.artwork_url.as_deref().map(|a| clip(a, REMOTE_MAX_STR)),
            ],
        )?;
    }
    tx.commit()?;
    Ok(rows.len())
}
