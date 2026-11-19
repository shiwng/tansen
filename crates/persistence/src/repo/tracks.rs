use crate::models::{Folder, Track};
use crate::{Db, Result};
use rusqlite::{params, OptionalExtension, Row};

/// Cached loudness measurement for a local file: (gain_db, measured_at).
pub fn get_track_loudness(db: &Db, path: &str) -> Result<Option<(f32, i64)>> {
    let conn = db.lock()?;
    Ok(conn
        .query_row(
            "SELECT gain_db, measured_at FROM track_loudness WHERE path = ?1",
            params![path],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

pub fn set_track_loudness(
    db: &Db,
    path: &str,
    gain_db: f32,
    integrated_lufs: f32,
    peak: f32,
    measured_at: i64,
) -> Result<()> {
    let conn = db.lock()?;
    conn.execute(
        "INSERT INTO track_loudness(path, gain_db, integrated_lufs, peak, measured_at)
         VALUES(?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(path) DO UPDATE SET gain_db = excluded.gain_db,
             integrated_lufs = excluded.integrated_lufs, peak = excluded.peak,
             measured_at = excluded.measured_at",
        params![
            path,
            f64::from(gain_db),
            f64::from(integrated_lufs),
            f64::from(peak),
            measured_at
        ],
    )?;
    Ok(())
}

pub fn add_folder(db: &Db, path: &str, now: i64) -> Result<i64> {
    let conn = db.lock()?;
    conn.execute(
        "INSERT INTO library_folders(path, added_at) VALUES(?1, ?2)
         ON CONFLICT(path) DO UPDATE SET path = excluded.path",
        params![path, now],
    )?;
    Ok(conn.query_row(
        "SELECT id FROM library_folders WHERE path = ?1",
        params![path],
        |r| r.get(0),
    )?)
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
        Ok(Folder {
            id: r.get(0)?,
            path: r.get(1)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Stored stat tuple for a library path: (mtime_sec, size_bytes, missing).
/// The scanner compares it against the filesystem to skip unchanged files
/// without parsing metadata.
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

/// Every stored path's stat tuple, keyed by path. A scan reads this once
/// instead of taking the connection lock per file inside the parallel walk.
pub fn track_stats(db: &Db) -> Result<std::collections::HashMap<String, (i64, i64, bool)>> {
    let conn = db.lock()?;
    let mut stmt = conn.prepare("SELECT path, mtime_sec, size_bytes, missing FROM tracks")?;
    let rows = stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?, r.get(3)?)))
    })?;
    let mut out = std::collections::HashMap::new();
    for row in rows {
        let (path, stat) = row?;
        out.insert(path, stat);
    }
    Ok(out)
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

/// Upserts a whole scan batch inside ONE transaction: the per-file
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

/// Track rows for a list of library ids: the queue-rows query the
/// frontend drives on `queue_rev` changes. Returns one row per distinct id
/// found, in input order, skipping missing rows. Chunked like `mark_missing`
/// so the IN list stays bounded.
/// Capped read-only LIKE search over the local library (title,
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
        let params: Vec<&dyn rusqlite::ToSql> =
            chunk.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
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
    conn.query_row(
        "SELECT * FROM tracks WHERE id = ?1",
        params![id],
        track_from_row,
    )
    .optional()
    .map_err(Into::into)
}

pub fn get_track_by_path(db: &Db, path: &str) -> Result<Option<Track>> {
    let conn = db.lock()?;
    conn.query_row(
        "SELECT * FROM tracks WHERE path = ?1",
        params![path],
        track_from_row,
    )
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

/// Candidate rows for the resolver's local-copy match: only tracks whose
/// duration falls inside the matcher's tolerance window. The match itself
/// still runs `sources::matcher` over these rows, so this changes no identity
/// rule — it just stops every cold remote resolve from loading the library.
pub fn tracks_in_duration_window(db: &Db, min_sec: f64, max_sec: f64) -> Result<Vec<Track>> {
    let conn = db.lock()?;
    let mut stmt =
        conn.prepare("SELECT * FROM tracks WHERE missing = 0 AND duration_sec BETWEEN ?1 AND ?2")?;
    let rows = stmt.query_map(params![min_sec, max_sec], track_from_row)?;
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

/// The newest additions to the library, newest first. `added_at` is set once
/// at first index and kept across rescans, so this is additions, not edits.
pub fn recently_added(db: &Db, limit: i64) -> Result<Vec<Track>> {
    let conn = db.lock()?;
    let mut stmt = conn.prepare(
        "SELECT * FROM tracks WHERE missing = 0
         ORDER BY added_at DESC, id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit.clamp(1, 500)], track_from_row)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Upsert one scanned file by path. Returns the track id.
pub fn upsert_track(
    db: &Db,
    t: &Track,
    folder_id: i64,
    mtime: i64,
    size: i64,
    now: i64,
) -> Result<i64> {
    let conn = db.lock()?;
    upsert_track_in(&conn, t, folder_id, mtime, size, now)
}

/// Transaction-scoped variant used by the scan batch.
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
    Ok(conn.query_row(
        "SELECT id FROM tracks WHERE path = ?1",
        params![t.path],
        |r| r.get(0),
    )?)
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
    let mut stmt =
        conn.prepare("SELECT id, path, mtime_sec FROM tracks WHERE art_dirty = 1 AND missing = 0")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn set_art_path(db: &Db, track_id: i64, art_path: Option<&str>) -> Result<()> {
    let conn = db.lock()?;
    set_art_path_in(&conn, track_id, art_path)
}

/// Transaction-scoped variant used by the scan batch.
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

fn track_from_row(r: &Row<'_>) -> rusqlite::Result<Track> {
    Ok(Track {
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
