//! Grouped views and playback-state persistence on top of the base schema.

use crate::models::TrackRef;
use crate::{Db, Result};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

/// One row of `playback_state` — the recoverable player.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybackState {
    pub tracks: Vec<TrackRef>,
    pub index: Option<usize>,
    pub position_sec: f64,
    pub repeat: String,
}

pub fn save_playback_state(db: &Db, s: &PlaybackState, now: i64) -> Result<()> {
    let json = serde_json::to_string(&s.tracks)
        .map_err(|e| crate::DbError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e)))?;
    let conn = db.lock()?;
    conn.execute(
        "INSERT INTO playback_state(id, tracks_json, index_num, position_sec, repeat_mode, updated_at)
         VALUES(0, ?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET
           tracks_json = excluded.tracks_json,
           index_num = excluded.index_num,
           position_sec = excluded.position_sec,
           repeat_mode = excluded.repeat_mode,
           updated_at = excluded.updated_at",
        params![json, s.index.map(|i| i as i64), s.position_sec, s.repeat, now],
    )?;
    Ok(())
}

pub fn load_playback_state(db: &Db) -> Result<Option<PlaybackState>> {
    let conn = db.lock()?;
    let row = conn
        .query_row(
            "SELECT tracks_json, index_num, position_sec, repeat_mode FROM playback_state WHERE id = 0",
            [],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<i64>>(1)?,
                    r.get::<_, f64>(2)?,
                    r.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?;
    let Some((json, index, position_sec, repeat)) = row else {
        return Ok(None);
    };
    let tracks: Vec<TrackRef> = serde_json::from_str(&json)
        .map_err(|e| crate::DbError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e)))?;
    Ok(Some(PlaybackState {
        tracks,
        index: index.map(|i| i as usize),
        position_sec,
        repeat,
    }))
}

pub fn clear_playback_state(db: &Db) -> Result<()> {
    let conn = db.lock()?;
    conn.execute("DELETE FROM playback_state WHERE id = 0", [])?;
    Ok(())
}

/// One row of `play_history` joined to its track, for the History page.
#[derive(Debug, Clone, Serialize)]
pub struct HistoryEntry {
    pub track: TrackRef,
    pub played_at: i64,
}

pub fn history(db: &Db, limit: i64) -> Result<Vec<HistoryEntry>> {
    let conn = db.lock()?;
    let mut stmt = conn.prepare(
        "SELECT t.id, t.title, t.artist, t.album, t.duration_sec, t.art_path, t.path, h.played_at
         FROM play_history h JOIN tracks t ON t.id = h.track_id
         WHERE t.missing = 0
         ORDER BY h.played_at DESC, h.id DESC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit], |r| {
        Ok(HistoryEntry {
            track: TrackRef {
                id: r.get(0)?,
                title: r.get(1)?,
                artist: r.get(2)?,
                album: r.get(3)?,
                duration_sec: r.get(4)?,
                art_path: r.get(5)?,
                path: r.get(6)?,
                video_id: None,
            },
            played_at: r.get(7)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// An album grouping: everything sharing album_artist + album.
#[derive(Debug, Clone, Serialize)]
pub struct AlbumGroup {
    pub key: String,
    pub album: String,
    pub album_artist: String,
    pub year: Option<i64>,
    pub art_path: Option<String>,
    pub track_count: i64,
    pub duration_sec: f64,
}

pub fn albums(db: &Db) -> Result<Vec<AlbumGroup>> {
    let conn = db.lock()?;
    let mut stmt = conn.prepare(
        "SELECT album_artist || '||' || album AS key,
                CASE WHEN album = '' THEN 'Unknown Album' ELSE album END,
                CASE WHEN album_artist = '' THEN 'Unknown Artist' ELSE album_artist END,
                MAX(year), art_by_count.art_path, COUNT(*), SUM(duration_sec)
         FROM tracks
         LEFT JOIN (SELECT album_artist aa, album al, art_path,
                           ROW_NUMBER() OVER (PARTITION BY album_artist, album ORDER BY art_path IS NULL, id) rn
                    FROM tracks WHERE missing = 0 AND art_path IS NOT NULL) art_by_count
              ON art_by_count.aa = tracks.album_artist AND art_by_count.al = tracks.album AND art_by_count.rn = 1
         WHERE missing = 0
         GROUP BY tracks.album_artist, tracks.album
         ORDER BY CASE WHEN tracks.album_artist = '' THEN 1 ELSE 0 END,
                  tracks.album_artist COLLATE NOCASE, tracks.album COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(AlbumGroup {
            key: r.get(0)?,
            album: r.get(1)?,
            album_artist: r.get(2)?,
            year: r.get(3)?,
            art_path: r.get(4)?,
            track_count: r.get(5)?,
            duration_sec: r.get::<_, Option<f64>>(6)?.unwrap_or(0.0),
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// An artist grouping by album_artist (falling back to track artist).
#[derive(Debug, Clone, Serialize)]
pub struct ArtistGroup {
    pub name: String,
    pub track_count: i64,
    pub album_count: i64,
    pub art_path: Option<String>,
}

pub fn artists(db: &Db) -> Result<Vec<ArtistGroup>> {
    let conn = db.lock()?;
    let mut stmt = conn.prepare(
        "SELECT CASE WHEN album_artist = '' THEN
                CASE WHEN artist = '' THEN 'Unknown Artist' ELSE artist END
             ELSE album_artist END AS name,
             COUNT(*) AS tracks,
             COUNT(DISTINCT CASE WHEN album = '' THEN '||id' || id ELSE album END) AS albums,
             art_by_name.art_path
         FROM tracks
         LEFT JOIN (SELECT COALESCE(NULLIF(album_artist,''), NULLIF(artist,''), 'Unknown Artist') an,
                           art_path,
                           ROW_NUMBER() OVER (PARTITION BY COALESCE(NULLIF(album_artist,''), NULLIF(artist,''), 'Unknown Artist')
                                              ORDER BY art_path IS NULL, id) rn
                    FROM tracks WHERE missing = 0 AND art_path IS NOT NULL) art_by_name
              ON art_by_name.an = CASE WHEN tracks.album_artist = '' THEN
                    CASE WHEN tracks.artist = '' THEN 'Unknown Artist' ELSE tracks.artist END
                  ELSE tracks.album_artist END
              AND art_by_name.rn = 1
         WHERE missing = 0
         GROUP BY name
         ORDER BY name COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(ArtistGroup {
            name: r.get(0)?,
            track_count: r.get(1)?,
            album_count: r.get(2)?,
            art_path: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Tracks of one album group, in disc/track order. Key format: `artist||album`.
pub fn album_tracks(db: &Db, key: &str) -> Result<Vec<TrackRef>> {
    let (artist, album) = key.split_once("||").unwrap_or(("", key));
    let conn = db.lock()?;
    let mut stmt = conn.prepare(
        "SELECT id, title, artist, album, duration_sec, art_path, path
         FROM tracks
         WHERE missing = 0 AND album_artist = ?1 AND album = ?2
         ORDER BY disc_no, track_no, title COLLATE NOCASE",
    )?;
    let rows = stmt.query_map(params![artist, album], |r| {
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

/// Tracks by one artist (album_artist or track artist), albums grouped.
pub fn artist_tracks(db: &Db, name: &str) -> Result<Vec<TrackRef>> {
    let conn = db.lock()?;
    let mut stmt = conn.prepare(
        "SELECT id, title, artist, album, duration_sec, art_path, path
         FROM tracks
         WHERE missing = 0 AND (album_artist = ?1 OR (album_artist = '' AND artist = ?1))
         ORDER BY album COLLATE NOCASE, disc_no, track_no, title COLLATE NOCASE",
    )?;
    let rows = stmt.query_map(params![name], |r| {
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
