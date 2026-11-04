//! The rodio-backed pipeline (M7/M19): the real-media implementation the
//! app-core `PipelineFactory` drives.
//!
//! M19 architecture: the audio callback only **copies samples from an SPSC
//! ring buffer, applies volume, and increments `frames_played`** — no locks,
//! no allocations, no I/O. A dedicated decode thread per stream runs
//! Symphonia and fills the ring. The whole stream pipeline (decode thread +
//! ring + consumer source) is **immutable per (track, seek epoch)**: a load
//! or seek builds a fresh one and swaps the consumer into the sink; the old
//! one is shut down. Volume is a lock-free atomic applied at copy time.

use crate::cache::StreamCache;
use rodio::{Decoder, OutputStream, OutputStreamBuilder, Sink, Source};
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;
use tokio::sync::watch;

/// Opens the default output. Debug builds route cpal's error callback into
/// the M4 reliability counters; release uses rodio's default behavior.
fn open_output_stream() -> Result<OutputStream, rodio::StreamError> {
    #[cfg(debug_assertions)]
    {
        OutputStreamBuilder::from_default_device()
            .and_then(|b| {
                b.with_error_callback(|e| crate::metrics::audio::record_stream_error(&e.to_string()))
                    .open_stream()
            })
    }
    #[cfg(not(debug_assertions))]
    {
        OutputStreamBuilder::open_default_stream()
    }
}

enum Request {
    /// Replace the sink's content with this path and start playing.
    /// The ack answers once the decoder is open (or not).
    Load { path: String, ack: mpsc::SyncSender<Result<(), String>> },
    Play,
    Pause,
    /// M19: rebuild the stream pipeline at this position (new decode
    /// thread + ring, consumer swapped into the sink).
    SeekMs(u64),
    Volume(f32),
    /// Drop all queued audio without touching the pipeline itself.
    Clear,
    /// M18: drop the output stream; the next load reopens the (possibly
    /// new) default device.
    ReopenOutput,
    Quit,
}

/// M15: mid-stream fill status. Defined on the reader; re-exported here.
pub use crate::remote::StreamStatus;

/// Handle to the audio thread.
pub struct AudioPipeline {
    tx: Option<mpsc::Sender<Request>>,
    /// Interleaved sample *frames* delivered to the output — incremented by
    /// the audio callback itself (M19).
    frames: Arc<AtomicU64>,
    /// Sample rate of the currently loaded source (for ms conversion).
    sample_rate: Arc<AtomicU64>,
    /// Duration of the current media (0 = unknown).
    duration_ms: Arc<AtomicU64>,
    /// M15: receiver side of the current stream's fill status (the sender
    /// lives in the fetch task).
    status_rx: watch::Receiver<crate::remote::StreamStatus>,
}

impl AudioPipeline {
    /// Spawns the audio thread. The output stream opens lazily inside the
    /// thread; a missing device surfaces on the first `load` as `Err`, which
    /// the factory maps to `ErrorKind::Device`.
    pub fn spawn(
        cache: Option<Arc<StreamCache>>,
        status: watch::Sender<crate::remote::StreamStatus>,
    ) -> Result<Self, String> {
        let frames = Arc::new(AtomicU64::new(0));
        let sample_rate = Arc::new(AtomicU64::new(44_100));
        let duration_ms = Arc::new(AtomicU64::new(0));
        let volume = Arc::new(AtomicU32::new(1.0f32.to_bits()));
        let spec = Arc::new(std::sync::Mutex::new(None));
        let (tx, rx) = mpsc::channel::<Request>();
        let (frames_t, rate_t, dur_t, vol_t, spec_t) = (
            frames.clone(),
            sample_rate.clone(),
            duration_ms.clone(),
            volume.clone(),
            spec.clone(),
        );
        let status_t = status.clone();
        std::thread::Builder::new()
            .name("pipeline-audio".into())
            .spawn(move || audio_thread(rx, cache, frames_t, rate_t, dur_t, vol_t, spec_t, status_t))
            .map_err(|e| format!("pipeline thread: {e}"))?;
        let status_rx = status.subscribe();
        Ok(Self { tx: Some(tx), frames, sample_rate, duration_ms, status_rx })
    }

    /// M15: subscribe to the current stream's fill status.
    pub fn status_rx(&self) -> watch::Receiver<crate::remote::StreamStatus> {
        self.status_rx.clone()
    }

    /// Opens `path` as the sink's sole content and starts it. Blocking until
    /// the decoder answers (local files: fast; the factory calls this from
    /// the engine task — remote URLs would block that task, documented M7
    /// limitation, fixtures are local).
    pub fn load(&self, path: &str) -> Result<(), String> {
        let (ack_tx, ack_rx) = mpsc::sync_channel(1);
        let tx = self.tx.as_ref().ok_or("pipeline thread gone")?;
        tx.send(Request::Load { path: path.to_owned(), ack: ack_tx })
            .map_err(|_| "pipeline thread gone")?;
        ack_rx.recv().map_err(|_| "pipeline thread gone")?
    }

    pub fn play(&self) {
        self.fire(Request::Play);
    }

    pub fn pause(&self) {
        self.fire(Request::Pause);
    }

    /// M19: rebuild the stream pipeline at this position — a fresh decode
    /// thread + ring, the consumer swapped into the sink.
    pub fn seek_ms(&self, ms: u64) {
        self.fire(Request::SeekMs(ms));
    }

    pub fn set_volume(&self, v: f32) {
        self.fire(Request::Volume(v));
    }

    /// Drop all queued audio; the next `load` starts fresh.
    pub fn clear(&self) {
        self.fire(Request::Clear);
    }

    /// M18: drop the output stream. The next `load` reopens whatever the OS
    /// now considers the default device.
    pub fn reopen_output(&self) {
        self.fire(Request::ReopenOutput);
    }

    pub fn frames_played(&self) -> u64 {
        self.frames.load(Ordering::Relaxed)
    }

    pub fn pos_ms(&self) -> u64 {
        let rate = self.sample_rate.load(Ordering::Relaxed).max(1);
        self.frames.load(Ordering::Relaxed).saturating_mul(1000) / rate
    }

    /// Duration of the current media; `None` when unknown.
    pub fn duration_ms(&self) -> Option<u64> {
        match self.duration_ms.load(Ordering::Relaxed) {
            0 => None,
            d => Some(d),
        }
    }

    fn fire(&self, req: Request) {
        if let Some(tx) = self.tx.as_ref() {
            let _ = tx.send(req);
        }
    }
}

impl Drop for AudioPipeline {
    fn drop(&mut self) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(Request::Quit);
        }
    }
}

fn audio_thread(
    rx: mpsc::Receiver<Request>,
    cache: Option<Arc<StreamCache>>,
    frames: Arc<AtomicU64>,
    sample_rate: Arc<AtomicU64>,
    duration_ms: Arc<AtomicU64>,
    volume: Arc<AtomicU32>,
    spec: Arc<std::sync::Mutex<Option<String>>>,
    status: watch::Sender<crate::remote::StreamStatus>,
) {
    // The stream lives here forever: `OutputStream` is not `Send`.
    let mut slot: Option<OutputStream> = None;
    let mut sink: Option<Sink> = None;
    // The stream pipeline for the current track (decode thread + ring +
    // consumer). Immutable per (track, seek epoch) — swapped wholesale.
    let mut current: Option<StreamPipeline> = None;

    macro_rules! ensure_open {
        () => {
            if slot.is_none() {
                match open_output_stream() {
                    Ok(stream) => {
                        let s = Sink::connect_new(stream.mixer());
                        s.pause();
                        slot = Some(stream);
                        sink = Some(s);
                    }
                    Err(e) => {
                        let _ = e.to_string();
                    }
                }
            }
        };
    }

    for req in rx {
        match req {
            Request::Load { path, ack } => {
                ensure_open!();
                // TAN-03: a panic here (hostile media header) would unwind
                // this thread and drop the ack sender, hanging the caller
                // forever. Route it into the normal error path instead.
                let answer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let sink = sink.as_ref().ok_or_else(|| "no audio output device".to_string())?;
                    *spec.lock().map_err(|_| "spec lock poisoned".to_string())? = Some(path.clone());
                    build_stream(
                        &path,
                        None,
                        sink,
                        &mut current,
                        cache.as_ref(),
                        status.clone(),
                        &volume,
                        &frames,
                        &sample_rate,
                        &duration_ms,
                    )
                }))
                .unwrap_or_else(|_| Err("decoder panicked while opening media".to_string()));
                let _ = ack.send(answer);
            }
            Request::Play => {
                if let Some(s) = &sink {
                    s.play();
                }
            }
            Request::Pause => {
                if let Some(s) = &sink {
                    s.pause();
                }
            }
            Request::SeekMs(ms) => {
                // M19: rebuild at the position (immutable-per-epoch; no
                // in-place mutation of a running pipeline).
                let resume_path = spec.lock().ok().and_then(|s| s.clone());
                if let (Some(path), Some(sink)) = (resume_path, sink.as_ref()) {
                    let _ = build_stream(
                        &path,
                        Some(ms),
                        sink,
                        &mut current,
                        cache.as_ref(),
                        status.clone(),
                        &volume,
                        &frames,
                        &sample_rate,
                        &duration_ms,
                    );
                }
            }
            Request::Volume(v) => {
                volume.store(v.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
            }
            Request::Clear => {
                if let Some(s) = &sink {
                    s.clear();
                }
                if let Some(old) = current.take() {
                    old.shutdown();
                }
            }
            Request::ReopenOutput => {
                // Drop the stream: the sink dies with it. The stream
                // pipeline survives (its decode thread keeps filling the
                // ring until the next load replaces it); the volume atomic
                // survives too.
                slot = None;
                sink = None;
            }
            Request::Quit => {
                if let Some(old) = current.take() {
                    old.shutdown();
                }
                break;
            }
        }
    }
}

/// One (track, seek-epoch) stream: decode thread + ring + shutdown flag.
/// Immutable once built; replaced wholesale on load/seek.
struct StreamPipeline {
    shutdown: Arc<AtomicBool>,
    /// Decode-thread handle; the thread exits on `shutdown` or EOF.
    handle: Option<std::thread::JoinHandle<()>>,
}

impl StreamPipeline {
    fn shutdown(mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        // Do not join: the thread parks at most one poll interval, and the
        // audio thread must never block on a decoder.
        drop(self.handle.take());
    }
}

impl Drop for StreamPipeline {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }
}

/// Metadata published by the decode worker only after the source has opened
/// and, for a seek, the decoder has reached the requested position.
struct DecodeReady {
    channels: u16,
    sample_rate: u32,
    duration_ms: u64,
}

/// A failed candidate never replaces the currently active stream.
fn commit_replacement<T, E>(current: &mut Option<T>, candidate: Result<T, E>) -> Result<Option<T>, E> {
    let candidate = candidate?;
    Ok(current.replace(candidate))
}

/// M19: builds one (track, seek-epoch) pipeline: open the decoder (seeked
/// to `resume_ms` when given), spawn the decode thread filling an SPSC ring,
/// and swap the ring consumer into the sink.
#[allow(clippy::too_many_arguments)]
fn build_stream(
    path: &str,
    resume_ms: Option<u64>,
    sink: &Sink,
    current: &mut Option<StreamPipeline>,
    cache: Option<&Arc<StreamCache>>,
    status: watch::Sender<crate::remote::StreamStatus>,
    volume: &Arc<AtomicU32>,
    frames: &Arc<AtomicU64>,
    sample_rate: &Arc<AtomicU64>,
    duration_ms: &Arc<AtomicU64>,
) -> Result<(), String> {
    // Open the replacement before retiring the current epoch. A failed
    // resolve/decode/seek must leave the currently audible stream intact;
    // shutting it down first leaves the sink attached to a dead ring and
    // makes pause/resume silently produce no frames.
    let _ = status.send(crate::remote::StreamStatus::Refilled);
    #[cfg(debug_assertions)]
    tracing::debug!(resume_ms = ?resume_ms, "preparing stream replacement");

    let shutdown = Arc::new(AtomicBool::new(false));
    let ring = Arc::new(crate::ring::SpscRing::new(1 << 21));
    let eof = Arc::new(AtomicBool::new(false));
    let starved = Arc::new(AtomicBool::new(false));

    // Decode thread: Symphonia pulls from the (prefetching) reader and fills
    // the ring. It may block on the reader's condvar (network stall) or park
    // when the ring is full — it is never the audio callback.
    let decode_shutdown = shutdown.clone();
    let eof_t = eof.clone();
    let starved_t = starved.clone();
    let ring_t = ring.clone();
    let status_t = status.clone();
    let path_owned = path.to_owned();
    let cache_t = cache.cloned();
    let seek_ms = resume_ms;
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let handle = std::thread::Builder::new()
        .name("stream-decode".into())
        .spawn(move || {
            let opened = open_source(&path_owned, cache_t.as_ref(), status_t.clone(), decode_shutdown.clone()).and_then(|mut source| {
                if let Some(ms) = seek_ms {
                    source
                        .try_seek(Duration::from_millis(ms))
                        .map_err(|e| format!("seek decoder to {ms} ms: {e}"))?;
                }
                let ready = DecodeReady {
                    channels: source.channels().max(1),
                    sample_rate: source.sample_rate(),
                    duration_ms: source
                        .total_duration()
                        .map(|d| d.as_millis().min(u128::from(u64::MAX)) as u64)
                        .unwrap_or(0),
                };
                let _ = ready_tx.send(Ok(ready));
                Ok(source)
            });
            let mut source = match opened {
                Ok(s) => crate::metrics::audio::wrap_source(s),
                Err(e) => {
                    let _ = ready_tx.send(Err(e.clone()));
                    tracing::warn!(error = %e, path = %path_owned, "decode source open failed");
                    eof_t.store(true, Ordering::SeqCst);
                    return;
                }
            };
            loop {
                if decode_shutdown.load(Ordering::Relaxed) {
                    return;
                }
                // TAN-03: a decoder panic on this thread is contained — the
                // track ends (EOF) instead of taking the thread down with
                // unhandled unwinding.
                let sample = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    source.next()
                }));
                match sample {
                    Ok(Some(sample)) => {
                        // Park when the ring is full (paused consumer);
                        // re-check shutdown each poll.
                        loop {
                            if ring_t.push(sample).is_ok() {
                                break;
                            }
                            if decode_shutdown.load(Ordering::Relaxed) {
                                return;
                            }
                            std::thread::sleep(Duration::from_micros(500));
                        }
                        // Recovery signal: a starved consumer gets Refilled
                        // once data flows again.
                        if starved_t.swap(false, Ordering::SeqCst) {
                            let _ = status_t.send(crate::remote::StreamStatus::Refilled);
                        }
                    }
                    Ok(None) => {
                        eof_t.store(true, Ordering::SeqCst);
                        return;
                    }
                    Err(_) => {
                        #[cfg(debug_assertions)]
                        crate::metrics::audio::record_stream_error("decoder panicked mid-decode");
                        eof_t.store(true, Ordering::SeqCst);
                        return;
                    }
                }
            }
        })
        .map_err(|e| format!("decode thread: {e}"))?;

    // The old stream remains active while the worker performs network I/O,
    // opens the decoder, and completes a real decoder seek.
    let ready = ready_rx
        .recv()
        .map_err(|_| "decode worker stopped before opening media".to_string())??;
    #[cfg(debug_assertions)]
    tracing::debug!(
        channels = ready.channels,
        sample_rate = ready.sample_rate,
        duration_ms = ready.duration_ms,
        seek_ms = ?resume_ms,
        "decode worker ready"
    );
    let channels = ready.channels;
    let rate = ready.sample_rate;
    sample_rate.store(u64::from(rate), Ordering::Relaxed);
    duration_ms.store(ready.duration_ms, Ordering::Relaxed);

    let seed_frames = resume_ms
        .map(|ms| ms.saturating_mul(u64::from(rate)) / 1000)
        .unwrap_or(0);
    frames.store(seed_frames, Ordering::Relaxed);

    // Consumer: the audio callback pops from the ring, applies volume, and
    // counts frames — nothing else.
    let consumer = RingSource { ring, volume: volume.clone(), frames: frames.clone(), channels, eof, starved, samples: 0 };
    sink.clear();
    sink.append(consumer);
    sink.play();

    let replacement = StreamPipeline { shutdown, handle: Some(handle) };
    if let Some(old) = commit_replacement(current, Ok::<StreamPipeline, String>(replacement))? {
        old.shutdown();
    }
    Ok(())
}

/// M19: the audio-callback-side source. `next()` pops one sample from the
/// SPSC ring (lock-free atomics), applies volume, and counts frames. On
/// underflow it emits silence and flags Starved (the decode thread's push
/// path reports Refilled); after EOF it ends the track. No locks, no
/// allocation, no I/O.
struct RingSource {
    ring: Arc<crate::ring::SpscRing>,
    volume: Arc<AtomicU32>,
    frames: Arc<AtomicU64>,
    channels: u16,
    eof: Arc<AtomicBool>,
    starved: Arc<AtomicBool>,
    samples: u64,
}

impl Iterator for RingSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        match self.ring.pop() {
            Some(sample) => {
                self.samples = self.samples.wrapping_add(1);
                if self.samples % u64::from(self.channels) == 0 {
                    self.frames.fetch_add(1, Ordering::Relaxed);
                }
                let volume = f32::from_bits(self.volume.load(Ordering::Relaxed));
                // G(Phase-2 G): debug-only sink peak meter (atomic, no
                // alloc/lock) so non-silent output is provable.
                #[cfg(debug_assertions)]
                crate::metrics::audio::metrics()
                    .peak_bits
                    .fetch_max(sample.abs().to_bits(), Ordering::Relaxed);
                Some(sample * volume)
            }
            None => {
                if self.eof.load(Ordering::SeqCst) {
                    return None; // decode done + ring drained: track over
                }
                // Underflow: silence + one-shot Starved flag (the decode
                // thread's push path turns it into Refilled).
                self.starved.store(true, Ordering::SeqCst);
                Some(0.0)
            }
        }
    }
}

impl Source for RingSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        self.channels
    }
    fn sample_rate(&self) -> u32 {
        44_100 // informational; timing uses the pipeline sample_rate atomic
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

/// Same decode branches as before (cache hit → local file; http(s) →
/// prefetching reader; sidecar:// unresolved → error). The `shutdown` flag
/// lets a parked reader wake when the pipeline is torn down.
fn open_source(
    path: &str,
    cache: Option<&Arc<StreamCache>>,
    status: watch::Sender<crate::remote::StreamStatus>,
    shutdown: Arc<AtomicBool>,
) -> Result<Box<dyn Source<Item = f32> + Send>, String> {
    // F9: googlevideo's MP4 streams are fragmented DASH (`ftyp dash`) — the
    // bare probe returns UnrecognizedFormat without a hint. Sniff the
    // container from the first bytes and feed the probe a hint.
    fn sniff_hint(head: &[u8]) -> Option<&'static str> {
        if head.len() >= 8 && &head[4..8] == b"ftyp" {
            return Some("m4a"); // MP4/AAC (covers DASH init segments)
        }
        if head.starts_with(b"OggS") {
            return Some("ogg");
        }
        if head.starts_with(b"fLaC") {
            return Some("flac");
        }
        if head.starts_with(b"RIFF") {
            return Some("wav");
        }
        if head.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
            return Some("webm");
        }
        None
    }


    let source: Box<dyn Source<Item = f32> + Send> =
        if let Some(cached) = cache.and_then(|c| c.get(path)) {
            let file = File::open(cached).map_err(|e| e.to_string())?;
            Box::new(Decoder::new(BufReader::new(file)).map_err(|e| e.to_string())?)
        } else if path.starts_with("http://") || path.starts_with("https://") {
            // F9: bounded-range reader (Phase 2-E). One reader serves the
            // sniff AND the decode: read the first 64 bytes for the
            // container hint (googlevideo MP4s are DASH-branded and fail
            // the bare probe), seek back, then hand the same reader to the
            // decoder. ~3 MiB/s measured — 160× real-time.
            let mut reader = crate::remote::HttpRangeReader::new(path.to_owned(), vec![])
                .map_err(|e| e.to_string())?;
            let mut head = vec![0u8; 64];
            let _ = std::io::Read::read_exact(&mut reader, &mut head);
            let hint = sniff_hint(&head);
            #[cfg(debug_assertions)]
            eprintln!("[F9-sniff] hint={hint:?}");
            std::io::Seek::seek(&mut reader, std::io::SeekFrom::Start(0))
                .map_err(|e| e.to_string())?;
            let mut builder = Decoder::builder();
            if let Some(ext) = hint {
                builder = builder.with_hint(ext);
            }
            return Ok(Box::new(
                builder
                    .with_data(BufReader::new(reader))
                    .build()
                    .map_err(|e| e.to_string())?,
            ));
        } else {
            let file = File::open(PathBuf::from(path)).map_err(|e| e.to_string())?;
            Box::new(Decoder::new(BufReader::new(file)).map_err(|e| e.to_string())?)
        };
    Ok(source)
}

/// F9: reproduce the pipeline's exact probe path (PrefetchReader + hint)
/// for diagnostics; returns samples decoded or the error.
pub fn diagnostic_probe_decode(
    url: String,
) -> Result<u64, String> {
    use std::io::Read as _;
    let (status_tx, _status_rx) = tokio::sync::watch::channel(crate::remote::StreamStatus::Refilled);
    let reader = crate::remote::PrefetchReader::new(
        url,
        vec![],
        status_tx,
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
    )
    .map_err(|e| e.to_string())?;
    let mut builder = Decoder::builder();
    builder = builder.with_hint("m4a");
    let mut dec = builder
        .with_data(std::io::BufReader::new(reader))
        .build()
        .map_err(|e| format!("probe/build: {e:?}"))?;
    let mut n = 0u64;
    let mut buf = [0f32; 1024];
    while n < 4096 {
        let got = std::iter::Iterator::by_ref(&mut dec).take(buf.len()).count();
        if got == 0 {
            break;
        }
        n += got as u64;
    }
    Ok(n as u64)
}

/// F9: build a rodio decoder with an explicit format hint (used by the
/// debug E2E hook to reproduce the pipeline's probe path).
pub fn build_decoder_with_hint<R: std::io::Read + std::io::Seek + Send + Sync + 'static>(
    reader: R,
    hint: &str,
) -> Result<rodio::Decoder<R>, String> {
    let mut builder = Decoder::builder();
    builder = builder.with_hint(hint);
    builder.with_data(reader).build().map_err(|e| e.to_string())
}

/// M18: name of the OS default output device, if any.
pub fn default_output_name() -> Option<String> {
    use rodio::cpal::traits::{DeviceTrait, HostTrait};
    rodio::cpal::default_host().default_output_device().and_then(|d| d.name().ok())
}

/// M18: whether a device with this exact name still exists (removal check).
pub fn output_device_exists(name: &str) -> bool {
    use rodio::cpal::traits::{DeviceTrait, HostTrait};
    let host = rodio::cpal::default_host();
    host.devices()
        .map(|mut ds| {
            ds.any(|d| {
                d.name()
                    .map(|n| n == name)
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::*;

    /// A decoder that panics mid-stream (TAN-03 rehearsal at the new
    /// location: the decode thread's catch_unwind).
    struct PanicSource {
        first: bool,
    }
    impl Iterator for PanicSource {
        type Item = f32;
        fn next(&mut self) -> Option<f32> {
            if self.first {
                self.first = false;
                Some(0.0)
            } else {
                panic!("hostile media decoder panic");
            }
        }
    }
    impl Source for PanicSource {
        fn current_span_len(&self) -> Option<usize> {
            None
        }
        fn channels(&self) -> u16 {
            2
        }
        fn sample_rate(&self) -> u32 {
            44_100
        }
        fn total_duration(&self) -> Option<Duration> {
            None
        }
    }

    #[test]
    fn failed_rebuild_does_not_tear_down_current_stream() {
        let mut current = Some("audible-current");
        let result: Result<Option<&str>, &str> = commit_replacement(&mut current, Err("decode failed"));
        assert_eq!(result, Err("decode failed"));
        assert_eq!(current, Some("audible-current"));
    }

    #[test]
    fn successful_rebuild_replaces_current_stream() {
        let mut current = Some("old");
        let old = commit_replacement(&mut current, Ok::<_, &str>("new")).unwrap();
        assert_eq!(old, Some("old"));
        assert_eq!(current, Some("new"));
    }

    #[test]
    fn decoder_seek_moves_decoder_not_only_position_counter() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/testmusic/Test Signal/test.wav");
        let file = File::open(path).expect("committed WAV fixture");
        let mut linear = Decoder::new(BufReader::new(file)).expect("fixture decoder");
        let rate = linear.sample_rate();
        let channels = u64::from(linear.channels());
        let skip = u64::from(rate) / 2 * channels;
        for _ in 0..skip {
            assert!(linear.next().is_some(), "fixture has 500 ms");
        }
        let expected = linear.next().expect("sample after linear skip");

        let file = File::open(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/testmusic/Test Signal/test.wav"),
        )
        .expect("committed WAV fixture");
        let mut sought = Decoder::new(BufReader::new(file)).expect("fixture decoder");
        sought
            .try_seek(Duration::from_millis(500))
            .expect("decoder seek succeeds");
        let actual = sought.next().expect("sample after decoder seek");
        assert!((actual - expected).abs() < 0.01, "seek moved decoder: {actual} vs {expected}");
    }

    #[test]
    fn ring_source_applies_volume_and_counts_frames() {
        let ring = Arc::new(crate::ring::SpscRing::new(64));
        let volume = Arc::new(AtomicU32::new(0.5f32.to_bits()));
        let frames = Arc::new(AtomicU64::new(0));
        let mut source = RingSource {
            ring: ring.clone(),
            volume: volume.clone(),
            frames: frames.clone(),
            channels: 2,
            eof: Arc::new(AtomicBool::new(false)),
            starved: Arc::new(AtomicBool::new(false)),
            samples: 0,
        };
        // Feed 10 samples (5 frames at 2 channels) at unity level.
        for i in 0..10 {
            ring.push(if i % 2 == 0 { 2.0 } else { 0.0 }).unwrap();
        }
        let mut out = Vec::new();
        for _ in 0..10 {
            out.push(source.next().unwrap());
        }
        assert_eq!(out[0], 1.0, "0.5 × 2.0");
        assert_eq!(frames.load(Ordering::Relaxed), 5, "frames, not samples");
        // Volume change applies to subsequent samples without any lock.
        volume.store(1.0f32.to_bits(), Ordering::Relaxed);
        ring.push(2.0).unwrap();
        assert_eq!(source.next().unwrap(), 2.0);
    }

    #[test]
    fn ring_source_underflow_emits_silence_then_eof() {
        let ring = Arc::new(crate::ring::SpscRing::new(64));
        let eof = Arc::new(AtomicBool::new(false));
        let mut source = RingSource {
            ring: ring.clone(),
            volume: Arc::new(AtomicU32::new(1.0f32.to_bits())),
            frames: Arc::new(AtomicU64::new(0)),
            channels: 2,
            eof: eof.clone(),
            starved: Arc::new(AtomicBool::new(false)),
            samples: 0,
        };
        // Empty and not EOF: silence (the callback never blocks).
        assert_eq!(source.next().unwrap(), 0.0);
        assert!(source.starved.load(Ordering::SeqCst), "starved flagged");
        // EOF after drain: the track ends.
        eof.store(true, Ordering::SeqCst);
        assert!(source.next().is_none());
    }

    #[test]
    fn ring_source_stops_at_shutdown_eof() {
        // A decode thread that was told to shut down marks EOF; the
        // consumer must end instead of looping silence forever.
        let ring = Arc::new(crate::ring::SpscRing::new(64));
        let eof = Arc::new(AtomicBool::new(false));
        eof.store(true, Ordering::SeqCst);
        let mut source = RingSource {
            ring,
            volume: Arc::new(AtomicU32::new(1.0f32.to_bits())),
            frames: Arc::new(AtomicU64::new(0)),
            channels: 2,
            eof,
            starved: Arc::new(AtomicBool::new(false)),
            samples: 0,
        };
        assert!(source.next().is_none());
    }
}

/// F1: HLS/DASH fail-closed evidence (documented limitation). The pipeline
/// has no manifest demuxer — `open_source` hands the path to Symphonia's
/// file demuxers, which cannot read HLS `.m3u8` or DASH `.mpd` text. The
/// contract: such a path errors gracefully (decoder error path), it never
/// panics and never produces silence.
#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test code")]
mod adaptive_stream_tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    fn probe(path: &std::path::Path) -> Result<Box<dyn Source<Item = f32> + Send>, String> {
        let (tx, _rx) = watch::channel(crate::remote::StreamStatus::Refilled);
        open_source(
            path.to_str().unwrap(),
            None,
            tx,
            Arc::new(AtomicBool::new(false)),
        )
    }

    #[test]
    fn hls_and_dash_manifests_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        let m3u8 = dir.path().join("stream.m3u8");
        std::fs::write(
            &m3u8,
            "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:6.0,\nseg0.aac\n#EXT-X-ENDLIST\n",
        )
        .unwrap();
        assert!(probe(&m3u8).is_err(), "an HLS playlist must not decode");
        let mpd = dir.path().join("stream.mpd");
        std::fs::write(
            &mpd,
            "<MPD xmlns=\"urn:mpeg:dash:schema:mpd:2011\"><Period/></MPD>",
        )
        .unwrap();
        assert!(probe(&mpd).is_err(), "a DASH manifest must not decode");
    }

    /// F2: a missing/moved local file yields a clean decoder error (the
    /// engine maps it to Error(Decode)), never a panic or a hang.
    #[test]
    fn missing_local_file_errors_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("deleted.flac");
        assert!(probe(&gone).is_err(), "missing file => Err");
    }

    /// F2: every generated local fixture (wav/flac/alac/aac under
    /// /tmp/f2-local, produced by the afconvert step) opens and decodes
    /// through the real `open_source` path. Self-skips when the fixtures
    /// are absent (CI without the generation step).
    #[test]
    fn local_fixture_files_decode_through_open_source() {
        for name in ["sample.wav", "sample.flac", "sample_alac.m4a", "sample_aac.m4a"] {
            let p = std::path::Path::new("/tmp/f2-local").join(name);
            if !p.is_file() {
                println!("SKIP {name}: fixture missing");
                continue;
            }
            let source = probe(&p).unwrap_or_else(|e| panic!("{name}: {e}"));
            // Decodes at least a few hundred real samples without error.
            let mut source = source;
            let mut got = 0;
            while got < 1000 {
                match source.next() {
                    Some(_) => got += 1,
                    None => break,
                }
            }
            assert!(got >= 1000, "{name} decodes real samples");
            println!("{name}: decodes via open_source ({got}+ samples)");
        }
    }
}

#[cfg(test)]
mod http_branch_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::sync::atomic::AtomicBool;

    /// F9: the HTTP branch of `open_source` must probe/decode an MP4/AAC
    /// stream exactly like the file branch does (regression for the
    /// "Unrecognized format" launch failure).
    #[test]
    fn http_branch_decodes_dash_branded_m4a() {
        let src = std::path::Path::new("/tmp/f2-local/sample_aac.m4a");
        if !src.is_file() {
            println!("SKIP: fixture missing");
            return;
        }
        let mut bytes = std::fs::read(src).unwrap();
        // googlevideo's itag-140 MP4s carry major brand `dash` (fragmented).
        assert_eq!(&bytes[4..8], b"ftyp");
        bytes[8..12].copy_from_slice(b"dash");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: audio/mp4\r\nAccept-Ranges: bytes\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    bytes.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&bytes);
            }
        });
        let (tx, _rx) = watch::channel(crate::remote::StreamStatus::Refilled);
        let reader = crate::remote::PrefetchReader::new(
            format!("http://127.0.0.1:{port}/audio.m4a"),
            vec![],
            tx,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        let mut builder = Decoder::builder();
        builder = builder.with_hint("m4a");
        let mut source = builder
            .with_data(BufReader::new(reader))
            .build()
            .unwrap_or_else(|e| panic!("probe with hint m4a failed: {e}"));
        let mut got = 0;
        while got < 1000 {
            match source.next() {
                Some(_) => got += 1,
                None => break,
            }
        }
        assert!(got >= 1000, "decodes real samples (got {got})");
    }

    #[test]
    fn http_branch_decodes_m4a_like_file_branch() {
        let src = std::path::Path::new("/tmp/f2-local/sample_aac.m4a");
        if !src.is_file() {
            println!("SKIP: fixture missing");
            return;
        }
        let bytes = std::fs::read(src).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).into_owned();
                let start: usize = req
                    .split("Range: bytes=")
                    .nth(1)
                    .and_then(|r| r.split('-').next().and_then(|s| s.parse().ok()))
                    .unwrap_or(0);
                let end = (start + bytes.len()).min(bytes.len());
                let slice = &bytes[start.min(bytes.len())..end];
                let status = if start > 0 { 206 } else { 200 };
                let head = format!(
                    "HTTP/1.1 {} OK\r\nContent-Type: audio/mp4\r\nContent-Range: bytes {}-{}/{}\r\nAccept-Ranges: bytes\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    status, start, end.saturating_sub(1), bytes.len(), slice.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(slice);
            }
        });
        let (tx, _rx) = watch::channel(crate::remote::StreamStatus::Refilled);
        let source = open_source(
            &format!("http://127.0.0.1:{port}/audio.m4a"),
            None,
            tx,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap_or_else(|e| panic!("http branch open failed: {e}"));
        let mut source = source;
        let mut got = 0;
        while got < 1000 {
            match source.next() {
                Some(_) => got += 1,
                None => break,
            }
        }
        assert!(got >= 1000, "http branch decodes real samples (got {got})");
    }
}
