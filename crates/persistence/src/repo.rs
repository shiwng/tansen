//! Typed queries over the schema in `lib.rs`.

use crate::models::{Folder, Track, TrackRef};
use crate::{Db, Result};
use rusqlite::{params, OptionalExtension, Row};

pub fn get_setting(db: &Db, key: &str) -> Result<Option<String>> {
    let conn = db.lock()?;
    Ok(conn
        .query_row("SELECT value FROM settings WHERE key = ?1", params![key], |r| r.get(0))
        .optional()?)
}

pub fn set_setting(db: &Db, key: &str, value: &str) -> Result<()> {
    let conn = db.lock()?;
    conn.execute(
        "INSERT INTO settings(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn list_settings(db: &Db) -> Result<Vec<(String, String)>> {
    let conn = db.lock()?;
    let mut stmt = conn.prepare("SELECT key, value FROM settings ORDER BY key")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn add_folder(db: &Db, path: &str, now: i64) -> Result<i64> {
    let conn = db.lock()?;
    conn.execute(
        "INSERT INTO library_folders(path, added_at) VALUES(?1, ?2)
         ON CONFLICT(path) DO UPDATE SET path = excluded.path",
        params![path, now],
    )?;
    Ok(conn.query_row("SELECT id FROM library_folders WHERE path = ?1", params![path], |r| r.get(0))?)
}

pub fn remove_folder(db: &Db, id: i64) -> Result<()> {
    let conn = db.lock()?;
    conn.execute("DELETE FROM library_folders WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn list_folders(db: &Db) -> Result<Vec<Folder>> {
    let conn = db.lock()?;
    let mut stmt = conn.prepare("SELECT id, path FROM library_folders ORDER BY id")?;
    let rows = stmt.query_map([], |r| {
        Ok(Folder { id: r.get(0)?, path: r.get(1)? })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Stored stat tuple for a library path: (mtime_sec, size_bytes, missing).
/// The scanner compares it against the filesystem to skip unchanged files
/// without parsing metadata (M12).
pub fn stat_by_path(db: &Db, path: &str) -> Result<Option<(i64, i64, bool)>> {
    let conn = db.lock()?;
    conn.query_row(
        "SELECT mtime_sec, size_bytes, missing FROM tracks WHERE path = ?1",
        params![path],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .optional()
    .map_err(Into::into)
}

/// One file's write for a scan batch: the parsed row plus its stat tuple and
/// artwork destination.
pub struct TrackWrite {
    pub track: Track,
    pub folder_id: i64,
    pub mtime: i64,
    pub size: i64,
    pub art_path: Option<String>,
}

/// Upserts a whole scan batch inside ONE transaction (M12): the per-file
/// autocommit pairs (upsert + art update) become a single commit per batch.
/// Returns the ids in input order.
pub fn upsert_batch(db: &Db, items: &[TrackWrite], now: i64) -> Result<Vec<i64>> {
    let mut conn = db.lock()?;
    let tx = conn.transaction()?;
    let mut ids = Vec::with_capacity(items.len());
    for item in items {
        let id = upsert_track_in(&tx, &item.track, item.folder_id, item.mtime, item.size, now)?;
        set_art_path_in(&tx, id, item.art_path.as_deref())?;
        ids.push(id);
    }
    tx.commit()?;
    Ok(ids)
}

/// Track rows for a list of library ids (M11): the queue-rows query the
/// frontend drives on `queue_rev` changes. Returns one row per distinct id
/// found, in input order, skipping missing rows. Chunked like `mark_missing`
/// so the IN list stays bounded.
/// F1: capped read-only LIKE search over the local library (title,
/// artist, album). LIKE metacharacters in the query are escaped.
pub fn search_local(db: &Db, query: &str, limit: usize) -> Result<Vec<Track>> {
    let conn = db.lock()?;
    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let pattern = format!("%{escaped}%");
    let mut stmt = conn.prepare(
        "SELECT * FROM tracks WHERE title LIKE ?1 ESCAPE '\\' \
         OR artist LIKE ?1 ESCAPE '\\' \
         OR album LIKE ?1 ESCAPE '\\' \
         ORDER BY title LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![pattern, limit.clamp(1, 200) as i64], track_from_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn tracks_by_ids(db: &Db, ids: &[i64]) -> Result<Vec<Track>> {
    let conn = db.lock()?;
    let mut by_id: std::collections::HashMap<i64, Track> = std::collections::HashMap::new();
    for chunk in ids.chunks(500) {
        let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!("SELECT * FROM tracks WHERE id IN ({placeholders})");
        let mut stmt = conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::ToSql> = chunk.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
        let rows = stmt.query_map(params.as_slice(), track_from_row)?;
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

pub fn get_track(db: &Db, id: i64) -> Result<Option<Track>> {
    let conn = db.lock()?;
    conn.query_row("SELECT * FROM tracks WHERE id = ?1", params![id], track_from_row)
        .optional()
        .map_err(Into::into)
}

pub fn get_track_by_path(db: &Db, path: &str) -> Result<Option<Track>> {
    let conn = db.lock()?;
    conn.query_row("SELECT * FROM tracks WHERE path = ?1", params![path], track_from_row)
        .optional()
        .map_err(Into::into)
}

/// All track ids+paths in a folder, including missing ones (rescan input).
pub fn track_paths_in_folder(db: &Db, folder_id: i64) -> Result<Vec<(i64, String)>> {
    let conn = db.lock()?;
    let mut stmt = conn.prepare("SELECT id, path FROM tracks WHERE folder_id = ?1")?;
    let rows = stmt.query_map(params![folder_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn list_tracks(db: &Db) -> Result<Vec<Track>> {
    let conn = db.lock()?;
    let mut stmt = conn.prepare(
        "SELECT * FROM tracks WHERE missing = 0
         ORDER BY album_artist COLLATE NOCASE, album COLLATE NOCASE, disc_no, track_no, title COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([], track_from_row)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Upsert one scanned file by path. Returns the track id.
pub fn upsert_track(db: &Db, t: &Track, folder_id: i64, mtime: i64, size: i64, now: i64) -> Result<i64> {
    let conn = db.lock()?;
    upsert_track_in(&conn, t, folder_id, mtime, size, now)
}

/// Transaction-scoped variant used by the scan batch (M12).
pub fn upsert_track_in(
    conn: &rusqlite::Connection,
    t: &Track,
    folder_id: i64,
    mtime: i64,
    size: i64,
    now: i64,
) -> Result<i64> {
    conn.execute(
        r#"INSERT INTO tracks(
               folder_id, path, file_name, title, artist, album, album_artist,
               track_no, disc_no, year, genre, duration_sec, codec, bitrate_kbps,
               sample_rate, bit_depth, channels, mtime_sec, size_bytes, missing,
               added_at, scanned_at)
           VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,0,?20,?20)
           ON CONFLICT(path) DO UPDATE SET
               folder_id = excluded.folder_id,
               file_name = excluded.file_name,
               title     = excluded.title,
               artist    = excluded.artist,
               album     = excluded.album,
               album_artist = excluded.album_artist,
               track_no  = excluded.track_no,
               disc_no   = excluded.disc_no,
               year      = excluded.year,
               genre     = excluded.genre,
               duration_sec = excluded.duration_sec,
               codec     = excluded.codec,
               bitrate_kbps = excluded.bitrate_kbps,
               sample_rate  = excluded.sample_rate,
               bit_depth    = excluded.bit_depth,
               channels  = excluded.channels,
               -- artwork stays cached across rescans unless tags changed it;
               -- library decides via art_dirty.
               mtime_sec = excluded.mtime_sec,
               size_bytes = excluded.size_bytes,
               missing   = 0,
               scanned_at = excluded.scanned_at"#,
        params![
            folder_id,
            t.path,
            t.file_name,
            t.title,
            t.artist,
            t.album,
            t.album_artist,
            t.track_no,
            t.disc_no,
            t.year,
            t.genre,
            t.duration_sec,
            t.codec,
            t.bitrate_kbps,
            t.sample_rate,
            t.bit_depth,
            t.channels,
            mtime,
            size,
            now,
        ],
    )?;
    Ok(conn.query_row("SELECT id FROM tracks WHERE path = ?1", params![t.path], |r| r.get(0))?)
}

/// Marks tracks whose file no longer exists. Returns affected ids.
pub fn mark_missing(db: &Db, ids: &[i64], missing: bool) -> Result<usize> {
    let conn = db.lock()?;
    let mut total = 0;
    for chunk in ids.chunks(500) {
        let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!("UPDATE tracks SET missing = ? WHERE id IN ({placeholders})");
        let mut p: Vec<&dyn rusqlite::ToSql> = vec![&missing];
        for id in chunk {
            p.push(id);
        }
        total += conn.execute(&sql, p.as_slice())?;
    }
    Ok(total)
}

pub fn purge_missing(db: &Db) -> Result<usize> {
    let conn = db.lock()?;
    conn.execute("DELETE FROM tracks WHERE missing = 1", [])
        .map_err(Into::into)
}

/// Tracks whose artwork needs (re)extraction after a scan changed their mtime.
pub fn dirty_art_tracks(db: &Db) -> Result<Vec<(i64, String, i64)>> {
    let conn = db.lock()?;
    let mut stmt = conn.prepare(
        "SELECT id, path, mtime_sec FROM tracks WHERE art_dirty = 1 AND missing = 0",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn set_art_path(db: &Db, track_id: i64, art_path: Option<&str>) -> Result<()> {
    let conn = db.lock()?;
    set_art_path_in(&conn, track_id, art_path)
}

/// Transaction-scoped variant used by the scan batch (M12).
pub fn set_art_path_in(
    conn: &rusqlite::Connection,
    track_id: i64,
    art_path: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE tracks SET art_path = ?2, art_dirty = 0 WHERE id = ?1",
        params![track_id, art_path],
    )?;
    Ok(())
}

pub fn record_play(db: &Db, track_id: i64, now: i64) -> Result<()> {
    let conn = db.lock()?;
    conn.execute(
        "INSERT INTO play_history(track_id, played_at) VALUES(?1, ?2)",
        params![track_id, now],
    )?;
    Ok(())
}

fn track_from_row(r: &Row<'_>) -> rusqlite::Result<Track> {    Ok(Track {
        id: r.get("id")?,
        folder_id: r.get("folder_id")?,
        path: r.get("path")?,
        file_name: r.get("file_name")?,
        title: r.get("title")?,
        artist: r.get("artist")?,
        album: r.get("album")?,
        album_artist: r.get("album_artist")?,
        track_no: r.get("track_no")?,
        disc_no: r.get("disc_no")?,
        year: r.get("year")?,
        genre: r.get("genre")?,
        duration_sec: r.get("duration_sec")?,
        codec: r.get("codec")?,
        bitrate_kbps: r.get("bitrate_kbps")?,
        sample_rate: r.get("sample_rate")?,
        bit_depth: r.get("bit_depth")?,
        channels: r.get("channels")?,
        art_path: r.get("art_path")?,
        missing: r.get::<_, i64>("missing")? != 0,
    })
}

/// Clears and rewrites one playlist's ordered contents.
pub fn set_playlist_tracks(db: &Db, playlist_id: i64, track_ids: &[i64]) -> Result<()> {
    let mut conn = db.lock()?;
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM playlist_tracks WHERE playlist_id = ?1", params![playlist_id])?;
    for (pos, id) in track_ids.iter().enumerate() {
        tx.execute(
            "INSERT INTO playlist_tracks(playlist_id, position, track_id) VALUES(?1, ?2, ?3)",
            params![playlist_id, pos as i64, id],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Appends tracks to a playlist's end. Returns the new length.
pub fn append_playlist_tracks(db: &Db, playlist_id: i64, track_ids: &[i64]) -> Result<i64> {
    let mut conn = db.lock()?;
    let tx = conn.transaction()?;
    let next: i64 = tx
        .query_row(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM playlist_tracks WHERE playlist_id = ?1",
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

pub fn playlist_tracks(db: &Db, playlist_id: i64) -> Result<Vec<TrackRef>> {
    let conn = db.lock()?;
    let mut stmt = conn.prepare(
        "SELECT t.id, t.title, t.artist, t.album, t.duration_sec, t.art_path, t.path
         FROM playlist_tracks pt JOIN tracks t ON t.id = pt.track_id
         WHERE pt.playlist_id = ?1 AND t.missing = 0
         ORDER BY pt.position",
    )?;
    let rows = stmt.query_map(params![playlist_id], |r| {
        Ok(TrackRef {
            id: r.get(0)?,
            title: r.get(1)?,
            artist: r.get(2)?,
            album: r.get(3)?,
            duration_sec: r.get(4)?,
            art_path: r.get(5)?,
            path: r.get(6)?,
            video_id: None,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Creates a playlist; returns its id. Empty name falls back to "New Playlist".
pub fn create_playlist(db: &Db, name: &str, now: i64) -> Result<i64> {
    let conn = db.lock()?;
    let name = if name.trim().is_empty() { "New Playlist" } else { name };
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

pub fn list_playlists(db: &Db) -> Result<Vec<(i64, String)>> {
    let conn = db.lock()?;
    let mut stmt = conn.prepare("SELECT id, name FROM playlists ORDER BY id")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Conversions between row counts and ids for tests.
pub fn count(db: &Db, table: &str) -> Result<i64> {
    // Table name comes from call sites only; never user input.
    let conn = db.lock()?;
    let sql = format!("SELECT COUNT(*) FROM {table}");
    conn.query_row(&sql, [], |r| r.get(0)).map_err(Into::into)
}

// ---- S6: remote track metadata -------------------------------------------

/// Bound on remote metadata text columns (S6). Oversized values are
/// truncated, never rejected — the UI should not break on a hostile title.
pub const REMOTE_MAX_STR: usize = 512;
pub const REMOTE_MAX_KEY: usize = 1024;

fn clip(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
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
        params![clip(key_source, REMOTE_MAX_KEY), clip(key_id, REMOTE_MAX_KEY)],
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
        let params: Vec<&dyn rusqlite::ToSql> = chunk.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
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

/// One remote-track row by its (source kind, id) key — the S7 resolver's
/// target-metadata lookup for fallback matching.
pub fn get_remote_track_by_key(db: &Db, key_source: &str, key_id: &str) -> Result<Option<RemoteTrack>> {
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
/// (source-keyed) track (S6). `kind` tells the frontend which identity
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

/// The queue display query: rows for BOTH kinds in one call (S6). Local ids
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
            album: if t.album.is_empty() { None } else { Some(t.album) },
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
