//! The addon HTTP adapter: a plain JSON protocol, nothing executed
//! client-side (upstream `AddonClient.kt` / `AddonModels.kt`).
//!
//! - `GET {base}/manifest.json` → identity, declared resources, settings.
//! - `GET {base}/search?q=&quality=` → `AddonSearchResponse{tracks}`.
//! - `GET {base}/stream/{id}?quality=` → `AddonStream{url, format, …}`.
//!
//! Repeated calls are deduped in-memory (manifest/search 10 min, stream
//! 5 min), mirroring upstream's SharedCalls, and per-instance 429s open a
//! short quiet period so a rate-limited server is not hammered.

use crate::{
    CatalogTrack, MusicSource, SourceError, SourceHealth, SourceKind, SourceStream, StreamFormat,
    StreamRequest,
};
use async_trait::async_trait;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const MANIFEST_TTL: Duration = Duration::from_secs(10 * 60);
const SEARCH_TTL: Duration = Duration::from_secs(10 * 60);
const STREAM_TTL: Duration = Duration::from_secs(5 * 60);
const QUIET_AFTER_429: Duration = Duration::from_secs(60);
const TIMEOUT: Duration = Duration::from_secs(12);

#[derive(Debug, Clone, Deserialize)]
pub struct AddonManifest {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    pub resources: Vec<String>,
    #[serde(default)]
    pub allow_downloads: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AddonTrack {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default, alias = "artworkURL", alias = "artwork_url")]
    pub artwork_url: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub quality: Option<String>,
    #[serde(default)]
    pub codec: Option<String>,
    #[serde(default)]
    pub bitrate: Option<u32>,
    #[serde(default)]
    pub sample_rate: Option<u32>,
    #[serde(default)]
    pub bit_depth: Option<u32>,
    /// Some addons embed the answer directly on the row.
    #[serde(default, alias = "stream_url")]
    pub stream_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AddonSearchResponse {
    pub tracks: Vec<AddonTrack>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AddonStream {
    pub url: String,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub quality: Option<String>,
    #[serde(default)]
    pub codec: Option<String>,
    #[serde(default)]
    pub bitrate: Option<u32>,
    #[serde(default)]
    pub sample_rate: Option<u32>,
    #[serde(default)]
    pub bit_depth: Option<u32>,
}

pub struct AddonSource {
    config_id: String,
    display_name: String,
    base: String,
    http: reqwest::Client,
    /// Manifests / searches / streams already fetched, by key.
    cache: Mutex<HashMap<String, (Instant, String)>>,
    /// Set when the server answers 429: requests are refused until it passes.
    quiet_until: Mutex<Option<Instant>>,
}

impl AddonSource {
    pub fn new(config_id: impl Into<String>, display_name: impl Into<String>, base: impl Into<String>) -> Self {
        let base: String = base.into();
        Self {
            config_id: config_id.into(),
            display_name: display_name.into(),
            base: base.trim_end_matches('/').to_string(),
            http: reqwest::Client::builder()
                .timeout(TIMEOUT)
                .user_agent("BitChordDesktop/0.2 (https://github.com/bitchord)")
                .build()
                .expect("reqwest client"),
            cache: Mutex::new(HashMap::new()),
            quiet_until: Mutex::new(None),
        }
    }

    fn base(&self) -> &str {
        &self.base
    }

    /// Cached GET: one request per key per TTL, however many callers ask.
    async fn get_cached(&self, key: &str, url: &str, ttl: Duration) -> Result<String, SourceError> {
        {
            let cache = self.cache.lock().unwrap();
            if let Some((at, body)) = cache.get(key) {
                if at.elapsed() < ttl {
                    return Ok(body.clone());
                }
            }
        }
        let body = self.get_uncached(url).await?;
        self.cache.lock().unwrap().insert(key.to_string(), (Instant::now(), body.clone()));
        Ok(body)
    }

    async fn get_uncached(&self, url: &str) -> Result<String, SourceError> {
        if let Some(until) = *self.quiet_until.lock().unwrap() {
            if until > Instant::now() {
                return Err(SourceError::Network("rate limited (quiet period)".into()));
            }
        }
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| SourceError::Network(e.to_string()))?;
        match response.status().as_u16() {
            200..=299 => Ok(response.text().await.map_err(|e| SourceError::Network(e.to_string()))?),
            429 => {
                *self.quiet_until.lock().unwrap() = Some(Instant::now() + QUIET_AFTER_429);
                Err(SourceError::Network("429 rate limited".into()))
            }
            code => Err(SourceError::Protocol(format!("HTTP {code} from {url}"))),
        }
    }

    async fn manifest(&self) -> Result<AddonManifest, SourceError> {
        let body = self
            .get_cached("manifest", &format!("{}/manifest.json", self.base()), MANIFEST_TTL)
            .await?;
        serde_json::from_str(&body).map_err(|e| SourceError::Protocol(format!("manifest.json: {e}")))
    }

    /// Manifest fetch for registration-time validation (no health wrap).
    pub async fn probe_manifest(&self) -> Result<AddonManifest, SourceError> {
        self.manifest().await
    }

    fn format_of(track: &AddonTrack) -> StreamFormat {
        let is_lossless = track
            .format
            .as_deref()
            .map(|f| matches!(f.to_ascii_uppercase().as_str(), "FLAC" | "ALAC" | "WAV" | "APE" | "WV" | "AIFF"));
        StreamFormat {
            codec: track
                .format
                .clone()
                .or_else(|| track.codec.clone())
                .unwrap_or_default()
                .to_ascii_uppercase(),
            is_lossless,
            kbps: track.bitrate,
            is_dolby_atmos: track
                .quality
                .as_deref()
                .is_some_and(|q| q.eq_ignore_ascii_case("dolby-atmos")),
        }
    }

    fn track_of(&self, t: AddonTrack) -> CatalogTrack {
        CatalogTrack {
            track_id: t.id.clone(),
            config_id: self.config_id.clone(),
            title: t.title.clone(),
            artist: t.artist.clone().unwrap_or_default(),
            album: t.album.clone(),
            duration_sec: t.duration.map(|d| d.round() as u32),
            artwork_url: t.artwork_url.clone(),
            format: Some(Self::format_of(&t)),
            embedded_stream_url: t.stream_url.clone(),
        }
    }

    /// The tier an outgoing stream request maps to on the wire, when the
    /// source grades its rows (upstream: `HIGH` for Best, `LOSSLESS` for
    /// lossless, a kbps ceiling otherwise).
    fn quality_param(request: &StreamRequest) -> Option<String> {
        match request {
            StreamRequest::Lossless => Some("lossless".into()),
            StreamRequest::Best => Some("high".into()),
            StreamRequest::Capped { max_kbps } => Some(format!("capped-{max_kbps}")),
        }
    }
}

#[async_trait]
impl MusicSource for AddonSource {
    fn config_id(&self) -> &str {
        &self.config_id
    }

    fn kind(&self) -> SourceKind {
        SourceKind::Addon
    }

    fn display_name(&self) -> &str {
        &self.display_name
    }

    async fn health(&self) -> Result<SourceHealth, SourceError> {
        match self.manifest().await {
            Ok(m) => {
                // A manifest that doesn't declare search+stream can answer,
                // but it cannot serve the player — that's a rejection, not a
                // network problem, and the two must not look alike.
                if !m.resources.iter().any(|r| r == "search") || !m.resources.iter().any(|r| r == "stream") {
                    Ok(SourceHealth::Rejected {
                        reason: "manifest does not declare search and stream".into(),
                    })
                } else {
                    Ok(SourceHealth::Ok { detail: Some(m.name.unwrap_or(m.id)) })
                }
            }
            Err(SourceError::Network(e)) => Ok(SourceHealth::Unreachable { reason: e }),
            Err(e) => Ok(SourceHealth::Rejected { reason: e.to_string() }),
        }
    }

    async fn search(&self, query: &str, limit: usize, _request: Option<&StreamRequest>) -> Result<Vec<CatalogTrack>, SourceError> {
        let url = format!(
            "{}/search?q={}&limit={}",
            self.base(),
            urlencoding_lite(query),
            limit
        );
        let body = self.get_cached(&format!("search:{query}:{limit}"), &url, SEARCH_TTL).await?;
        let parsed: AddonSearchResponse =
            serde_json::from_str(&body).map_err(|e| SourceError::Protocol(format!("search: {e}")))?;
        Ok(parsed.tracks.into_iter().take(limit).map(|t| self.track_of(t)).collect())
    }

    async fn stream(&self, track_id: &str, request: &StreamRequest) -> Result<Option<SourceStream>, SourceError> {
        let mut url = format!("{}/stream/{}", self.base(), urlencoding_lite(track_id));
        if let Some(q) = Self::quality_param(request) {
            url.push_str(&format!("?quality={q}"));
        }
        let body = self.get_cached(&format!("stream:{track_id}:{url}"), &url, STREAM_TTL).await?;
        let parsed: AddonStream =
            serde_json::from_str(&body).map_err(|e| SourceError::Protocol(format!("stream: {e}")))?;
        let is_lossless = parsed
            .format
            .as_deref()
            .map(|f| matches!(f.to_ascii_uppercase().as_str(), "FLAC" | "ALAC" | "WAV" | "APE" | "WV" | "AIFF"));
        Ok(Some(SourceStream {
            url: parsed.url,
            format: StreamFormat {
                codec: parsed.format.clone().unwrap_or_default().to_ascii_uppercase(),
                is_lossless,
                kbps: parsed.bitrate,
                is_dolby_atmos: parsed
                    .quality
                    .as_deref()
                    .is_some_and(|q| q.eq_ignore_ascii_case("dolby-atmos")),
            },
            headers: vec![],
            below_request: false,
            duration_sec: None,
        }))
    }
}

/// The one character set that actually breaks URLs here; everything else is
/// already path/query-safe from the catalogues.
fn urlencoding_lite(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}
