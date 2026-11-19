//! HTTP range reader: a `Read + Seek` over a remote file, fetching fixed-size
//! chunks with `Range` requests.
//!
//! Why ranges: googlevideo serves unbounded responses at a crawl (~15 kB/s,
//! measured upstream) while ranged requests run at line rate — the same
//! finding that produced the phone's `ChunkedDataSource`. The MP4 demuxer
//! seeks (it hunts for `moov`), so a seekable reader is what lets playback
//! start before the whole file is on disk.

use std::io::{Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use tokio::sync::watch;

const CHUNK: u64 = 512 * 1024;
const TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, thiserror::Error)]
pub enum RangeError {
    #[error("http: {0}")]
    Http(String),
    #[error("server ignored range requests (status {0})")]
    NoRangeSupport(u16),
    /// 403/410: the URL expired or was revoked. Not retryable — the
    /// media must be re-resolved.
    #[error("url rejected by server ({0})")]
    Forbidden(u16),
}

pub struct HttpRangeReader {
    url: String,
    client: reqwest::blocking::Client,
    /// Absolute position of the next byte `read` would return.
    pos: u64,
    /// Total length from the Content-Range of the probe, when known.
    len: Option<u64>,
    /// Fetched window; `buf_start` is its absolute start position.
    buf: Vec<u8>,
    buf_start: u64,
    headers: Vec<(String, String)>,
}

/// The media-fetch redirect policy shared by HttpRangeReader,
/// PrefetchReader and StreamCache. Follows a redirect only when the target
/// is http(s), not a private/loopback/link-local host (unless the original
/// URL's host was itself local), and never downgrades https→http — CDN
/// redirects keep working while a source server cannot pivot a media fetch
/// at internal networks or strip transport encryption.
pub(crate) fn media_redirect_verdict(
    previous_hops: usize,
    target: &reqwest::Url,
    original_local: bool,
    original_scheme: &str,
) -> Result<(), &'static str> {
    if previous_hops >= 5 {
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
    // Host names are not enough: a public name such as `localtest.me` can
    // resolve to loopback. Resolve the hop and refuse a local answer before
    // reqwest connects. (The downloader re-resolves and pins every hop; the
    // media readers still share this check rather than a per-hop client.)
    if !original_local {
        if let Ok(addr) = netguard::resolve_validated(target) {
            if netguard::ip_is_local(addr.ip()) {
                return Err("redirect resolves to a local/private address");
            }
        }
    }
    Ok(())
}

/// Whether a media URL's host is loopback/private/link-local. Judged from
/// the URL's parsed host via the shared netguard policy — the previous
/// string re-parse of `host_str()` never parsed bracketed IPv6, so every
/// IPv6 URL host (loopback `::1` included) was classified non-local.
pub(crate) fn media_host_is_local(url: &str) -> bool {
    reqwest::Url::parse(url)
        .ok()
        .map(|u| netguard::url_host_is_local(&u))
        .unwrap_or(false)
}

impl HttpRangeReader {
    /// Stream URLs carry credentials (subsonic token+salt, googlevideo
    /// signed params). Logs get scheme://host/path only — never the query.
    fn redacted(url: &str) -> &str {
        url.split('?').next().unwrap_or(url)
    }

    /// Creates a blocking HTTP client internally — like rodio's decoders,
    /// this type must be constructed and dropped off the async runtime (the
    /// engine's audio thread qualifies).
    pub fn new(url: impl Into<String>, headers: Vec<(String, String)>) -> Result<Self, RangeError> {
        let url = url.into();
        let original_local = media_host_is_local(&url);
        let original_scheme = reqwest::Url::parse(&url)
            .map(|u| u.scheme().to_string())
            .unwrap_or_default();
        // DNS-rebinding guard: resolve the original host once, refuse a
        // non-local name that answers with a local/private address, and
        // pin the client to the validated address for its lifetime — the
        // connection cannot land anywhere else, whatever the resolver
        // answers later.
        let parsed =
            reqwest::Url::parse(&url).map_err(|e| RangeError::Http(format!("media url: {e}")))?;
        let mut builder = reqwest::blocking::Client::builder()
            .timeout(TIMEOUT)
            .user_agent("Tansen/0.3");
        if let Some(domain) = netguard::dns_host(&parsed) {
            let addr = netguard::resolve_validated(&parsed).map_err(RangeError::Http)?;
            builder = builder.resolve(&domain, addr);
        }
        let mut reader = Self {
            url,
            client: builder
                .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                    match media_redirect_verdict(
                        attempt.previous().len(),
                        attempt.url(),
                        original_local,
                        &original_scheme,
                    ) {
                        Ok(()) => attempt.follow(),
                        Err(e) => attempt.error(e),
                    }
                }))
                .build()
                .map_err(|e| RangeError::Http(e.to_string()))?,
            pos: 0,
            len: None,
            buf: Vec::new(),
            buf_start: 0,
            headers,
        };
        reader.fetch_chunk(); // probe: first chunk + length
        Ok(reader)
    }

    fn fetch_chunk(&mut self) {
        let start = self.pos;
        // saturating: a decoder seek near u64::MAX must wrap into a
        // malformed Range request, not panic the decode thread.
        let end = start.saturating_add(CHUNK - 1);
        let mut request = self
            .client
            .get(&self.url)
            .header("Range", format!("bytes={start}-{end}"));
        for (name, value) in &self.headers {
            request = request.header(name, value);
        }
        let response = match request.send() {
            Ok(r) => r,
            Err(e) => {
                // reqwest's Display appends the full URL (Subsonic token+salt,
                // googlevideo signed params); log the cause without it.
                let error = e.without_url();
                tracing::warn!(url = %Self::redacted(&self.url), error = %error, "range fetch failed");
                return;
            }
        };
        #[cfg(debug_assertions)]
        tracing::debug!(
            transport = "http-range",
            start,
            end,
            status = response.status().as_u16(),
            "range response"
        );
        match response.status().as_u16() {
            206 => {
                if let Some(range) = response
                    .headers()
                    .get("Content-Range")
                    .and_then(|v| v.to_str().ok())
                {
                    // "bytes start-end/total"
                    if let Some(total) =
                        range.rsplit('/').next().and_then(|t| t.parse::<u64>().ok())
                    {
                        self.len = Some(total);
                    }
                }
            }
            200 => {
                // Server ignored the range header. Not expected from
                // googlevideo; play it safe and refuse rather than pulling a
                // throttled unbounded body into the audio thread.
                tracing::warn!(url = %Self::redacted(&self.url), status = 200, "no range support");
                return; // empty buffer; read() hits EOF and the track errors
            }
            status => {
                tracing::warn!(url = %Self::redacted(&self.url), status, "range fetch refused");
                return;
            }
        }
        // A 206 body is still only trusted up to the requested window: a
        // hostile server answering 206 may stream far beyond CHUNK, and the
        // 20 s timeout is not a size bound. Refuse anything overlong.
        let mut bytes = Vec::new();
        match response.take((CHUNK + 1) as u64).read_to_end(&mut bytes) {
            Ok(_) if bytes.len() as u64 > CHUNK => {
                tracing::warn!(url = %Self::redacted(&self.url), requested = CHUNK, got = bytes.len(), "oversized range body refused");
            }
            Ok(_) => {
                #[cfg(debug_assertions)]
                tracing::debug!(
                    transport = "http-range",
                    start,
                    requested_end = end,
                    bytes = bytes.len(),
                    "range body"
                );
                self.buf_start = start;
                self.buf = bytes;
            }
            Err(e) => {
                tracing::warn!(url = %Self::redacted(&self.url), error = %e, "range body read failed")
            }
        }
    }
}

/// A live broadcast stream: one unbounded HTTP body, no ranges and no
/// seeking. Read+Seek because rodio's decoder demands it, but every seek
/// except back to the start is refused — a broadcast has no timeline to
/// jump around in. `hint` is the decoder extension guessed from the
/// Content-Type (or the first bytes), `None` meaning "let the decoder
/// sniff".
pub struct LiveReader {
    response: reqwest::blocking::Response,
    head: Vec<u8>,
    head_pos: usize,
    pos: u64,
}

impl LiveReader {
    fn redacted(url: &str) -> &str {
        url.split('?').next().unwrap_or(url)
    }

    pub fn open(url: &str) -> Result<(Self, Option<&'static str>), String> {
        let original_local = media_host_is_local(url);
        let original_scheme = reqwest::Url::parse(url)
            .map(|u| u.scheme().to_string())
            .unwrap_or_default();
        let parsed = reqwest::Url::parse(url).map_err(|e| format!("stream url: {e}"))?;
        // No overall timeout: a live stream stays open for hours. Only the
        // connect phase gets a deadline.
        let mut builder = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(12))
            .user_agent("Tansen/0.3");
        if let Some(domain) = netguard::dns_host(&parsed) {
            let addr = netguard::resolve_validated(&parsed).map_err(|e| format!("resolve: {e}"))?;
            builder = builder.resolve(&domain, addr);
        }
        let client = builder
            .redirect(reqwest::redirect::Policy::custom(
                move |attempt| match media_redirect_verdict(
                    attempt.previous().len(),
                    attempt.url(),
                    original_local,
                    &original_scheme,
                ) {
                    Ok(()) => attempt.follow(),
                    Err(e) => attempt.error(e),
                },
            ))
            .build()
            .map_err(|e| format!("stream client: {e}"))?;
        let mut response = client
            .get(url)
            .header("Icy-MetaData", "0")
            .send()
            .map_err(|e| format!("stream open failed: {}", e.without_url()))?;
        if !response.status().is_success() {
            return Err(format!(
                "station refused the stream (HTTP {})",
                response.status().as_u16()
            ));
        }
        let mime = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        if mime.contains("mpegurl") || mime.contains("dash") {
            return Err("this station streams HLS/DASH, which Tansen cannot decode".into());
        }
        let mut head = vec![0u8; 64];
        let n = std::io::Read::read(&mut response, &mut head).unwrap_or(0);
        head.truncate(n);
        let hint = mime_hint(&mime).or_else(|| sniff_live_hint(&head));
        tracing::debug!(url = %Self::redacted(url), mime = %mime, hint = ?hint, "live stream open");
        Ok((
            Self {
                response,
                head,
                head_pos: 0,
                pos: 0,
            },
            hint,
        ))
    }
}

fn mime_hint(mime: &str) -> Option<&'static str> {
    if mime.contains("mpeg") && !mime.contains("mpegurl") {
        return Some("mp3");
    }
    if mime.contains("aac") || mime.contains("aacp") {
        return Some("aac");
    }
    if mime.contains("ogg") || mime.contains("opus") {
        return Some("ogg");
    }
    if mime.contains("flac") {
        return Some("flac");
    }
    if mime.contains("mp4") || mime.contains("m4a") {
        return Some("m4a");
    }
    if mime.contains("wav") {
        return Some("wav");
    }
    None
}

fn sniff_live_hint(head: &[u8]) -> Option<&'static str> {
    if head.len() >= 8 && &head[4..8] == b"ftyp" {
        return Some("m4a");
    }
    if head.starts_with(b"OggS") {
        return Some("ogg");
    }
    if head.starts_with(b"fLaC") {
        return Some("flac");
    }
    if head.starts_with(b"ID3") {
        return Some("mp3");
    }
    // ADTS AAC: 12-bit sync, layer 00 in the second byte's low bits.
    if head.len() >= 2 && head[0] == 0xFF && (head[1] & 0xF6) == 0xF0 {
        return Some("aac");
    }
    None
}

impl Read for LiveReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.head_pos < self.head.len() {
            let n = (self.head.len() - self.head_pos).min(out.len());
            out[..n].copy_from_slice(&self.head[self.head_pos..self.head_pos + n]);
            self.head_pos += n;
            self.pos += n as u64;
            return Ok(n);
        }
        let n = std::io::Read::read(&mut self.response, out)?;
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for LiveReader {
    fn seek(&mut self, target: SeekFrom) -> std::io::Result<u64> {
        // The decoder may re-read from the top; nothing else is meaningful.
        if matches!(target, SeekFrom::Start(0)) && self.pos == 0 {
            return Ok(0);
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "a live stream cannot seek",
        ))
    }
}

impl Read for HttpRangeReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        // Past the end (or nothing fetched): EOF.
        if let Some(len) = self.len {
            if self.pos >= len {
                return Ok(0);
            }
        }
        // Ensure the buffer covers the current position.
        if self.buf.is_empty()
            || self.pos < self.buf_start
            || self.pos >= self.buf_start + self.buf.len() as u64
        {
            self.fetch_chunk();
            if self.buf.is_empty() {
                return Ok(0); // fetch failed; treat as EOF so the track errors cleanly
            }
        }
        let offset = (self.pos - self.buf_start) as usize;
        let n = out.len().min(self.buf.len() - offset);
        out[..n].copy_from_slice(&self.buf[offset..offset + n]);
        self.pos += n as u64;
        if n == 0 {
            return Ok(0);
        }
        Ok(n)
    }
}

impl Seek for HttpRangeReader {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let target = match pos {
            SeekFrom::Start(n) => n as i64,
            SeekFrom::Current(n) => self.pos as i64 + n,
            SeekFrom::End(n) => match self.len {
                Some(len) => len as i64 + n,
                None => {
                    // Force one fetch to learn the length before seeking to End.
                    self.fetch_chunk();
                    match self.len {
                        Some(len) => len as i64 + n,
                        None => {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::Unsupported,
                                "seek from End without a known length",
                            ))
                        }
                    }
                }
            },
        };
        self.pos = target.max(0) as u64;
        // Position outside the buffer is fine — the next read fetches it.
        Ok(self.pos)
    }
}

/// `Read + Seek` over HTTP Range with a bounded read-ahead buffer
/// filled by a dedicated fetch task. The audio callback only ever pops from
/// the in-memory buffer — it performs no network I/O itself. When the buffer
/// runs dry (a network stall outpaced the read-ahead), reads wait on the
/// in-memory condvar while the fetch task keeps working, and the caller is
/// told via the status watch: `StreamStatus::Starved` → the engine publishes
/// `Buffering`; the task refilling above the watermark sends `Refilled` →
/// `Playing`. A seek drops the buffer and issues a fresh Range request at
/// the target offset.
pub struct PrefetchReader {
    shared: Arc<PrefetchShared>,
    /// Absolute decode position.
    pos: u64,
    /// Total length from the probe's Content-Range, when known.
    len: Option<u64>,
}

/// Mid-stream fill status of a prefetching stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamStatus {
    /// The read-ahead buffer ran dry; the decoder is waiting for the fetch
    /// task to catch up.
    Starved,
    /// The buffer refilled past the low watermark.
    Refilled,
    /// The URL is dead — the server answered 403/410 (expired or revoked).
    /// The engine must re-resolve and rebuild the stream.
    UrlDead,
}

/// Buffer bounds: the fetch task tops up at HIGH and re-kicks below LOW.
/// 4 MiB at 131 kbps ≈ 4 min of audio ahead.
const HIGH_WATER: usize = 4 * 1024 * 1024;
const LOW_WATER: usize = 512 * 1024;
const FETCH_CHUNK: u64 = 512 * 1024;

struct PrefetchShared {
    url: String,
    headers: Vec<(String, String)>,
    client: reqwest::blocking::Client,
    state: Mutex<FetchState>,
    /// Wakes the fetch thread: seek requested, buffer below the low
    /// watermark, shutdown.
    fetch_signal: Condvar,
    /// Wakes readers: data appended, seek acknowledged, shutdown.
    read_signal: Condvar,
    status: watch::Sender<StreamStatus>,
    /// Set by the owner (and on Drop) — parked readers and the fetch
    /// task check it and unwind/return promptly.
    shutdown: Arc<AtomicBool>,
}

struct FetchState {
    /// Read-ahead bytes covering `[buf_start, buf_start + buf.len())`.
    buf: std::collections::VecDeque<u8>,
    buf_start: u64,
    /// Next absolute byte the fetch task will request.
    fetch_offset: u64,
    len: Option<u64>,
    eof: bool,
    /// Pending seek target from a reader; the fetch thread consumes it.
    seek_req: Option<u64>,
    starved: bool,
    /// The URL was answered 403/410: terminal for this resolution.
    url_dead: bool,
}

impl PrefetchReader {
    /// Creates the reader and its fetch task. The first chunk is probed
    /// synchronously (on the loading thread — never the audio callback) to
    /// learn the length.
    pub fn new(
        url: impl Into<String>,
        headers: Vec<(String, String)>,
        status: watch::Sender<StreamStatus>,
        shutdown: Arc<AtomicBool>,
    ) -> Result<Self, RangeError> {
        let url = url.into();
        let original_local = media_host_is_local(&url);
        let original_scheme = reqwest::Url::parse(&url)
            .map(|u| u.scheme().to_string())
            .unwrap_or_default();
        // DNS-rebinding guard: same resolve-validate-pin as HttpRangeReader.
        let parsed =
            reqwest::Url::parse(&url).map_err(|e| RangeError::Http(format!("media url: {e}")))?;
        let mut builder = reqwest::blocking::Client::builder()
            .timeout(TIMEOUT)
            .user_agent("Tansen/0.3");
        if let Some(domain) = netguard::dns_host(&parsed) {
            let addr = netguard::resolve_validated(&parsed).map_err(RangeError::Http)?;
            builder = builder.resolve(&domain, addr);
        }
        let shared = Arc::new(PrefetchShared {
            url: url.clone(),
            headers,
            client: builder
                .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                    match media_redirect_verdict(
                        attempt.previous().len(),
                        attempt.url(),
                        original_local,
                        &original_scheme,
                    ) {
                        Ok(()) => attempt.follow(),
                        Err(e) => attempt.error(e),
                    }
                }))
                .build()
                .map_err(|e| RangeError::Http(e.to_string()))?,
            state: Mutex::new(FetchState {
                buf: std::collections::VecDeque::new(),
                buf_start: 0,
                fetch_offset: 0,
                len: None,
                eof: false,
                seek_req: None,
                starved: false,
                url_dead: false,
            }),
            fetch_signal: Condvar::new(),
            read_signal: Condvar::new(),
            status,
            shutdown,
        });

        // Probe: first chunk synchronously so `len` is known.
        let probe = fetch_range_into(&shared, 0, FETCH_CHUNK)?;
        if probe.is_empty() {
            return Err(RangeError::Http("probe fetch failed".into()));
        }
        let len = shared
            .state
            .lock()
            .map_err(|_| RangeError::Http("lock poisoned".into()))?
            .len;

        let thread_shared = shared.clone();
        std::thread::Builder::new()
            .name("prefetch-fetch".into())
            .spawn(move || fetch_loop(thread_shared))
            .map_err(|e| RangeError::Http(format!("fetch task: {e}")))?;
        Ok(Self {
            shared,
            pos: 0,
            len,
        })
    }

    /// The probed total stream length, when the server declared it.
    /// Used by the decode path to mark the source seekable for Symphonia.
    pub fn byte_len(&self) -> Option<u64> {
        self.len
    }
}

impl Read for PrefetchReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        let shared = &self.shared;
        let mut st = shared.state.lock().map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::Other, "prefetch lock poisoned")
        })?;
        loop {
            if shared.shutdown.load(Ordering::SeqCst) {
                return Ok(0); // reader dropped mid-stall: treat as clean end
            }
            if let Some(len) = st.len {
                if self.pos >= len {
                    return Ok(0); // natural EOF
                }
            }
            let available = if self.pos >= st.buf_start {
                (st.buf.len() as u64).saturating_sub(self.pos - st.buf_start) as usize
            } else {
                0
            };
            if available > 0 {
                let offset = (self.pos - st.buf_start) as usize;
                let n = out.len().min(available);
                for (i, b) in st.buf.iter().skip(offset).take(n).enumerate() {
                    out[i] = *b;
                }
                // Consume the prefix so the buffer stays bounded.
                for _ in 0..offset {
                    st.buf.pop_front();
                }
                st.buf_start = self.pos;
                self.pos += n as u64;
                if st.buf.len() < LOW_WATER {
                    shared.fetch_signal.notify_one();
                }
                return Ok(n);
            }
            if st.eof || st.url_dead {
                // Dead URL: end the old decode cleanly; the engine's
                // re-resolve (UrlDead on the status watch) rebuilds the
                // pipeline at the current position.
                return Ok(0);
            }
            // Underflow: tell the engine (once per stall) and wait on the
            // in-memory condvar — the fetch task owns all network I/O.
            if !st.starved {
                st.starved = true;
                let _ = shared.status.send(StreamStatus::Starved);
            }
            st = shared.read_signal.wait(st).map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::Other, "prefetch lock poisoned")
            })?;
        }
    }
}

impl Seek for PrefetchReader {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let target = match pos {
            SeekFrom::Start(n) => n,
            SeekFrom::Current(n) => (self.pos as i64 + n).max(0) as u64,
            SeekFrom::End(n) => match self.len {
                Some(len) => (len as i64 + n).max(0) as u64,
                None => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Unsupported,
                        "seek from End without a known length",
                    ))
                }
            },
        };
        let shared = &self.shared;
        let mut st = shared.state.lock().map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::Other, "prefetch lock poisoned")
        })?;
        loop {
            if shared.shutdown.load(Ordering::SeqCst) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "prefetch reader dropped",
                ));
            }
            st.seek_req = Some(target);
            shared.fetch_signal.notify_one();
            let (st2, _) = shared
                .read_signal
                .wait_timeout(st, Duration::from_millis(200))
                .map_err(|_| {
                    std::io::Error::new(std::io::ErrorKind::Other, "prefetch lock poisoned")
                })?;
            st = st2;
            if st.seek_req.is_none() && st.buf_start == target && st.fetch_offset == target {
                break; // the fetch thread acknowledged the reset
            }
        }
        self.pos = target;
        Ok(target)
    }
}

impl Drop for PrefetchReader {
    fn drop(&mut self) {
        self.shared.shutdown.store(true, Ordering::SeqCst);
        self.shared.fetch_signal.notify_all();
        self.shared.read_signal.notify_all();
    }
}

/// One ranged fetch, appended to the buffer. Returns the bytes. A response
/// shorter than requested marks EOF. In-flight results are discarded if a
/// seek changed the target while the request was running.
fn fetch_range_into(shared: &PrefetchShared, start: u64, end: u64) -> Result<Vec<u8>, RangeError> {
    let mut request = shared
        .client
        .get(&shared.url)
        .header("Range", format!("bytes={start}-{end}"));
    for (name, value) in &shared.headers {
        request = request.header(name, value);
    }
    let response = request
        .send()
        // Signed media URLs stay out of transport errors.
        .map_err(|e| RangeError::Http(e.without_url().to_string()))?;
    let status = response.status().as_u16();
    #[cfg(debug_assertions)]
    tracing::debug!(transport = "prefetch", start, end, status, "range response");
    match status {
        206 => {}
        200 => return Err(RangeError::NoRangeSupport(200)),
        // Expired/revoked URL — terminal, the engine re-resolves.
        403 | 410 => return Err(RangeError::Forbidden(status)),
        status => return Err(RangeError::Http(format!("range refused ({status})"))),
    }
    if let Some(range) = response
        .headers()
        .get("Content-Range")
        .and_then(|v| v.to_str().ok())
    {
        if let Some(total) = range.rsplit('/').next().and_then(|t| t.parse::<u64>().ok()) {
            let mut st = shared
                .state
                .lock()
                .map_err(|_| RangeError::Http("lock poisoned".into()))?;
            st.len = Some(total);
        }
    }
    // Same server-bound as fetch_chunk: a 206 body is trusted only up to
    // the requested window, never the wire.
    let mut bytes = Vec::new();
    response
        .take((FETCH_CHUNK + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| RangeError::Http(e.to_string()))?;
    if bytes.len() as u64 > FETCH_CHUNK {
        return Err(RangeError::Http("oversized range body".into()));
    }
    #[cfg(debug_assertions)]
    tracing::debug!(
        transport = "prefetch",
        start,
        requested_end = end,
        bytes = bytes.len(),
        "range body"
    );
    let mut st = shared
        .state
        .lock()
        .map_err(|_| RangeError::Http("lock poisoned".into()))?;
    if st.fetch_offset == start && st.seek_req.is_none() {
        for b in &bytes {
            st.buf.push_back(*b);
        }
        st.fetch_offset = start + bytes.len() as u64;
    }
    Ok(bytes.to_vec())
}

/// The fetch task: owns every network operation for one stream. Tops the
/// buffer up to the high watermark, idles above it, consumes seek requests,
/// and reports Refilled once a starved stream has data again.
fn fetch_loop(shared: Arc<PrefetchShared>) {
    let mut st = match shared.state.lock() {
        Ok(s) => s,
        Err(_) => return,
    };
    loop {
        if shared.shutdown.load(Ordering::SeqCst) {
            return;
        }
        if let Some(target) = st.seek_req.take() {
            st.buf.clear();
            st.buf_start = target;
            st.fetch_offset = target;
            st.eof = false;
            shared.read_signal.notify_all(); // seek acknowledged
            continue;
        }
        if st.eof || st.buf.len() >= HIGH_WATER {
            let (st2, _) = shared
                .fetch_signal
                .wait_timeout(st, Duration::from_millis(500))
                .unwrap_or_else(|p| p.into_inner());
            st = st2;
            continue;
        }
        let start = st.fetch_offset;
        drop(st);
        let result = fetch_range_into(&shared, start, start.saturating_add(FETCH_CHUNK - 1));
        st = shared.state.lock().unwrap_or_else(|p| p.into_inner());
        match result {
            Ok(bytes) => {
                if bytes.len() < FETCH_CHUNK as usize {
                    st.eof = true;
                }
                if st.starved && (st.buf.len() >= LOW_WATER || st.eof) {
                    st.starved = false;
                    let _ = shared.status.send(StreamStatus::Refilled);
                }
                shared.read_signal.notify_all();
            }
            Err(RangeError::Forbidden(_status)) => {
                // The URL is dead. Terminal for this resolution — the
                // engine re-resolves on the status watch. Park until the
                // reader is dropped (shutdown) or seeked away.
                st.url_dead = true;
                st.eof = true;
                let _ = shared.status.send(StreamStatus::UrlDead);
                shared.read_signal.notify_all();
                while st.seek_req.is_none() && !shared.shutdown.load(Ordering::SeqCst) {
                    st = shared
                        .fetch_signal
                        .wait(st)
                        .unwrap_or_else(|p| p.into_inner());
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "prefetch fetch failed; retrying in 1 s");
                let (st2, _) = shared
                    .fetch_signal
                    .wait_timeout(st, Duration::from_secs(1))
                    .unwrap_or_else(|p| p.into_inner());
                st = st2;
            }
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use std::io::Write as _;
    use std::net::TcpListener;

    /// A ranged HTTP server over a fixed byte blob (206 + Content-Range).
    fn range_server(data: &'static [u8]) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut buf = [0u8; 4096];
                let n = std::io::Read::read(&mut &stream, &mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]);
                let range = request
                    .lines()
                    .find(|l| l.to_ascii_lowercase().starts_with("range:"))
                    .and_then(|l| l.split("bytes=").nth(1))
                    .map(|s| s.trim().to_string());
                let (status, body, extra) = match range.as_deref().and_then(parse_range) {
                    Some((a, b)) if a < data.len() as u64 => {
                        let b = (b.min(data.len() as u64 - 1)) as usize;
                        let a = a as usize;
                        (
                            "206 Partial Content",
                            &data[a..=b],
                            format!("Content-Range: bytes {}-{}/{}\r\n", a, b, data.len()),
                        )
                    }
                    _ => ("200 OK", data, String::new()),
                };
                let response = format!(
                    "HTTP/1.1 {}\r\nContent-Type: application/octet-stream\r\n{}Accept-Ranges: bytes\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    status,
                    extra,
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(body);
            }
        });
        port
    }

    fn parse_range(spec: &str) -> Option<(u64, u64)> {
        let (a, b) = spec.split_once('-')?;
        Some((a.parse().ok()?, b.parse().ok()?))
    }

    #[test]
    fn read_seek_and_length_across_chunks() {
        // 1.5 MiB of a recognizable pattern: more than one 512 KiB chunk.
        let data: Vec<u8> = (0..1_572_864u32).map(|i| (i % 251) as u8).collect();
        let boxed: &'static [u8] = Box::leak(data.into_boxed_slice());
        let port = range_server(boxed);
        let mut reader =
            HttpRangeReader::new(format!("http://127.0.0.1:{port}/audio.mp4"), vec![]).unwrap();

        assert_eq!(reader.len, Some(boxed.len() as u64));

        // Sequential read crosses the first chunk boundary.
        let mut out = Vec::new();
        reader.read_to_end(&mut out).unwrap();
        assert_eq!(out.len(), boxed.len());
        assert_eq!(out[600_000], boxed[600_000]);
        assert_eq!(*out.last().unwrap(), *boxed.last().unwrap());

        // Seek back into the middle of a previous chunk and read.
        reader.seek(SeekFrom::Start(700_000)).unwrap();
        let mut mid = [0u8; 16];
        reader.read_exact(&mut mid).unwrap();
        assert_eq!(&mid, &boxed[700_000..700_016]);

        // SeekFrom::End and Current.
        reader.seek(SeekFrom::End(-4)).unwrap();
        let mut tail = [0u8; 4];
        reader.read_exact(&mut tail).unwrap();
        assert_eq!(&tail, &boxed[boxed.len() - 4..]);
        reader.seek(SeekFrom::Current(-2)).unwrap();
        let mut two = [0u8; 2];
        reader.read_exact(&mut two).unwrap();
        assert_eq!(&two, &boxed[boxed.len() - 2..]);
    }

    /// The engine path end to end: decode audio served ONLY over range
    /// requests — no download, no device. Hermetic: the blob is the committed
    /// test WAV.
    #[test]
    fn decodes_audio_from_a_ranged_http_stream() {
        let wav = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/testmusic/Test Signal/test.wav"),
        )
        .expect("fixture");
        let wav: &'static [u8] = Box::leak(wav.into_boxed_slice());
        let port = range_server(wav);

        let reader =
            HttpRangeReader::new(format!("http://127.0.0.1:{port}/test.wav"), vec![]).unwrap();
        let rms = crate::probe::decode_rms(reader, Duration::from_secs(2)).expect("decodes");
        assert!(rms > 0.005, "decoded remote audio is silence (RMS {rms})");
    }
}

#[cfg(test)]
mod prefetch_tests {
    use super::*;
    use std::io::Write as _;
    use std::net::TcpListener;

    /// Ranged server over a blob; captures every Range header it saw.
    fn range_server_capturing(
        data: &'static [u8],
        ranges: Arc<Mutex<Vec<String>>>,
        stall_after: Option<Arc<AtomicBool>>,
    ) -> (u16, Arc<AtomicBool>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown2 = shutdown.clone();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                if shutdown2.load(Ordering::SeqCst) {
                    return;
                }
                let mut buf = [0u8; 4096];
                let n = std::io::Read::read(&mut &stream, &mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]);
                let range = request
                    .lines()
                    .find(|l| l.to_ascii_lowercase().starts_with("range:"))
                    .map(|l| l.trim().to_owned())
                    .unwrap_or_default();
                ranges.lock().unwrap().push(range);

                if stall_after
                    .as_ref()
                    .is_some_and(|s| s.load(Ordering::SeqCst))
                {
                    // Accept the request, never answer: the fetch task must
                    // own the wait.
                    std::thread::sleep(Duration::from_secs(30));
                    continue;
                }
                let start = request
                    .lines()
                    .find(|l| l.to_ascii_lowercase().starts_with("range:"))
                    .and_then(|l| l.split('=').nth(1))
                    .and_then(|r| r.split('-').next())
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(0);
                let end = (start + FETCH_CHUNK as usize).min(data.len());
                let body = &data[start..end];
                let head = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{end}/{total}\r\nContent-Length: {body_len}\r\nConnection: close\r\n\r\n",
                    end = end.saturating_sub(1),
                    total = data.len(),
                    body_len = body.len(),
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(body);
            }
        });
        (port, shutdown)
    }

    #[test]
    fn sequential_read_matches_blob_and_stays_bounded() {
        let data: &'static [u8] = Box::leak(vec![42u8; 300_000].into_boxed_slice());
        let ranges = Arc::new(Mutex::new(Vec::new()));
        let (port, _shutdown) = range_server_capturing(data, ranges.clone(), None);
        let (tx, rx) = tokio::sync::watch::channel(StreamStatus::Refilled);
        let mut reader = PrefetchReader::new(
            format!("http://127.0.0.1:{port}/a.flac"),
            vec![],
            tx,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();

        let mut got = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let n = reader.read(&mut chunk).unwrap();
            if n == 0 {
                break;
            }
            got.extend_from_slice(&chunk[..n]);
        }
        assert_eq!(got, data, "sequential read is byte-exact");
        // Bounded: never more than HIGH buffered.
        let st = reader.shared.state.lock().unwrap();
        assert!(st.buf.len() <= HIGH_WATER);
        // No starvation on a fast local server.
        assert!(!rx.has_changed().unwrap() || *rx.borrow() == StreamStatus::Refilled);
    }

    #[test]
    fn seek_issues_a_new_range_request_at_the_target() {
        let data: &'static [u8] = Box::leak(vec![7u8; 200_000].into_boxed_slice());
        let ranges = Arc::new(Mutex::new(Vec::new()));
        let (port, _shutdown) = range_server_capturing(data, ranges.clone(), None);
        let (tx, _rx) = tokio::sync::watch::channel(StreamStatus::Refilled);
        let mut reader = PrefetchReader::new(
            format!("http://127.0.0.1:{port}/a.flac"),
            vec![],
            tx,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();

        reader.seek(SeekFrom::Start(64_000)).unwrap();
        let mut got = Vec::new();
        reader.read_to_end(&mut got).unwrap();
        assert_eq!(got, &data[64_000..], "post-seek bytes are exact");
        let seen = ranges.lock().unwrap();
        assert!(
            seen.iter().any(|r| r.contains("bytes=64000-")),
            "seek issued a fresh Range at the target: {seen:?}"
        );
    }

    #[test]
    fn forbidden_url_reports_url_dead_and_ends_cleanly() {
        let data: &'static [u8] = Box::leak(vec![3u8; 600_000].into_boxed_slice());
        let forbidden = Arc::new(AtomicBool::new(false));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let forbidden2 = forbidden.clone();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut buf = [0u8; 4096];
                let n = std::io::Read::read(&mut &stream, &mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]);
                let start = request
                    .lines()
                    .find(|l| l.to_ascii_lowercase().starts_with("range:"))
                    .and_then(|l| l.split('=').nth(1))
                    .and_then(|r| r.split('-').next())
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(0);
                if forbidden2.load(Ordering::SeqCst) && start > 0 {
                    let _ = stream.write_all(
                        b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                    continue;
                }
                let end = (start + FETCH_CHUNK as usize).min(data.len());
                let body = &data[start..end];
                let head = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{}/{total}\r\nContent-Length: {body_len}\r\nConnection: close\r\n\r\n",
                    end.saturating_sub(1),
                    total = data.len(),
                    body_len = body.len(),
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(body);
            }
        });

        let (tx, mut rx) = tokio::sync::watch::channel(StreamStatus::Refilled);
        let mut reader = PrefetchReader::new(
            format!("http://127.0.0.1:{port}/a.flac"),
            vec![],
            tx,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();

        // 403 for every fetch past the probe: the drain consumes the primed
        // 512 KiB, the next fetch hits the 403, and the reader must report
        // UrlDead and end the old decode cleanly.
        forbidden.store(true, Ordering::SeqCst);
        let mut chunk = [0u8; 8192];
        while reader.read(&mut chunk).unwrap() > 0 {}
        rx.mark_changed();
        assert_eq!(*rx.borrow_and_update(), StreamStatus::UrlDead);
    }

    #[test]
    fn underflow_reports_starved_then_refilled() {
        let data: &'static [u8] = Box::leak(vec![9u8; 2 * HIGH_WATER + 1000].into_boxed_slice());
        let ranges = Arc::new(Mutex::new(Vec::new()));
        let stall = Arc::new(AtomicBool::new(false));
        let (port, _shutdown) = range_server_capturing(data, ranges.clone(), Some(stall.clone()));
        let (tx, mut rx) = tokio::sync::watch::channel(StreamStatus::Refilled);
        let reader = PrefetchReader::new(
            format!("http://127.0.0.1:{port}/a.flac"),
            vec![],
            tx,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();

        // Stall the server right after the probe: the fetch task's next
        // Range request hangs, so the decode thread starves once the primed
        // buffer is drained — with no network I/O on its own thread.
        stall.store(true, Ordering::SeqCst);
        let reader = std::sync::Mutex::new(reader);
        let decode_thread = std::thread::spawn(move || {
            let mut r = reader.lock().unwrap();
            let mut chunk = [0u8; 65536];
            for _ in 0..100 {
                let n = r.read(&mut chunk).unwrap();
                if n == 0 {
                    break;
                }
            }
        });

        // The decode thread is now blocked on the in-memory condvar; the
        // status watch must have reported Starved.
        rx.mark_changed();
        let mut starved = false;
        for _ in 0..50 {
            if *rx.borrow_and_update() == StreamStatus::Starved {
                starved = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(starved, "underflow reported while the decoder waits");

        // Un-stall: the fetch task completes its request, the decode thread
        // finishes, and the recovery is reported.
        stall.store(false, Ordering::SeqCst);
        decode_thread.join().unwrap();
        rx.mark_changed();
        let mut refill_seen = false;
        for _ in 0..40 {
            if *rx.borrow_and_update() == StreamStatus::Refilled {
                refill_seen = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        assert!(refill_seen, "refill reported after the stall cleared");
    }
}

#[cfg(test)]
mod redaction_tests {
    use super::HttpRangeReader;

    #[test]
    fn redacted_strips_the_query_string() {
        // Subsonic tokens and googlevideo signatures ride in the query.
        let url =
            "https://rr5.example.googlevideo.com/videoplayback?expire=1&signature=ABC&sparams=id";
        assert_eq!(
            HttpRangeReader::redacted(url),
            "https://rr5.example.googlevideo.com/videoplayback"
        );
        assert_eq!(
            HttpRangeReader::redacted("https://x/y.flac"),
            "https://x/y.flac"
        );
    }
}

#[cfg(test)]
mod redirect_policy_tests {
    use super::*;

    #[test]
    fn redirect_verdict_refuses_downgrade_private_and_long_chains() {
        let url = |s: &str| reqwest::Url::parse(s).unwrap();
        assert!(media_redirect_verdict(0, &url("https://1.1.1.1/x"), false, "https").is_ok());
        assert!(
            media_redirect_verdict(0, &url("http://1.1.1.1/x"), false, "https").is_err(),
            "https may not downgrade"
        );
        assert!(
            media_redirect_verdict(0, &url("https://127.0.0.1/x"), false, "https").is_err(),
            "loopback is refused for a public source"
        );
        assert!(
            media_redirect_verdict(0, &url("https://127.0.0.1/x"), true, "http").is_ok(),
            "a local source may stay local"
        );
        assert!(
            media_redirect_verdict(5, &url("https://1.1.1.1/x"), false, "https").is_err(),
            "more than five hops is refused"
        );
        assert!(
            media_redirect_verdict(0, &url("file:///etc/passwd"), false, "https").is_err(),
            "non-http scheme is refused"
        );
    }

    #[test]
    fn media_host_locality() {
        assert!(media_host_is_local("http://127.0.0.1:9000/x"));
        assert!(media_host_is_local("http://10.0.0.5/x"));
        assert!(media_host_is_local("http://169.254.1.1/x"));
        assert!(media_host_is_local("http://localhost:8080/x"));
        assert!(!media_host_is_local(
            "https://rr3.example.googlevideo.com/videoplayback"
        ));
        assert!(!media_host_is_local("not a url"));
    }

    /// Regression (media-redirect-verdict
    /// .private-host-classification-gaps): the classifier used to re-parse
    /// `host_str()`, whose bracketed IPv6 serialization never parses as
    /// IpAddr — every IPv6 URL host, loopback `::1` included, classified
    /// non-local and its redirect was followed.
    #[test]
    fn ipv6_mapped_and_unspecified_hosts_classify_local() {
        assert!(
            media_host_is_local("http://[::1]:9000/x"),
            "::1 must classify local"
        );
        assert!(
            media_host_is_local("http://[fd00::1]/x"),
            "ULA must classify local"
        );
        assert!(
            media_host_is_local("http://[fe80::1]/x"),
            "v6 link-local must classify local"
        );
        assert!(
            media_host_is_local("http://[::ffff:127.0.0.1]:9000/x"),
            "IPv4-mapped loopback must classify local"
        );
        assert!(
            media_host_is_local("http://[::ffff:10.0.0.5]:9000/x"),
            "IPv4-mapped private must classify local"
        );
        assert!(
            media_host_is_local("http://0.0.0.0:9000/x"),
            "unspecified must classify local"
        );
        // Non-canonical IPv4 spellings were already safe: the url crate
        // canonicalizes them before host_str() (kept as pinned behavior).
        assert!(
            media_host_is_local("http://2130706433/x"),
            "decimal IPv4 canonicalizes to 127.0.0.1"
        );
        assert!(
            !media_host_is_local("http://[2001:4860:4860::8888]/x"),
            "public v6 stays non-local"
        );
    }

    /// The rebinding guard accepts loopback literals without DNS.
    #[test]
    fn resolve_validated_accepts_loopback_literal() {
        let parsed = reqwest::Url::parse("http://127.0.0.1:1/x").unwrap();
        let addr = netguard::resolve_validated(&parsed).unwrap();
        assert_eq!(addr.ip().to_string(), "127.0.0.1");
    }

    /// Regression (media-range-fetch
    /// .response-size-not-client-bounded): any 206 body used to be read in
    /// full via response.bytes() with no check against the requested
    /// window; a hostile server could push throughput x 20 s into the
    /// decode thread. The body is now hard-capped at CHUNK + refused.
    /// Loopback-only: the mock server is a local TcpListener.
    #[test]
    fn oversized_206_body_is_refused_not_buffered() {
        use std::io::{Read as _, Write as _};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut stream = stream;
            // Drain the small request head (never parsed; the test only
            // needs it consumed).
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let head = "HTTP/1.1 206 Partial Content\r\n\
                        Content-Range: bytes 0-524287/524288\r\n\
                        Content-Type: application/octet-stream\r\n\
                        Connection: close\r\n\r\n";
            let _ = stream.write_all(head.as_bytes());
            // 4 MiB: 8x the requested window; the old code buffered it all.
            // The corrected reader stops at CHUNK+1 and drops the
            // connection, so these writes fail with BrokenPipe — the
            // refusal is the behavior under test, so write errors are
            // expected and ignored.
            let _ = stream.write_all(&vec![0u8; 4 * 1024 * 1024]);
        });

        let mut reader =
            HttpRangeReader::new(format!("http://127.0.0.1:{port}/x"), vec![]).unwrap();
        let mut out = [0u8; 64];
        let n = std::io::Read::read(&mut reader, &mut out).unwrap();
        assert_eq!(n, 0, "oversized 206 body must be refused, not buffered");
        server.join().unwrap();
    }
}
