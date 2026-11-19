use super::{clip, REMOTE_MAX_STR};
use crate::{Db, Result};
use rusqlite::{params, OptionalExtension, Row};

/// One row of the persistent download queue.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    pub id: i64,
    pub kind: String,
    pub source: String,
    pub track_id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_sec: f64,
    pub artwork_url: Option<String>,
    pub status: String,
    pub progress: i64,
    pub total: i64,
    pub path: Option<String>,
    pub error: Option<String>,
    pub quality: String,
    pub created_at: i64,
    pub updated_at: i64,
    /// Filled by the download manager's snapshot, never stored: the real
    /// on-disk size of `path`, or `None` when the file is missing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

fn download_from_row(r: &Row<'_>) -> rusqlite::Result<Download> {
    Ok(Download {
        id: r.get(0)?,
        kind: r.get(1)?,
        source: r.get(2)?,
        track_id: r.get(3)?,
        title: r.get(4)?,
        artist: r.get(5)?,
        album: r.get(6)?,
        duration_sec: r.get(7)?,
        artwork_url: r.get(8)?,
        status: r.get(9)?,
        progress: r.get(10)?,
        total: r.get(11)?,
        path: r.get(12)?,
        error: r.get(13)?,
        quality: r.get(14)?,
        created_at: r.get(15)?,
        updated_at: r.get(16)?,
        size_bytes: None,
    })
}

const DOWNLOAD_COLUMNS: &str = "id, kind, source, track_id, title, artist, album, duration_sec,
    artwork_url, status, progress, total, path, error, quality, created_at, updated_at";

pub fn list_downloads(db: &Db) -> Result<Vec<Download>> {
    let conn = db.lock()?;
    let mut stmt = conn.prepare(&format!(
        "SELECT {DOWNLOAD_COLUMNS} FROM downloads ORDER BY created_at DESC, id DESC"
    ))?;
    let rows = stmt.query_map([], download_from_row)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn get_download(db: &Db, id: i64) -> Result<Option<Download>> {
    let conn = db.lock()?;
    Ok(conn
        .query_row(
            &format!("SELECT {DOWNLOAD_COLUMNS} FROM downloads WHERE id = ?1"),
            params![id],
            download_from_row,
        )
        .optional()?)
}

pub fn find_download(
    db: &Db,
    source: &str,
    track_id: &str,
    quality: &str,
) -> Result<Option<Download>> {
    let conn = db.lock()?;
    Ok(conn
        .query_row(
            &format!(
                "SELECT {DOWNLOAD_COLUMNS} FROM downloads
                 WHERE source = ?1 AND track_id = ?2 AND quality = ?3"
            ),
            params![source, track_id, quality],
            download_from_row,
        )
        .optional()?)
}

#[allow(clippy::too_many_arguments, reason = "one row, explicit fields")]
pub fn insert_download(
    db: &Db,
    kind: &str,
    source: &str,
    track_id: &str,
    title: &str,
    artist: &str,
    album: Option<&str>,
    duration_sec: f64,
    artwork_url: Option<&str>,
    quality: &str,
    now: i64,
) -> Result<i64> {
    let conn = db.lock()?;
    conn.execute(
        "INSERT INTO downloads(
             kind, source, track_id, title, artist, album, duration_sec,
             artwork_url, status, progress, total, quality, created_at, updated_at)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'queued', 0, 0, ?9, ?10, ?10)",
        params![
            kind,
            source,
            track_id,
            clip(title, REMOTE_MAX_STR),
            clip(artist, REMOTE_MAX_STR),
            album.map(|a| clip(a, REMOTE_MAX_STR)),
            duration_sec,
            artwork_url.map(|a| clip(a, REMOTE_MAX_STR)),
            quality,
            now,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn set_download_status(
    db: &Db,
    id: i64,
    status: &str,
    error: Option<&str>,
    now: i64,
) -> Result<()> {
    let conn = db.lock()?;
    conn.execute(
        "UPDATE downloads SET status = ?2, error = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, status, error, now],
    )?;
    Ok(())
}

pub fn set_download_progress(db: &Db, id: i64, progress: i64, total: i64, now: i64) -> Result<()> {
    let conn = db.lock()?;
    conn.execute(
        "UPDATE downloads SET progress = ?2, total = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, progress, total, now],
    )?;
    Ok(())
}

pub fn finish_download(db: &Db, id: i64, path: &str, total: i64, now: i64) -> Result<()> {
    let conn = db.lock()?;
    conn.execute(
        "UPDATE downloads SET status = 'complete', path = ?2, progress = ?3, total = ?3,
             error = NULL, updated_at = ?4 WHERE id = ?1",
        params![id, path, total, now],
    )?;
    Ok(())
}

pub fn reset_download(db: &Db, id: i64, now: i64) -> Result<()> {
    let conn = db.lock()?;
    conn.execute(
        "UPDATE downloads SET status = 'queued', progress = 0, total = 0, path = NULL,
             error = NULL, updated_at = ?2 WHERE id = ?1",
        params![id, now],
    )?;
    Ok(())
}

pub fn delete_download(db: &Db, id: i64) -> Result<()> {
    let conn = db.lock()?;
    conn.execute("DELETE FROM downloads WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn clear_finished_downloads(db: &Db) -> Result<Vec<String>> {
    let conn = db.lock()?;
    let mut stmt = conn.prepare(
        "SELECT path FROM downloads
         WHERE status IN ('complete', 'failed', 'cancelled') AND path IS NOT NULL",
    )?;
    let paths: Vec<String> = stmt
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    conn.execute(
        "DELETE FROM downloads WHERE status IN ('complete', 'failed', 'cancelled')",
        [],
    )?;
    Ok(paths)
}

/// Rows that were mid-flight when the app exited are restarted on launch.
pub fn requeue_interrupted_downloads(db: &Db, now: i64) -> Result<()> {
    let conn = db.lock()?;
    conn.execute(
        "UPDATE downloads SET status = 'queued', updated_at = ?1
         WHERE status = 'downloading'",
        params![now],
    )?;
    Ok(())
}
