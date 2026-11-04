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

/// S10 discovery typeahead rows: playable tracks plus browse cards.
#[derive(Debug, serde::Serialize)]
pub struct DiscoverySuggest {
    pub tracks: Vec<sources::yt::DiscoveryTrack>,
    pub cards: Vec<sources::yt::DiscoveryCard>,
    pub queries: Vec<String>,
    pub continuation: Option<String>,
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
        let key = sources::TrackKey::new(sources::SourceKind::YouTube, id)
            .map_err(|e| e.to_string())?;
        let result = await_sync(async move { source.stream(&key, &StreamRequest::Best).await })
            .map_err(|e| e.to_string())?;
        result
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "no stream for track".to_string())
    }

    /// Mixed search across the catalogue.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<CatalogRow>, String> {
        let rows = await_sync(self.source.search(query, limit))
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

    // ---- S10: discovery (read-only, unauthenticated upstream) ----------------

    /// Blocking typeahead read (sidecar round trip, per-call deadline in the
    /// client). Unauthenticated: no account sign-in, cookies or tokens.
    pub fn suggest(&self, query: &str) -> Result<DiscoverySuggest, String> {
        let page = await_sync(self.source.suggest(query))
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        Ok(DiscoverySuggest {
            tracks: page.tracks,
            cards: page.cards,
            queries: page.queries,
            continuation: page.continuation,
        })
    }

    /// Blocking album-page read (first page, capped).
    pub fn album(&self, id: &str) -> Result<sources::yt::DiscoveryPage, String> {
        await_sync(self.source.album(id))
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())
    }

    /// Blocking artist-page read.
    pub fn artist(&self, id: &str) -> Result<sources::yt::DiscoveryPage, String> {
        await_sync(self.source.artist(id))
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())
    }

    /// Blocking playlist-page read.
    pub fn playlist(&self, id: &str) -> Result<sources::yt::DiscoveryPage, String> {
        await_sync(self.source.playlist(id))
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())
    }

    pub fn search_page(
        &self,
        query: &str,
        filter: &str,
        continuation: Option<String>,
    ) -> Result<sources::yt::DiscoverySearchPage, String> {
        let continuation = continuation.as_deref();
        await_sync(self.source.search_page(query, filter, continuation))
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())
    }

    pub fn browse_continuation(&self, token: &str) -> Result<sources::yt::DiscoveryPage, String> {
        await_sync(self.source.browse_continuation(token))
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())
    }

    pub fn home(&self) -> Result<sources::yt::DiscoveryHome, String> {
        await_sync(self.source.home())
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())
    }

    pub fn home_continuation(&self, token: &str) -> Result<sources::yt::DiscoveryHome, String> {
        await_sync(self.source.home_continuation(token))
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())
    }

    pub fn explore(&self, section: &str) -> Result<sources::yt::DiscoveryHome, String> {
        await_sync(self.source.explore(section))
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())
    }
}
