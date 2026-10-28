//! The sidecar-backed YouTube handle: the launched source plus the DTOs the
//! Tauri commands and the resolve-ahead loop share.

use sources::yt::YtMusicSource;
use sources::{MusicSource as _, StreamRequest};
use std::sync::Arc;

pub struct YtHandle {
    pub source: Arc<YtMusicSource>,
}

/// One catalogue track as the frontend sends it and the queue stores it.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct RemoteTrack {
    pub video_id: String,
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub duration_sec: f64,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlayMode {
    /// Replace the queue and play from `start`.
    Load,
    /// Insert after the current track.
    InsertNext,
    /// Add to the end of the queue.
    Append,
}

/// Search results for the UI: the subset of `Song` the track rows render.
#[derive(Debug, serde::Serialize)]
pub struct CatalogRow {
    pub video_id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_sec: Option<f64>,
    pub thumbnail_url: Option<String>,
}

/// Await a future from sync code. Callers here are Tauri sync commands and
/// the resolve-ahead thread — no ambient runtime, so a one-shot
/// current-thread runtime is enough (and block_on from inside one would
/// panic loudly if that ever changes; fix at the call site then).
fn await_sync<F: std::future::Future>(fut: F) -> Result<F::Output, String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    Ok(rt.block_on(fut))
}

impl YtHandle {
    /// Blocking stream resolve (sidecar round trip, ~1–2 s). Never the audio
    /// thread.
    pub fn stream_blocking(&self, video_id: &str) -> Result<sources::SourceStream, String> {
        let source = self.source.clone();
        let id = video_id.to_string();
        let result = await_sync(async move { source.stream(&id, &StreamRequest::Best).await })
            .map_err(|e| e.to_string())?;
        result
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "no stream for track".to_string())
    }

    /// Mixed search across the catalogue.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<CatalogRow>, String> {
        let rows = await_sync(self.source.search(query, limit, None))
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|t| CatalogRow {
                video_id: t.track_id,
                title: t.title,
                artist: t.artist,
                album: t.album,
                duration_sec: t.duration_sec.map(|d| d as f64),
                thumbnail_url: t.artwork_url,
            })
            .collect())
    }

    /// Health read for the Settings screen.
    pub fn health(&self) -> Result<sources::SourceHealth, String> {
        await_sync(self.source.health())
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())
    }
}
