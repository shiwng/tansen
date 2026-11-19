//! Internet radio: station discovery (Radio Browser), station artwork and
//! the playback source that turns a station URL into live audio.
//!
//! Discovery is read-only and unauthenticated. Responses are cached briefly
//! per query so typing and scrolling never hammer the public directory, and
//! every station is validated before it can reach the engine: http(s) only,
//! a real public host, no HLS playlists (the decoder cannot read them).
//!
//! Playback reuses the normal Rust engine: a station is just a `Radio`
//! TrackKey whose id is the stream URL, resolved here to itself and opened
//! by the pipeline as one unbounded, unseekable body.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// One public Radio Browser host; the directory is a volunteer cluster, and
/// this is the canonical European entry point.
const RADIO_BROWSER: &str = "https://de1.api.radio-browser.info";
const USER_AGENT: &str = "Tansen/0.3";
const API_TIMEOUT: Duration = Duration::from_secs(12);
/// Discovery cache: repeated searches and paging back are free for 10 min.
const CACHE_TTL: Duration = Duration::from_secs(10 * 60);
const MAX_PAGE: u32 = 100;
const STATION_MAX_FIELD: usize = 512;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioStation {
    pub id: String,
    pub name: String,
    /// The playable stream URL (already redirected by the directory).
    pub url: String,
    pub homepage: String,
    pub favicon: String,
    pub tags: String,
    pub country: String,
    pub country_code: String,
    pub language: String,
    pub codec: String,
    pub bitrate: u32,
    pub votes: u32,
    pub click_count: u32,
    /// Radio Browser's last reachability probe. 0 = known dead right now.
    pub last_check_ok: u8,
    pub hls: u8,
}

impl RadioStation {
    /// A station is playable when its URL is a real public http(s) endpoint
    /// and it is not an HLS playlist.
    pub fn playable(&self) -> bool {
        !self.url.is_empty() && !self.url.contains(".m3u8") && station_url_allowed(&self.url)
    }

    /// What the player shows under the station name: country and the first
    /// tags, never a fabricated "now playing" song.
    pub fn detail(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if !self.country.is_empty() {
            parts.push(&self.country);
        }
        for tag in self
            .tags
            .split(',')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .take(3)
        {
            parts.push(tag);
        }
        parts.join(" • ")
    }
}

/// Public http(s) host only: a directory row must not probe the LAN.
pub fn station_url_allowed(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    matches!(parsed.scheme(), "http" | "https")
        && parsed.host_str().is_some()
        && !netguard::url_host_is_local(&parsed)
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RadioQuery {
    /// Free-text station name search.
    #[serde(default)]
    pub search: String,
    #[serde(default)]
    pub country_code: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub tag: String,
    /// `clickcount` (popular), `votes`, `lastcheckok`, `name`.
    #[serde(default)]
    pub order: String,
    #[serde(default)]
    pub offset: u32,
}

/// A country / language / genre facet with its station count.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioFacet {
    pub name: String,
    pub code: String,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioFacets {
    pub countries: Vec<RadioFacet>,
    pub languages: Vec<RadioFacet>,
    pub genres: Vec<RadioFacet>,
}

/// Raw row shape from `/json/stations/*` (snake_case, numeric strings).
#[derive(Debug, Deserialize)]
struct RawStation {
    #[serde(default)]
    stationuuid: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    url_resolved: String,
    #[serde(default)]
    homepage: String,
    #[serde(default)]
    favicon: String,
    #[serde(default)]
    tags: String,
    #[serde(default)]
    country: String,
    #[serde(default)]
    countrycode: String,
    #[serde(default)]
    language: String,
    #[serde(default)]
    codec: String,
    #[serde(default)]
    bitrate: u32,
    #[serde(default)]
    votes: u32,
    #[serde(default)]
    clickcount: u32,
    #[serde(default)]
    lastcheckok: u8,
    #[serde(default)]
    hls: u8,
}

fn clip(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= STATION_MAX_FIELD {
        return trimmed.to_string();
    }
    trimmed.chars().take(STATION_MAX_FIELD).collect()
}

impl From<RawStation> for RadioStation {
    fn from(raw: RawStation) -> Self {
        let url = if raw.url_resolved.trim().is_empty() {
            raw.url
        } else {
            raw.url_resolved
        };
        Self {
            id: clip(&raw.stationuuid),
            name: clip(&raw.name),
            url: clip(&url),
            homepage: clip(&raw.homepage),
            favicon: clip(&raw.favicon),
            tags: clip(&raw.tags),
            country: clip(&raw.country),
            country_code: clip(&raw.countrycode).to_uppercase(),
            language: clip(&raw.language),
            codec: clip(&raw.codec).to_uppercase(),
            bitrate: raw.bitrate.min(1411),
            votes: raw.votes,
            click_count: raw.clickcount,
            last_check_ok: raw.lastcheckok,
            hls: raw.hls,
        }
    }
}

struct CacheEntry {
    at: Instant,
    body: String,
}

/// The directory's GET cache. One map for every endpoint keeps the rules in
/// one place: same URL within the TTL, no network.
fn cache() -> &'static Mutex<HashMap<String, CacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

async fn get_json(url: String) -> Result<String, String> {
    {
        let guard = cache()
            .lock()
            .map_err(|_| "radio cache poisoned".to_string())?;
        if let Some(hit) = guard.get(&url) {
            if hit.at.elapsed() < CACHE_TTL {
                return Ok(hit.body.clone());
            }
        }
    }
    let client = reqwest::Client::builder()
        .timeout(API_TIMEOUT)
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| format!("radio http client: {e}"))?;
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("radio directory unreachable: {}", e.without_url()))?;
    if !response.status().is_success() {
        return Err(format!(
            "radio directory answered HTTP {}",
            response.status().as_u16()
        ));
    }
    let body = crate::bounded_body::text(response, crate::bounded_body::TEXT_MAX_BYTES)
        .await
        .map_err(|e| format!("radio directory body: {e}"))?;
    if let Ok(mut guard) = cache().lock() {
        // Bound the cache when a long session visits many countries.
        if guard.len() > 256 {
            guard.clear();
        }
        guard.insert(
            url,
            CacheEntry {
                at: Instant::now(),
                body: body.clone(),
            },
        );
    }
    Ok(body)
}

fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// One page of stations for the discovery UI. Always hides broken and HLS
/// rows, and keeps only stations this decoder can actually open.
async fn stations(query: RadioQuery) -> Result<Vec<RadioStation>, String> {
    let limit = MAX_PAGE.min(60);
    let mut url = format!(
        "{RADIO_BROWSER}/json/stations/search?hidebroken=true&limit={limit}&offset={}&order={}&reverse=true",
        query.offset.min(20_000),
        encode(order_of(&query.order)),
    );
    if !query.search.trim().is_empty() {
        url.push_str(&format!("&name={}", encode(query.search.trim())));
    }
    if query.country_code.len() == 2 {
        url.push_str(&format!("&countrycode={}", encode(&query.country_code)));
    }
    if !query.language.trim().is_empty() {
        url.push_str(&format!("&language={}", encode(query.language.trim())));
    }
    if !query.tag.trim().is_empty() {
        url.push_str(&format!("&tag={}", encode(query.tag.trim())));
    }
    let body = get_json(url).await?;
    let rows: Vec<RawStation> =
        serde_json::from_str(&body).map_err(|e| format!("radio directory parse: {e}"))?;
    Ok(rows
        .into_iter()
        .map(RadioStation::from)
        .filter(|s| s.playable() && !s.name.is_empty())
        .collect())
}

fn order_of(order: &str) -> &'static str {
    match order {
        "votes" => "votes",
        "name" => "name",
        _ => "clickcount",
    }
}

/// Counts come from `/json/countries`, `/json/languages`, `/json/tags`.
#[derive(Debug, Deserialize)]
struct RawFacet {
    #[serde(default)]
    name: String,
    #[serde(default)]
    iso_3166_1: String,
    #[serde(default)]
    stationcount: u32,
}

async fn facets(kind: &str) -> Result<Vec<RadioFacet>, String> {
    let endpoint = match kind {
        "countries" => "countries",
        "languages" => "languages",
        _ => "tags",
    };
    let url = format!(
        "{RADIO_BROWSER}/json/{endpoint}?hidebroken=true&order=stationcount&reverse=true&limit=200"
    );
    let body = get_json(url).await?;
    let rows: Vec<RawFacet> =
        serde_json::from_str(&body).map_err(|e| format!("radio facets parse: {e}"))?;
    Ok(rows
        .into_iter()
        .filter(|r| !r.name.trim().is_empty() && r.stationcount > 0)
        .map(|r| RadioFacet {
            name: clip(&r.name),
            code: clip(&r.iso_3166_1).to_uppercase(),
            count: r.stationcount,
        })
        .collect())
}

// Commands

#[tauri::command]
pub async fn radio_stations(query: RadioQuery) -> Result<Vec<RadioStation>, String> {
    stations(query).await
}

#[tauri::command]
pub async fn radio_facets() -> Result<RadioFacets, String> {
    // Three GETs, cached for 10 min; a failing endpoint does not take the
    // others down (a doorway page needs at least one shelf).
    let (countries, languages, genres) =
        tokio::join!(facets("countries"), facets("languages"), facets("genres"));
    Ok(RadioFacets {
        countries: countries.unwrap_or_default(),
        languages: languages.unwrap_or_default(),
        genres: genres.unwrap_or_default(),
    })
}

/// Radio Browser counts a click per station; this is how stations gain
/// popularity, so it is best-effort and never blocks playback.
#[tauri::command]
pub async fn radio_click(station_id: String) -> Result<(), String> {
    if station_id.is_empty() || station_id.len() > 64 {
        return Err("invalid station id".into());
    }
    let url = format!("{RADIO_BROWSER}/json/url/{}", encode(&station_id));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(6))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| e.to_string())?;
    let _ = client.get(url).send().await;
    Ok(())
}

/// Station favicons come from arbitrary publishers, so they bypass the
/// catalogue artwork allowlist — but only for https, public hosts and real
/// image responses, and the bytes land in the same local art cache.
#[tauri::command]
pub async fn radio_artwork(state: crate::StateHandle<'_>, url: String) -> Result<String, String> {
    let parsed = reqwest::Url::parse(&url).map_err(|e| format!("bad artwork url: {e}"))?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || netguard::url_host_is_local(&parsed)
    {
        return Err("artwork host not allowed".into());
    }
    use sha1::Digest as _;
    let mut h = sha1::Sha1::new();
    h.update(url.as_bytes());
    let key = format!("{:x}", h.finalize());
    let dir = state.data_dir.join("art").join("radio");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    for ext in ["jpg", "png", "gif", "bmp", "webp", "img"] {
        let cached = dir.join(format!("{key}.{ext}"));
        if cached.is_file() {
            return Ok(cached.to_string_lossy().into_owned());
        }
    }
    const MAX: u64 = 3 * 1024 * 1024;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent(USER_AGENT)
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let ok =
                attempt.url().scheme() == "https" && !netguard::url_host_is_local(attempt.url());
            if attempt.previous().len() >= 6 {
                attempt.error("too many redirects")
            } else if ok {
                attempt.follow()
            } else {
                attempt.error("artwork redirect not allowed")
            }
        }))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("artwork unreachable: {}", e.without_url()))?;
    if !response.status().is_success() {
        return Err(format!(
            "artwork fetch failed: HTTP {}",
            response.status().as_u16()
        ));
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !content_type.starts_with("image/") {
        return Err(format!("not an image: {content_type}"));
    }
    let ext = if content_type.contains("png") {
        "png"
    } else if content_type.contains("gif") {
        "gif"
    } else if content_type.contains("webp") {
        "webp"
    } else if content_type.contains("bmp") {
        "bmp"
    } else {
        "jpg"
    };
    let bytes = crate::bounded_body::bytes(response, MAX)
        .await
        .map_err(|e| format!("artwork body: {e}"))?;
    let path = dir.join(format!("{key}.{ext}"));
    std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

/// The engine's Radio source: the key id IS the live stream URL, so
/// resolution returns it unchanged. No search, no catalogue, no fallback —
/// a broadcast has exactly one rendition.
pub struct RadioSource;

#[async_trait::async_trait]
impl sources::MusicSource for RadioSource {
    fn config_id(&self) -> &str {
        "radio"
    }
    fn kind(&self) -> sources::SourceKind {
        sources::SourceKind::Radio
    }
    fn display_name(&self) -> &str {
        "Radio"
    }
    async fn health(&self) -> Result<sources::SourceHealth, sources::SourceError> {
        Ok(sources::SourceHealth::Ok { detail: None })
    }
    async fn search(
        &self,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<sources::CatalogTrack>, sources::SourceError> {
        Ok(vec![])
    }
    async fn stream(
        &self,
        _key: &sources::TrackKey,
        _request: &sources::StreamRequest,
    ) -> Result<Option<sources::SourceStream>, sources::SourceError> {
        Err(sources::SourceError::Unsupported)
    }
    async fn resolve(
        &self,
        key: &sources::TrackKey,
        _request: &sources::StreamRequest,
    ) -> Result<Option<sources::ResolvedMedia>, sources::SourceError> {
        if !station_url_allowed(&key.id) {
            return Err(sources::SourceError::Permanent(
                "station stream URL is not a public http(s) endpoint".into(),
            ));
        }
        // The directory supplies the host name, so a public name such as
        // `localtest.me` could resolve to loopback and pivot the media fetch
        // at a local service. Resolve once off the async runtime before the
        // engine opens the stream. (The initial connection is not pinned;
        // the reader's own DNS check leaves a documented TOCTOU residual.)
        let parsed = reqwest::Url::parse(&key.id)
            .map_err(|e| sources::SourceError::Permanent(format!("station url: {e}")))?;
        if netguard::dns_host(&parsed).is_some() {
            tauri::async_runtime::spawn_blocking(move || netguard::resolve_validated(&parsed))
                .await
                .map_err(|e| sources::SourceError::Network(format!("station resolve join: {e}")))?
                .map_err(|e| {
                    sources::SourceError::Permanent(format!("station host rejected: {e}"))
                })?;
        }
        Ok(Some(sources::ResolvedMedia {
            url: key.id.clone(),
            headers: vec![],
            expiry: None,
            length: None,
            mime: None,
            format: None,
            container: None,
            source: sources::SourceKind::Radio,
            below_request: false,
        }))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::*;
    use sources::MusicSource as _;

    #[test]
    fn station_urls_must_be_public_http() {
        assert!(station_url_allowed("https://stream.example.com/live.mp3"));
        assert!(station_url_allowed("http://80.1.2.3:8000/stream"));
        assert!(!station_url_allowed("https://127.0.0.1/stream"));
        assert!(!station_url_allowed("https://192.168.1.5/stream"));
        assert!(!station_url_allowed("file:///tmp/x.mp3"));
        assert!(!station_url_allowed("ftp://example.com/x"));
    }

    #[test]
    fn raw_station_prefers_resolved_url_and_rejects_hls() {
        let raw: RawStation = serde_json::from_value(serde_json::json!({
            "stationuuid": "uuid-1",
            "name": "  Test FM ",
            "url": "http://old.example/a",
            "url_resolved": "http://new.example/b",
            "tags": "jazz, lounge ,,",
            "country": "India",
            "countrycode": "in",
            "codec": "mp3",
            "bitrate": 128,
            "lastcheckok": 1,
            "hls": 0
        }))
        .unwrap();
        let station = RadioStation::from(raw);
        assert_eq!(station.name, "Test FM");
        assert_eq!(station.url, "http://new.example/b");
        assert_eq!(station.country_code, "IN");
        assert_eq!(station.codec, "MP3");
        assert_eq!(station.detail(), "India • jazz • lounge");
        assert!(station.playable());

        let hls: RawStation = serde_json::from_value(serde_json::json!({
            "stationuuid": "uuid-2",
            "name": "HLS FM",
            "url_resolved": "https://cdn.example/live/index.m3u8",
            "hls": 1
        }))
        .unwrap();
        assert!(!RadioStation::from(hls).playable());
    }

    #[test]
    fn facet_and_order_defaults_are_bounded() {
        assert_eq!(order_of(""), "clickcount");
        assert_eq!(order_of("votes"), "votes");
        assert_eq!(order_of("javascript:"), "clickcount");
    }

    #[tokio::test]
    async fn radio_source_returns_the_url_it_was_keyed_with() {
        let source = RadioSource;
        // An IP literal needs no DNS, so this test stays offline; a name
        // would now exercise the resolve-before-open guard.
        let key =
            sources::TrackKey::new(sources::SourceKind::Radio, "https://1.1.1.1/live").unwrap();
        let media = source
            .resolve(&key, &sources::StreamRequest::Best)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(media.url, "https://1.1.1.1/live");
        assert_eq!(media.source, sources::SourceKind::Radio);
        let local =
            sources::TrackKey::new(sources::SourceKind::Radio, "http://127.0.0.1/a").unwrap();
        assert!(source
            .resolve(&local, &sources::StreamRequest::Best)
            .await
            .is_err());
    }
}
