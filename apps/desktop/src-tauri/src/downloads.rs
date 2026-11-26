//! Persistent download queue.
//!
//! Supported honestly:
//! - **Subsonic/OpenSubsonic**: the server's own `rest/download` (original) or
//!   `rest/stream` transcode at the chosen quality.
//! - **Addon servers**: their direct progressive stream URL, when declared.
//! - **Local files**: copied into the destination folder.
//!
//! **Unsupported by design**: YouTube Music and Spotify audio. Neither the
//! sidecar nor Spotify offers a downloadable file; the UI reports that instead
//! of pretending. Credential-bearing URLs are never stored: the worker asks
//! the source for a fresh URL at run time.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use persistence::{repo, Db};
use serde::{Deserialize, Serialize};
use sources::{AudioQuality, MusicSource, SourceKind, StreamRequest, TrackKey};
use tauri::{AppHandle, Emitter, Manager};

/// Concurrent transfers. Small enough to stay polite to a home server.
const MAX_CONCURRENT: usize = 2;
/// Hard per-track download ceiling (1 GiB) to prevent disk exhaustion from a
/// misbehaving or malicious remote source while accommodating Hi-Res FLACs.
const MAX_DOWNLOAD_BYTES: u64 = 1024 * 1024 * 1024;
const SETTING_DIR: &str = "downloads_dir";
const PART_PREFIX: &str = ".tansen-";

pub struct DownloadManager {
    db: Arc<Db>,
    app: AppHandle,
    paused: Mutex<HashSet<i64>>,
    cancelled: Mutex<HashSet<i64>>,
    running: Mutex<HashSet<i64>>,
    permits: tokio::sync::Semaphore,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewDownload {
    #[serde(default)]
    pub kind: Option<String>,
    pub source: String,
    pub track_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub duration_sec: f64,
    #[serde(default)]
    pub artwork_url: Option<String>,
    #[serde(default)]
    pub quality: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddOutcome {
    pub added: usize,
    pub duplicates: usize,
    pub rejected: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageInfo {
    pub bytes: u64,
    pub files: u64,
    /// Set when the destination itself cannot be read; `files`/`bytes` then
    /// describe whatever individual paths could still be stat'ed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCapability {
    pub kind: String,
    pub supported: bool,
    pub reason: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadCapabilities {
    pub destination: Option<String>,
    pub connected: bool,
    pub rows: Vec<repo::Download>,
    pub storage: StorageInfo,
    pub providers: Vec<ProviderCapability>,
}

enum JobEnd {
    Paused,
    Cancelled,
    Failed(String),
}

const MAX_DOWNLOAD_REDIRECTS: usize = 5;

/// The redirect gate for downloads. A source URL may redirect, but
/// never to another scheme family (https→http), and never into local/private
/// space unless the original URL was already local. The target is resolved and
/// pinned again by [`download_client`] on the next hop, so a public name that
/// answers with a loopback address is rejected too.
fn download_redirect_verdict(
    previous_hops: usize,
    target: &reqwest::Url,
    original_local: bool,
    original_scheme: &str,
) -> Result<(), &'static str> {
    if previous_hops >= MAX_DOWNLOAD_REDIRECTS {
        return Err("too many redirects");
    }
    if target.scheme() != "https" && target.scheme() != "http" {
        return Err("redirect scheme not allowed");
    }
    if original_scheme == "https" && target.scheme() != "https" {
        return Err("redirect would downgrade https");
    }
    if netguard::url_host_is_local(target) && !original_local {
        return Err("redirect to a local/private host is not allowed");
    }
    Ok(())
}

/// Builds a download client pinned to the URL host's validated address.
/// Every redirect hop calls this again, so the DNS-rebinding guard covers the
/// whole chain rather than only the URL the source first returned.
async fn download_client(url: &reqwest::Url) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(15))
        // Redirects are followed manually, one validated hop at a time.
        .redirect(reqwest::redirect::Policy::none());
    if let Some(domain) = netguard::dns_host(url) {
        let to_resolve = url.clone();
        let addr = tokio::task::spawn_blocking(move || netguard::resolve_validated(&to_resolve))
            .await
            .map_err(|e| format!("resolve join: {e}"))?
            .map_err(|e| format!("download host rejected: {e}"))?;
        builder = builder.resolve(&domain, addr);
    }
    builder.build().map_err(|e| format!("HTTP client: {e}"))
}

/// Sends the download request, following up to five validator-approved hops.
/// Custom source headers (which may carry addon credentials) are only sent to
/// the original host; a cross-host redirect never receives them.
async fn send_download(
    url: &str,
    headers: &[(String, String)],
    start: u64,
    original_local: bool,
    original_scheme: &str,
) -> Result<reqwest::Response, String> {
    let mut current = reqwest::Url::parse(url).map_err(|e| format!("download url: {e}"))?;
    let initial_host = current.host_str().map(str::to_owned);
    for hop in 0..=MAX_DOWNLOAD_REDIRECTS {
        let client = download_client(&current).await?;
        let cross_host = current.host_str().map(str::to_owned) != initial_host;
        let mut request = client.get(current.clone());
        if !cross_host {
            for (key, value) in headers {
                request = request.header(key, value);
            }
        }
        if start > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={start}-"));
        }
        let response = request
            .send()
            .await
            // reqwest's Display appends the full URL, including Subsonic
            // `u`/`t`/`s` auth and signed addon query parameters.
            .map_err(|e| format!("network: {}", e.without_url()))?;
        if !response.status().is_redirection() {
            return Ok(response);
        }
        let location = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| "redirect without a Location header".to_string())?;
        let next = current
            .join(location)
            .map_err(|e| format!("redirect target: {e}"))?;
        download_redirect_verdict(hop, &next, original_local, original_scheme)?;
        current = next;
    }
    Err("too many redirects".into())
}

impl DownloadManager {
    pub fn new(db: Arc<Db>, app: AppHandle) -> Arc<Self> {
        Arc::new(Self {
            db,
            app,
            paused: Mutex::new(HashSet::new()),
            cancelled: Mutex::new(HashSet::new()),
            running: Mutex::new(HashSet::new()),
            permits: tokio::sync::Semaphore::new(MAX_CONCURRENT),
        })
    }

    /// Restart anything left mid-flight by the previous run and spawn the
    /// queued rows. Called once at startup, never blocks the UI.
    pub fn bootstrap(self: &Arc<Self>) {
        let now = now_secs();
        if let Err(e) = repo::requeue_interrupted_downloads(&self.db, now) {
            tracing::warn!(error = %e, "download queue: could not requeue interrupted rows");
        }
        if let Ok(rows) = repo::list_downloads(&self.db) {
            for row in rows.into_iter().filter(|r| r.status == "queued") {
                self.spawn(row.id);
            }
        }
    }

    fn destination_path(&self) -> PathBuf {
        if let Some(saved) = repo::get_setting(&self.db, SETTING_DIR).ok().flatten() {
            let trimmed = saved.trim();
            if !trimmed.is_empty() {
                return PathBuf::from(trimmed);
            }
        }
        self.app
            .path()
            .download_dir()
            .map(|p| p.join("Tansen"))
            .unwrap_or_else(|_| std::env::temp_dir().join("Tansen"))
    }

    pub fn snapshot(&self) -> DownloadCapabilities {
        let mut rows = repo::list_downloads(&self.db).unwrap_or_default();
        let destination = self.destination_path();
        let connected = destination.is_dir();
        let error = if connected {
            None
        } else {
            Some(format!(
                "Download folder is unavailable: {}",
                destination.display()
            ))
        };
        let storage = storage_totals(&mut rows);
        DownloadCapabilities {
            destination: Some(destination.to_string_lossy().into_owned()),
            connected,
            rows,
            storage: StorageInfo { error, ..storage },
            providers: self.providers(),
        }
    }

    fn providers(&self) -> Vec<ProviderCapability> {
        let configs = sources::registry::load_configs(&self.db).unwrap_or_default();
        let has_enabled = |kind: SourceKind| configs.iter().any(|c| c.enabled && c.kind == kind);
        let subsonic = has_enabled(SourceKind::Subsonic);
        let addon = has_enabled(SourceKind::Addon);
        let local = app_core::state::local_files_enabled(&self.db);
        vec![
            ProviderCapability {
                kind: "subsonic".into(),
                supported: subsonic,
                reason: if subsonic {
                    "Downloads the server's original file; Low/Medium request a server-side transcode.".into()
                } else {
                    "No enabled Subsonic server.".into()
                },
            },
            ProviderCapability {
                kind: "addon".into(),
                supported: addon,
                reason: if addon {
                    "Direct progressive streams when the server's manifest permits downloads; HLS/DASH and encrypted sources are refused.".into()
                } else {
                    "No enabled addon server.".into()
                },
            },
            ProviderCapability {
                kind: "local".into(),
                supported: local,
                reason: if local {
                    "Copies library files into the downloads folder.".into()
                } else {
                    "Local files are off — no library files are offered.".into()
                },
            },
        ]
    }

    pub fn set_destination(&self, path: &str) -> Result<(), String> {
        let path = path.trim();
        if path.is_empty() {
            return Err("downloads folder is empty".into());
        }
        if destination_is_startup_dir(Path::new(path)) {
            return Err(
                "a login-time startup folder cannot hold downloads: a downloaded file there runs without being opened"
                    .into(),
            );
        }
        let dir = PathBuf::from(path);
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
        if std::fs::symlink_metadata(&dir).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(format!("{} is a symlink", dir.display()));
        }
        repo::set_setting(&self.db, SETTING_DIR, path).map_err(|e| e.to_string())?;
        self.emit();
        Ok(())
    }

    pub fn add(self: &Arc<Self>, items: Vec<NewDownload>) -> Result<AddOutcome, String> {
        if items.is_empty() {
            return Err("no tracks to download".into());
        }
        let dir = self.destination_path();
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
        let now = now_secs();
        let mut outcome = AddOutcome {
            added: 0,
            duplicates: 0,
            rejected: vec![],
        };
        let mut to_start: Vec<i64> = Vec::new();

        for item in items {
            let source = item.source.trim().to_ascii_lowercase();
            let track_id = item.track_id.trim().to_string();
            if track_id.is_empty() {
                outcome
                    .rejected
                    .push(format!("{}: missing track id", display_name(&item)));
                continue;
            }
            if !matches!(source.as_str(), "subsonic" | "addon" | "local") {
                let reason = match source.as_str() {
                    "youtube" => "YouTube Music audio is not downloadable — the sidecar serves streams, not files.",
                    "spotify" => "Spotify audio is never downloaded; imported playlists play through YouTube Music.",
                    _ => "this source cannot be downloaded",
                };
                outcome
                    .rejected
                    .push(format!("{}: {reason}", display_name(&item)));
                continue;
            }
            if source == "local" {
                if !app_core::state::local_files_enabled(&self.db) {
                    outcome.rejected.push(format!(
                        "{}: local files are off in Settings",
                        display_name(&item)
                    ));
                    continue;
                }
                if resolve_local_library_path(&self.db, &track_id).is_none() {
                    outcome.rejected.push(format!(
                        "{}: track is not in the local library",
                        display_name(&item)
                    ));
                    continue;
                }
            }
            let quality = normalize_quality(item.quality.as_deref());
            match repo::find_download(&self.db, &source, &track_id, &quality) {
                Ok(Some(existing)) => {
                    // Complete files and live queue entries are duplicates.
                    if existing.status == "complete"
                        || existing.status == "queued"
                        || existing.status == "downloading"
                        || existing.status == "paused"
                    {
                        outcome.duplicates += 1;
                        continue;
                    }
                    if repo::reset_download(&self.db, existing.id, now).is_ok() {
                        outcome.added += 1;
                        to_start.push(existing.id);
                    }
                }
                Ok(None) => {
                    match repo::insert_download(
                        &self.db,
                        item.kind.as_deref().unwrap_or("track"),
                        &source,
                        &track_id,
                        &item.title,
                        &item.artist,
                        item.album.as_deref(),
                        item.duration_sec,
                        item.artwork_url.as_deref(),
                        &quality,
                        now,
                    ) {
                        Ok(id) => {
                            outcome.added += 1;
                            to_start.push(id);
                        }
                        Err(e) => outcome
                            .rejected
                            .push(format!("{}: {e}", display_name(&item))),
                    }
                }
                Err(e) => outcome
                    .rejected
                    .push(format!("{}: {e}", display_name(&item))),
            }
        }

        for id in to_start {
            self.spawn(id);
        }
        self.emit();
        Ok(outcome)
    }

    pub fn pause(&self, id: i64) -> Result<(), String> {
        if let Ok(mut set) = self.paused.lock() {
            set.insert(id);
        }
        let now = now_secs();
        if let Some(row) = repo::get_download(&self.db, id).map_err(|e| e.to_string())? {
            if row.status == "downloading" || row.status == "queued" {
                repo::set_download_status(&self.db, id, "paused", None, now)
                    .map_err(|e| e.to_string())?;
            }
        }
        self.emit();
        Ok(())
    }

    pub fn resume(self: &Arc<Self>, id: i64) -> Result<(), String> {
        if let Ok(mut set) = self.paused.lock() {
            set.remove(&id);
        }
        let now = now_secs();
        if let Some(row) = repo::get_download(&self.db, id).map_err(|e| e.to_string())? {
            match row.status.as_str() {
                "paused" | "failed" | "cancelled" => {
                    repo::set_download_status(&self.db, id, "queued", None, now)
                        .map_err(|e| e.to_string())?;
                }
                "queued" => {}
                _ => return Ok(()),
            }
        }
        self.spawn(id);
        self.emit();
        Ok(())
    }

    pub fn retry(self: &Arc<Self>, id: i64) -> Result<(), String> {
        if let Ok(mut set) = self.cancelled.lock() {
            set.remove(&id);
        }
        if let Ok(mut set) = self.paused.lock() {
            set.remove(&id);
        }
        let _ = std::fs::remove_file(self.part_path(id));
        repo::reset_download(&self.db, id, now_secs()).map_err(|e| e.to_string())?;
        self.spawn(id);
        self.emit();
        Ok(())
    }

    pub fn cancel(self: &Arc<Self>, id: i64) -> Result<(), String> {
        if let Ok(mut set) = self.cancelled.lock() {
            set.insert(id);
        }
        if let Ok(mut set) = self.paused.lock() {
            set.remove(&id);
        }
        let now = now_secs();
        if let Some(row) = repo::get_download(&self.db, id).map_err(|e| e.to_string())? {
            if row.status == "queued" || row.status == "paused" {
                // No worker owns it: mark and remove the partial immediately.
                let _ = std::fs::remove_file(self.part_path(id));
                if let Ok(mut set) = self.cancelled.lock() {
                    set.remove(&id);
                }
                repo::set_download_status(&self.db, id, "cancelled", None, now)
                    .map_err(|e| e.to_string())?;
            } else if row.status == "downloading" {
                repo::set_download_status(&self.db, id, "cancelled", None, now)
                    .map_err(|e| e.to_string())?;
            }
        }
        self.emit();
        Ok(())
    }

    pub fn remove(&self, id: i64) -> Result<(), String> {
        if let Some(row) = repo::get_download(&self.db, id).map_err(|e| e.to_string())? {
            if row.status == "downloading" {
                if let Ok(mut set) = self.cancelled.lock() {
                    set.insert(id);
                }
            }
            if let Some(path) = row.path.as_deref() {
                let _ = std::fs::remove_file(path);
            }
        }
        let _ = std::fs::remove_file(self.part_path(id));
        repo::delete_download(&self.db, id).map_err(|e| e.to_string())?;
        self.emit();
        Ok(())
    }

    pub fn clear_finished(&self) -> Result<(), String> {
        let paths = repo::clear_finished_downloads(&self.db).map_err(|e| e.to_string())?;
        for path in paths {
            let _ = std::fs::remove_file(path);
        }
        self.emit();
        Ok(())
    }

    fn part_path(&self, id: i64) -> PathBuf {
        self.destination_path()
            .join(format!("{PART_PREFIX}{id}.part"))
    }

    fn emit(&self) {
        let _ = self.app.emit("downloads_changed", ());
    }

    fn spawn(self: &Arc<Self>, id: i64) {
        {
            let Ok(mut running) = self.running.lock() else {
                return;
            };
            if !running.insert(id) {
                return; // already owned by a worker
            }
        }
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            me.clone().run(id).await;
            if let Ok(mut running) = me.running.lock() {
                running.remove(&id);
            }
            // A resume/retry may have raced with the worker unwinding: if the
            // row is queued again and no worker owns it, pick it back up.
            if let Ok(Some(row)) = repo::get_download(&me.db, id) {
                if row.status == "queued" {
                    me.spawn(id);
                }
            }
        });
    }

    async fn run(self: Arc<Self>, id: i64) {
        let Ok(_permit) = self.permits.acquire().await else {
            return;
        };
        let Ok(Some(row)) = repo::get_download(&self.db, id) else {
            return;
        };
        // A pause/cancel may have landed while this job waited for a permit.
        if row.status != "queued" {
            return;
        }
        {
            let cancelled = self
                .cancelled
                .lock()
                .map(|s| s.contains(&id))
                .unwrap_or(false);
            if cancelled {
                let _ = std::fs::remove_file(self.part_path(id));
                let _ = repo::set_download_status(&self.db, id, "cancelled", None, now_secs());
                self.emit();
                return;
            }
        }
        let _ = repo::set_download_status(&self.db, id, "downloading", None, now_secs());
        self.emit();

        let outcome = self.transfer(&row).await;
        let now = now_secs();
        match outcome {
            Ok(()) => {}
            Err(JobEnd::Paused) => {
                let _ = repo::set_download_status(&self.db, id, "paused", None, now);
            }
            Err(JobEnd::Cancelled) => {
                let _ = std::fs::remove_file(self.part_path(id));
                let _ = repo::set_download_status(&self.db, id, "cancelled", None, now);
            }
            Err(JobEnd::Failed(error)) => {
                // Keep the .part: retry resumes with a Range request when the
                // server supports it.
                let _ = repo::set_download_status(&self.db, id, "failed", Some(&error), now);
            }
        }
        self.emit();
    }

    async fn transfer(&self, row: &repo::Download) -> Result<(), JobEnd> {
        let part = self.part_path(row.id);
        if row.source == "local" {
            return self.copy_local(row, &part).await;
        }
        let resolved = self.resolve(row).await.map_err(JobEnd::Failed)?;
        let start = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        let parsed = reqwest::Url::parse(&resolved.url)
            .map_err(|e| JobEnd::Failed(format!("download url: {e}")))?;
        let original_local = netguard::url_host_is_local(&parsed);
        let original_scheme = parsed.scheme().to_string();
        let response = send_download(
            &resolved.url,
            &resolved.headers,
            start,
            original_local,
            &original_scheme,
        )
        .await
        .map_err(JobEnd::Failed)?;
        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(JobEnd::Failed(format!(
                "server refused authentication ({}) — check the account credentials",
                status.as_u16()
            )));
        }
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(JobEnd::Failed(format!(
                "track unavailable on the server (404): {}",
                row.title
            )));
        }
        if !status.is_success() {
            return Err(JobEnd::Failed(format!(
                "server answered {} for {}",
                status.as_u16(),
                row.title
            )));
        }
        let resumed = status == reqwest::StatusCode::PARTIAL_CONTENT && start > 0;
        let start = if resumed { start } else { 0 };
        let total = response
            .content_length()
            .map(|len| len.saturating_add(start))
            .or(resolved.total)
            .unwrap_or(0);
        if exceeds_download_cap(total) {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(JobEnd::Failed("download exceeds 1 GiB size cap".into()));
        }
        let mime = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let disposition = response
            .headers()
            .get(reqwest::header::CONTENT_DISPOSITION)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        // Subsonic reports failures such as bad credentials inside a 200 JSON
        // body; an HTML error page is the same trap for addons. Never save a
        // text body with an audio extension.
        let looks_like_error = mime.as_deref().is_some_and(|m| {
            let m = m.to_ascii_lowercase();
            m.contains("json") || m.contains("xml") || m.contains("html")
        });
        let mut response = response;
        if looks_like_error {
            let mut body = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|e| JobEnd::Failed(format!("read server error: {}", e.without_url())))?
            {
                body.extend_from_slice(&chunk);
                if body.len() >= 8192 {
                    break;
                }
            }
            return Err(JobEnd::Failed(server_error_message(
                &String::from_utf8_lossy(&body),
            )));
        }

        let mut file = if resumed {
            tokio::fs::OpenOptions::new()
                .append(true)
                .open(&part)
                .await
                .map_err(|e| JobEnd::Failed(format!("open partial: {e}")))?
        } else {
            tokio::fs::File::create(&part)
                .await
                .map_err(|e| JobEnd::Failed(format!("create file: {e}")))?
        };

        use tokio::io::AsyncWriteExt;
        let mut written = start;
        let mut last_report = Instant::now();
        loop {
            if self.is_cancelled(row.id) {
                let _ = file.flush().await;
                return Err(JobEnd::Cancelled);
            }
            if self.is_paused(row.id) {
                let _ = file.flush().await;
                let _ = repo::set_download_progress(
                    &self.db,
                    row.id,
                    written as i64,
                    total as i64,
                    now_secs(),
                );
                return Err(JobEnd::Paused);
            }
            let chunk = match response.chunk().await {
                Ok(Some(chunk)) => chunk,
                Ok(None) => break,
                Err(e) => {
                    let _ = file.flush().await;
                    return Err(JobEnd::Failed(format!(
                        "transfer interrupted: {}",
                        e.without_url()
                    )));
                }
            };
            written = written.saturating_add(chunk.len() as u64);
            if exceeds_download_cap(written) {
                drop(file);
                let _ = tokio::fs::remove_file(&part).await;
                return Err(JobEnd::Failed("download exceeds 1 GiB size cap".into()));
            }
            file.write_all(&chunk)
                .await
                .map_err(|e| JobEnd::Failed(format!("write: {e}")))?;
            if last_report.elapsed() >= Duration::from_millis(400) {
                last_report = Instant::now();
                let _ = repo::set_download_progress(
                    &self.db,
                    row.id,
                    written as i64,
                    total as i64,
                    now_secs(),
                );
                self.emit();
            }
        }
        file.flush()
            .await
            .map_err(|e| JobEnd::Failed(format!("flush: {e}")))?;
        drop(file);

        let extension = extension_for(
            mime.as_deref(),
            disposition.as_deref(),
            &resolved.url,
            &resolved.extension,
        );
        let final_path = unique_path(
            &self.destination_path(),
            &file_stem(&row.artist, &row.title, &row.track_id),
            &extension,
        );
        if tokio::fs::rename(&part, &final_path).await.is_err() {
            // Cross-device (downloads folder on another volume): copy.
            tokio::fs::copy(&part, &final_path)
                .await
                .map_err(|e| JobEnd::Failed(format!("save: {e}")))?;
            let _ = tokio::fs::remove_file(&part).await;
        }
        repo::finish_download(
            &self.db,
            row.id,
            &final_path.to_string_lossy(),
            written as i64,
            now_secs(),
        )
        .map_err(|e| JobEnd::Failed(e.to_string()))?;
        Ok(())
    }

    async fn copy_local(&self, row: &repo::Download, part: &Path) -> Result<(), JobEnd> {
        if !app_core::state::local_files_enabled(&self.db) {
            return Err(JobEnd::Failed("local files are off in Settings".into()));
        }
        // Local rows must resolve to an indexed library track (by id or
        // tracked path). Arbitrary filesystem paths from IPC are refused.
        let source = PathBuf::from(
            resolve_local_library_path(&self.db, &row.track_id).ok_or_else(|| {
                JobEnd::Failed("local path is not an indexed library track".into())
            })?,
        );
        let meta = tokio::fs::metadata(&source)
            .await
            .map_err(|e| JobEnd::Failed(format!("local file unavailable: {e}")))?;
        if !meta.is_file() {
            return Err(JobEnd::Failed("local path is not a file".into()));
        }
        let total = meta.len();
        tokio::fs::copy(&source, part)
            .await
            .map_err(|e| JobEnd::Failed(format!("copy: {e}")))?;
        if self.is_paused(row.id) {
            return Err(JobEnd::Paused);
        }
        if self.is_cancelled(row.id) {
            let _ = tokio::fs::remove_file(part).await;
            return Err(JobEnd::Cancelled);
        }
        let ext = source
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("audio")
            .to_ascii_lowercase();
        let final_path = unique_path(
            &self.destination_path(),
            &file_stem(&row.artist, &row.title, &row.track_id),
            &ext,
        );
        if tokio::fs::rename(part, &final_path).await.is_err() {
            tokio::fs::copy(part, &final_path)
                .await
                .map_err(|e| JobEnd::Failed(format!("save: {e}")))?;
            let _ = tokio::fs::remove_file(part).await;
        }
        repo::finish_download(
            &self.db,
            row.id,
            &final_path.to_string_lossy(),
            total as i64,
            now_secs(),
        )
        .map_err(|e| JobEnd::Failed(e.to_string()))?;
        Ok(())
    }

    fn is_paused(&self, id: i64) -> bool {
        self.paused.lock().map(|s| s.contains(&id)).unwrap_or(false)
    }

    // Local library path resolution is handled by `resolve_local_library_path`.

    fn is_cancelled(&self, id: i64) -> bool {
        self.cancelled
            .lock()
            .map(|s| s.contains(&id))
            .unwrap_or(false)
    }

    /// Ask the concrete source for a fresh, credential-bearing URL.
    async fn resolve(&self, row: &repo::Download) -> Result<Resolved, String> {
        let configs = sources::registry::load_configs(&self.db).map_err(|e| e.to_string())?;
        match row.source.as_str() {
            "subsonic" => {
                let config = configs
                    .iter()
                    .find(|c| c.enabled && c.kind == SourceKind::Subsonic)
                    .ok_or_else(|| "no enabled Subsonic server".to_string())?;
                let source = sources::subsonic::SubsonicSource::new(
                    config.config_id.clone(),
                    config.name.clone(),
                    &config.base_url,
                    config.username.as_deref().unwrap_or_default(),
                    config.allow_http,
                )
                .map_err(|e| format!("Subsonic: {e}"))?;
                let url = source
                    .download_url(&row.track_id, &row.quality)
                    .map_err(|e| format!("Subsonic URL: {e}"))?;
                Ok(Resolved {
                    url: url.to_string(),
                    headers: vec![],
                    extension: String::new(),
                    total: None,
                })
            }
            "addon" => {
                let config = configs
                    .iter()
                    .find(|c| c.enabled && c.kind == SourceKind::Addon)
                    .ok_or_else(|| "no enabled addon server".to_string())?;
                let source = sources::addon::AddonSource::new(
                    config.config_id.clone(),
                    config.name.clone(),
                    config.base_url.clone(),
                    config.allow_http,
                )
                .map_err(|e| format!("addon: {e}"))?;
                // A manifest that explicitly refuses downloads is believed;
                // a server that says nothing keeps the historical behaviour.
                let manifest = source
                    .probe_manifest()
                    .await
                    .map_err(|e| format!("addon manifest: {e}"))?;
                if manifest.allow_downloads == Some(false) {
                    return Err(download_url_forbidden(&config.name));
                }
                let key = TrackKey::new(SourceKind::Addon, row.track_id.clone())
                    .map_err(|e| format!("addon track id: {e}"))?;
                let stream = source
                    .stream(&key, &quality_request(&row.quality))
                    .await
                    .map_err(|e| format!("addon stream: {e}"))?
                    .ok_or_else(|| "addon has no stream for this track".to_string())?;
                let extension = codec_extension(&stream.format.codec);
                Ok(Resolved {
                    url: stream.url,
                    headers: stream.headers,
                    extension,
                    total: None,
                })
            }
            other => Err(format!("{other} cannot be downloaded")),
        }
    }
}

struct Resolved {
    url: String,
    headers: Vec<(String, String)>,
    extension: String,
    total: Option<u64>,
}

/// Stats every completed row's real file on disk once per physical path.
/// Duplicate DB rows therefore contribute one file and one size; a row whose
/// file was deleted keeps `size_bytes: None` so the UI can say so instead of
/// counting a phantom file.
fn storage_totals(rows: &mut [repo::Download]) -> StorageInfo {
    let mut sizes: HashMap<PathBuf, Option<u64>> = HashMap::new();
    for row in rows.iter().filter(|r| r.status == "complete") {
        let Some(raw) = row.path.as_deref() else {
            continue;
        };
        let path = PathBuf::from(raw);
        let key = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        sizes.entry(key).or_insert_with(|| {
            std::fs::metadata(&path)
                .ok()
                .filter(|m| m.is_file())
                .map(|m| m.len())
        });
    }
    for row in rows.iter_mut().filter(|r| r.status == "complete") {
        let Some(raw) = row.path.as_deref() else {
            continue;
        };
        let path = PathBuf::from(raw);
        let key = std::fs::canonicalize(&path).unwrap_or(path);
        row.size_bytes = sizes.get(&key).copied().flatten();
    }
    StorageInfo {
        bytes: sizes.values().flatten().sum(),
        files: sizes.values().filter(|v| v.is_some()).count() as u64,
        error: None,
    }
}

fn exceeds_download_cap(bytes: u64) -> bool {
    bytes > MAX_DOWNLOAD_BYTES
}

/// Resolves a local download target strictly against the indexed `tracks`
/// table (accepting `local:<id>`, `<id>`, or an exact tracked library path).
/// Untracked or missing filesystem paths return `None` so IPC callers cannot
/// copy arbitrary files into the downloads directory.
fn resolve_local_library_path(db: &Db, track_id: &str) -> Option<String> {
    let raw = track_id.strip_prefix("local:").unwrap_or(track_id);
    if let Ok(id) = raw.parse::<i64>() {
        return repo::get_track(db, id)
            .ok()
            .flatten()
            .filter(|t| !t.missing)
            .map(|t| t.path);
    }
    repo::get_track_by_path(db, track_id)
        .ok()
        .flatten()
        .filter(|t| !t.missing)
        .map(|t| t.path)
}

fn download_url_forbidden(name: &str) -> String {
    format!("addon '{name}' does not permit downloads (manifest allow_downloads: false)")
}

fn quality_request(quality: &str) -> StreamRequest {
    match quality {
        "low" => StreamRequest::Capped { max_kbps: 64 },
        "medium" => StreamRequest::Capped { max_kbps: 128 },
        "lossless" => StreamRequest::Lossless,
        _ => StreamRequest::Best,
    }
}

fn normalize_quality(raw: Option<&str>) -> String {
    match raw.unwrap_or("high").to_ascii_lowercase().as_str() {
        "low" => "low",
        "medium" => "medium",
        "lossless" | "original" | "flac" => "original",
        _ => "high",
    }
    .to_string()
}

/// Keeps the UI's labels aligned with `AudioQuality` without re-exporting the
/// enum through the IPC boundary.
#[allow(dead_code, reason = "documents the mapping for future callers")]
fn audio_quality(quality: &str) -> AudioQuality {
    match quality {
        "low" => AudioQuality::Low,
        "medium" => AudioQuality::Medium,
        "original" | "lossless" => AudioQuality::Lossless,
        _ => AudioQuality::High,
    }
}

fn display_name(item: &NewDownload) -> String {
    if item.title.is_empty() {
        item.track_id.clone()
    } else {
        item.title.clone()
    }
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn codec_extension(codec: &str) -> String {
    match codec.to_ascii_lowercase().as_str() {
        "flac" => "flac",
        "mp3" | "mpeg" => "mp3",
        "mp4" | "m4a" | "aac" => "m4a",
        "ogg" | "vorbis" => "ogg",
        "opus" => "opus",
        "wav" | "pcm" => "wav",
        _ => "",
    }
    .to_string()
}

fn extension_for(
    mime: Option<&str>,
    disposition: Option<&str>,
    url: &str,
    fallback: &str,
) -> String {
    let by_mime = mime.and_then(|m| {
        let m = m.split(';').next().unwrap_or(m).trim().to_ascii_lowercase();
        Some(match m.as_str() {
            "audio/flac" | "audio/x-flac" => "flac",
            "audio/mpeg" | "audio/mp3" => "mp3",
            "audio/mp4" | "audio/x-m4a" | "audio/aac" => "m4a",
            "audio/ogg" | "application/ogg" => "ogg",
            "audio/opus" => "opus",
            "audio/wav" | "audio/x-wav" => "wav",
            _ => return None,
        })
    });
    if let Some(ext) = by_mime {
        return ext.to_string();
    }
    // Subsonic's `rest/download` often serves `application/octet-stream` but
    // names the original file in Content-Disposition; that suffix is the most
    // faithful extension available.
    if let Some(ext) = disposition_extension(disposition) {
        return ext;
    }
    if !fallback.is_empty() {
        return fallback.to_string();
    }
    if let Ok(parsed) = url::Url::parse(url) {
        let ext = parsed
            .path()
            .rsplit('.')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if matches!(
            ext.as_str(),
            "flac" | "mp3" | "m4a" | "aac" | "ogg" | "opus" | "wav"
        ) {
            return ext;
        }
    }
    "audio".into()
}

/// `attachment; filename="Artist - Song.flac"` (or RFC 5987 `filename*=`).
fn disposition_extension(value: Option<&str>) -> Option<String> {
    let value = value?;
    let raw = value.split(';').find_map(|part| {
        let part = part.trim();
        part.strip_prefix("filename*=")
            .or_else(|| part.strip_prefix("filename="))
    })?;
    let file = raw.trim().trim_matches('"');
    let ext = file.rsplit('.').next()?.to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "flac" | "mp3" | "m4a" | "aac" | "ogg" | "opus" | "wav" | "aiff" | "ape" | "wv"
    )
    .then_some(ext)
}

/// A readable reason from a JSON/XML/HTML error body, bounded for the UI.
fn server_error_message(body: &str) -> String {
    let message = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.pointer("/subsonic-response/error/message")
                .or_else(|| v.pointer("/error/message"))
                .or_else(|| v.get("message"))
                .or_else(|| v.get("error"))
                .and_then(|m| m.as_str().map(str::to_owned))
        })
        .or_else(|| {
            let start = body.find("<error")?;
            let rest = &body[start..];
            let attr = rest.find("message=\"")? + "message=\"".len();
            let end = rest[attr..].find('"')?;
            Some(rest[attr..attr + end].to_string())
        })
        .unwrap_or_else(|| body.lines().next().unwrap_or("").trim().to_string());
    let message = if message.is_empty() {
        "server returned an error instead of audio"
    } else {
        message.as_str()
    };
    format!(
        "server returned an error instead of audio: {}",
        message.chars().take(240).collect::<String>()
    )
}

fn file_stem(artist: &str, title: &str, track_id: &str) -> String {
    let raw = if !artist.trim().is_empty() && !title.trim().is_empty() {
        format!("{artist} - {title}")
    } else if !title.trim().is_empty() {
        title.to_string()
    } else {
        track_id.rsplit('/').next().unwrap_or(track_id).to_string()
    };
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    if trimmed.is_empty() {
        "download".into()
    } else {
        trimmed.chars().take(140).collect()
    }
}

/// Directories whose contents a login session runs by itself. A download folder
/// there makes a downloaded file something the user never chooses to open.
/// shortcut: a named list, not a policy; a Windows Startup entry under a renamed
/// profile is not caught.
fn destination_is_startup_dir(path: &Path) -> bool {
    let mut candidate = path.to_string_lossy().replace('\\', "/");
    while candidate.ends_with('/') {
        candidate.pop();
    }
    let mut held = vec![
        "/Library/LaunchAgents".to_string(),
        "/Library/LaunchDaemons".to_string(),
        "/System/Library/LaunchAgents".to_string(),
        "/System/Library/LaunchDaemons".to_string(),
        "/etc/cron.d".to_string(),
        "/etc/cron.daily".to_string(),
        "/etc/cron.hourly".to_string(),
    ];
    for home in [std::env::var("HOME"), std::env::var("APPDATA")]
        .into_iter()
        .flatten()
    {
        let home = home.trim_end_matches('/').replace('\\', "/");
        held.push(format!("{home}/Library/LaunchAgents"));
        held.push(format!("{home}/Library/LaunchDaemons"));
        held.push(format!("{home}/.config/autostart"));
        held.push(format!(
            "{home}/Microsoft/Windows/Start Menu/Programs/Startup"
        ));
    }
    held.iter()
        .any(|dir| candidate == *dir || candidate.starts_with(&format!("{dir}/")))
}

fn unique_path(dir: &Path, stem: &str, extension: &str) -> PathBuf {
    let ext = if extension.is_empty() {
        "audio"
    } else {
        extension
    };
    let first = dir.join(format!("{stem}.{ext}"));
    if !first.exists() {
        return first;
    }
    for n in 1..10_000 {
        let candidate = dir.join(format!("{stem} ({n}).{ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!("{stem}-{}.{ext}", now_secs()))
}

// Tauri commands

#[tauri::command]
pub fn downloads_get(
    manager: tauri::State<'_, Arc<DownloadManager>>,
) -> Result<DownloadCapabilities, String> {
    Ok(manager.snapshot())
}

#[tauri::command]
pub fn downloads_add(
    manager: tauri::State<'_, Arc<DownloadManager>>,
    tracks: Vec<NewDownload>,
) -> Result<AddOutcome, String> {
    manager.add(tracks)
}

#[tauri::command]
pub fn downloads_set_destination(
    manager: tauri::State<'_, Arc<DownloadManager>>,
    path: String,
) -> Result<DownloadCapabilities, String> {
    manager.set_destination(&path)?;
    Ok(manager.snapshot())
}

#[tauri::command]
pub fn downloads_pause(
    manager: tauri::State<'_, Arc<DownloadManager>>,
    id: i64,
) -> Result<(), String> {
    manager.pause(id)
}

#[tauri::command]
pub fn downloads_resume(
    manager: tauri::State<'_, Arc<DownloadManager>>,
    id: i64,
) -> Result<(), String> {
    manager.resume(id)
}

#[tauri::command]
pub fn downloads_cancel(
    manager: tauri::State<'_, Arc<DownloadManager>>,
    id: i64,
) -> Result<(), String> {
    manager.cancel(id)
}

#[tauri::command]
pub fn downloads_retry(
    manager: tauri::State<'_, Arc<DownloadManager>>,
    id: i64,
) -> Result<(), String> {
    manager.retry(id)
}

#[tauri::command]
pub fn downloads_remove(
    manager: tauri::State<'_, Arc<DownloadManager>>,
    id: i64,
) -> Result<(), String> {
    manager.remove(id)
}

#[tauri::command]
pub fn downloads_clear_completed(
    manager: tauri::State<'_, Arc<DownloadManager>>,
) -> Result<(), String> {
    manager.clear_finished()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_names_are_bounded_to_the_supported_ladder() {
        assert_eq!(normalize_quality(Some("low")), "low");
        assert_eq!(normalize_quality(Some("MEDIUM")), "medium");
        assert_eq!(normalize_quality(Some("lossless")), "original");
        assert_eq!(normalize_quality(Some("original")), "original");
        assert_eq!(normalize_quality(Some("nonsense")), "high");
        assert_eq!(normalize_quality(None), "high");
    }

    #[test]
    fn extensions_prefer_the_served_mime_type() {
        assert_eq!(
            extension_for(Some("audio/flac; charset=x"), None, "https://x/y", ""),
            "flac"
        );
        assert_eq!(
            extension_for(Some("audio/mpeg"), None, "https://x/y", ""),
            "mp3"
        );
        assert_eq!(extension_for(None, None, "https://x/y/song.m4a", ""), "m4a");
        assert_eq!(extension_for(None, None, "https://x/y", "opus"), "opus");
        assert_eq!(extension_for(None, None, "https://x/y/stream", ""), "audio");
    }

    #[test]
    fn content_disposition_recovers_the_original_extension() {
        assert_eq!(
            extension_for(
                Some("application/octet-stream"),
                Some("attachment; filename=\"Artist - Song.flac\""),
                "https://x/rest/download?id=1",
                ""
            ),
            "flac"
        );
        assert_eq!(
            disposition_extension(Some("inline; filename=track.MP3")).as_deref(),
            Some("mp3")
        );
        assert_eq!(
            disposition_extension(Some("attachment; filename*=UTF-8''a%20b.opus")).as_deref(),
            Some("opus")
        );
        assert_eq!(
            disposition_extension(Some("attachment; filename=\"no-extension\"")),
            None
        );
    }

    #[test]
    fn server_error_bodies_become_readable_failures() {
        let json = r#"{"subsonic-response":{"status":"failed","error":{"code":40,"message":"Wrong username or password"}}}"#;
        assert!(server_error_message(json).contains("Wrong username or password"));
        let xml = r#"<subsonic-response status="failed"><error code="40" message="Wrong username or password"/></subsonic-response>"#;
        assert!(server_error_message(xml).contains("Wrong username or password"));
        assert!(server_error_message("Unauthorized").contains("Unauthorized"));
    }

    #[test]
    fn storage_counts_real_files_once_and_flags_missing_ones() {
        let dir = std::env::temp_dir().join(format!("tansen-downloads-test-{}", now_secs()));
        std::fs::create_dir_all(&dir).unwrap();
        let present = dir.join("song.flac");
        std::fs::write(&present, [0u8; 7]).unwrap();
        let missing = dir.join("deleted.mp3");
        let mut rows = vec![
            download_row("complete", Some(&present)),
            download_row("complete", Some(&present)), // duplicate DB row
            download_row("complete", Some(&missing)),
            download_row("queued", Some(&present)), // not complete: ignored
        ];
        let totals = storage_totals(&mut rows);
        assert_eq!(totals.files, 1);
        assert_eq!(totals.bytes, 7);
        assert_eq!(rows[0].size_bytes, Some(7));
        assert_eq!(rows[1].size_bytes, Some(7));
        assert_eq!(rows[2].size_bytes, None);
        assert_eq!(rows[3].size_bytes, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn download_row(status: &str, path: Option<&Path>) -> repo::Download {
        repo::Download {
            id: 0,
            kind: "track".into(),
            source: "subsonic".into(),
            track_id: "1".into(),
            title: "Song".into(),
            artist: "Artist".into(),
            album: None,
            duration_sec: 0.0,
            artwork_url: None,
            status: status.into(),
            progress: 0,
            total: 0,
            path: path.map(|p| p.to_string_lossy().into_owned()),
            error: None,
            quality: "high".into(),
            created_at: 0,
            updated_at: 0,
            size_bytes: None,
        }
    }

    #[test]
    fn file_names_are_sanitized_and_bounded() {
        assert_eq!(
            file_stem("AC/DC", "Back: in Black", "1"),
            "AC_DC - Back_ in Black"
        );
        assert_eq!(file_stem("", "", "https://x/song.flac"), "song.flac");
        assert!(!file_stem("a", "b", "1").contains('/'));
        assert!(
            file_stem(&"x".repeat(400), &"y".repeat(400), "1")
                .chars()
                .count()
                <= 140
        );
    }

    #[test]
    fn download_redirects_cannot_pivot_local_or_downgrade() {
        let public = reqwest::Url::parse("https://cdn.example/song.flac").unwrap();
        assert!(download_redirect_verdict(0, &public, false, "https").is_ok());
        assert!(download_redirect_verdict(5, &public, false, "https").is_err());

        let loopback = reqwest::Url::parse("http://127.0.0.1:4533/rest/download").unwrap();
        assert!(download_redirect_verdict(0, &loopback, false, "https").is_err());
        assert!(download_redirect_verdict(0, &loopback, true, "http").is_ok());

        let downgrade = reqwest::Url::parse("http://cdn.example/song.flac").unwrap();
        assert!(download_redirect_verdict(0, &downgrade, false, "https").is_err());
        assert!(download_redirect_verdict(0, &downgrade, false, "http").is_ok());

        let exotic = reqwest::Url::parse("file:///etc/passwd").unwrap();
        assert!(download_redirect_verdict(0, &exotic, true, "http").is_err());
    }

    #[tokio::test]
    async fn download_network_errors_drop_the_credential_query() {
        let error = send_download(
            "http://127.0.0.1:1/rest/download?id=1&u=alice&t=deadbeef&s=cafe",
            &[],
            0,
            false,
            "http",
        )
        .await
        .expect_err("connection to port 1 must fail");
        assert!(!error.contains("deadbeef"), "token leaked: {error}");
        assert!(!error.contains("u=alice"), "username leaked: {error}");
        assert!(error.contains("network"), "{error}");
    }

    #[test]
    fn local_library_path_rejects_untracked_filesystem_paths() {
        let db = Db::in_memory().unwrap();
        let folder_id = repo::add_folder(&db, "/music", 1).unwrap();
        let track = persistence::models::Track {
            id: 0,
            folder_id,
            path: "/music/song.flac".into(),
            file_name: "song.flac".into(),
            title: "Song".into(),
            artist: "Artist".into(),
            album: "Album".into(),
            album_artist: "Artist".into(),
            track_no: None,
            disc_no: None,
            year: None,
            genre: String::new(),
            duration_sec: 180.0,
            codec: "flac".into(),
            bitrate_kbps: None,
            sample_rate: None,
            bit_depth: None,
            channels: None,
            art_path: None,
            missing: false,
        };
        let id = repo::upsert_track(&db, &track, folder_id, 1, 100, 1).unwrap();

        assert_eq!(
            resolve_local_library_path(&db, &format!("local:{id}")).as_deref(),
            Some("/music/song.flac")
        );
        assert_eq!(
            resolve_local_library_path(&db, &id.to_string()).as_deref(),
            Some("/music/song.flac")
        );
        assert_eq!(
            resolve_local_library_path(&db, "/music/song.flac").as_deref(),
            Some("/music/song.flac")
        );
        assert_eq!(resolve_local_library_path(&db, "/etc/hosts"), None);
        assert_eq!(
            resolve_local_library_path(&db, "/music/../outside.flac"),
            None
        );
        assert_eq!(resolve_local_library_path(&db, "local:999999"), None);
    }

    #[test]
    fn download_size_cap_rejects_oversized_transfers() {
        assert!(!exceeds_download_cap(0));
        assert!(!exceeds_download_cap(MAX_DOWNLOAD_BYTES));
        assert!(exceeds_download_cap(MAX_DOWNLOAD_BYTES + 1));
    }

    #[test]
    fn a_login_startup_folder_is_refused_as_a_download_destination() {
        for dir in [
            "/Library/LaunchAgents",
            "/Library/LaunchAgents/",
            "/System/Library/LaunchDaemons/com.example.plist",
            "/etc/cron.daily",
        ] {
            assert!(destination_is_startup_dir(Path::new(dir)), "{dir}");
        }
        for dir in ["/Users/x/Music/Downloads", "/tmp/launchagents", "/Library"] {
            assert!(!destination_is_startup_dir(Path::new(dir)), "{dir}");
        }
        let home = std::env::temp_dir()
            .join("tansen-dest-test")
            .join("Library");
        assert!(!destination_is_startup_dir(&home));
    }
}
