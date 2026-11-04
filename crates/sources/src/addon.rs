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
    StreamRequest, TrackKey,
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
    /// F3 contract: capabilities mirror `resources` for new-style addons;
    /// health accepts either spelling.
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub allow_downloads: Option<bool>,
}

impl AddonManifest {
    /// `true` when search and stream are declared in either spelling.
    pub fn declares_search_and_stream(&self) -> bool {
        let has = |list: &[String], want: &str| list.iter().any(|r| r == want);
        (has(&self.resources, "search") || has(&self.capabilities, "search"))
            && (has(&self.resources, "stream") || has(&self.capabilities, "stream"))
    }
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
    #[serde(rename = "sampleRate", default)]
    pub sample_rate: Option<u32>,
    #[serde(rename = "bitDepth", default)]
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
    /// Optional: an in-band error payload carries no url (the `error`
    /// check fires before the url is ever used).
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub codec: Option<String>,
    /// none | hls | dash — hls/dash are refused (Unsupported).
    #[serde(default)]
    pub manifest: Option<String>,
    #[serde(default)]
    pub container: Option<String>,
    #[serde(default)]
    pub quality: Option<String>,
    #[serde(default)]
    pub bitrate: Option<u32>,
    #[serde(rename = "sampleRate", default)]
    pub sample_rate: Option<u32>,
    #[serde(rename = "bitDepth", default)]
    pub bit_depth: Option<u8>,
    /// Any DRM/encryption indication: refused with `SourceError::Encrypted`.
    #[serde(default)]
    pub encrypted: Option<bool>,
    /// In-band error: surfaced as `Permanent` before anything else.
    #[serde(default)]
    pub error: Option<String>,
    /// Optional per-URL request headers, filtered to the allowlist.
    #[serde(default)]
    pub headers: Option<std::collections::HashMap<String, String>>,
}

pub struct AddonSource {
    config_id: String,
    display_name: String,
    base: String,
    /// Built lazily on the first request and DNS-pinned to the validated
    /// address (see `http_client`); `None` until then, so construction
    /// never touches DNS.
    http: Mutex<Option<reqwest::Client>>,
    /// Manifests / searches / streams already fetched, by key.
    cache: Mutex<HashMap<String, (Instant, String)>>,
    /// Set when the server answers 429: requests are refused until it passes.
    quiet_until: Mutex<Option<Instant>>,
}

/// Cap on every addon response body (manifest, search, stream).
const BODY_MAX_BYTES: u64 = 4 * 1024 * 1024;
/// Id and query length caps (F3 contract).
pub const ADDON_ID_MAX_LEN: usize = 128;
pub const ADDON_QUERY_MAX_LEN: usize = 200;
/// The only request headers an addon may attach to a stream URL.
const HEADER_ALLOWLIST: [&str; 3] = ["user-agent", "referer", "authorization"];

/// Base-URL policy (F3): https default; http only with the per-addon
/// opt-in (and, as everywhere, no userinfo in the URL).
fn validate_base(base: &str, allow_http: bool) -> Result<url::Url, SourceError> {
    let url = url::Url::parse(base)
        .map_err(|e| SourceError::Permanent(format!("bad addon base url: {e}")))?;
    if url.username() != "" || url.password().is_some() {
        return Err(SourceError::Permanent("addon base url must not carry userinfo".into()));
    }
    match url.scheme() {
        "https" => {}
        "http" if allow_http => {}
        "http" => return Err(SourceError::Permanent("http addon requires the per-addon opt-in".into())),
        other => return Err(SourceError::Permanent(format!("addon scheme not allowed: {other}"))),
    }
    if url.host_str().is_none() {
        return Err(SourceError::Permanent("addon base url has no host".into()));
    }
    Ok(url)
}

impl AddonSource {
    /// F3: the base URL is validated at construction (Result, like the
    /// Subsonic source); redirects never leave the base host; every
    /// response body is size-capped.
    pub fn new(
        config_id: impl Into<String>,
        display_name: impl Into<String>,
        base: impl Into<String>,
        allow_http: bool,
    ) -> Result<Self, SourceError> {
        let base_url = validate_base(&base.into(), allow_http)?;
        let base = base_url.to_string();
        let base = base.trim_end_matches('/').to_string();
        Ok(Self {
            config_id: config_id.into(),
            display_name: display_name.into(),
            base,
            http: Mutex::new(None),
            cache: Mutex::new(HashMap::new()),
            quiet_until: Mutex::new(None),
        })
    }

    /// The redirect policy every client variant carries: same-host +
    /// same-scheme, <= 5 hops.
    fn redirect_policy(host: String, scheme: String) -> reqwest::redirect::Policy {
        reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= 5 {
                attempt.error("too many redirects")
            } else if attempt.url().host_str() == Some(host.as_str())
                && matches!(attempt.url().scheme(), "https" | "http")
                && attempt.url().scheme() == scheme
            {
                attempt.follow()
            } else {
                attempt.error("redirect target not allowed")
            }
        })
    }

    fn client_builder(host: String, scheme: String) -> reqwest::ClientBuilder {
        reqwest::Client::builder()
            .timeout(TIMEOUT)
            .user_agent("Tansen/0.3")
            .redirect(Self::redirect_policy(host, scheme))
    }

    /// The HTTP client, built on first use: the base host is resolved
    /// through netguard's rebinding guard (a non-local name that answers
    /// with a local/private address is refused) and the client is pinned
    /// to the validated address for its lifetime, so check and connect
    /// can never see different IPs. DNS runs off the async runtime.
    async fn http_client(&self) -> Result<reqwest::Client, SourceError> {
        if let Some(client) = &*self.http.lock().unwrap() {
            return Ok(client.clone());
        }
        let base = url::Url::parse(self.base())
            .map_err(|e| SourceError::Permanent(format!("addon base url: {e}")))?;
        let host = base.host_str().unwrap_or_default().to_string();
        let scheme = base.scheme().to_string();
        let to_resolve = base.clone();
        let resolved =
            tokio::task::spawn_blocking(move || netguard::resolve_validated(&to_resolve))
                .await
                .map_err(|e| SourceError::Network(format!("resolve join: {e}")))?;
        let mut builder = Self::client_builder(host, scheme);
        if let Some(domain) = netguard::dns_host(&base) {
            let addr = resolved.map_err(SourceError::Network)?;
            builder = builder.resolve(&domain, addr);
        }
        let client = builder
            .build()
            .map_err(|e| SourceError::Permanent(format!("client build: {e}")))?;
        *self.http.lock().unwrap() = Some(client.clone());
        Ok(client)
    }

    fn base(&self) -> &str {
        &self.base
    }

    /// F3: the addon-returned stream URL is validated before use — http(s)
    /// only, no userinfo, and no loopback/link-local/private host unless
    /// the addon's own base host is local/private.
    fn validate_stream_url(&self, url: &str) -> Result<(), SourceError> {
        let parsed = url::Url::parse(url)
            .map_err(|e| SourceError::Permanent(format!("addon stream url: {e}")))?;
        if parsed.username() != "" || parsed.password().is_some() {
            return Err(SourceError::Permanent("addon stream url must not carry userinfo".into()));
        }
        if !matches!(parsed.scheme(), "https" | "http") {
            return Err(SourceError::Permanent(format!(
                "addon stream scheme not allowed: {}",
                parsed.scheme()
            )));
        }
        // Locality on both sides is judged from the URL's parsed host —
        // url exposes Ipv6 hosts pre-parsed, so bracketed literals like
        // "[::1]" classify correctly (the string re-parse never did).
        let base_local = url::Url::parse(self.base())
            .map(|base| netguard::url_host_is_local(&base))
            .unwrap_or(false);
        if !base_local && netguard::url_host_is_local(&parsed) {
            return Err(SourceError::Permanent("addon stream host is local/private but the addon is not".into()));
        }
        Ok(())
    }

    fn base_url_host(&self) -> String {
        url::Url::parse(self.base())
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or_default()
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
        let client = self.http_client().await?;
        let response = client
            .get(url)
            .send()
            .await
            .map_err(|e| SourceError::Network(e.to_string()))?;
        match response.status().as_u16() {
            200..=299 => {
                // Size-capped body: Content-Length refused upfront, chunked
                // reads capped in accumulation (never trusts the header).
                if let Some(len) = response.content_length() {
                    if len > BODY_MAX_BYTES {
                        return Err(SourceError::Permanent("addon response too large".into()));
                    }
                }
                let mut body: Vec<u8> = Vec::new();
                let mut response = response;
                while let Some(chunk) = response
                    .chunk()
                    .await
                    .map_err(|e| SourceError::Network(e.to_string()))?
                {
                    body.extend_from_slice(&chunk);
                    if body.len() as u64 > BODY_MAX_BYTES {
                        return Err(SourceError::Permanent("addon response too large".into()));
                    }
                }
                String::from_utf8(body).map_err(|_| SourceError::Protocol("addon response not utf-8".into()))
            }
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
            sample_rate: None,
            bit_depth: None,
            kbps_inferred: false,
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
    fn quality_param(request: &StreamRequest) -> &'static str {
        match request {
            StreamRequest::Lossless => "LOSSLESS",
            StreamRequest::Best => "HIGH",
            StreamRequest::Capped { .. } => "LOW",
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
                if !m.declares_search_and_stream() {
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

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<CatalogTrack>, SourceError> {
        if query.len() > ADDON_QUERY_MAX_LEN {
            return Err(SourceError::Permanent(format!(
                "query too long (cap {ADDON_QUERY_MAX_LEN})"
            )));
        }
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

    async fn stream(&self, key: &TrackKey, request: &StreamRequest) -> Result<Option<SourceStream>, SourceError> {
        let track_id = &key.id;
        if track_id.is_empty() || track_id.len() > ADDON_ID_MAX_LEN {
            return Err(SourceError::Permanent(format!(
                "addon track id out of bounds (cap {ADDON_ID_MAX_LEN})"
            )));
        }
        let mut url = format!("{}/stream/{}", self.base(), urlencoding_lite(track_id));
        url.push_str(&format!("?quality={}", Self::quality_param(request)));
        let body = self.get_cached(&format!("stream:{track_id}:{url}"), &url, STREAM_TTL).await?;
        let parsed: AddonStream =
            serde_json::from_str(&body).map_err(|e| SourceError::Protocol(format!("stream: {e}")))?;
        // In-band error first: the addon says so.
        if let Some(err) = &parsed.error {
            return Err(SourceError::Permanent(format!("addon: {err}")));
        }
        // Any encryption/DRM indication: refused, never negotiated.
        if parsed.encrypted == Some(true) {
            return Err(SourceError::Encrypted);
        }
        // Adaptive manifests are outside the decode stack: refused as
        // Unsupported (HLS/DASH stay a documented limitation).
        match parsed.manifest.as_deref().map(|m| m.to_ascii_lowercase()) {
            Some(m) if m == "hls" || m == "dash" => {
                return Err(SourceError::Unsupported);
            }
            _ => {}
        }
        // The returned URL is validated against the F3 policy before use.
        let url = parsed.url.ok_or_else(|| {
            SourceError::Permanent("addon stream answer has neither url nor error".into())
        })?;
        self.validate_stream_url(&url)?;
        // Lossless is decided by codec (the canonical table), never by the
        // addon's declared quality.
        let codec = parsed
            .format
            .clone()
            .or_else(|| parsed.codec.clone())
            .unwrap_or_default()
            .to_ascii_uppercase();
        let is_lossless = StreamFormat::is_lossless(&codec);
        // Request headers: allowlist only.
        let headers: Vec<(String, String)> = parsed
            .headers
            .map(|h| {
                h.into_iter()
                    .filter(|(k, _)| HEADER_ALLOWLIST.contains(&k.to_ascii_lowercase().as_str()))
                    .collect()
            })
            .unwrap_or_default();
        let container = parsed.container.as_deref().map(|c| match c.to_ascii_lowercase().as_str() {
            "mp4" | "m4a" => "mp4",
            "webm" => "webm",
            _ => c,
        });
        Ok(Some(SourceStream {
            url,
            format: StreamFormat {
                codec,
                is_lossless,
                kbps: parsed.bitrate,
                is_dolby_atmos: parsed
                    .quality
                    .as_deref()
                    .is_some_and(|q| q.eq_ignore_ascii_case("dolby-atmos")),
                sample_rate: parsed.sample_rate,
                bit_depth: parsed.bit_depth,
                kbps_inferred: false,
            },
            headers,
            below_request: false,
            duration_sec: None,
            // The addon protocol carries no MIME or expiry; both stay unknown.
            mime: None,
            expiry_ms: None,
            container: container.map(|c| crate::Container::Progressive),
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

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test code")]
mod url_policy_tests {
    use super::*;

    #[test]
    fn base_url_policy() {
        assert!(validate_base("https://addons.example", false).is_ok());
        assert!(validate_base("https://addons.example", true).is_ok());
        assert!(validate_base("http://addons.example", false).is_err(), "http without opt-in");
        assert!(validate_base("http://addons.example", true).is_ok(), "opt-in allows http");
        assert!(validate_base("https://u:p@addons.example", true).is_err(), "userinfo rejected");
        assert!(validate_base("ftp://addons.example", true).is_err(), "scheme allowlist");
        assert!(validate_base("not a url", true).is_err());
    }

    #[test]
    fn stream_url_policy_rejects_private_hosts_unless_base_is_local() {
        // A public addon may not pivot to loopback/private/link-local.
        let public = AddonSource::new("p", "P", "https://addons.example", false).unwrap();
        assert!(public.validate_stream_url("https://cdn.example/t.flac").is_ok());
        assert!(public.validate_stream_url("http://127.0.0.1:9000/x").is_err(), "loopback refused");
        assert!(public.validate_stream_url("http://10.0.0.5/x").is_err(), "private refused");
        assert!(public.validate_stream_url("http://169.254.1.1/x").is_err(), "link-local refused");
        assert!(public.validate_stream_url("https://u:p@cdn.example/x").is_err(), "userinfo refused");
        assert!(public.validate_stream_url("file:///etc/passwd").is_err(), "scheme refused");
        // A local addon may serve local stream URLs (same trust zone).
        let local = AddonSource::new("l", "L", "http://127.0.0.1:9000", true).unwrap();
        assert!(local.validate_stream_url("http://127.0.0.1:9000/x").is_ok());
        assert!(local.validate_stream_url("https://cdn.example/t.flac").is_ok());
    }

    /// Regression (audit run-1: sources.addon.host_is_local
    /// .ipv6-bracketed-hoststr-and-0.0.0.0-unclassified): the old
    /// classifier re-parsed host_str(), whose bracketed IPv6 serialization
    /// never parses as IpAddr — every IPv6 URL host (loopback ::1
    /// included) plus 0.0.0.0 classified non-local and passed the gate.
    /// Locality is now judged on the URL's parsed host.
    #[test]
    fn stream_url_policy_rejects_ipv6_mapped_unspecified_and_0addr() {
        let public = AddonSource::new("p", "P", "https://addons.example", false).unwrap();
        for url in [
            "http://[::1]:9000/x",
            "http://[fd00::1]/x",
            "http://[fe80::1]/x",
            "http://[::ffff:127.0.0.1]:9000/x",
            "http://[::ffff:10.0.0.5]:9000/x",
            "http://0.0.0.0:9000/x",
        ] {
            assert!(public.validate_stream_url(url).is_err(), "{url} must be refused");
        }
        // Public IPv6 stream URLs keep working.
        assert!(public.validate_stream_url("http://[2001:4860:4860::8888]/x").is_ok());
    }
}
