//! Row types shared between persistence, app-core and the frontend (via serde).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: i64,
    pub folder_id: i64,
    pub path: String,
    pub file_name: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub track_no: Option<i64>,
    pub disc_no: Option<i64>,
    pub year: Option<i64>,
    pub genre: String,
    pub duration_sec: f64,
    pub codec: String,
    pub bitrate_kbps: Option<i64>,
    pub sample_rate: Option<i64>,
    pub bit_depth: Option<i64>,
    pub channels: Option<i64>,
    pub art_path: Option<String>,
    pub missing: bool,
}

/// What a queue entry needs to identify and display a track.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackRef {
    pub id: i64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_sec: f64,
    pub art_path: Option<String>,
    pub path: String,
    /// The catalogue video id for remote rows (id is 0 for those). Lets a
    /// restored queue re-resolve expired stream URLs.
    #[serde(default)]
    pub video_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Folder {
    pub id: i64,
    pub path: String,
}

impl From<Track> for TrackRef {
    fn from(t: Track) -> Self {
        Self {
            id: t.id,
            title: t.title,
            artist: t.artist,
            album: t.album,
            duration_sec: t.duration_sec,
            art_path: t.art_path,
            path: t.path,
            video_id: None,
        }
    }
}
