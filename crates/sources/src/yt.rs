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
    CatalogTrack, MusicSource, SourceError, SourceHealth, SourceKind, SourceStream, StreamFormat,
    StreamRequest,
};
use async_trait::async_trait;
use serde::Deserialize;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const HEALTH_TIMEOUT: Duration = Duration::from_secs(20);
const HEALTH_POLL: Duration = Duration::from_millis(400);
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);

pub struct YtMusicSource {
    port: u16,
    child: Arc<Mutex<Child>>,
    http: reqwest::Client,
    version: Mutex<Option<String>>,
}

#[derive(Debug, Deserialize)]
struct HealthResponse {
    ok: bool,
    #[serde(default)]
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    tracks: Vec<SidecarSong>,
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
    #[serde(default)]
    duration_sec: Option<u32>,
    #[serde(default)]
    thumbnail_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamResponse {
    url: String,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    itag: Option<u32>,
    #[serde(default)]
    kbps: Option<u32>,
    #[serde(default)]
    container: Option<String>,
}

impl YtMusicSource {
    /// Spawns the sidecar jar on a free loopback port and waits for /health.
    ///
    /// Jar and java discovery (first hit wins):
    /// 1. `BITCHORD_SIDECAR_JAR` env,
    /// 2. the `sidecar_jar` setting,
    /// 3. `sidecar/build/libs/sidecar-all.jar` next to the running executable
    ///    or in the workspace checkout.
    /// Java: the `java_path` setting, then `$JAVA_HOME/bin/java`, then `java`
    /// on PATH.
    pub fn launch(jar_path: Option<PathBuf>, java_path: Option<String>) -> Result<Self, SourceError> {
        let jar = jar_path.or_else(discover_jar).ok_or_else(|| {
            SourceError::Protocol("sidecar jar not found (set BITCHORD_SIDECAR_JAR or the sidecar_jar setting)".into())
        })?;
        let java = java_path.or_else(discover_java).ok_or_else(|| {
            SourceError::Protocol("no Java runtime found (set the java_path setting or JAVA_HOME)".into())
        })?;
        let port = free_port()?;

        let mut child = Command::new(java)
            .arg("-jar")
            .arg(&jar)
            .arg("--port")
            .arg(port.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| SourceError::Protocol(format!("sidecar spawn: {e}")))?;

        let source = Self {
            port,
            child: Arc::new(Mutex::new(child)),
            http: reqwest::Client::builder()
                .timeout(HTTP_TIMEOUT)
                .user_agent("Tansen/0.3")
                .build()
                .expect("reqwest client"),
            version: Mutex::new(None),
        };

        // Health with retries: a cold JVM binds fast, but give a slow disk its
        // due. `launch` is sync but may be called inside a Tauri/tokio
        // runtime, and reqwest's blocking client must not be created *or
        // dropped* in an async context — so the whole poll phase runs on a
        // dedicated OS thread and reports back through a channel.
        let port = source.port;
        let child = source.child.clone();
        let child2 = child.clone();
        let healthy = std::thread::scope(move |scope| {
            scope
                .spawn(move || {
                    let client = reqwest::blocking::Client::builder()
                        .timeout(Duration::from_secs(3))
                        .build()
                        .ok()?;
                    let deadline = Instant::now() + HEALTH_TIMEOUT;
                    loop {
                        let answered = client
                            .get(format!("http://127.0.0.1:{port}/health"))
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
                        if let Ok(Some(_)) = child2.lock().unwrap().try_wait() {
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
            return Err(SourceError::Protocol("sidecar did not become healthy in time".into()));
        };
        *source.version.lock().unwrap() = version;
        Ok(source)
    }

    async fn get(&self, path: &str) -> Result<reqwest::Response, SourceError> {
        self.http
            .get(format!("http://127.0.0.1:{}{path}", self.port))
            .send()
            .await
            .map_err(|e| SourceError::Network(format!("sidecar: {e}")))
    }

    async fn health_url(&self) -> Result<String, SourceError> {
        let response = self.get("/health").await?;
        let parsed: HealthResponse = response.json().await.map_err(|e| SourceError::Protocol(e.to_string()))?;
        if !parsed.ok {
            return Err(SourceError::Protocol("sidecar reported not-ok".into()));
        }
        *self.version.lock().unwrap() = parsed.version.clone();
        Ok(parsed.version.unwrap_or_default())
    }

    pub fn version(&self) -> Option<String> {
        self.version.lock().unwrap().clone()
    }
}

impl Drop for YtMusicSource {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn free_port() -> Result<u16, SourceError> {
    TcpListener::bind("127.0.0.1:0")
        .map(|l| l.local_addr().unwrap().port())
        .map_err(|e| SourceError::Network(e.to_string()))
}

fn discover_jar() -> Option<PathBuf> {
    if let Ok(env) = std::env::var("BITCHORD_SIDECAR_JAR") {
        let p = PathBuf::from(env);
        if p.is_file() {
            return Some(p);
        }
    }
    // Candidate bases: the running executable's directory (installed app),
    // its parent (target/debug), the working directory, and the dev
    // checkout's root (two up from the crate dir under `cargo test`).
    let mut bases: Vec<PathBuf> = Vec::new();
    if let Some(exe_dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(PathBuf::from)) {
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
            PathBuf::from("BitChord/sidecar/build/libs/sidecar-all.jar"),
            PathBuf::from("../BitChord/sidecar/build/libs/sidecar-all.jar"),
            PathBuf::from("sidecar/build/libs/sidecar-all.jar"),
        ] {
            let candidate = base.join(&relative);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn discover_java() -> Option<String> {
    if let Ok(home) = std::env::var("JAVA_HOME") {
        let java = PathBuf::from(home).join("bin").join("java");
        if java.is_file() {
            return Some(java.to_string_lossy().into_owned());
        }
    }
    // A PATH lookup by spawning `java -version` is the cheapest existence
    // check. Success means exit 0 — the macOS stub binary runs but exits 1.
    if Command::new("java")
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        return Some("java".into());
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
            Ok(version) if !version.is_empty() => Ok(SourceHealth::Ok { detail: Some(version) }),
            Ok(_) => Ok(SourceHealth::Ok { detail: None }),
            Err(SourceError::Network(e)) => Ok(SourceHealth::Unreachable { reason: e }),
            Err(e) => Ok(SourceHealth::Rejected { reason: e.to_string() }),
        }
    }

    async fn search(&self, query: &str, limit: usize, _request: Option<&StreamRequest>) -> Result<Vec<CatalogTrack>, SourceError> {
        let response = self
            .get(&format!(
                "/search?q={}&filter=songs",
                urlencoding_lite(query)
            ))
            .await?
            .error_for_status()
            .map_err(|e| SourceError::Protocol(format!("sidecar search: {e}")))?;
        let parsed: SearchResponse = response.json().await.map_err(|e| SourceError::Protocol(e.to_string()))?;
        Ok(parsed
            .tracks
            .into_iter()
            .take(limit)
            .map(|t| CatalogTrack {
                track_id: t.video_id,
                config_id: self.config_id().to_string(),
                title: t.title,
                artist: t.artist,
                album: t.album,
                duration_sec: t.duration_sec,
                artwork_url: t.thumbnail_url,
                // Rows don't grade themselves; the stream answer does.
                format: None,
                embedded_stream_url: None,
            })
            .collect())
    }

    async fn stream(&self, track_id: &str, _request: &StreamRequest) -> Result<Option<SourceStream>, SourceError> {
        // `prefer=aac` asks the sidecar for a progressive MP4/AAC rendition —
        // the container Symphonia decodes directly. Opus/WebM answers are the
        // recorded phase-4 gap; they are not requested here.
        let response = self
            .get(&format!("/track/{}/stream?prefer=aac", urlencoding_lite(track_id)))
            .await?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let response = response
            .error_for_status()
            .map_err(|e| SourceError::Protocol(format!("sidecar stream: {e}")))?;
        let parsed: StreamResponse = response.json().await.map_err(|e| SourceError::Protocol(e.to_string()))?;
        Ok(Some(SourceStream {
            url: parsed.url,
            format: StreamFormat {
                codec: "AAC".into(),
                is_lossless: Some(false),
                kbps: parsed.kbps,
                is_dolby_atmos: false,
            },
            headers: vec![],
            below_request: false,
            duration_sec: None,
        }))
    }
}

/// The one character set that breaks URLs here.
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

/// Download at most `max_bytes` of a stream URL, for probe-style tests. Reads
/// are plain GETs; googlevideo serves full-file progressive bytes.
pub async fn fetch_prefix(url: &str, max_bytes: usize) -> Result<Vec<u8>, SourceError> {
    let response = reqwest::get(url)
        .await
        .map_err(|e| SourceError::Network(e.to_string()))?
        .error_for_status()
        .map_err(|e| SourceError::Network(e.to_string()))?;
    let mut bytes = Vec::with_capacity(max_bytes.min(1 << 20));
    let mut response = response;
    while bytes.len() < max_bytes {
        match response.chunk().await.map_err(|e| SourceError::Network(e.to_string()))? {
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
