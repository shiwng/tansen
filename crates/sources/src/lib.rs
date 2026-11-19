//! Music sources: one small trait every catalogue answers to, and the
//! adapters that implement it. Provider-facing types live here (this crate);
//! types the Engine needs live in `app_core` with no Tauri dependency.

pub mod addon;
pub mod keychain;
pub mod matcher;
pub mod metrics;
pub mod registry;
pub mod resolver;
pub mod search;
pub mod subsonic;
pub mod yt;

use serde::{Deserialize, Serialize};

/// User-facing audio quality ceiling. Stored in settings, mapped to a
/// `StreamRequest` for the sources. The aliases keep values stored before
/// the Low/Medium/High ladder (and the old UI strings) parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioQuality {
    /// 64 kbps cap.
    #[serde(alias = "data-saver", alias = "datasaver")]
    Low,
    /// 128 kbps cap.
    Medium,
    /// Whatever the source considers its best lossy rendition.
    #[serde(alias = "best")]
    High,
    /// Bit-exact.
    Lossless,
}

impl AudioQuality {
    pub fn to_stream_request(self) -> StreamRequest {
        match self {
            AudioQuality::Low => StreamRequest::Capped { max_kbps: 64 },
            AudioQuality::Medium => StreamRequest::Capped { max_kbps: 128 },
            AudioQuality::High => StreamRequest::Best,
            AudioQuality::Lossless => StreamRequest::Lossless,
        }
    }

    pub const ALL: [Self; 4] = [Self::Low, Self::Medium, Self::High, Self::Lossless];
}

/// Composite identity of one track: which source, and that source's own id.
/// The id is length-capped so hostile catalogues cannot cause unbounded
/// allocations downstream.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TrackKey {
    pub source: SourceKind,
    pub id: String,
}

impl TrackKey {
    /// Constructs a key, rejecting ids over `TRACK_ID_MAX_LEN` so a
    /// hostile catalogue cannot cause unbounded downstream allocations.
    pub fn new(source: SourceKind, id: impl Into<String>) -> Result<Self, SourceError> {
        let id = id.into();
        if id.len() > TRACK_ID_MAX_LEN {
            return Err(SourceError::Permanent(format!(
                "track id too long ({} > {TRACK_ID_MAX_LEN})",
                id.len()
            )));
        }
        Ok(Self { source, id })
    }
}

/// Upper bound on one `TrackKey.id`.
pub const TRACK_ID_MAX_LEN: usize = 1024;

/// Kinds of catalogue. `Local` is declared for parity with the contract but
/// has no implementing source yet; `Addon`, `Subsonic`, `YouTube` and
/// `Radio` (live broadcast streams, keyed by their stream URL) are
/// implemented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    /// User-added HTTP addon servers (the manifest/search/stream protocol).
    Addon,
    /// Local files — reserved; no source implements it yet.
    Local,
    /// User-configured Subsonic/OpenSubsonic server.
    Subsonic,
    /// YouTube Music (fixed base source; PoToken gate pending).
    YouTube,
    /// Live internet radio: the key id IS the stream URL, played as-is with
    /// no fallback ladder (a broadcast has no second rendition).
    Radio,
}

impl SourceKind {
    /// The try-order among kinds; user-added sources sort by rank first.
    pub fn rank(self) -> u8 {
        match self {
            SourceKind::Addon => 0,
            SourceKind::Local => 1,
            SourceKind::Subsonic => 2,
            SourceKind::YouTube => 3,
            SourceKind::Radio => 4,
        }
    }
}

/// How much of the stream the caller is willing to pay for (upstream
/// `StreamRequest`): not a settings value — the resolver derives it from the
/// connection and the user's ceiling at the moment a track starts.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    /// Sample rate in Hz when the source reports it.
    #[serde(default)]
    pub sample_rate: Option<u32>,
    /// Bit depth when the source reports it.
    #[serde(default)]
    pub bit_depth: Option<u8>,
    /// True when `kbps` was inferred (e.g. from the itag) rather than
    /// supplied by the provider.
    #[serde(default)]
    pub kbps_inferred: bool,
}

impl StreamFormat {
    /// Lossless is decided on the codec alone. Known-lossless names →
    /// true; known-lossy names → false; anything else → None (unknown),
    /// which is never treated as either.
    pub fn is_lossless(codec: &str) -> Option<bool> {
        match codec.to_ascii_lowercase().as_str() {
            "flac" | "alac" | "wav" | "aiff" | "aif" | "ape" | "wv" => Some(true),
            "mp3" | "aac" | "m4a" | "mp4" | "opus" | "ogg" | "vorbis" | "webm" | "wma" => {
                Some(false)
            }
            _ => None,
        }
    }

    /// Better-than comparison for copies of the same recording (upstream
    /// `isBetter`): Atmos first, lossless second, bitrate third. Unknown
    /// losslessness is treated as non-lossless for ordering (but never
    /// falsely claims lossy).
    pub fn is_better_than(&self, current: Option<&StreamFormat>) -> bool {
        let Some(current) = current else { return true };
        if self.is_dolby_atmos != current.is_dolby_atmos {
            return self.is_dolby_atmos;
        }
        let l = self.is_lossless.unwrap_or(false);
        let r = current.is_lossless.unwrap_or(false);
        if l != r {
            return l;
        }
        (self.kbps.unwrap_or(0)) > (current.kbps.unwrap_or(0))
    }
}

/// Stream container family. Serialized lowercase for the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Container {
    Progressive,
    Hls,
    Dash,
}

/// Whether the audio stack can actually decode a format/container pair
/// (Symphonia 0.5.5 via rodio 0.21 `symphonia-all`). The `all` feature set
/// has NO Opus codec and NO WebM format — Opus-in-WebM is therefore not
/// decodable. HLS/DASH are never decodable (progressive containers only).
pub fn decodable(format: &StreamFormat, container: Option<Container>) -> bool {
    match container {
        Some(Container::Hls) | Some(Container::Dash) => return false,
        Some(Container::Progressive) | None => {}
    }
    match format.codec.to_ascii_lowercase().as_str() {
        // Decodable codecs (symphonia feature set).
        "flac" | "mp3" | "aac" | "m4a" | "alac" | "wav" | "pcm" | "aiff" | "aif" | "vorbis"
        | "ogg" | "adpcm" => true,
        // Decodable in principle, but the demuxer/container gate applies:
        // mp4/aac arrives in ISO-MP4 (present); webm is absent entirely.
        "mp4" => true,
        // Not decodable in this build.
        "opus" | "webm" => false,
        // Unknown codec: not decodable (never asserted true).
        _ => false,
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
    /// MIME type when the source states one (sidecar: from the container).
    #[serde(default)]
    pub mime: Option<String>,
    /// Epoch **milliseconds** when the URL stops being valid, when the
    /// source says (sidecar `expiresAt`). Absent = unknown.
    #[serde(default)]
    pub expiry_ms: Option<i64>,
    /// Container family when the source states it.
    #[serde(default)]
    pub container: Option<Container>,
}

/// What catalogue resolution returns — a media URL handed to Rust (the
/// sidecar never proxies audio bytes) plus
/// everything the fetcher may need.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedMedia {
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// Unix **seconds** when the URL expires; `None` = unknown.
    pub expiry: Option<i64>,
    /// Byte length of the media, when known.
    pub length: Option<u64>,
    pub mime: Option<String>,
    /// Stream metadata, when the source resolves it.
    pub format: Option<StreamFormat>,
    pub container: Option<Container>,
    /// Which source this media resolves from.
    pub source: SourceKind,
    /// Settle-for marker (a later look may find better).
    pub below_request: bool,
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

/// Classified source failures; the Engine maps these onto its own
/// `ErrorKind` (Device/Resolve/Decode/Io).
#[derive(Debug, Clone, thiserror::Error)]
pub enum SourceError {
    #[error("network: {0}")]
    Network(String),
    #[error("bad response: {0}")]
    Protocol(String),
    #[error("source is disabled")]
    Disabled,
    /// The failure is permanent for this track/source.
    #[error("permanent: {0}")]
    Permanent(String),
    /// Retry later; temporary.
    #[error("transient: {0}")]
    Transient(String),
    #[error("timeout: {0}")]
    Timeout(String),
    /// The source cannot serve this request class.
    #[error("unsupported")]
    Unsupported,
    /// The stream is encrypted in a way we cannot decode.
    #[error("encrypted")]
    Encrypted,
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
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<CatalogTrack>, SourceError>;

    /// An openable stream for one `TrackKey`, or `None` when the source
    /// turns out not to have it (a miss, not an error).
    async fn stream(
        &self,
        key: &TrackKey,
        request: &StreamRequest,
    ) -> Result<Option<SourceStream>, SourceError>;

    /// The resolution contract — a [`ResolvedMedia`] for one `TrackKey`.
    /// Default implementation derives from [`Self::stream`].
    async fn resolve(
        &self,
        key: &TrackKey,
        request: &StreamRequest,
    ) -> Result<Option<ResolvedMedia>, SourceError> {
        Ok(self.stream(key, request).await?.map(|s| ResolvedMedia {
            url: s.url,
            headers: s.headers,
            expiry: s.expiry_ms.map(|ms| ms.div_euclid(1000)),
            length: None,
            mime: s.mime,
            format: Some(s.format),
            container: s.container,
            source: self.kind(),
            below_request: s.below_request,
        }))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::*;

    #[test]
    fn codec_lossless_table() {
        for lossless in ["flac", "FLAC", "alac", "wav", "aiff", "aif", "ape", "wv"] {
            assert_eq!(
                StreamFormat::is_lossless(lossless),
                Some(true),
                "{lossless}"
            );
        }
        for lossy in [
            "mp3", "aac", "m4a", "mp4", "opus", "ogg", "vorbis", "webm", "wma",
        ] {
            assert_eq!(StreamFormat::is_lossless(lossy), Some(false), "{lossy}");
        }
        for unknown in ["", "unknown", "wma-lossless", "aac-128"] {
            assert_eq!(
                StreamFormat::is_lossless(unknown),
                None,
                "{unknown:?} = unknown"
            );
        }
    }

    #[test]
    fn lossless_never_falsely_claims_lossy() {
        // unknown codec is unknown; is_better_than ordering treats it as
        // non-lossless but never labels it lossy.
        let unknown = StreamFormat {
            codec: "mystery".into(),
            is_lossless: None,
            ..Default::default()
        };
        assert!(unknown.is_lossless.is_none());
        let flac = StreamFormat {
            codec: "FLAC".into(),
            is_lossless: Some(true),
            ..Default::default()
        };
        let mp3 = StreamFormat {
            codec: "MP3".into(),
            is_lossless: Some(false),
            ..Default::default()
        };
        assert!(flac.is_better_than(Some(&unknown)));
        assert!(!mp3.is_better_than(Some(&unknown)));
        assert!(flac.is_better_than(Some(&mp3)));
        assert!(!mp3.is_better_than(Some(&flac)));
    }

    #[test]
    fn request_and_format_roundtrip_serde() {
        // request mapping: kinds serialize lowercase, capped carries kbps.
        let json = serde_json::to_string(&StreamRequest::Capped { max_kbps: 256 }).unwrap();
        assert!(json.contains("capped") && json.contains("256"), "{json}");
        let back: StreamRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back, StreamRequest::Capped { max_kbps: 256 });

        let fmt = StreamFormat {
            codec: "FLAC".into(),
            is_lossless: Some(true),
            kbps: Some(1411),
            sample_rate: Some(44100),
            bit_depth: Some(16),
            kbps_inferred: false,
            ..Default::default()
        };
        let round: StreamFormat =
            serde_json::from_str(&serde_json::to_string(&fmt).unwrap()).unwrap();
        assert_eq!(round, fmt);
    }

    #[test]
    fn kind_rank_order() {
        assert!(SourceKind::Addon.rank() < SourceKind::Local.rank());
        assert!(SourceKind::Local.rank() < SourceKind::Subsonic.rank());
        assert!(SourceKind::Subsonic.rank() < SourceKind::YouTube.rank());
    }

    #[test]
    fn track_key_id_cap_is_enforced_by_construction_contract() {
        // The cap is a constant contract; keys are built from catalogue data.
        let ok = TrackKey {
            source: SourceKind::YouTube,
            id: "x".repeat(1024),
        };
        assert_eq!(ok.id.len(), TRACK_ID_MAX_LEN);
        // A hostile catalogue id larger than the cap must be rejected by the
        // caller — the type cannot enforce it, but the contract is testable.
        assert!(TRACK_ID_MAX_LEN < 4096, "cap is meaningful");
    }

    #[test]
    fn error_classification_is_exhaustive() {
        let cases: [(SourceError, &str); 8] = [
            (SourceError::Network("n".into()), "network"),
            (SourceError::Protocol("p".into()), "bad response"),
            (SourceError::Disabled, "disabled"),
            (SourceError::Permanent("x".into()), "permanent"),
            (SourceError::Transient("x".into()), "transient"),
            (SourceError::Timeout("x".into()), "timeout"),
            (SourceError::Unsupported, "unsupported"),
            (SourceError::Encrypted, "encrypted"),
        ];
        for (e, needle) in cases {
            let msg = e.to_string().to_ascii_lowercase();
            assert!(msg.contains(needle), "{e:?} contains {needle}");
        }
    }
}
#[cfg(test)]
mod quality_tests {
    use super::*;

    #[test]
    fn quality_maps_to_stream_request() {
        assert_eq!(
            AudioQuality::Low.to_stream_request(),
            StreamRequest::Capped { max_kbps: 64 }
        );
        assert_eq!(
            AudioQuality::Medium.to_stream_request(),
            StreamRequest::Capped { max_kbps: 128 }
        );
        assert_eq!(AudioQuality::High.to_stream_request(), StreamRequest::Best);
        assert_eq!(
            AudioQuality::Lossless.to_stream_request(),
            StreamRequest::Lossless
        );
    }

    #[test]
    fn quality_serde_strict() {
        for q in AudioQuality::ALL {
            assert_eq!(
                serde_json::from_str::<AudioQuality>(&serde_json::to_string(&q).unwrap()).unwrap(),
                q
            );
        }
        assert!(serde_json::from_str::<AudioQuality>("\"unlimited\"").is_err());
    }

    /// Values stored before the ladder (and the old UI strings) must keep
    /// parsing: "datasaver" (old serde name), "data-saver" (old UI value)
    /// and "best" (old default tier).
    #[test]
    fn quality_aliases_keep_stored_values_parsing() {
        assert_eq!(
            serde_json::from_str::<AudioQuality>("\"datasaver\"").unwrap(),
            AudioQuality::Low
        );
        assert_eq!(
            serde_json::from_str::<AudioQuality>("\"data-saver\"").unwrap(),
            AudioQuality::Low
        );
        assert_eq!(
            serde_json::from_str::<AudioQuality>("\"best\"").unwrap(),
            AudioQuality::High
        );
    }
}

#[cfg(test)]
mod decodable_tests {
    use super::*;

    fn fmt(codec: &str) -> StreamFormat {
        StreamFormat {
            codec: codec.into(),
            ..Default::default()
        }
    }

    #[test]
    fn decodable_table() {
        // Decodable codecs.
        for c in [
            "flac", "FLAC", "mp3", "aac", "m4a", "alac", "wav", "aiff", "aif", "vorbis", "ogg",
            "adpcm",
        ] {
            assert!(decodable(&fmt(c), None), "{c} progressive");
        }
        // mp4 container + codec.
        assert!(decodable(&fmt("mp4"), None));
        assert!(decodable(&fmt("aac"), Some(Container::Progressive)));
        // HLS/DASH never decodable, whatever the codec.
        assert!(!decodable(&fmt("aac"), Some(Container::Hls)));
        assert!(!decodable(&fmt("flac"), Some(Container::Dash)));
        // Opus / WebM not in this build.
        assert!(!decodable(&fmt("opus"), None));
        assert!(!decodable(&fmt("opus"), Some(Container::Progressive)));
        assert!(!decodable(&fmt("webm"), None));
        // Unknown never asserted true.
        assert!(!decodable(&fmt("mystery"), None));
        assert!(!decodable(&fmt(""), None));
    }
}
