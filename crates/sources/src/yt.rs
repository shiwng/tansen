//! The YouTube Music source: talks to the Kotlin sidecar (see
//! `docs/sidecar-api.md` in the app repo). The sidecar is `:shared`'s
//! catalogue client served over local HTTP; this module spawns, supervises
//! and translates it.
//!
//! Process ownership: `YtMusicSource::launch` starts
//! `java -jar sidecar-all.jar --port <free>` bound to 127.0.0.1, health-checks
//! it with retries, and kills it when the source is dropped. Local sources
//! stay the default — this source is only built when the catalogue flag is
//! on and a jar is discoverable.

use crate::{
    CatalogTrack, Container, MusicSource, SourceError, SourceHealth, SourceKind, SourceStream,
    StreamFormat, StreamRequest, TrackKey,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

const HEALTH_TIMEOUT: Duration = Duration::from_secs(20);
const HEALTH_POLL: Duration = Duration::from_millis(400);
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);

/// Cap on discovery rows per call (first page only; continuations are
/// never followed).
pub const DISCOVERY_MAX_ROWS: usize = 50;
/// Cap on suggest tracks and browse cards per call.
pub const DISCOVERY_MAX_SUGGEST: usize = 20;
/// Per-call deadline for one discovery read.
pub const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(15);
/// Cap on related (radio) rows per fetch.
pub const RELATED_MAX_ROWS: usize = 25;
/// Maximum JSON discovery response accepted from the loopback sidecar.
pub const DISCOVERY_MAX_BYTES: usize = 4 * 1024 * 1024;

/// The only media hosts the sidecar legitimately resolves for
/// YouTube (measured live: `rr{N}---sn-XXXX.googlevideo.com`, the cache
/// node varies per request — only the registrable domain is stable).
pub fn yt_stream_host_allowed(url: &str) -> bool {
    let Ok(u) = url::Url::parse(url) else {
        return false;
    };
    if u.scheme() != "https" {
        return false;
    }
    u.host_str().map_or(false, |h| {
        h == "googlevideo.com" || h.ends_with(".googlevideo.com")
    })
}

/// The browse-id charset YouTube Music uses (`UC…`, `VL…`, `MPREb…`,
/// `LM`): ASCII alphanumerics, `-`, `_`; short. Anything else is rejected
/// before it reaches a URL path segment.
pub fn valid_browse_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub fn valid_continuation(token: &str) -> bool {
    !token.is_empty() && token.len() <= 4096 && token.bytes().all(|b| !b.is_ascii_control())
}

pub struct YtMusicSource {
    port: Mutex<u16>,
    child: RwLock<Arc<Mutex<Child>>>,
    http: reqwest::Client,
    version: Mutex<Option<String>>,
    /// Per-launch shared secret: the sidecar requires it on every request
    /// (`X-Sidecar-Token`), so a local process that merely discovers the
    /// loopback port cannot use the API. It cannot stop a process that
    /// wins the bind race and observes the header — that residual needs
    /// OS peer credentials.
    token: String,
    /// Kept so the supervisor can respawn after an unexpected exit.
    jar: PathBuf,
    java: String,
    restart: Mutex<Restart>,
}

/// Exponential-backoff state for sidecar restarts: 500 ms doubling to
/// a 30 s cap, reset by one healthy health check.
struct Restart {
    backoff: Duration,
    next_restart: Option<Instant>,
}

impl Restart {
    const INITIAL: Duration = Duration::from_millis(500);
    const MAX: Duration = Duration::from_secs(30);

    fn new() -> Self {
        Self {
            backoff: Self::INITIAL,
            next_restart: None,
        }
    }

    /// `Ok(())` = a restart attempt may run now (the gate is consumed);
    /// `Err(wait)` = still inside the backoff window.
    fn gate(&mut self, now: Instant) -> Result<(), Duration> {
        if let Some(at) = self.next_restart {
            if now < at {
                return Err(at - now);
            }
        }
        // Arm the next window before spawning: a failed attempt must not
        // retry in a tight loop.
        self.next_restart = Some(now + self.backoff);
        self.backoff = (self.backoff * 2).min(Self::MAX);
        Ok(())
    }

    fn record_success(&mut self) {
        self.backoff = Self::INITIAL;
        self.next_restart = None;
    }
}

#[derive(Debug, Deserialize)]
struct HealthResponse {
    ok: bool,
    #[serde(default)]
    version: Option<String>,
}

/// Header carrying the per-launch sidecar secret (see `YtMusicSource::token`).
const SIDECAR_TOKEN_HEADER: &str = "X-Sidecar-Token";

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    tracks: Vec<SidecarSong>,
    #[serde(default)]
    browse: Vec<SuggestCard>,
    #[serde(default)]
    continuation: Option<String>,
    #[serde(default)]
    queries: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct SidecarArtist {
    name: String,
    #[serde(rename = "browseId", default)]
    browse_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SidecarSong {
    #[serde(rename = "videoId")]
    video_id: String,
    title: String,
    #[serde(default)]
    artist: String,
    #[serde(default)]
    album: Option<String>,
    #[serde(rename = "albumId", default)]
    album_id: Option<String>,
    // The sidecar's songJson emits camelCase names (Json.kt).
    #[serde(rename = "durationSec", default)]
    duration_sec: Option<u32>,
    #[serde(rename = "thumbnailUrl", default)]
    thumbnail_url: Option<String>,
    #[serde(default)]
    artists: Vec<SidecarArtist>,
    #[serde(rename = "isVideo", default)]
    is_video: bool,
    #[serde(rename = "isExplicit", default)]
    is_explicit: bool,
}

/// Conformance (metadata normalization): the flat `artist` string wins;
/// when a source row omits it, the credited `artists` list joins with
/// ", " so every source produces the same single-string shape.
fn join_artist(song: &SidecarSong) -> String {
    if !song.artist.trim().is_empty() {
        song.artist.trim().to_string()
    } else {
        song.artists
            .iter()
            .map(|a| a.name.trim())
            .filter(|n| !n.is_empty())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[derive(Debug, Deserialize)]
struct StreamResponse {
    url: String,
    #[serde(default)]
    kbps: Option<u32>,
    #[serde(default)]
    container: Option<String>,
    /// Epoch milliseconds when the URL expires (sidecar STREAM_TTL). The
    /// wire name is camelCase (`expiresAt`, Json.kt/Main.kt); the rename is
    /// required or the resolver's URL-TTL clamp does not apply.
    #[serde(rename = "expiresAt", default)]
    expires_at: Option<i64>,
}

/// The wire shape of one browse page (`/album/{id}`, `/artist/{id}`,
/// `/playlist/{id}`): header when the response carried one.
#[derive(Debug, Deserialize)]
pub(crate) struct BrowsePageResponse {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    subtitle: Option<String>,
    #[serde(rename = "thumbnailUrl", default)]
    thumbnail_url: Option<String>,
    #[serde(default)]
    tracks: Vec<SidecarSong>,
    #[serde(default)]
    suggested: Vec<SidecarSong>,
    #[serde(default)]
    continuation: Option<String>,
}

/// The wire shape of `/suggest?q=` (the typeahead page): playable rows plus
/// browse cards (artist/album/playlist links).
#[derive(Debug, Deserialize)]
pub(crate) struct SuggestResponse {
    #[serde(default)]
    tracks: Vec<SidecarSong>,
    #[serde(default)]
    browse: Vec<SuggestCard>,
    #[serde(default)]
    continuation: Option<String>,
    #[serde(default)]
    queries: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SuggestCard {
    kind: String,
    #[serde(rename = "browseId")]
    browse_id: String,
    title: String,
    #[serde(default)]
    subtitle: Option<String>,
    #[serde(rename = "thumbnailUrl", default)]
    thumbnail_url: Option<String>,
}

/// One playable discovery row — the TrackKey plus display metadata.
#[derive(Debug, Clone, Serialize)]
pub struct DiscoveryTrack {
    pub key: TrackKey,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_sec: Option<u32>,
    pub thumbnail_url: Option<String>,
    pub artists: Vec<DiscoveryArtist>,
    pub album_id: Option<String>,
    pub is_video: bool,
    pub explicit: bool,
    pub playable: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscoveryArtist {
    pub name: String,
    pub browse_id: Option<String>,
}

/// One browse card from typeahead (an artist/album/playlist link).
#[derive(Debug, Clone, Serialize)]
pub struct DiscoveryCard {
    pub kind: String,
    pub browse_id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscoverySearchPage {
    pub tracks: Vec<DiscoveryTrack>,
    pub cards: Vec<DiscoveryCard>,
    pub queries: Vec<String>,
    pub continuation: Option<String>,
}

/// One browse page (album/artist/playlist), first page only.
#[derive(Debug, Clone, Serialize)]
pub struct DiscoveryPage {
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub thumbnail_url: Option<String>,
    pub tracks: Vec<DiscoveryTrack>,
    pub suggested: Vec<DiscoveryTrack>,
    pub continuation: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct HomeResponse {
    #[serde(default)]
    shelves: Vec<HomeShelfWire>,
    #[serde(default)]
    continuation: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct HomeShelfWire {
    title: String,
    #[serde(default)]
    subtitle: String,
    #[serde(default)]
    layout: String,
    #[serde(rename = "moreBrowseId", default)]
    more_browse_id: Option<String>,
    #[serde(rename = "moreParams", default)]
    more_params: Option<String>,
    #[serde(default)]
    items: Vec<HomeItemWire>,
}

#[derive(Debug, Clone, Deserialize)]
struct HomeItemWire {
    title: String,
    #[serde(default)]
    subtitle: String,
    #[serde(rename = "thumbnailUrl", default)]
    thumbnail_url: Option<String>,
    #[serde(rename = "videoId", default)]
    video_id: Option<String>,
    #[serde(rename = "browseId", default)]
    browse_id: Option<String>,
    #[serde(rename = "isVideo", default)]
    is_video: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscoveryHome {
    pub shelves: Vec<DiscoveryShelf>,
    pub continuation: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscoveryShelf {
    pub title: String,
    pub subtitle: String,
    pub layout: String,
    pub more_browse_id: Option<String>,
    pub more_params: Option<String>,
    pub items: Vec<DiscoveryHomeItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscoveryHomeItem {
    pub title: String,
    pub subtitle: String,
    pub thumbnail_url: Option<String>,
    pub video_id: Option<String>,
    pub browse_id: Option<String>,
    pub is_video: bool,
}

/// Maps one sidecar song row to a discovery row; `TrackKey::new` rejects
/// over-length ids rather than truncating.
fn discovery_track(t: SidecarSong) -> Result<DiscoveryTrack, SourceError> {
    let playable = !t.video_id.is_empty();
    let key = TrackKey::new(SourceKind::YouTube, t.video_id.clone())?;
    let artist = join_artist(&t);
    Ok(DiscoveryTrack {
        key,
        title: t.title,
        artist,
        album: t.album,
        duration_sec: t.duration_sec,
        thumbnail_url: t.thumbnail_url,
        artists: t
            .artists
            .into_iter()
            .map(|a| DiscoveryArtist {
                name: a.name,
                browse_id: a.browse_id,
            })
            .collect(),
        album_id: t.album_id,
        is_video: t.is_video,
        explicit: t.is_explicit,
        playable,
    })
}

/// Maps a `/suggest` wire answer to typed rows under the caller's caps.
pub(crate) fn map_suggest(
    parsed: SuggestResponse,
    cap_tracks: usize,
    cap_cards: usize,
) -> Result<(Vec<DiscoveryTrack>, Vec<DiscoveryCard>), SourceError> {
    let tracks = parsed
        .tracks
        .into_iter()
        .take(cap_tracks)
        .map(discovery_track)
        .collect::<Result<Vec<_>, _>>()?;
    let cards = parsed
        .browse
        .into_iter()
        .take(cap_cards)
        .map(|c| DiscoveryCard {
            kind: c.kind,
            browse_id: c.browse_id,
            title: c.title,
            subtitle: c.subtitle,
            thumbnail_url: c.thumbnail_url,
        })
        .collect();
    Ok((tracks, cards))
}

fn map_search(
    parsed: SearchResponse,
    cap_tracks: usize,
    cap_cards: usize,
) -> Result<DiscoverySearchPage, SourceError> {
    let tracks = parsed
        .tracks
        .into_iter()
        .take(cap_tracks)
        .map(discovery_track)
        .collect::<Result<Vec<_>, _>>()?;
    let cards = parsed
        .browse
        .into_iter()
        .take(cap_cards)
        .map(|c| DiscoveryCard {
            kind: c.kind,
            browse_id: c.browse_id,
            title: c.title,
            subtitle: c.subtitle,
            thumbnail_url: c.thumbnail_url,
        })
        .collect();
    Ok(DiscoverySearchPage {
        tracks,
        cards,
        queries: parsed.queries,
        continuation: parsed.continuation,
    })
}

/// Maps a browse-page wire answer (album/artist/playlist) under the cap.
pub(crate) fn map_browse(
    parsed: BrowsePageResponse,
    cap: usize,
) -> Result<DiscoveryPage, SourceError> {
    let tracks = parsed
        .tracks
        .into_iter()
        .take(cap)
        .map(discovery_track)
        .collect::<Result<Vec<_>, _>>()?;
    let suggested = parsed
        .suggested
        .into_iter()
        .take(cap)
        .map(discovery_track)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(DiscoveryPage {
        title: parsed.title,
        subtitle: parsed.subtitle,
        thumbnail_url: parsed.thumbnail_url,
        tracks,
        suggested,
        continuation: parsed.continuation,
    })
}

fn map_home(parsed: HomeResponse, cap_shelves: usize, cap_items: usize) -> DiscoveryHome {
    DiscoveryHome {
        shelves: parsed
            .shelves
            .into_iter()
            .take(cap_shelves)
            .map(|shelf| DiscoveryShelf {
                title: shelf.title,
                subtitle: shelf.subtitle,
                layout: shelf.layout,
                more_browse_id: shelf.more_browse_id,
                more_params: shelf.more_params,
                items: shelf
                    .items
                    .into_iter()
                    .take(cap_items)
                    .map(|item| DiscoveryHomeItem {
                        title: item.title,
                        subtitle: item.subtitle,
                        thumbnail_url: item.thumbnail_url,
                        video_id: item.video_id,
                        browse_id: item.browse_id,
                        is_video: item.is_video,
                    })
                    .collect(),
            })
            .collect(),
        continuation: parsed.continuation,
    }
}

/// Container name on the sidecar wire → MIME type for `ResolvedMedia`.
pub(crate) fn container_to_mime(container: &str) -> Option<&'static str> {
    match container.to_ascii_lowercase().as_str() {
        "mp4" | "m4a" => Some("audio/mp4"),
        "webm" => Some("audio/webm"),
        _ => None,
    }
}

/// Spawns `java -jar <jar> --port <free>` and polls /health until it answers
/// or the deadline passes (shared by `launch` and the supervisor's
/// restart path). Runs the poll on a dedicated OS thread — reqwest's
/// blocking client must not be created or dropped inside an async runtime.
/// 128-bit random per-launch secret for the sidecar API (hex). getrandom
/// is the OS CSPRNG; no fallback — token generation must not silently
/// weaken.
fn sidecar_token() -> Result<String, SourceError> {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes)
        .map_err(|e| SourceError::Protocol(format!("sidecar token: {e}")))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// The jar and the JVM run as this user, so a path another account can replace is not
/// a path to execute: it must be a regular file and not writable by group or other.
/// A symlink is refused for the same reason, and so is anything that is not a file.
fn spawnable(path: &Path) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if !meta.file_type().is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o022 == 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn spawn_and_await(
    jar: &Path,
    java: &str,
    token: &str,
) -> Result<(u16, Arc<Mutex<Child>>, Option<String>), SourceError> {
    let port = free_port()?;
    let child = java_command(java)
        .arg("-jar")
        .arg(jar)
        .arg("--port")
        .arg(port.to_string())
        .env("TANSEN_SIDECAR_TOKEN", token)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| SourceError::Protocol(format!("sidecar spawn: {e}")))?;
    let child = Arc::new(Mutex::new(child));

    let poll_child = child.clone();
    let token = token.to_string();
    let healthy = std::thread::scope(move |scope| {
        scope
            .spawn(move || {
                let client = reqwest::blocking::Client::builder()
                    .timeout(Duration::from_secs(3))
                    .redirect(reqwest::redirect::Policy::none()) // sidecar never redirects
                    .build()
                    .ok()?;
                let deadline = Instant::now() + HEALTH_TIMEOUT;
                loop {
                    let answered = client
                        .get(format!("http://127.0.0.1:{port}/health"))
                        .header(SIDECAR_TOKEN_HEADER, &token)
                        .send()
                        .ok()
                        .and_then(|r| r.json::<HealthResponse>().ok());
                    if let Some(h) = answered {
                        if h.ok {
                            return Some(h.version);
                        }
                    }
                    if Instant::now() > deadline {
                        return None;
                    }
                    // A sidecar that exited (bad jar path, port clash)
                    // should not be waited on for the full deadline.
                    if let Ok(Some(_)) = poll_child.lock().unwrap().try_wait() {
                        return None;
                    }
                    std::thread::sleep(HEALTH_POLL);
                }
            })
            .join()
            .ok()
            .flatten()
    });

    let Some(version) = healthy else {
        let _ = child.lock().unwrap().kill();
        return Err(SourceError::Protocol(
            "sidecar did not become healthy in time".into(),
        ));
    };
    // The poll thread has joined, so this Arc is unique again.
    Ok((port, child, version))
}

impl YtMusicSource {
    /// Spawns the sidecar jar on a free loopback port and waits for /health.
    ///
    /// Jar and java discovery (first hit wins):
    /// 1. `TANSEN_SIDECAR_JAR` env,
    /// 2. the `sidecar_jar` setting,
    /// 3. `sidecar/build/libs/sidecar-all.jar` next to the running executable
    ///    or in the workspace checkout.
    /// Java: the `java_path` setting, then `$JAVA_HOME/bin/java`, then `java`
    /// on PATH.
    pub fn launch(
        jar_path: Option<PathBuf>,
        java_path: Option<String>,
    ) -> Result<Self, SourceError> {
        let token = sidecar_token()?;
        let jar = jar_path.or_else(discover_jar).ok_or_else(|| {
            SourceError::Protocol(
                "sidecar jar not found (set TANSEN_SIDECAR_JAR or the sidecar_jar setting)".into(),
            )
        })?;
        let java = java_path.or_else(discover_java).ok_or_else(|| {
            SourceError::Protocol(
                "no Java runtime found (set the java_path setting or JAVA_HOME)".into(),
            )
        })?;
        if !spawnable(&jar) {
            return Err(SourceError::Protocol(format!(
                "sidecar jar {} is not a regular file or is writable by another account",
                jar.display()
            )));
        }
        if !spawnable(Path::new(&java)) {
            return Err(SourceError::Protocol(format!(
                "java runtime {java} is not a regular file or is writable by another account"
            )));
        }
        let (port, child, version) = spawn_and_await(&jar, &java, &token)?;
        Ok(Self {
            port: Mutex::new(port),
            child: RwLock::new(child),
            http: reqwest::Client::builder()
                .timeout(HTTP_TIMEOUT)
                .user_agent("Tansen/0.3")
                .redirect(reqwest::redirect::Policy::none()) // the sidecar never redirects
                .build()
                .map_err(|e| SourceError::Protocol(format!("http client: {e}")))?,
            version: Mutex::new(version),
            token,
            jar,
            java,
            restart: Mutex::new(Restart::new()),
        })
    }

    /// Supervisor: make sure the JVM is alive before a call. A dead or
    /// exited process is restarted through the exponential-backoff gate —
    /// inside the window the caller gets an error immediately instead of a
    /// spawn attempt. Never panics: every path returns a `SourceError`.
    fn ensure_running(&self) -> Result<(), SourceError> {
        let dead = {
            let child = self
                .child
                .read()
                .map_err(|_| SourceError::Protocol("sidecar supervisor lock poisoned".into()))?;
            let mut guard = child
                .lock()
                .map_err(|_| SourceError::Protocol("sidecar supervisor lock poisoned".into()))?;
            matches!(guard.try_wait(), Ok(Some(_)) | Err(_))
        };
        if !dead {
            return Ok(());
        }
        {
            let mut restart = self
                .restart
                .lock()
                .map_err(|_| SourceError::Protocol("sidecar supervisor lock poisoned".into()))?;
            if let Err(wait) = restart.gate(Instant::now()) {
                return Err(SourceError::Protocol(format!(
                    "sidecar restarting (backoff {:.1}s)",
                    wait.as_secs_f64()
                )));
            }
        }
        let (port, child, version) = spawn_and_await(&self.jar, &self.java, &self.token)?;
        *self
            .child
            .write()
            .map_err(|_| SourceError::Protocol("sidecar supervisor lock poisoned".into()))? = child;
        *self
            .port
            .lock()
            .map_err(|_| SourceError::Protocol("sidecar supervisor lock poisoned".into()))? = port;
        *self
            .version
            .lock()
            .map_err(|_| SourceError::Protocol("sidecar supervisor lock poisoned".into()))? =
            version;
        self.restart
            .lock()
            .map_err(|_| SourceError::Protocol("sidecar supervisor lock poisoned".into()))?
            .record_success();
        Ok(())
    }

    /// Process id of the live JVM, for supervision tests and diagnostics.
    pub fn pid(&self) -> Option<u32> {
        self.child
            .read()
            .ok()
            .and_then(|c| c.lock().ok().map(|ch| ch.id()))
    }

    async fn get(&self, path: &str) -> Result<reqwest::Response, SourceError> {
        self.ensure_running()?;
        let port = *self
            .port
            .lock()
            .map_err(|_| SourceError::Protocol("sidecar supervisor lock poisoned".into()))?;
        self.http
            .get(format!("http://127.0.0.1:{port}{path}"))
            .header(SIDECAR_TOKEN_HEADER, &self.token)
            .send()
            .await
            .map_err(|e| SourceError::Network(format!("sidecar: {e}")))
    }

    async fn json_limited<T: serde::de::DeserializeOwned>(
        response: reqwest::Response,
    ) -> Result<T, SourceError> {
        // Cap while reading: `Response::bytes()` would buffer an unbounded
        // body before the size check below could run.
        if let Some(len) = response.content_length() {
            if len > DISCOVERY_MAX_BYTES as u64 {
                return Err(SourceError::Permanent(
                    "sidecar discovery response too large".into(),
                ));
            }
        }
        let mut response = response;
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| SourceError::Network(format!("sidecar body: {e}")))?
        {
            bytes.extend_from_slice(&chunk);
            if bytes.len() > DISCOVERY_MAX_BYTES {
                return Err(SourceError::Permanent(
                    "sidecar discovery response too large".into(),
                ));
            }
        }
        serde_json::from_slice(&bytes)
            .map_err(|e| SourceError::Protocol(format!("sidecar discovery JSON: {e}")))
    }

    async fn health_url(&self) -> Result<String, SourceError> {
        self.ensure_running()?;
        let response = self.get("/health").await?;
        let parsed: HealthResponse = Self::json_limited(response).await?;
        if !parsed.ok {
            return Err(SourceError::Protocol("sidecar reported not-ok".into()));
        }
        *self.version.lock().unwrap() = parsed.version.clone();
        Ok(parsed.version.unwrap_or_default())
    }

    pub fn version(&self) -> Option<String> {
        self.version.lock().unwrap().clone()
    }

    // Discovery (inherent client methods; NOT on the source trait)

    /// Typeahead rows: playable tracks + browse cards. Unauthenticated
    /// upstream (the sidecar's typeahead path strips the session cookie) —
    /// no account sign-in, cookies or tokens involved.
    pub async fn suggest(&self, query: &str) -> Result<DiscoverySearchPage, SourceError> {
        tokio::time::timeout(DISCOVERY_TIMEOUT, async {
            let response = self
                .get(&format!("/suggest?q={}", urlencoding_lite(query)))
                .await?
                .error_for_status()
                .map_err(|e| SourceError::Protocol(format!("sidecar suggest: {e}")))?;
            let parsed: SuggestResponse = Self::json_limited(response).await?;
            let queries = parsed.queries.clone();
            let continuation = parsed.continuation.clone();
            let (tracks, cards) =
                map_suggest(parsed, DISCOVERY_MAX_SUGGEST, DISCOVERY_MAX_SUGGEST)?;
            Ok(DiscoverySearchPage {
                tracks,
                cards,
                queries,
                continuation,
            })
        })
        .await
        .map_err(|_| SourceError::Timeout("discovery timed out".into()))?
    }

    /// One album's first page.
    pub async fn album(&self, id: &str) -> Result<DiscoveryPage, SourceError> {
        self.browse("album", id).await
    }

    /// One artist's page (top tracks via the sidecar's layout-agnostic walk).
    pub async fn artist(&self, id: &str) -> Result<DiscoveryPage, SourceError> {
        self.browse("artist", id).await
    }

    /// One playlist's first page (the sidecar VL-prefixes bare ids).
    pub async fn playlist(&self, id: &str) -> Result<DiscoveryPage, SourceError> {
        self.browse("playlist", id).await
    }

    pub async fn browse_continuation(&self, token: &str) -> Result<DiscoveryPage, SourceError> {
        if !valid_continuation(token) {
            return Err(SourceError::Permanent("invalid continuation".into()));
        }
        tokio::time::timeout(DISCOVERY_TIMEOUT, async {
            let response = self
                .get(&format!(
                    "/browse/continuation?token={}",
                    urlencoding_lite(token)
                ))
                .await?
                .error_for_status()
                .map_err(|e| SourceError::Protocol(format!("sidecar continuation: {e}")))?;
            let parsed: BrowsePageResponse = Self::json_limited(response).await?;
            map_browse(parsed, DISCOVERY_MAX_ROWS)
        })
        .await
        .map_err(|_| SourceError::Timeout("discovery timed out".into()))?
    }

    pub async fn search_page(
        &self,
        query: &str,
        filter: &str,
        continuation: Option<&str>,
    ) -> Result<DiscoverySearchPage, SourceError> {
        if query.trim().is_empty() || query.len() > crate::search::SEARCH_MAX_QUERY {
            return Err(SourceError::Permanent("invalid search query".into()));
        }
        if !matches!(
            filter,
            "all" | "songs" | "videos" | "albums" | "artists" | "playlists"
        ) {
            return Err(SourceError::Permanent("invalid search filter".into()));
        }
        if let Some(token) = continuation {
            if !valid_continuation(token) {
                return Err(SourceError::Permanent("invalid continuation".into()));
            }
        }
        tokio::time::timeout(DISCOVERY_TIMEOUT, async {
            let mut path = format!("/search?q={}&filter={}", urlencoding_lite(query), filter);
            if let Some(token) = continuation {
                path.push_str("&continuation=");
                path.push_str(&urlencoding_lite(token));
            }
            let response = self
                .get(&path)
                .await?
                .error_for_status()
                .map_err(|e| SourceError::Protocol(format!("sidecar search: {e}")))?;
            let parsed: SearchResponse = Self::json_limited(response).await?;
            map_search(parsed, DISCOVERY_MAX_ROWS, DISCOVERY_MAX_SUGGEST)
        })
        .await
        .map_err(|_| SourceError::Timeout("discovery timed out".into()))?
    }

    pub async fn home(&self) -> Result<DiscoveryHome, SourceError> {
        self.home_page(None).await
    }

    pub async fn home_continuation(&self, token: &str) -> Result<DiscoveryHome, SourceError> {
        self.home_page(Some(token)).await
    }

    pub async fn explore(&self, section: &str) -> Result<DiscoveryHome, SourceError> {
        if !matches!(section, "explore" | "new-releases") {
            return Err(SourceError::Permanent("invalid explore section".into()));
        }
        tokio::time::timeout(DISCOVERY_TIMEOUT, async {
            let response = self
                .get(&format!("/explore?section={}", urlencoding_lite(section)))
                .await?
                .error_for_status()
                .map_err(|e| SourceError::Protocol(format!("sidecar explore: {e}")))?;
            let parsed: HomeResponse = Self::json_limited(response).await?;
            Ok(map_home(parsed, 24, 24))
        })
        .await
        .map_err(|_| SourceError::Timeout("discovery timed out".into()))?
    }

    async fn home_page(&self, token: Option<&str>) -> Result<DiscoveryHome, SourceError> {
        if let Some(token) = token {
            if !valid_continuation(token) {
                return Err(SourceError::Permanent("invalid continuation".into()));
            }
        }
        tokio::time::timeout(DISCOVERY_TIMEOUT, async {
            let path = token
                .map(|token| format!("/home?continuation={}", urlencoding_lite(token)))
                .unwrap_or_else(|| "/home".into());
            let response = self
                .get(&path)
                .await?
                .error_for_status()
                .map_err(|e| SourceError::Protocol(format!("sidecar home: {e}")))?;
            let parsed: HomeResponse = Self::json_limited(response).await?;
            Ok(map_home(parsed, 24, 24))
        })
        .await
        .map_err(|_| SourceError::Timeout("discovery timed out".into()))?
    }

    async fn browse(&self, kind: &str, id: &str) -> Result<DiscoveryPage, SourceError> {
        if !valid_browse_id(id) {
            return Err(SourceError::Permanent("invalid browse id".into()));
        }
        tokio::time::timeout(DISCOVERY_TIMEOUT, async {
            let response = self
                .get(&format!("/{}/{}", kind, urlencoding_lite(id)))
                .await?
                .error_for_status()
                .map_err(|e| SourceError::Protocol(format!("sidecar {kind}: {e}")))?;
            let parsed: BrowsePageResponse = Self::json_limited(response).await?;
            map_browse(parsed, DISCOVERY_MAX_ROWS)
        })
        .await
        .map_err(|_| SourceError::Timeout("discovery timed out".into()))?
    }

    /// Related tracks for the playing entry (the sidecar's radio
    /// endpoint). Same caps and per-call deadline as the other discovery
    /// reads.
    pub async fn related(&self, key: &TrackKey) -> Result<Vec<DiscoveryTrack>, SourceError> {
        if key.source != SourceKind::YouTube || !valid_browse_id(&key.id) {
            return Err(SourceError::Permanent("invalid track id".into()));
        }
        tokio::time::timeout(DISCOVERY_TIMEOUT, async {
            let response = self
                .get(&format!("/radio/{}", urlencoding_lite(&key.id)))
                .await?
                .error_for_status()
                .map_err(|e| SourceError::Protocol(format!("sidecar radio: {e}")))?;
            let parsed: SearchResponse = Self::json_limited(response).await?;
            let rows = parsed
                .tracks
                .into_iter()
                .take(RELATED_MAX_ROWS)
                .map(discovery_track)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
        .map_err(|_| SourceError::Timeout("related timed out".into()))?
    }
}

impl YtMusicSource {
    /// Kill the sidecar JVM. The shell calls this on app exit; `Drop` does the
    /// same, but a process that exits without dropping its state would leave
    /// an orphaned listener for the next launch.
    pub fn shutdown(&self) {
        if let Ok(child) = self.child.read() {
            if let Ok(mut ch) = child.lock() {
                let _ = ch.kill();
                let _ = ch.wait();
            }
        }
    }
}

impl Drop for YtMusicSource {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn free_port() -> Result<u16, SourceError> {
    TcpListener::bind("127.0.0.1:0")
        .map(|l| l.local_addr().unwrap().port())
        .map_err(|e| SourceError::Network(e.to_string()))
}

fn discover_jar() -> Option<PathBuf> {
    if let Ok(env) = std::env::var("TANSEN_SIDECAR_JAR") {
        let p = PathBuf::from(env);
        if p.is_file() {
            return Some(p);
        }
    }
    // Candidate bases: the running executable's directory (installed app),
    // its parent (target/debug), the working directory, and the dev
    // checkout's root (two up from the crate dir under `cargo test`).
    let mut bases: Vec<PathBuf> = Vec::new();
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(PathBuf::from))
    {
        bases.push(exe_dir.clone());
        if let Some(parent) = exe_dir.parent() {
            bases.push(parent.to_path_buf());
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        bases.push(cwd.clone());
        bases.push(cwd.parent().map(|p| p.to_path_buf()).unwrap_or(cwd));
    }
    for base in bases {
        for relative in [
            PathBuf::from("resources/sidecar-all.jar"),
            PathBuf::from("apps/desktop/src-tauri/resources/sidecar-all.jar"),
            PathBuf::from("BitChord/sidecar/build/libs/sidecar-all.jar"),
            PathBuf::from("../BitChord/sidecar/build/libs/sidecar-all.jar"),
        ] {
            let candidate = base.join(&relative);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// The JRE launcher's file name on this platform.
fn java_binary_name() -> &'static str {
    if cfg!(windows) {
        "java.exe"
    } else {
        "java"
    }
}

/// A JVM command that opens no console window on Windows.
fn java_command(java: &str) -> Command {
    #[allow(unused_mut)]
    let mut command = Command::new(java);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

fn discover_java() -> Option<String> {
    if let Ok(home) = std::env::var("JAVA_HOME") {
        let java = PathBuf::from(home).join("bin").join(java_binary_name());
        if java.is_file() {
            return Some(java.to_string_lossy().into_owned());
        }
    }
    // A PATH lookup by spawning `java -version` is the cheapest existence
    // check. Success means exit 0 — the platform stub binary exits 1.
    if java_command(java_binary_name())
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        // Same name the probe used: "java" on Unix, "java.exe" on Windows.
        return Some(java_binary_name().into());
    }
    None
}

#[async_trait]
impl MusicSource for YtMusicSource {
    fn config_id(&self) -> &str {
        "yt"
    }

    fn kind(&self) -> SourceKind {
        SourceKind::YouTube
    }

    fn display_name(&self) -> &str {
        "YouTube Music"
    }

    async fn health(&self) -> Result<SourceHealth, SourceError> {
        match self.health_url().await {
            Ok(version) if !version.is_empty() => Ok(SourceHealth::Ok {
                detail: Some(version),
            }),
            Ok(_) => Ok(SourceHealth::Ok { detail: None }),
            Err(SourceError::Network(e)) => Ok(SourceHealth::Unreachable { reason: e }),
            Err(e) => Ok(SourceHealth::Rejected {
                reason: e.to_string(),
            }),
        }
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<CatalogTrack>, SourceError> {
        let response = self
            .get(&format!(
                "/search?q={}&filter=songs",
                urlencoding_lite(query)
            ))
            .await?
            .error_for_status()
            .map_err(|e| SourceError::Protocol(format!("sidecar search: {e}")))?;
        let parsed: SearchResponse = Self::json_limited(response).await?;
        Ok(parsed
            .tracks
            .into_iter()
            .take(limit)
            .map(|t| CatalogTrack {
                track_id: t.video_id.clone(),
                config_id: self.config_id().to_string(),
                title: t.title.trim().to_string(),
                artist: join_artist(&t),
                album: t.album,
                duration_sec: t.duration_sec,
                artwork_url: t.thumbnail_url,
                // Rows don't grade themselves; the stream answer does.
                format: None,
                embedded_stream_url: None,
            })
            .collect())
    }

    async fn stream(
        &self,
        key: &TrackKey,
        request: &StreamRequest,
    ) -> Result<Option<SourceStream>, SourceError> {
        let track_id = &key.id;
        // `prefer=aac` asks the sidecar for a progressive MP4/AAC rendition —
        // the container Symphonia decodes directly. The hint is sent for
        // EVERY tier including Lossless — measured supply (live sidecar):
        // the no-prefer answer is itag 251 = Opus/WebM, which no released
        // symphonia decodes (`symphonia-codec-opus` does not exist on
        // crates.io for 0.5 or 0.6), so an un-preferenced Lossless resolve
        // failed decode. YouTube Music has no lossless supply; AAC itag 140
        // is the best available and the resolver marks below-request.
        let hint = prefer_codec_hint(request);
        let response = self
            .get(&format!(
                "/track/{}/stream?prefer=aac{hint}",
                urlencoding_lite(track_id)
            ))
            .await?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let response = response
            .error_for_status()
            .map_err(|e| SourceError::Protocol(format!("sidecar stream: {e}")))?;
        let parsed: StreamResponse = Self::json_limited(response).await?;
        // Media host allowlist — https + googlevideo.com suffix only.
        if !yt_stream_host_allowed(&parsed.url) {
            return Err(SourceError::Permanent(
                "sidecar stream host outside the googlevideo.com allowlist".into(),
            ));
        }
        Ok(Some(SourceStream {
            url: parsed.url,
            format: StreamFormat {
                codec: "AAC".into(),
                is_lossless: Some(false),
                kbps: parsed.kbps,
                is_dolby_atmos: false,
                sample_rate: None,
                bit_depth: None,
                kbps_inferred: parsed.kbps.is_some(),
            },
            headers: vec![],
            below_request: false,
            duration_sec: None,
            mime: parsed
                .container
                .as_deref()
                .and_then(container_to_mime)
                .map(str::to_owned),
            expiry_ms: parsed.expires_at,
            container: Some(Container::Progressive),
        }))
    }
}

/// The codec-preference hint for a stream request: every tier asks the
/// sidecar for AAC — the only container family the decode stack accepts
/// from this source (see the `stream` doc comment for the measured supply).
fn prefer_codec_hint(request: &StreamRequest) -> &'static str {
    match request {
        StreamRequest::Lossless => "&prefer_codec=aac",
        StreamRequest::Capped { .. } | StreamRequest::Best => "&prefer_codec=aac",
    }
}

/// The one character set that breaks URLs here.
fn urlencoding_lite(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Download at most `max_bytes` of a stream URL, for probe-style tests. Reads
/// are plain GETs; googlevideo serves full-file progressive bytes.
pub async fn fetch_prefix(url: &str, max_bytes: usize) -> Result<Vec<u8>, SourceError> {
    let response = reqwest::get(url)
        .await
        // googlevideo URLs carry signed query parameters; never echo them.
        .map_err(|e| SourceError::Network(e.without_url().to_string()))?
        .error_for_status()
        .map_err(|e| SourceError::Network(e.without_url().to_string()))?;
    let mut bytes = Vec::with_capacity(max_bytes.min(1 << 20));
    let mut response = response;
    while bytes.len() < max_bytes {
        match response
            .chunk()
            .await
            .map_err(|e| SourceError::Network(e.without_url().to_string()))?
        {
            Some(chunk) => {
                let take = chunk.len().min(max_bytes - bytes.len());
                bytes.extend_from_slice(&chunk[..take]);
                if bytes.len() >= max_bytes {
                    break;
                }
            }
            None => break,
        }
    }
    Ok(bytes)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test code")]
mod mime_tests {
    use super::container_to_mime;

    #[test]
    fn container_mime_mapping() {
        assert_eq!(container_to_mime("mp4"), Some("audio/mp4"));
        assert_eq!(
            container_to_mime("M4A"),
            Some("audio/mp4"),
            "case-insensitive"
        );
        assert_eq!(container_to_mime("webm"), Some("audio/webm"));
        assert_eq!(
            container_to_mime("opus"),
            None,
            "unsupported container stays unknown"
        );
        assert_eq!(container_to_mime(""), None);
    }
}

#[cfg(test)]
mod sidecar_token_tests {
    use super::sidecar_token;

    /// The per-launch secret must be 32 hex chars and never repeat: it is
    /// the only thing standing between a local port-scanner and the
    /// sidecar API (Main.kt enforces it on every route).
    #[test]
    fn token_is_32_hex_chars_and_unique_per_launch() {
        let a = sidecar_token().unwrap();
        let b = sidecar_token().unwrap();
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b, "CSPRNG output must not repeat");
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test code")]
mod restart_tests {
    use super::*;

    #[test]
    fn stream_response_parses_camel_case_expiry() {
        // The wire field is `expiresAt`; without the rename the resolver's
        // URL TTL never clamps to it.
        let parsed: StreamResponse = serde_json::from_str(
            r#"{"url":"https://x/y","format":"audio/mp4","kbps":131,"expiresAt":1799999999999}"#,
        )
        .unwrap();
        assert_eq!(parsed.expires_at, Some(1_799_999_999_999));
    }

    #[test]
    fn join_artist_prefers_flat_and_joins_credits() {
        let mk = |artist: &str, names: Vec<&str>| SidecarSong {
            video_id: "v".into(),
            title: "t".into(),
            artist: artist.into(),
            album: None,
            duration_sec: None,
            thumbnail_url: None,
            artists: names
                .into_iter()
                .map(|n| SidecarArtist {
                    name: n.into(),
                    browse_id: None,
                })
                .collect(),
            album_id: None,
            is_video: false,
            is_explicit: false,
        };
        assert_eq!(join_artist(&mk("Flat Artist", vec![])), "Flat Artist");
        assert_eq!(
            join_artist(&mk("", vec!["A", "B"])),
            "A, B",
            "credits joined when flat is empty"
        );
        assert_eq!(
            join_artist(&mk("  Flat  ", vec!["A"])),
            "Flat",
            "flat wins + trimmed"
        );
        assert_eq!(join_artist(&mk("", vec![])), "", "nothing to join");
    }

    #[test]
    fn prefer_codec_hint_always_aac() {
        // The supply-side fix — no tier may let the sidecar answer
        // Opus/WebM (itag 251), which the decode stack cannot read.
        assert_eq!(
            prefer_codec_hint(&StreamRequest::Lossless),
            "&prefer_codec=aac"
        );
        assert_eq!(prefer_codec_hint(&StreamRequest::Best), "&prefer_codec=aac");
        assert_eq!(
            prefer_codec_hint(&StreamRequest::Capped { max_kbps: 128 }),
            "&prefer_codec=aac"
        );
    }

    #[test]
    fn backoff_doubles_and_caps_and_resets() {
        let mut r = Restart::new();
        let mut now = Instant::now();
        let _ = &mut now;
        // First gate passes and arms a 500 ms window.
        r.gate(now).unwrap();
        // Inside the window: refused, and the refusal names the REMAINING
        // wait (not the configured interval).
        let wait = r.gate(now).unwrap_err();
        assert_eq!(wait, Duration::from_millis(500));
        // Past the window: passes again; backoff had doubled to 1 s, so the
        // next window is a full second out.
        now += Duration::from_millis(500);
        r.gate(now).unwrap();
        now += Duration::from_millis(500);
        assert_eq!(r.gate(now).unwrap_err(), Duration::from_millis(500));
        now += Duration::from_millis(500);
        r.gate(now).unwrap();
        // Success resets to the initial window.
        r.record_success();
        assert!(r.gate(now).is_ok());
    }

    #[test]
    fn backoff_never_exceeds_the_cap() {
        let mut r = Restart::new();
        let mut now = Instant::now();
        for _ in 0..20 {
            if r.gate(now).is_ok() {
                now += Duration::from_secs(60);
            }
        }
        assert!(
            r.backoff <= Restart::MAX,
            "capped at 30 s, got {:?}",
            r.backoff
        );
    }
}

/// Discovery parsing, validation and caps against saved fixture
/// responses (hand-authored wire shapes mirroring the sidecar's JSON
/// builders; sanitized — synthetic ids, no network captures, no secrets).
#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test code")]
mod discovery_tests {
    use super::*;

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/",).to_string() + name,
        )
        .unwrap()
    }

    #[test]
    fn suggest_fixture_maps_to_typed_rows() {
        let parsed: SuggestResponse =
            serde_json::from_str(&fixture("discovery_suggest.json")).unwrap();
        let (tracks, cards) =
            map_suggest(parsed, DISCOVERY_MAX_SUGGEST, DISCOVERY_MAX_SUGGEST).unwrap();
        assert_eq!(tracks.len(), 3);
        assert_eq!(tracks[0].key.source, SourceKind::YouTube);
        assert_eq!(tracks[0].key.id, "dQw4w9WgXcQ_1");
        assert_eq!(tracks[0].title, "Fixture Song One");
        assert_eq!(tracks[0].album.as_deref(), Some("Fixture Album"));
        assert_eq!(tracks[0].duration_sec, Some(201));
        assert_eq!(tracks[1].album, None, "optional album stays None");
        assert_eq!(tracks[1].thumbnail_url, None, "optional artwork stays None");
        assert_eq!(cards.len(), 3);
        assert_eq!(cards[0].kind, "artist");
        assert_eq!(cards[0].browse_id, "UC_fixture_artist_01");
        assert_eq!(cards[2].kind, "playlist");
        assert_eq!(cards[1].thumbnail_url, None);
    }

    #[test]
    fn suggest_caps_apply() {
        let parsed: SuggestResponse =
            serde_json::from_str(&fixture("discovery_suggest.json")).unwrap();
        let (tracks, cards) = map_suggest(parsed, 2, 1).unwrap();
        assert_eq!(tracks.len(), 2, "track cap");
        assert_eq!(cards.len(), 1, "card cap");
    }

    #[test]
    fn browse_fixture_maps_header_and_rows() {
        let parsed: BrowsePageResponse =
            serde_json::from_str(&fixture("discovery_browse.json")).unwrap();
        let page = map_browse(parsed, DISCOVERY_MAX_ROWS).unwrap();
        assert_eq!(page.title.as_deref(), Some("Fixture Album"));
        assert_eq!(page.subtitle.as_deref(), Some("Fixture Artist • 2024"));
        assert_eq!(
            page.thumbnail_url.as_deref(),
            Some("https://example.test/thumb/album.jpg")
        );
        assert_eq!(page.tracks.len(), 4);
        assert!(page
            .tracks
            .iter()
            .all(|t| t.key.source == SourceKind::YouTube));
        assert_eq!(page.tracks[3].key.id, "dQw4w9WgXcQ_4");
    }

    #[test]
    fn browse_cap_applies() {
        let parsed: BrowsePageResponse =
            serde_json::from_str(&fixture("discovery_browse.json")).unwrap();
        let page = map_browse(parsed, 2).unwrap();
        assert_eq!(page.tracks.len(), 2, "row cap");
        // Header survives the cap.
        assert_eq!(page.title.as_deref(), Some("Fixture Album"));
    }

    #[test]
    fn search_fixture_maps_typed_rows_and_continuation() {
        let parsed: SearchResponse =
            serde_json::from_str(&fixture("discovery_search.json")).unwrap();
        let page = map_search(parsed, DISCOVERY_MAX_ROWS, DISCOVERY_MAX_SUGGEST).unwrap();
        assert_eq!(page.tracks.len(), 1);
        assert_eq!(page.tracks[0].explicit, true);
        assert_eq!(
            page.tracks[0].album_id.as_deref(),
            Some("MPREb_fixture_album_1")
        );
        assert_eq!(page.cards.len(), 1);
        assert_eq!(page.continuation.as_deref(), Some("opaque-next-token"));
    }

    #[test]
    fn over_length_track_id_is_rejected_not_truncated() {
        // 1025-char id over the TrackKey cap (TRACK_ID_MAX_LEN).
        let long_id = "a".repeat(1025);
        let parsed: SuggestResponse = serde_json::from_str(&format!(
            r#"{{"tracks":[{{"videoId":"{long_id}","title":"t","artist":"a"}}]}}"#
        ))
        .unwrap();
        assert!(
            map_suggest(parsed, 10, 10).is_err(),
            "over-length id rejected"
        );
    }

    #[test]
    fn browse_id_validation() {
        assert!(valid_browse_id("UC_fixture_artist_01"));
        assert!(valid_browse_id("VL_fixture_playlist_1"));
        assert!(valid_browse_id("MPREb_fixture_album_1"));
        assert!(valid_browse_id("LM"));
        assert!(!valid_browse_id(""), "empty");
        assert!(!valid_browse_id(&"a".repeat(65)), "over 64");
        assert!(!valid_browse_id("UC../etc"), "path traversal charset");
        assert!(!valid_browse_id("UC x"), "space");
        assert!(!valid_browse_id("UC/slash"), "slash");
        assert!(!valid_browse_id("UC%41"), "percent");
    }

    #[test]
    fn continuation_validation_rejects_control_and_oversized_tokens() {
        assert!(valid_continuation("opaque-token_1"));
        assert!(!valid_continuation(""));
        assert!(!valid_continuation("opaque\n token"));
        assert!(!valid_continuation(&"x".repeat(4097)));
    }
}

#[cfg(test)]
mod host_allowlist_tests {
    use super::yt_stream_host_allowed;

    #[test]
    fn allows_googlevideo_https_suffix() {
        assert!(yt_stream_host_allowed(
            "https://rr3---sn-gwpa-pmfe.googlevideo.com/videoplayback?x=1"
        ));
        assert!(yt_stream_host_allowed(
            "https://googlevideo.com/videoplayback"
        ));
    }

    #[test]
    fn rejects_other_schemes_and_hosts() {
        assert!(!yt_stream_host_allowed(
            "http://rr3---sn-gwpa-pmfe.googlevideo.com/videoplayback"
        ));
        assert!(!yt_stream_host_allowed(
            "https://evil.example/videoplayback"
        ));
        assert!(!yt_stream_host_allowed(
            "https://googlevideo.com.evil.example/x"
        ));
        assert!(!yt_stream_host_allowed("file:///etc/passwd"));
        assert!(!yt_stream_host_allowed("not a url"));
    }
}

#[cfg(test)]
mod spawn_tests {
    use super::spawnable;
    use std::path::Path;

    #[test]
    fn only_a_regular_file_another_account_cannot_write_is_executed() {
        let dir = std::env::temp_dir().join(format!("tansen-spawn-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let jar = dir.join("sidecar-all.jar");
        std::fs::write(&jar, b"x").unwrap();
        assert!(spawnable(&jar));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&jar, std::fs::Permissions::from_mode(0o666)).unwrap();
            assert!(!spawnable(&jar), "group and other writable is refused");
            std::fs::set_permissions(&jar, std::fs::Permissions::from_mode(0o644)).unwrap();

            let link = dir.join("link.jar");
            std::os::unix::fs::symlink(&jar, &link).unwrap();
            assert!(!spawnable(&link), "a symlink is refused");
        }

        assert!(!spawnable(&dir), "a directory is refused");
        assert!(!spawnable(&dir.join("missing.jar")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
