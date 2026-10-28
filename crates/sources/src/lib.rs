//! Music sources: one small trait every catalogue answers to, and the
//! adapters that implement it. A direct port of the upstream `MusicSource.kt`
//! contract — `health`, `search`, `stream` — with the quality tiers carried
//! verbatim so the resolver's rules (phase 4's next step) translate.

pub mod addon;
pub mod registry;
pub mod yt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    /// User-added HTTP addon servers (the manifest/search/stream protocol).
    Addon,
    /// YouTube Music (fixed base source; PoToken gate pending).
    YouTube,
}

impl SourceKind {
    /// The try-order among kinds; user-added sources sort by rank first.
    pub fn rank(self) -> u8 {
        match self {
            SourceKind::Addon => 0,
            SourceKind::YouTube => 3,
        }
    }
}

/// How much of the stream the caller is willing to pay for (upstream
/// `StreamRequest`): not a settings value — the resolver derives it from the
/// connection and the user's ceiling at the moment a track starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum StreamRequest {
    /// Bit-exact, whatever it costs. A source that can't should say so.
    Lossless,
    /// A transcode at or below this ceiling.
    Capped { max_kbps: u32 },
    /// Whatever the source considers its best lossy rendition.
    Best,
}

/// What a stream is, as far as choosing between copies goes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamFormat {
    /// Codec/container shorthand for the UI ("FLAC", "MP4", "Opus").
    pub codec: String,
    /// None = unknown (never treated as lossless, never treated as lossy).
    pub is_lossless: Option<bool>,
    pub kbps: Option<u32>,
    pub is_dolby_atmos: bool,
}

impl StreamFormat {
    /// Better-than comparison for copies of the same recording (upstream
    /// `isBetter`): Atmos first, lossless second, bitrate third.
    pub fn is_better_than(&self, current: Option<&StreamFormat>) -> bool {
        let Some(current) = current else { return true };
        if self.is_dolby_atmos != current.is_dolby_atmos {
            return self.is_dolby_atmos;
        }
        if self.is_lossless != current.is_lossless {
            return self.is_lossless == Some(true);
        }
        (self.kbps.unwrap_or(0)) > (current.kbps.unwrap_or(0))
    }
}

/// An openable stream. `below_request` marks a settle-for: playing it is
/// right, but a second look during playback may find better (upstream
/// `QualityUpgrade`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceStream {
    pub url: String,
    pub format: StreamFormat,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    pub below_request: bool,
    /// The catalogue's duration for this recording, when it says.
    #[serde(default)]
    pub duration_sec: Option<u32>,
}

/// Reachability vs credentials: a down server is worth retrying and stays
/// enabled; one that refused is neither (upstream `SourceHealth`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum SourceHealth {
    Ok { detail: Option<String> },
    Unreachable { reason: String },
    Rejected { reason: String },
}

/// One track row from a source's catalogue, tagged with this source's id so
/// playing it comes back to the same source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogTrack {
    /// This source's own id for the track (opaque; passed back to `stream`).
    pub track_id: String,
    pub config_id: String,
    pub title: String,
    pub artist: String,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub duration_sec: Option<u32>,
    #[serde(default)]
    pub artwork_url: Option<String>,
    /// The tier the row advertises, when the catalogue grades its rows.
    #[serde(default)]
    pub format: Option<StreamFormat>,
    /// The catalogue's own answer to "how do I open this", when included in
    /// search rows (addons may embed `streamURL`); usually resolved later.
    #[serde(default)]
    pub embedded_stream_url: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("network: {0}")]
    Network(String),
    #[error("bad response: {0}")]
    Protocol(String),
    #[error("source is disabled")]
    Disabled,
}

/// One place tracks can come from. Deliberately small: search its catalogue,
/// turn one of its own ids into something openable (upstream `MusicSource`).
#[async_trait::async_trait]
pub trait MusicSource: Send + Sync {
    fn config_id(&self) -> &str;
    fn kind(&self) -> SourceKind;
    fn display_name(&self) -> &str;

    async fn health(&self) -> Result<SourceHealth, SourceError>;

    /// Tracks matching `query`, tagged with this source's id.
    async fn search(&self, query: &str, limit: usize, request: Option<&StreamRequest>)
        -> Result<Vec<CatalogTrack>, SourceError>;

    /// An openable stream for the source's own `track_id`, or `None` when the
    /// source turns out not to have it (a miss, not an error).
    async fn stream(&self, track_id: &str, request: &StreamRequest)
        -> Result<Option<SourceStream>, SourceError>;
}
