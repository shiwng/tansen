use super::{clip, REMOTE_MAX_KEY, REMOTE_MAX_STR};
use crate::{Db, Result};
use rusqlite::{params, OptionalExtension};

/// One playback event: local library rows use `key_source = "local"` and the
/// track id; remote rows keep their catalogue kind/id. `played_ms` is filled
/// when the next entry starts (or playback stops).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ListeningEvent {
    pub id: i64,
    pub key_source: String,
    pub key_id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub artwork_url: Option<String>,
    pub duration_ms: i64,
    pub played_ms: i64,
    pub completed: bool,
    pub started_at: i64,
}

pub fn start_listening_event(
    db: &Db,
    key_source: &str,
    key_id: &str,
    title: &str,
    artist: &str,
    album: Option<&str>,
    artwork_url: Option<&str>,
    duration_ms: i64,
    now: i64,
) -> Result<i64> {
    let conn = db.lock()?;
    conn.execute(
        r#"INSERT INTO listening_events(
               key_source, key_id, title, artist, album, artwork_url,
               duration_ms, played_ms, completed, started_at)
           VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, 0, ?8)"#,
        params![
            clip(key_source, REMOTE_MAX_KEY),
            clip(key_id, REMOTE_MAX_KEY),
            clip(title, REMOTE_MAX_STR),
            clip(artist, REMOTE_MAX_STR),
            album.map(|a| clip(a, REMOTE_MAX_STR)),
            artwork_url.map(|a| clip(a, REMOTE_MAX_STR)),
            duration_ms.max(0),
            now,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Closes an event. `played_ms` is clamped to the track duration; completion
/// means all but the last three seconds were heard.
pub fn finish_listening_event(db: &Db, id: i64, played_ms: i64) -> Result<()> {
    let conn = db.lock()?;
    let duration: i64 = conn
        .query_row(
            "SELECT duration_ms FROM listening_events WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(0);
    let played = if duration > 0 {
        played_ms.clamp(0, duration)
    } else {
        played_ms.max(0)
    };
    let completed = duration > 0 && played + 3_000 >= duration;
    conn.execute(
        "UPDATE listening_events SET played_ms = ?2, completed = ?3 WHERE id = ?1",
        params![id, played, completed as i64],
    )?;
    Ok(())
}

pub fn listening_events(db: &Db, limit: i64) -> Result<Vec<ListeningEvent>> {
    let conn = db.lock()?;
    let mut stmt = conn.prepare(
        "SELECT id, key_source, key_id, title, artist, album, artwork_url,
                duration_ms, played_ms, completed, started_at
         FROM listening_events
         ORDER BY started_at DESC, id DESC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit], |r| {
        Ok(ListeningEvent {
            id: r.get(0)?,
            key_source: r.get(1)?,
            key_id: r.get(2)?,
            title: r.get(3)?,
            artist: r.get(4)?,
            album: r.get(5)?,
            artwork_url: r.get(6)?,
            duration_ms: r.get(7)?,
            played_ms: r.get(8)?,
            completed: r.get::<_, i64>(9)? != 0,
            started_at: r.get(10)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// One (kind, id) pair for the explicit feedback tables.
#[derive(Debug, Clone, serde::Serialize)]
pub struct KeyRef {
    pub key_source: String,
    pub key_id: String,
}

fn key_refs(conn: &rusqlite::Connection, table: &str, order: &str) -> Result<Vec<KeyRef>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT key_source, key_id FROM {table} ORDER BY {order} DESC"
    ))?;
    let rows = stmt.query_map([], |r| {
        Ok(KeyRef {
            key_source: r.get(0)?,
            key_id: r.get(1)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn liked_tracks(db: &Db) -> Result<Vec<KeyRef>> {
    let conn = db.lock()?;
    key_refs(&conn, "liked_tracks", "liked_at")
}

pub fn liked_albums(db: &Db) -> Result<Vec<KeyRef>> {
    let conn = db.lock()?;
    key_refs(&conn, "liked_albums", "liked_at")
}

pub fn set_liked_track(
    db: &Db,
    key_source: &str,
    key_id: &str,
    liked: bool,
    now: i64,
) -> Result<()> {
    set_liked_in(db, "liked_tracks", key_source, key_id, liked, now)
}

pub fn set_liked_album(
    db: &Db,
    key_source: &str,
    key_id: &str,
    liked: bool,
    now: i64,
) -> Result<()> {
    set_liked_in(db, "liked_albums", key_source, key_id, liked, now)
}

fn set_liked_in(
    db: &Db,
    table: &str,
    key_source: &str,
    key_id: &str,
    liked: bool,
    now: i64,
) -> Result<()> {
    let conn = db.lock()?;
    if liked {
        conn.execute(
            &format!(
                "INSERT OR REPLACE INTO {table}(key_source, key_id, liked_at) VALUES(?1, ?2, ?3)"
            ),
            params![
                clip(key_source, REMOTE_MAX_KEY),
                clip(key_id, REMOTE_MAX_KEY),
                now
            ],
        )?;
    } else {
        conn.execute(
            &format!("DELETE FROM {table} WHERE key_source = ?1 AND key_id = ?2"),
            params![
                clip(key_source, REMOTE_MAX_KEY),
                clip(key_id, REMOTE_MAX_KEY)
            ],
        )?;
    }
    Ok(())
}

pub fn followed_artists(db: &Db) -> Result<Vec<(KeyRef, String)>> {
    let conn = db.lock()?;
    let mut stmt = conn
        .prepare("SELECT key_source, key_id, name FROM followed_artists ORDER BY liked_at DESC")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            KeyRef {
                key_source: r.get(0)?,
                key_id: r.get(1)?,
            },
            r.get::<_, String>(2)?,
        ))
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn set_followed_artist(
    db: &Db,
    key_source: &str,
    key_id: &str,
    name: &str,
    followed: bool,
    now: i64,
) -> Result<()> {
    let conn = db.lock()?;
    if followed {
        conn.execute(
            "INSERT OR REPLACE INTO followed_artists(key_source, key_id, name, liked_at)
             VALUES(?1, ?2, ?3, ?4)",
            params![
                clip(key_source, REMOTE_MAX_KEY),
                clip(key_id, REMOTE_MAX_KEY),
                clip(name, REMOTE_MAX_STR),
                now
            ],
        )?;
    } else {
        conn.execute(
            "DELETE FROM followed_artists WHERE key_source = ?1 AND key_id = ?2",
            params![
                clip(key_source, REMOTE_MAX_KEY),
                clip(key_id, REMOTE_MAX_KEY)
            ],
        )?;
    }
    Ok(())
}

pub fn dismissed_recommendations(db: &Db) -> Result<Vec<KeyRef>> {
    let conn = db.lock()?;
    key_refs(&conn, "dismissed_recommendations", "dismissed_at")
}

pub fn set_recommendation_dismissed(
    db: &Db,
    key_source: &str,
    key_id: &str,
    dismissed: bool,
    now: i64,
) -> Result<()> {
    let conn = db.lock()?;
    if dismissed {
        conn.execute(
            "INSERT OR REPLACE INTO dismissed_recommendations(key_source, key_id, dismissed_at)
             VALUES(?1, ?2, ?3)",
            params![
                clip(key_source, REMOTE_MAX_KEY),
                clip(key_id, REMOTE_MAX_KEY),
                now
            ],
        )?;
    } else {
        conn.execute(
            "DELETE FROM dismissed_recommendations WHERE key_source = ?1 AND key_id = ?2",
            params![
                clip(key_source, REMOTE_MAX_KEY),
                clip(key_id, REMOTE_MAX_KEY)
            ],
        )?;
    }
    Ok(())
}

/// Cheap change detector for the cached home feed: event and feedback counts.
pub fn recommendation_counts(db: &Db) -> Result<(i64, i64, i64, i64, i64)> {
    let conn = db.lock()?;
    let count = |table: &str| -> Result<i64> {
        Ok(conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?)
    };
    Ok((
        count("listening_events")?,
        count("liked_tracks")?,
        count("liked_albums")?,
        count("followed_artists")?,
        count("dismissed_recommendations")?,
    ))
}
