//! S9: Subsonic / OpenSubsonic source over the shared `MusicSource` trait.
//!
//! - health: `rest/ping` (auth-exercising)
//! - search: `rest/search3` (songs only)
//! - stream: `rest/stream?id=..&format=raw` (original format), sniffed for
//!   Content-Type to build the lossless/`StreamFormat` hint; the URL is
//!   handed to Rust (no audio bytes cross this adapter).
//!
//! Security posture: base URL must be https by default; http is allowed only
//! with an explicit opt-in AND a loopback/private host. Redirects never
//! leave the configured host. Responses are size-capped. Encrypted/DRM-
//! flagged streams are rejected.
//!
//! Credentials: the username lives in the settings DB (not secret); the
//! password is fetched per-request from the macOS Keychain through
//! [`SecretProvider`] — never stored in SQLite, logs, events or frontend
//! state. Auth is the Subsonic token scheme: `token = md5(password + salt)`
//! over HTTPS, which also accepts server API keys as the secret.

use crate::{
    CatalogTrack, MusicSource, SourceError, SourceHealth, SourceKind, SourceStream, StreamFormat,
    StreamRequest, TrackKey,
};
use async_trait::async_trait;
use md5::{Digest, Md5};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use url::Url;

const TIMEOUT: Duration = Duration::from_secs(20);
/// Cap on JSON API responses (search/ping).
const JSON_MAX_BYTES: u64 = 4 * 1024 * 1024;
/// Cap on the stream probe (headers + a slice of the body).
const PROBE_MAX_BYTES: u64 = 64 * 1024;
const API_VERSION: &str = "1.16.1";
const CLIENT_NAME: &str = "tansen";
/// Max redirect hops for the accept-policy.
const MAX_REDIRECTS: usize = 5;

/// Where a secret (password/API key) comes from. Default reads the macOS
/// Keychain; tests inject a fixed value.
pub trait SecretProvider: Send + Sync {
    fn get(&self, account: &str) -> Option<String>;
}

/// The real provider: macOS Keychain (S9 decision A). Reads the
/// server-bound key with the legacy username-only fallback.
pub struct KeychainSecretProvider;
impl SecretProvider for KeychainSecretProvider {
    fn get(&self, account: &str) -> Option<String> {
        crate::keychain::get_password_server_bound(account).ok().flatten()
    }
}

/// A fixed provider for tests (never touches the Keychain).
pub struct FixedSecretProvider(pub String);
impl SecretProvider for FixedSecretProvider {
    fn get(&self, _account: &str) -> Option<String> {
        Some(self.0.clone())
    }
}

fn is_loopback_or_private(host: &str) -> bool {
    // Shared policy: localhost/.local names and full private-address IP
    // literals (both families, bracketed IPv6 accepted); DNS names outside
    // localhost/.local are not "LAN" for the http opt-in.
    netguard::host_is_local(host)
}

/// Validates the base URL against the S9 URL policy.
pub fn validate_base_url(base: &str, allow_http: bool) -> Result<Url, SourceError> {
    let url = Url::parse(base).map_err(|e| SourceError::Permanent(format!("bad base url: {e}")))?;
    if url.username() != "" || url.password().is_some() {
        return Err(SourceError::Permanent("credentials must not be embedded in the URL".into()));
    }
    let Some(host) = url.host_str() else {
        return Err(SourceError::Permanent("base url has no host".into()));
    };
    match url.scheme() {
        "https" => {}
        "http" if allow_http && is_loopback_or_private(host) => {}
        "http" => {
            return Err(SourceError::Permanent(
                "http requires the LAN/localhost opt-in and a private host".into(),
            ));
        }
        other => return Err(SourceError::Permanent(format!("scheme not allowed: {other}"))),
    }
    Ok(url)
}

/// Maps a stream Content-Type to a codec label + lossless hint.
fn content_type_to_format(ct: &str) -> Option<StreamFormat> {
    let ct = ct.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    let codec = match ct.as_str() {
        "audio/flac" | "audio/x-flac" => "flac",
        "audio/mpeg" | "audio/mp3" => "mp3",
        "audio/mp4" | "audio/aac" | "audio/m4a" | "audio/x-m4a" => "m4a",
        "audio/ogg" | "application/ogg" => "ogg",
        "audio/wav" | "audio/x-wav" => "wav",
        "audio/aiff" | "audio/x-aiff" => "aiff",
        "audio/opus" => "opus",
        "audio/webm" => "webm",
        _ => return None,
    };
    Some(StreamFormat {
        codec: codec.to_string(),
        is_lossless: StreamFormat::is_lossless(codec),
        kbps: None,
        is_dolby_atmos: false,
        sample_rate: None,
        bit_depth: None,
        kbps_inferred: false,
    })
}

pub struct SubsonicSource {
    config_id: String,
    display_name: String,
    base: Url,
    username: String,
    /// Keychain account the credential is read from: username@server
    /// (with a legacy username-only fallback), so same-username servers
    /// cannot read each other's passwords.
    secret_account: String,
    secrets: Arc<dyn SecretProvider>,
    /// Built lazily on the first request and DNS-pinned to the validated
    /// address (see `http_client`); `None` until then, so construction
    /// never touches DNS.
    http: Mutex<Option<reqwest::Client>>,
}

impl SubsonicSource {
    /// Validates the base URL and builds the source (timeouts, same-host
    /// redirect policy on the lazily built client). `secrets` supplies the
    /// password/API key.
    pub fn new_with_secrets(
        config_id: impl Into<String>,
        display_name: impl Into<String>,
        base: &str,
        username: &str,
        allow_http: bool,
        secrets: Arc<dyn SecretProvider>,
    ) -> Result<Self, SourceError> {
        let base = validate_base_url(base, allow_http)?;
        let secret_account = crate::keychain::credential_account(username, base.as_str());
        Ok(Self {
            config_id: config_id.into(),
            display_name: display_name.into(),
            base,
            username: username.to_string(),
            secret_account,
            secrets,
            http: Mutex::new(None),
        })
    }

    /// The redirect policy every client variant carries: same-host +
    /// same-scheme, <= MAX_REDIRECTS hops.
    fn redirect_policy(host: String, scheme: String) -> reqwest::redirect::Policy {
        reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= MAX_REDIRECTS {
                attempt.error("too many redirects")
            } else {
                let same_host = attempt.url().host_str() == Some(host.as_str());
                let scheme_ok = matches!(attempt.url().scheme(), "https" | "http")
                    && attempt.url().scheme() == scheme;
                if same_host && scheme_ok {
                    attempt.follow()
                } else {
                    attempt.error("redirect target not allowed")
                }
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
    /// to the validated address for its lifetime. DNS runs off the async
    /// runtime.
    async fn http_client(&self) -> Result<reqwest::Client, SourceError> {
        if let Some(client) = &*self.http.lock().unwrap() {
            return Ok(client.clone());
        }
        let host = self.base.host_str().unwrap_or_default().to_string();
        let scheme = self.base.scheme().to_string();
        let to_resolve = self.base.clone();
        let resolved =
            tokio::task::spawn_blocking(move || netguard::resolve_validated(&to_resolve))
                .await
                .map_err(|e| SourceError::Network(format!("resolve join: {e}")))?;
        let mut builder = Self::client_builder(host, scheme);
        if let Some(domain) = netguard::dns_host(&self.base) {
            let addr = resolved.map_err(SourceError::Network)?;
            builder = builder.resolve(&domain, addr);
        }
        let client = builder
            .build()
            .map_err(|e| SourceError::Permanent(format!("client build: {e}")))?;
        *self.http.lock().unwrap() = Some(client.clone());
        Ok(client)
    }

    pub fn new(
        config_id: impl Into<String>,
        display_name: impl Into<String>,
        base: &str,
        username: &str,
        allow_http: bool,
    ) -> Result<Self, SourceError> {
        Self::new_with_secrets(
            config_id,
            display_name,
            base,
            username,
            allow_http,
            Arc::new(KeychainSecretProvider),
        )
    }

    fn auth_params(&self) -> Vec<(String, String)> {
        // Server-bound key with the pre-fix username-only fallback: a
        // credential belongs to (username, server), never to the username
        // alone.
        let password = self.secrets.get(&self.secret_account).unwrap_or_default();
        let salt = format!("{:x}", std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0));
        let token = hex_lower(&Md5::digest(format!("{password}{salt}")));
        vec![
            ("u".to_string(), self.username.clone()),
            ("t".to_string(), token),
            ("s".to_string(), salt),
            ("v".to_string(), API_VERSION.to_string()),
            ("c".to_string(), CLIENT_NAME.to_string()),
            ("f".to_string(), "json".to_string()),
        ]
    }

    fn endpoint(&self, path: &str) -> Result<Url, SourceError> {
        self.base
            .join(path)
            .map_err(|e| SourceError::Permanent(format!("bad endpoint: {e}")))
    }

    /// GET returning a sized-capped body; rejects oversized responses. The
    /// client is DNS-pinned at construction (see `new_with_secrets`).
    async fn get_capped(&self, path: &str, params: &[(String, String)], cap: u64) -> Result<Vec<u8>, SourceError> {
        let url = self.endpoint(path)?;
        let client = self.http_client().await?;
        let mut resp = client
            .get(url)
            .query(params)
            .send()
            .await
            .map_err(|e| SourceError::Network(e.to_string()))?;
        if let Some(len) = resp.content_length() {
            if len > cap {
                return Err(SourceError::Permanent("response too large".into()));
            }
        }
        let mut body = Vec::new();
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| SourceError::Network(e.to_string()))?
        {
            body.extend_from_slice(&chunk);
            if body.len() as u64 > cap {
                return Err(SourceError::Permanent("response too large".into()));
            }
        }
        Ok(body)
    }

    /// Extracts `subsonic-response`; error when status != ok.
    fn response_json(body: &[u8]) -> Result<serde_json::Value, SourceError> {
        let v: serde_json::Value = serde_json::from_slice(body)
            .map_err(|e| SourceError::Protocol(format!("bad json: {e}")))?;
        let resp = v
            .get("subsonic-response")
            .ok_or_else(|| SourceError::Protocol("missing subsonic-response".into()))?;
        if resp.get("status").and_then(|s| s.as_str()) != Some("ok") {
            let msg = resp
                .pointer("/error/message")
                .and_then(|m| m.as_str())
                .unwrap_or("subsonic error")
                .to_string();
            return Err(SourceError::Permanent(msg));
        }
        Ok(resp.clone())
    }

    fn stream_url(&self, track_id: &str) -> Result<Url, SourceError> {
        let mut url = self.endpoint("rest/stream")?;
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("id", track_id);
            pairs.append_pair("format", "raw");
            pairs.append_pair("estimateContentLength", "true");
            for (k, v) in self.auth_params() {
                pairs.append_pair(&k, &v);
            }
        }
        Ok(url)
    }
}

#[async_trait]
impl MusicSource for SubsonicSource {
    fn config_id(&self) -> &str {
        &self.config_id
    }

    fn kind(&self) -> SourceKind {
        SourceKind::Subsonic
    }

    fn display_name(&self) -> &str {
        &self.display_name
    }

    async fn health(&self) -> Result<SourceHealth, SourceError> {
        let body = match self.get_capped("rest/ping", &self.auth_params(), JSON_MAX_BYTES).await {
            Ok(b) => b,
            Err(SourceError::Network(_)) => {
                return Ok(SourceHealth::Unreachable { reason: "server unreachable".into() });
            }
            Err(e) => return Ok(SourceHealth::Rejected { reason: e.to_string() }),
        };
        match Self::response_json(&body) {
            Ok(_) => Ok(SourceHealth::Ok { detail: None }),
            Err(e) => Ok(SourceHealth::Rejected { reason: e.to_string() }),
        }
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<CatalogTrack>, SourceError> {
        let mut params = self.auth_params();
        params.push(("query".to_string(), query.to_string()));
        params.push(("songCount".to_string(), limit.to_string()));
        params.push(("artistCount".to_string(), "0".to_string()));
        params.push(("albumCount".to_string(), "0".to_string()));
        let body = self.get_capped("rest/search3", &params, JSON_MAX_BYTES).await?;
        let resp = Self::response_json(&body)?;
        let songs = resp.pointer("/searchResult3/song").and_then(|s| s.as_array());
        let Some(songs) = songs else { return Ok(vec![]) };
        let mut out = Vec::with_capacity(songs.len());
        for song in songs {
            if song.get("isVideo").and_then(|v| v.as_bool()).unwrap_or(false) {
                continue;
            }
            let Some(id) = song.get("id").and_then(|i| i.as_str()) else { continue };
            let artwork_url = song.get("coverArt").and_then(|c| c.as_str()).map(|a| {
                format!("{}rest/getCoverArt?id={}", self.base, a)
            });
            out.push(CatalogTrack {
                track_id: id.to_string(),
                config_id: self.config_id.clone(),
                title: song.get("title").and_then(|t| t.as_str()).unwrap_or("").to_string(),
                artist: song.get("artist").and_then(|t| t.as_str()).unwrap_or("").to_string(),
                album: song.get("album").and_then(|t| t.as_str()).map(str::to_string),
                duration_sec: song.get("duration").and_then(|d| d.as_u64()).map(|d| d as u32),
                artwork_url,
                format: None,
                embedded_stream_url: None,
            });
        }
        Ok(out)
    }

    async fn stream(
        &self,
        key: &TrackKey,
        _request: &StreamRequest,
    ) -> Result<Option<SourceStream>, SourceError> {
        let url = self.stream_url(&key.id)?;
        // Probe with a zero-byte range to sniff Content-Type / DRM flags.
        let client = self.http_client().await?;
        let probe = client
            .get(url.clone())
            .header(reqwest::header::RANGE, "bytes=0-0")
            .send()
            .await
            .map_err(|e| SourceError::Network(e.to_string()))?;
        if probe.headers().get("x-subsonic-drm").and_then(|v| v.to_str().ok()) == Some("true") {
            return Err(SourceError::Permanent("drm-flagged stream rejected".into()));
        }
        let ct = probe
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        if ct.to_ascii_lowercase().contains("encrypted") {
            return Err(SourceError::Permanent("encrypted stream rejected".into()));
        }
        let format = content_type_to_format(&ct).or_else(|| {
            // Unknown type: keep the stream but without a lossless claim.
            Some(StreamFormat { codec: String::new(), ..Default::default() })
        });
        Ok(Some(SourceStream {
            url: url.to_string(),
            format: format.unwrap(),
            headers: vec![],
            below_request: false,
            duration_sec: None,
            mime: Some(ct),
            expiry_ms: None,
            container: None,
        }))
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_policy() {
        assert!(validate_base_url("https://music.example", false).is_ok());
        assert!(validate_base_url("https://music.example", true).is_ok());
        assert!(validate_base_url("http://music.example", false).is_err(), "http without opt-in");
        assert!(validate_base_url("http://music.example", true).is_err(), "http to a public DNS name");
        assert!(validate_base_url("http://127.0.0.1:4533", true).is_ok());
        assert!(validate_base_url("http://localhost:4533", true).is_ok());
        assert!(validate_base_url("http://10.0.0.5:4533", true).is_ok());
        assert!(validate_base_url("http://192.168.1.5:4533", true).is_ok());
        assert!(validate_base_url("ftp://music.example", true).is_err(), "scheme allowlist");
        assert!(validate_base_url("https://user:pw@music.example", true).is_err(), "no embedded creds");
        assert!(validate_base_url("not a url", true).is_err());
    }

    #[test]
    fn content_type_maps_to_lossless_hints() {
        assert_eq!(content_type_to_format("audio/flac").unwrap().codec, "flac");
        assert_eq!(content_type_to_format("audio/flac").unwrap().is_lossless, Some(true));
        assert_eq!(content_type_to_format("audio/mpeg").unwrap().is_lossless, Some(false));
        assert_eq!(content_type_to_format("audio/mp4; codecs=mp4a.40.2").unwrap().codec, "m4a");
        assert!(content_type_to_format("application/octet-stream").is_none());
        assert_eq!(content_type_to_format("audio/wav").unwrap().is_lossless, Some(true));
    }
}
