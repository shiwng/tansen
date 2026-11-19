//! The rodio-backed pipeline: the real-media implementation the
//! app-core `PipelineFactory` drives.
//!
//! Architecture: the audio callback only **copies samples from an SPSC
//! ring buffer, applies volume + DSP, and increments `frames_played`** — no
//! locks, no allocations, no I/O. A dedicated decode thread per stream runs
//! Symphonia and fills the ring. The whole stream pipeline (decode thread +
//! ring + consumer source) is **immutable per (track, seek epoch)**: a load
//! or seek builds a fresh one and swaps the consumer into the sink; the old
//! one is shut down. Volume, EQ and loudness are atomics applied at copy
//! time.
//!
//! Gapless and crossfade: `preload` builds the next stream ahead of the
//! transition. For gapless the consumer is appended to the same sink (the
//! sink runs straight into it); for a crossfade it lives on a second sink
//! that starts at the transition while both fade ramps run in the callback.

use crate::cache::StreamCache;
use crate::dsp::{DspParams, EqProcessor, SilenceSkipConfig};
use rodio::{Decoder, OutputStream, OutputStreamBuilder, Sink, Source};
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use tokio::sync::watch;

/// Output bit-depth preference. 16 and 32 are the only supported hardware
/// formats; internal processing is always 32-bit float.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputDepth {
    Bits16,
    Bits32,
}

impl OutputDepth {
    pub fn bits(self) -> u32 {
        match self {
            OutputDepth::Bits16 => 16,
            OutputDepth::Bits32 => 32,
        }
    }
    pub fn from_bits(bits: u32) -> Self {
        if bits == 16 {
            OutputDepth::Bits16
        } else {
            OutputDepth::Bits32
        }
    }
}

/// Shared, atomically-updated pipeline configuration. Written by the shell
/// and engine threads; read by the audio/decode threads.
/// Crossfade gain curves. Linear is the classic straight ramp; equal-power
/// keeps the summed loudness constant through the overlap.
pub const FADE_CURVE_LINEAR: u32 = 0;
pub const FADE_CURVE_EQUAL_POWER: u32 = 1;

pub struct PipelineConfig {
    /// Requested hardware output depth (16 or 32).
    pub output_bits: AtomicU32,
    /// The depth the open stream actually negotiated.
    pub negotiated_bits: AtomicU32,
    /// Crossfade duration in ms; 0 = gapless/no overlap.
    pub crossfade_ms: AtomicU64,
    /// Fade shape for crossfades (`FADE_CURVE_*`).
    pub crossfade_curve: Arc<AtomicU32>,
    /// Preload the next stream into the same sink (0 ms handoff).
    pub gapless: AtomicBool,
    pub skip_silence: AtomicBool,
    pub skip_threshold_db: AtomicU32,
    pub skip_min_ms: AtomicU64,
    /// Human-readable negotiated format, e.g. "i16 44.1 kHz stereo".
    pub negotiated_format: Mutex<Option<String>>,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            output_bits: AtomicU32::new(32),
            negotiated_bits: AtomicU32::new(32),
            crossfade_ms: AtomicU64::new(0),
            crossfade_curve: Arc::new(AtomicU32::new(FADE_CURVE_LINEAR)),
            gapless: AtomicBool::new(false),
            skip_silence: AtomicBool::new(false),
            skip_threshold_db: AtomicU32::new((-50.0f32).to_bits()),
            skip_min_ms: AtomicU64::new(2_000),
            negotiated_format: Mutex::new(None),
        }
    }
}

impl PipelineConfig {
    pub fn silence_config(&self) -> SilenceSkipConfig {
        SilenceSkipConfig {
            enabled: self.skip_silence.load(Ordering::Relaxed),
            threshold_db: f32::from_bits(self.skip_threshold_db.load(Ordering::Relaxed)),
            min_duration_ms: self.skip_min_ms.load(Ordering::Relaxed),
        }
    }

    pub fn crossfade_ms(&self) -> u64 {
        self.crossfade_ms.load(Ordering::Relaxed)
    }

    pub fn crossfade_curve(&self) -> u32 {
        self.crossfade_curve.load(Ordering::Relaxed)
    }

    pub fn gapless(&self) -> bool {
        self.gapless.load(Ordering::Relaxed)
    }
}

/// Bit depth of a concrete CoreAudio/cpal sample format, for the Settings
/// report. Integer and float formats both map to their storage width.
fn sample_format_bits(format: rodio::cpal::SampleFormat) -> u32 {
    use rodio::cpal::SampleFormat;
    match format {
        SampleFormat::I8 | SampleFormat::U8 => 8,
        SampleFormat::I16 | SampleFormat::U16 => 16,
        SampleFormat::I24 => 24,
        SampleFormat::I32 | SampleFormat::U32 | SampleFormat::F32 => 32,
        SampleFormat::I64 | SampleFormat::U64 | SampleFormat::F64 => 64,
        _ => 32,
    }
}

fn sample_format_description(format: rodio::cpal::SampleFormat) -> &'static str {
    use rodio::cpal::SampleFormat;
    match format {
        SampleFormat::I16 | SampleFormat::U16 => "16-bit int",
        SampleFormat::I24 => "24-bit int",
        SampleFormat::I32 | SampleFormat::U32 => "32-bit int",
        SampleFormat::F32 => "32-bit float",
        SampleFormat::F64 => "64-bit float",
        _ => "PCM",
    }
}

/// Opens the output with the requested hardware depth. Falls back through
/// the device's supported formats: a requested 16-bit stream on a device that
/// only accepts float yields a working 32-bit stream (and the negotiated
/// value says so), never silence.
fn open_output_stream(bits: u32) -> Result<(OutputStream, u32, String), rodio::StreamError> {
    use rodio::cpal::traits::{DeviceTrait, HostTrait};
    use rodio::cpal::SampleFormat;

    let host = rodio::cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or(rodio::StreamError::NoDevice)?;
    let device_name = device.name().unwrap_or_else(|_| "Output".into());

    // Build the preference order: exact requested format first, then the
    // other common formats, then the device default.
    let mut order: Vec<SampleFormat> = Vec::new();
    if bits == 16 {
        order.push(SampleFormat::I16);
        order.push(SampleFormat::F32);
        order.push(SampleFormat::I32);
    } else {
        order.push(SampleFormat::F32);
        order.push(SampleFormat::I16);
        order.push(SampleFormat::I32);
    }

    let supported: Vec<_> = device
        .supported_output_configs()
        .map(|c| c.collect())
        .unwrap_or_default();
    for fmt in order {
        if let Some(config) = supported.iter().find(|c| c.sample_format() == fmt) {
            // Prefer 44.1 kHz when the device supports it, otherwise its max.
            let chosen = {
                let min = config.min_sample_rate().0;
                let max = config.max_sample_rate().0;
                if (min..=max).contains(&44_100) {
                    config.with_sample_rate(rodio::cpal::SampleRate(44_100))
                } else {
                    config.with_max_sample_rate()
                }
            };
            let stream = OutputStreamBuilder::from_device(device.clone())
                .ok()
                .map(|b| b.with_supported_config(&chosen).open_stream_or_fallback())
                .and_then(|r| r.ok())
                .or_else(|| {
                    OutputStreamBuilder::from_default_device()
                        .ok()
                        .and_then(|b| b.open_stream_or_fallback().ok())
                });
            if let Some(stream) = stream {
                // `open_stream_or_fallback` may silently negotiate a different
                // format than requested. Report the stream that actually
                // opened, never the one we asked for.
                let actual = stream.config();
                let negotiated = sample_format_bits(actual.sample_format());
                let format = format!(
                    "{} {} · {} Hz · {} ch",
                    sample_format_description(actual.sample_format()),
                    device_name,
                    actual.sample_rate(),
                    actual.channel_count()
                );
                return Ok((stream, negotiated, format));
            }
        }
    }

    // Last resort: the device default. If this fails the caller surfaces
    // ErrorKind::Device; we never pretend playback is possible.
    let stream = OutputStreamBuilder::open_default_stream()?;
    let actual = stream.config();
    let negotiated = sample_format_bits(actual.sample_format());
    let format = format!(
        "{} {} · {} Hz · {} ch · device default",
        sample_format_description(actual.sample_format()),
        device_name,
        actual.sample_rate(),
        actual.channel_count()
    );
    Ok((stream, negotiated, format))
}

/// Public capability report for the Settings UI. Reads the real default
/// device through CoreAudio (cpal) — no guesses.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputCapabilities {
    pub device_name: Option<String>,
    pub channels: Option<u16>,
    pub sample_rate: Option<u32>,
    /// Format labels the device advertises, e.g. "f32", "i16".
    pub formats: Vec<String>,
    pub supports_16_bit: bool,
    pub supports_32_bit: bool,
    /// Why the Atmos path is not available in this build, when it isn't.
    pub atmos_supported: bool,
    pub atmos_reason: String,
}

/// Real device negotiation facts. Atmos is only claimed when a complete
/// supported path exists — this pipeline decodes to PCM and renders stereo,
/// so it is never claimed here.
pub fn output_capabilities() -> OutputCapabilities {
    use rodio::cpal::traits::{DeviceTrait, HostTrait};
    use rodio::cpal::SampleFormat;

    let host = rodio::cpal::default_host();
    let device = host.default_output_device();
    let mut formats = Vec::new();
    let mut channels = None;
    let mut sample_rate = None;
    let mut supports_16 = false;
    let mut supports_32 = false;
    if let Some(device) = &device {
        if let Ok(configs) = device.supported_output_configs() {
            for c in configs {
                if !formats
                    .iter()
                    .any(|f| f == &sample_format_label(c.sample_format()))
                {
                    formats.push(sample_format_label(c.sample_format()).to_string());
                }
                supports_16 |= matches!(c.sample_format(), SampleFormat::I16 | SampleFormat::U16);
                supports_32 |= matches!(c.sample_format(), SampleFormat::F32 | SampleFormat::I32);
                channels = channels.or(Some(c.channels()));
                sample_rate = sample_rate.or(Some(c.max_sample_rate().0));
            }
        }
    }
    let device_name = device.and_then(|d| d.name().ok());
    OutputCapabilities {
        device_name,
        channels,
        sample_rate,
        formats,
        supports_16_bit: supports_16,
        supports_32_bit: supports_32,
        atmos_supported: false,
        atmos_reason: "This build decodes every source to PCM and renders a stereo mix; Dolby Atmos on macOS needs a Dolby-rendered path (Apple Music/AVFoundation) that Tansen does not implement. Atmos-track metadata is shown on sources that provide it, but it is never relabelled or simulated."
            .into(),
    }
}

fn sample_format_label(fmt: rodio::cpal::SampleFormat) -> &'static str {
    use rodio::cpal::SampleFormat::*;
    match fmt {
        I8 => "i8",
        I16 => "i16",
        I24 => "i24",
        I32 => "i32",
        I64 => "i64",
        U8 => "u8",
        U16 => "u16",
        U32 => "u32",
        U64 => "u64",
        F32 => "f32",
        F64 => "f64",
        _ => "unknown",
    }
}

enum Request {
    /// Replace the sink's content with this path and start playing.
    /// The ack answers once the decoder is open (or not).
    Load {
        path: String,
        live: bool,
        ack: mpsc::SyncSender<Result<(), String>>,
    },
    Play,
    Pause,
    /// Rebuild the stream pipeline at this position (new decode
    /// thread + ring, consumer swapped into the sink).
    SeekMs(u64),
    Volume(f32),
    /// Drop all queued audio without touching the pipeline itself.
    Clear,
    /// Drop the output stream; the next load reopens the (possibly
    /// new) default device.
    ReopenOutput,
    /// Build the next stream ahead of the transition.
    Preload {
        path: String,
        ack: mpsc::SyncSender<Result<PreloadMeta, String>>,
    },
    /// Promote the preloaded stream; `crossfade_ms` > 0 starts the overlap
    /// and fade ramps.
    AdoptPreloaded {
        crossfade_ms: u64,
        ack: mpsc::SyncSender<Result<(), String>>,
    },
    /// Drop a preloaded stream (skip/seek/queue change).
    DiscardPreloaded,
    /// Live DSP update (EQ gains, loudness target).
    Dsp,
    Quit,
}

/// Mid-stream fill status. Defined on the reader; re-exported here.
pub use crate::remote::StreamStatus;

/// Handle to the audio thread.
pub struct AudioPipeline {
    tx: Option<mpsc::Sender<Request>>,
    /// Interleaved sample *frames* delivered to the output — incremented by
    /// the audio callback itself.
    frames: Arc<AtomicU64>,
    /// Sample rate of the currently loaded source (for ms conversion).
    sample_rate: Arc<AtomicU64>,
    /// Duration of the current media (0 = unknown).
    duration_ms: Arc<AtomicU64>,
    /// Receiver side of the current stream's fill status (the sender
    /// lives in the fetch task).
    status_rx: watch::Receiver<crate::remote::StreamStatus>,
    pub params: Arc<DspParams>,
    pub config: Arc<PipelineConfig>,
}

/// Metadata of a preloaded stream, returned to the factory so the engine's
/// next state uses the right duration/rate.
#[derive(Debug, Clone, Copy)]
pub struct PreloadMeta {
    pub channels: u16,
    pub sample_rate: u32,
    pub duration_ms: u64,
}

impl AudioPipeline {
    /// Spawns the audio thread. The output stream opens lazily inside the
    /// thread; a missing device surfaces on the first `load` as `Err`, which
    /// the factory maps to `ErrorKind::Device`.
    pub fn spawn(
        cache: Option<Arc<StreamCache>>,
        status: watch::Sender<crate::remote::StreamStatus>,
    ) -> Result<Self, String> {
        Self::spawn_with_config(
            cache,
            status,
            Arc::new(DspParams::default()),
            Arc::new(PipelineConfig::default()),
        )
    }

    pub fn spawn_with_config(
        cache: Option<Arc<StreamCache>>,
        status: watch::Sender<crate::remote::StreamStatus>,
        params: Arc<DspParams>,
        config: Arc<PipelineConfig>,
    ) -> Result<Self, String> {
        let frames = Arc::new(AtomicU64::new(0));
        let sample_rate = Arc::new(AtomicU64::new(44_100));
        let duration_ms = Arc::new(AtomicU64::new(0));
        let (tx, rx) = mpsc::channel::<Request>();
        let status_rx = status.subscribe();
        let frames_thread = frames.clone();
        let sample_rate_thread = sample_rate.clone();
        let duration_thread = duration_ms.clone();
        let params_thread = params.clone();
        let config_thread = config.clone();
        // The CoreAudio output stream is not `Send` on macOS (cpal keeps a
        // property-listener callback), so it is constructed *inside* the
        // audio thread rather than moved into it.
        std::thread::Builder::new()
            .name("pipeline-audio".into())
            .spawn(move || {
                let ctx = AudioThread {
                    rx,
                    cache,
                    status,
                    frames: frames_thread,
                    sample_rate: sample_rate_thread,
                    duration_ms: duration_thread,
                    params: params_thread,
                    config: config_thread,
                    slot: None,
                    sink: None,
                    retired: Vec::new(),
                    retired_streams: Vec::new(),
                    current: None,
                    preloaded: None,
                    current_path: None,
                    current_live: false,
                    last_meta: PreloadMeta {
                        channels: 2,
                        sample_rate: 44_100,
                        duration_ms: 0,
                    },
                };
                ctx.run();
            })
            .map_err(|e| format!("pipeline thread: {e}"))?;
        Ok(Self {
            tx: Some(tx),
            frames,
            sample_rate,
            duration_ms,
            status_rx,
            params,
            config,
        })
    }

    /// Subscribe to the current stream's fill status.
    pub fn status_rx(&self) -> watch::Receiver<crate::remote::StreamStatus> {
        self.status_rx.clone()
    }

    /// Opens `path` as the sink's sole content and starts it. A `live` stream
    /// (internet radio) is read as one unbounded, unseekable body.
    pub fn load(&self, path: &str) -> Result<(), String> {
        self.load_with(path, false)
    }

    pub fn load_live(&self, path: &str) -> Result<(), String> {
        self.load_with(path, true)
    }

    fn load_with(&self, path: &str, live: bool) -> Result<(), String> {
        let (ack_tx, ack_rx) = mpsc::sync_channel(1);
        let tx = self.tx.as_ref().ok_or("pipeline thread gone")?;
        tx.send(Request::Load {
            path: path.to_owned(),
            live,
            ack: ack_tx,
        })
        .map_err(|_| "pipeline thread gone")?;
        ack_rx.recv().map_err(|_| "pipeline thread gone")?
    }

    /// Builds the next stream without swapping it in. Returns its metadata
    /// (sample rate/duration) so the caller can arm the transition.
    pub fn preload(&self, path: &str) -> Result<PreloadMeta, String> {
        let (ack_tx, ack_rx) = mpsc::sync_channel(1);
        let tx = self.tx.as_ref().ok_or("pipeline thread gone")?;
        tx.send(Request::Preload {
            path: path.to_owned(),
            ack: ack_tx,
        })
        .map_err(|_| "pipeline thread gone")?;
        ack_rx.recv().map_err(|_| "pipeline thread gone")?
    }

    /// Promotes the preloaded stream. `crossfade_ms` 0 = gapless handoff.
    pub fn adopt_preloaded(&self, crossfade_ms: u64) -> Result<(), String> {
        let (ack_tx, ack_rx) = mpsc::sync_channel(1);
        let tx = self.tx.as_ref().ok_or("pipeline thread gone")?;
        tx.send(Request::AdoptPreloaded {
            crossfade_ms,
            ack: ack_tx,
        })
        .map_err(|_| "pipeline thread gone")?;
        ack_rx.recv().map_err(|_| "pipeline thread gone")?
    }

    pub fn discard_preloaded(&self) {
        self.fire(Request::DiscardPreloaded);
    }

    pub fn play(&self) {
        self.fire(Request::Play);
    }

    pub fn pause(&self) {
        self.fire(Request::Pause);
    }

    /// Rebuild the stream pipeline at this position — a fresh decode
    /// thread + ring, the consumer swapped into the sink.
    pub fn seek_ms(&self, ms: u64) {
        self.fire(Request::SeekMs(ms));
    }

    pub fn set_volume(&self, v: f32) {
        self.params.set_master_volume(v);
        self.fire(Request::Volume(v));
    }

    /// Applies an EQ configuration; coefficients are recomputed in the audio
    /// callback without locks or allocations.
    pub fn set_eq(&self, enabled: bool, gains: &[f32; 10]) {
        self.params.set_eq(enabled, gains);
        self.fire(Request::Dsp);
    }

    pub fn set_loudness_gain_db(&self, gain_db: f32) {
        self.params.set_loudness_gain_db(gain_db);
        self.fire(Request::Dsp);
    }

    /// Drop all queued audio; the next `load` starts fresh.
    pub fn clear(&self) {
        self.fire(Request::Clear);
    }

    /// Drop the output stream. The next `load` reopens whatever the OS
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

    /// The format the currently open stream negotiated.
    pub fn negotiated_bits(&self) -> u32 {
        self.config.negotiated_bits.load(Ordering::Relaxed)
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

/// One (track, seek-epoch) stream: decode thread + ring + shutdown flag.
struct StreamPipeline {
    shutdown: Arc<AtomicBool>,
    /// Set when a preload is discarded before adoption: the consumer returns
    /// `None` at its turn instead of playing, so dropping a gapless preload
    /// never cuts the current track.
    cancelled: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
    /// Whether this stream's consumer increments the shared frames counter.
    counting: Arc<AtomicBool>,
    /// The stream's fade ramp control (crossfades).
    fade_target: Arc<AtomicU32>,
    fade_step: Arc<AtomicU32>,
    fade_current: Arc<AtomicU32>,
}

impl StreamPipeline {
    fn shutdown(mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        drop(self.handle.take());
    }
}

impl Drop for StreamPipeline {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }
}

/// A preloaded stream waiting for the transition.
struct Preloaded {
    pipe: StreamPipeline,
    meta: PreloadMeta,
    /// Decoder path, promoted to `current_path` on adoption so a later seek
    /// rebuilds the right track.
    path: String,
    /// Separate sink for a crossfade; `None` for a gapless append.
    sink: Option<Sink>,
}

struct AudioThread {
    rx: mpsc::Receiver<Request>,
    cache: Option<Arc<StreamCache>>,
    status: watch::Sender<crate::remote::StreamStatus>,
    frames: Arc<AtomicU64>,
    sample_rate: Arc<AtomicU64>,
    duration_ms: Arc<AtomicU64>,
    params: Arc<DspParams>,
    config: Arc<PipelineConfig>,
    slot: Option<OutputStream>,
    sink: Option<Sink>,
    /// Sinks kept alive while their stream fades out.
    retired: Vec<Sink>,
    /// Decode threads that must outlive the pipeline swap so a crossfading
    /// stream keeps its buffer fed until the fade completes.
    retired_streams: Vec<StreamPipeline>,
    current: Option<StreamPipeline>,
    preloaded: Option<Preloaded>,
    /// Path of the current stream, kept so a seek (or a crossfade follow-up)
    /// can rebuild the decoder without the engine re-sending it.
    current_path: Option<String>,
    /// The current stream is a live broadcast: a seek cannot be honoured and
    /// is ignored instead of rebuilding a non-seekable body.
    current_live: bool,
    /// Metadata of the most recently built stream (used by `build_preload`).
    last_meta: PreloadMeta,
}

impl AudioThread {
    fn ensure_open(&mut self) {
        if self.slot.is_some() {
            return;
        }
        let bits = self.config.output_bits.load(Ordering::Relaxed);
        match open_output_stream(bits) {
            Ok((stream, negotiated, label)) => {
                self.config
                    .negotiated_bits
                    .store(negotiated, Ordering::Relaxed);
                if let Ok(mut f) = self.config.negotiated_format.lock() {
                    *f = Some(label);
                }
                let s = Sink::connect_new(stream.mixer());
                s.pause();
                self.slot = Some(stream);
                self.sink = Some(s);
            }
            Err(err) => {
                tracing::warn!(error = %err, "audio output open failed");
            }
        }
    }

    /// Recreates the primary sink if a panicking build dropped it. The output
    /// stream is still open, so this only loses queued audio.
    fn restore_sink(&mut self) {
        if self.sink.is_none() {
            if let Some(slot) = self.slot.as_ref() {
                let sink = Sink::connect_new(slot.mixer());
                sink.pause();
                self.sink = Some(sink);
            }
        }
    }

    fn drop_output(&mut self) {
        self.discard_preloaded();
        if let Some(old) = self.current.take() {
            old.shutdown();
        }
        self.retired.clear();
        for stream in self.retired_streams.drain(..) {
            stream.shutdown();
        }
        self.sink = None;
        self.slot = None;
    }

    fn discard_preloaded(&mut self) {
        if let Some(pre) = self.preloaded.take() {
            pre.pipe.cancelled.store(true, Ordering::SeqCst);
            if let Some(sink) = pre.sink {
                // Crossfade preloads live on their own sink: clear it so the
                // prepared track never leaks into the mix.
                sink.clear();
            }
            pre.pipe.shutdown();
        }
    }

    fn run(mut self) {
        while let Ok(req) = self.rx.recv() {
            match req {
                Request::Load { path, live, ack } => {
                    self.ensure_open();
                    let answer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        // A new load always supersedes any preloaded stream.
                        self.discard_preloaded();
                        let sink = self
                            .sink
                            .take()
                            .ok_or_else(|| "no audio output device".to_string())?;
                        let result = self.build_and_swap(&path, None, &sink, false, live);
                        self.sink = Some(sink);
                        result
                    }))
                    .unwrap_or_else(|_| Err("decoder panicked while opening media".to_string()));
                    self.restore_sink();
                    let _ = ack.send(answer);
                }
                Request::Preload { path, ack } => {
                    self.ensure_open();
                    let answer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let crossfade = self.config.crossfade_ms() > 0;
                        let sink = self
                            .sink
                            .take()
                            .ok_or_else(|| "no audio output device".to_string())?;
                        let result = self.build_preload(&path, &sink, crossfade);
                        self.sink = Some(sink);
                        result
                    }))
                    .unwrap_or_else(|_| Err("decoder panicked while preloading media".to_string()));
                    self.restore_sink();
                    let _ = ack.send(answer);
                }
                Request::AdoptPreloaded { crossfade_ms, ack } => {
                    let answer = self.adopt(crossfade_ms);
                    let _ = ack.send(answer);
                }
                Request::DiscardPreloaded => self.discard_preloaded(),
                Request::Play => {
                    if let Some(s) = &self.sink {
                        s.play();
                    }
                    for s in &self.retired {
                        s.play();
                    }
                }
                Request::Pause => {
                    if let Some(s) = &self.sink {
                        s.pause();
                    }
                    for s in &self.retired {
                        s.pause();
                    }
                }
                Request::SeekMs(ms) => {
                    self.discard_preloaded();
                    // A live stream has no timeline: seeking is meaningless.
                    if self.current_live {
                        continue;
                    }
                    if let Some(path) = self.current_path() {
                        if let Some(sink) = self.sink.take() {
                            let _ = self.build_and_swap(&path, Some(ms), &sink, false, false);
                            self.sink = Some(sink);
                        }
                    }
                }
                Request::Volume(v) => {
                    self.params.set_master_volume(v);
                }
                Request::Dsp => {}
                Request::Clear => {
                    self.discard_preloaded();
                    if let Some(s) = &self.sink {
                        s.clear();
                    }
                    self.retired.clear();
                    for stream in self.retired_streams.drain(..) {
                        stream.shutdown();
                    }
                    if let Some(old) = self.current.take() {
                        old.shutdown();
                    }
                }
                Request::ReopenOutput => {
                    self.drop_output();
                }
                Request::Quit => {
                    self.discard_preloaded();
                    if let Some(old) = self.current.take() {
                        old.shutdown();
                    }
                    break;
                }
            }
        }
    }

    fn current_path(&self) -> Option<String> {
        self.current_path.clone()
    }

    /// Builds a stream and swaps it into `sink` as the current one.
    fn build_and_swap(
        &mut self,
        path: &str,
        resume_ms: Option<u64>,
        sink: &Sink,
        start_paused: bool,
        live: bool,
    ) -> Result<(), String> {
        // Build the replacement first; a failure leaves the current stream
        // audible (never a dead ring).
        let counting = Arc::new(AtomicBool::new(true));
        let built = self.build_stream(path, resume_ms, sink, counting, false, live)?;
        if let Some(old) = self.current.replace(built) {
            old.shutdown();
        }
        self.current_path = Some(path.to_owned());
        self.current_live = live;
        sink.play();
        if start_paused {
            sink.pause();
        }
        Ok(())
    }

    /// Builds the replacement stream into `sink`, not yet swapping state.
    fn build_stream(
        &mut self,
        path: &str,
        resume_ms: Option<u64>,
        sink: &Sink,
        counting: Arc<AtomicBool>,
        append: bool,
        live: bool,
    ) -> Result<StreamPipeline, String> {
        self.prepare_build_with_fade(path, resume_ms, sink, counting, append, false, true, live)
    }

    fn build_preload(
        &mut self,
        path: &str,
        sink: &Sink,
        crossfade: bool,
    ) -> Result<PreloadMeta, String> {
        self.discard_preloaded();
        let counting = Arc::new(AtomicBool::new(false));
        if crossfade {
            // A second sink plays concurrently at the transition.
            let stream = self.slot.as_ref().ok_or("no audio output device")?;
            let next_sink = Sink::connect_new(stream.mixer());
            next_sink.pause();
            let pipe = self.prepare_build_with_fade(
                path, None, &next_sink, counting, false, true, false, false,
            )?;
            let meta = self.last_meta;
            self.preloaded = Some(Preloaded {
                pipe,
                meta,
                path: path.to_owned(),
                sink: Some(next_sink),
            });
        } else {
            // Same sink: the consumer is appended and starts when the current
            // source ends — a true gapless transition.
            let pipe = self
                .prepare_build_with_fade(path, None, sink, counting, true, false, false, false)?;
            let meta = self.last_meta;
            self.preloaded = Some(Preloaded {
                pipe,
                meta,
                path: path.to_owned(),
                sink: None,
            });
        }
        Ok(self
            .preloaded
            .as_ref()
            .map(|p| p.meta)
            .unwrap_or(PreloadMeta {
                channels: 2,
                sample_rate: 44_100,
                duration_ms: 0,
            }))
    }

    fn adopt(&mut self, crossfade_ms: u64) -> Result<(), String> {
        let Some(pre) = self.preloaded.take() else {
            return Err("no preloaded stream".into());
        };
        if pre.pipe.cancelled.load(Ordering::SeqCst) {
            return Err("preloaded stream was discarded".into());
        }
        let frames = self.frames.clone();
        // The old stream stops counting frames; the new one starts at zero.
        if let Some(old) = &self.current {
            old.counting.store(false, Ordering::SeqCst);
            if crossfade_ms > 0 {
                let rate = self.sample_rate.load(Ordering::Relaxed).max(1);
                let frames = crossfade_ms.saturating_mul(rate) / 1000;
                old.fade_target.store(0f32.to_bits(), Ordering::SeqCst);
                old.fade_step
                    .store((1.0 / frames.max(1) as f32).to_bits(), Ordering::SeqCst);
            } else {
                // Gapless append: no overlap; the old source has ended.
                old.fade_target.store(1f32.to_bits(), Ordering::SeqCst);
            }
        }
        frames.store(0, Ordering::SeqCst);
        self.current_path = Some(pre.path);
        self.sample_rate
            .store(u64::from(pre.meta.sample_rate.max(1)), Ordering::SeqCst);
        self.duration_ms
            .store(pre.meta.duration_ms, Ordering::SeqCst);
        pre.pipe.counting.store(true, Ordering::SeqCst);
        pre.pipe
            .fade_current
            .store(0f32.to_bits(), Ordering::SeqCst);
        pre.pipe.fade_target.store(1f32.to_bits(), Ordering::SeqCst);
        let rate = f64::from(pre.meta.sample_rate.max(1));
        let fade_frames = (f64::from(crossfade_ms as u32) * rate / 1000.0).max(1.0) as f32;
        pre.pipe
            .fade_step
            .store((1.0 / fade_frames).to_bits(), Ordering::SeqCst);

        if let Some(new_sink) = pre.sink {
            new_sink.play();
            if let Some(old_sink) = self.sink.replace(new_sink) {
                self.retired.push(old_sink);
            }
        }
        // Retire finished fades before adding another.
        self.retired_streams.retain(|stream| {
            let done = stream.fade_target.load(Ordering::Relaxed) == 0f32.to_bits()
                && stream.fade_current.load(Ordering::Relaxed) == 0f32.to_bits();
            if done {
                false
            } else {
                true
            }
        });
        if let Some(old) = self.current.replace(pre.pipe) {
            if crossfade_ms == 0 {
                old.shutdown();
            } else {
                // Keep feeding the fading stream until its ramp reaches zero.
                self.retired_streams.push(old);
            }
        }
        Ok(())
    }

    /// Shared build path. `append` = push the consumer after the current
    /// sink content (gapless preload); otherwise clear the sink first.
    /// `publish` = update the shared rate/duration/position atomics (true for
    /// a real load or seek, false for a preload that has not been adopted).
    #[allow(clippy::too_many_arguments)]
    fn prepare_build_with_fade(
        &mut self,
        path: &str,
        resume_ms: Option<u64>,
        sink: &Sink,
        counting: Arc<AtomicBool>,
        append: bool,
        start_silent: bool,
        publish: bool,
        live: bool,
    ) -> Result<StreamPipeline, String> {
        let silence = self.config.silence_config();
        let status = self.status.clone();
        let cache = self.cache.clone();
        let params = self.params.clone();
        let frames = self.frames.clone();
        let fade_curve = self.config.crossfade_curve.clone();
        let mut built = build_ring_stream(
            path,
            resume_ms,
            sink,
            cache.as_ref(),
            status,
            params,
            frames,
            fade_curve,
            counting,
            silence,
            append,
            start_silent,
            live,
        )?;
        self.last_meta = built.meta;
        // Update the shared atomics only for the current load, not a preload:
        // the caller decides. `adopt` / `build_and_swap` set them.
        if publish {
            self.sample_rate
                .store(u64::from(built.meta.sample_rate.max(1)), Ordering::Relaxed);
            self.duration_ms
                .store(built.meta.duration_ms, Ordering::Relaxed);
            let seed = resume_ms
                .map(|ms| ms.saturating_mul(u64::from(built.meta.sample_rate.max(1))) / 1000)
                .unwrap_or(0);
            self.frames.store(seed, Ordering::Relaxed);
        }
        Ok(StreamPipeline {
            shutdown: built
                .shutdown
                .take()
                .unwrap_or_else(|| Arc::new(AtomicBool::new(false))),
            cancelled: built.cancelled,
            handle: built.handle.take(),
            counting: built.counting,
            fade_target: built.fade_target,
            fade_step: built.fade_step,
            fade_current: built.fade_current,
        })
    }
}

/// Built-but-not-yet-adopted pieces of one stream.
struct BuiltStream {
    shutdown: Option<Arc<AtomicBool>>,
    cancelled: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
    counting: Arc<AtomicBool>,
    fade_target: Arc<AtomicU32>,
    fade_step: Arc<AtomicU32>,
    fade_current: Arc<AtomicU32>,
    meta: PreloadMeta,
}

/// Metadata published by the decode worker only after the source has opened
/// and, for a seek, the decoder has reached the requested position.
struct DecodeReady {
    channels: u16,
    sample_rate: u32,
    duration_ms: u64,
}

#[allow(clippy::too_many_arguments)]
fn build_ring_stream(
    path: &str,
    resume_ms: Option<u64>,
    sink: &Sink,
    cache: Option<&Arc<StreamCache>>,
    status: watch::Sender<crate::remote::StreamStatus>,
    params: Arc<DspParams>,
    frames: Arc<AtomicU64>,
    fade_curve: Arc<AtomicU32>,
    counting: Arc<AtomicBool>,
    silence: SilenceSkipConfig,
    append: bool,
    start_silent: bool,
    live: bool,
) -> Result<BuiltStream, String> {
    let _ = status.send(crate::remote::StreamStatus::Refilled);

    let shutdown = Arc::new(AtomicBool::new(false));
    let cancelled = Arc::new(AtomicBool::new(false));
    let ring = Arc::new(crate::ring::SpscRing::new(1 << 21));
    let eof = Arc::new(AtomicBool::new(false));
    let starved = Arc::new(AtomicBool::new(false));

    let decode_shutdown = shutdown.clone();
    let eof_t = eof.clone();
    let starved_t = starved.clone();
    let ring_t = ring.clone();
    let status_t = status.clone();
    let counting_t = counting.clone();
    let frames_t = frames.clone();
    let path_owned = path.to_owned();
    let cache_t = cache.cloned();
    let seek_ms = resume_ms;
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let handle = std::thread::Builder::new()
        .name("stream-decode".into())
        .spawn(move || {
            let opened = open_source(
                &path_owned,
                cache_t.as_ref(),
                status_t.clone(),
                decode_shutdown.clone(),
                live,
            )
            .and_then(|mut source| {
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
            // Silence skipping state (decode thread only — never the audio
            // callback). The first `min_duration` of silence is still heard;
            // after that the decoder fast-forwards, counting the skipped
            // frames so the seek position stays truthful.
            let rate = u64::from(source.sample_rate().max(1));
            let channels = u64::from(source.channels().max(1));
            let min_silent_frames = silence.min_duration_ms.saturating_mul(rate) / 1000;
            let threshold = silence.threshold();
            let mut silent_frames = 0u64;
            let mut skipping = false;
            let mut skipped_frames = 0u64;
            let mut frame_peak = 0f32;
            let mut frame_channel = 0u64;
            loop {
                if decode_shutdown.load(Ordering::Relaxed) {
                    return;
                }
                let sample =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| source.next()));
                match sample {
                    Ok(Some(sample)) => {
                        if silence.enabled && channels > 0 {
                            frame_peak = frame_peak.max(sample.abs());
                            frame_channel += 1;
                            if frame_channel >= channels {
                                let is_silent = frame_peak < threshold;
                                frame_peak = 0.0;
                                frame_channel = 0;
                                if skipping {
                                    if is_silent {
                                        skipped_frames += 1;
                                        continue;
                                    }
                                    // Sound returned: publish the skipped
                                    // span into the position counter.
                                    if counting_t.load(Ordering::Relaxed) && skipped_frames > 0 {
                                        frames_t.fetch_add(skipped_frames, Ordering::Relaxed);
                                    }
                                    skipping = false;
                                    silent_frames = 0;
                                } else if is_silent {
                                    silent_frames += 1;
                                    if min_silent_frames > 0 && silent_frames >= min_silent_frames {
                                        skipping = true;
                                        skipped_frames = 0;
                                    }
                                } else {
                                    silent_frames = 0;
                                }
                            }
                        }
                        loop {
                            if ring_t.push(sample).is_ok() {
                                break;
                            }
                            if decode_shutdown.load(Ordering::Relaxed) {
                                return;
                            }
                            std::thread::sleep(Duration::from_micros(500));
                        }
                        if starved_t.swap(false, Ordering::SeqCst) {
                            let _ = status_t.send(crate::remote::StreamStatus::Refilled);
                        }
                    }
                    Ok(None) => {
                        if skipping && counting_t.load(Ordering::Relaxed) && skipped_frames > 0 {
                            frames_t.fetch_add(skipped_frames, Ordering::Relaxed);
                        }
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

    let ready = ready_rx
        .recv()
        .map_err(|_| "decode worker stopped before opening media".to_string())??;
    let meta = PreloadMeta {
        channels: ready.channels,
        sample_rate: ready.sample_rate,
        duration_ms: ready.duration_ms,
    };

    let fade_target = Arc::new(AtomicU32::new(
        if start_silent { 0f32 } else { 1f32 }.to_bits(),
    ));
    let fade_step = Arc::new(AtomicU32::new(1.0f32.to_bits()));
    let fade_current = Arc::new(AtomicU32::new(
        if start_silent { 0f32 } else { 1f32 }.to_bits(),
    ));
    let eq = EqProcessor::new(meta.channels, meta.sample_rate as f32);
    // The audio-callback-side source: ring pops, volume, EQ, fade and frame
    // counting only.
    let consumer = RingSource {
        ring,
        params,
        frames,
        counting: counting.clone(),
        channels: meta.channels,
        sample_rate: meta.sample_rate,
        eof,
        starved,
        cancelled: cancelled.clone(),
        samples: 0,
        eq,
        fade_current: fade_current.clone(),
        fade_target: fade_target.clone(),
        fade_step: fade_step.clone(),
        fade_curve,
        loudness: 1.0,
        loudness_initialized: false,
    };
    if append {
        // Gapless: queue after the current source; the sink's own queue
        // makes the handoff sample-contiguous.
        sink.append(consumer);
    } else {
        sink.clear();
        sink.append(consumer);
    }
    if start_silent {
        // A preloaded sink is paused until adoption.
    } else {
        sink.play();
    }
    Ok(BuiltStream {
        shutdown: Some(shutdown),
        cancelled,
        handle: Some(handle),
        counting,
        fade_target,
        fade_step,
        fade_current,
        meta,
    })
}

/// The audio-callback-side source. `next()` pops one sample from the
/// SPSC ring (lock-free atomics), applies loudness, EQ, fade and volume, and
/// counts frames. On underflow it emits silence and flags Starved (the
/// decode thread's push path reports Refilled); after EOF it ends the track.
/// No locks, no allocation, no I/O.
struct RingSource {
    ring: Arc<crate::ring::SpscRing>,
    params: Arc<DspParams>,
    frames: Arc<AtomicU64>,
    counting: Arc<AtomicBool>,
    channels: u16,
    sample_rate: u32,
    eof: Arc<AtomicBool>,
    starved: Arc<AtomicBool>,
    /// A discarded preload ends silently at its turn.
    cancelled: Arc<AtomicBool>,
    samples: u64,
    eq: EqProcessor,
    fade_current: Arc<AtomicU32>,
    fade_target: Arc<AtomicU32>,
    fade_step: Arc<AtomicU32>,
    fade_curve: Arc<AtomicU32>,
    loudness: f32,
    loudness_initialized: bool,
}

impl Iterator for RingSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.cancelled.load(Ordering::Relaxed) {
            return None;
        }
        match self.ring.pop() {
            Some(sample) => {
                let channel = (self.samples % u64::from(self.channels.max(1))) as usize;
                let counting = self.counting.load(Ordering::Relaxed);
                if counting && channel == 0 {
                    self.frames.fetch_add(1, Ordering::Relaxed);
                }
                self.samples = self.samples.wrapping_add(1);

                // Loudness gain (smooth; no jumps between tracks or when a
                // background analysis lands mid-play).
                let target_db = self.params.loudness_gain_db();
                if !self.loudness_initialized {
                    self.loudness = crate::dsp::db_to_linear(target_db);
                    self.loudness_initialized = true;
                } else {
                    let target = crate::dsp::db_to_linear(target_db);
                    // ~40 ms time constant at 44.1 kHz.
                    self.loudness += (target - self.loudness) * 0.00055;
                }
                let mut out = sample * self.loudness;

                // EQ: channel bank, coefficients refreshed every 32 frames.
                if channel == 0 {
                    self.eq.tick(&self.params);
                }
                out = self.eq.process(out, channel, &self.params);

                // Crossfade / fade-out ramp.
                let target = f32::from_bits(self.fade_target.load(Ordering::Relaxed));
                let step = f32::from_bits(self.fade_step.load(Ordering::Relaxed)).max(0.0);
                let mut current = f32::from_bits(self.fade_current.load(Ordering::Relaxed));
                if current < target {
                    current = (current + step).min(target);
                    self.fade_current
                        .store(current.to_bits(), Ordering::Relaxed);
                } else if current > target {
                    current = (current - step).max(target);
                    self.fade_current
                        .store(current.to_bits(), Ordering::Relaxed);
                }
                let curve_gain =
                    if self.fade_curve.load(Ordering::Relaxed) == FADE_CURVE_EQUAL_POWER {
                        // sin() and cos() are complementary: the outgoing stream
                        // (current 1→0) and incoming stream (0→1) sum to roughly
                        // constant power through the overlap.
                        (current.clamp(0.0, 1.0) * std::f32::consts::FRAC_PI_2).sin()
                    } else {
                        current
                    };
                out *= curve_gain * self.params.master_volume();

                // Never wrap: a hot normalized track clips symmetrically
                // instead of folding over.
                if out > 1.0 {
                    out = 1.0;
                } else if out < -1.0 {
                    out = -1.0;
                }

                #[cfg(debug_assertions)]
                crate::metrics::audio::metrics()
                    .peak_bits
                    .fetch_max(out.abs().to_bits(), Ordering::Relaxed);
                Some(out)
            }
            None => {
                if self.eof.load(Ordering::SeqCst) {
                    return None; // decode done + ring drained: track over
                }
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
        // The mixer resamples using this; reporting the real rate keeps
        // non-44.1 kHz sources in tune.
        self.sample_rate.max(1)
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
    _status: watch::Sender<crate::remote::StreamStatus>,
    _shutdown: Arc<AtomicBool>,
    live: bool,
) -> Result<Box<dyn Source<Item = f32> + Send>, String> {
    fn sniff_hint(head: &[u8]) -> Option<&'static str> {
        if head.len() >= 8 && &head[4..8] == b"ftyp" {
            return Some("m4a");
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

    let source: Box<dyn Source<Item = f32> + Send> = if let Some(cached) =
        cache.and_then(|c| c.get(path))
    {
        let file = File::open(cached).map_err(|e| e.to_string())?;
        Box::new(Decoder::new(BufReader::new(file)).map_err(|e| e.to_string())?)
    } else if live {
        // A broadcast: one unbounded body, no ranges, no seeking.
        let (reader, hint) = crate::remote::LiveReader::open(path)?;
        let mut builder = Decoder::builder();
        if let Some(hint) = hint {
            builder = builder.with_hint(hint);
        }
        return Ok(Box::new(
            builder
                .with_data(BufReader::new(reader))
                .build()
                .map_err(|e| format!("unsupported station stream: {e}"))?,
        ));
    } else if path.starts_with("http://") || path.starts_with("https://") {
        let mut reader = crate::remote::HttpRangeReader::new(path.to_owned(), vec![])
            .map_err(|e| e.to_string())?;
        let mut head = vec![0u8; 64];
        let _ = std::io::Read::read_exact(&mut reader, &mut head);
        let hint = sniff_hint(&head);
        std::io::Seek::seek(&mut reader, std::io::SeekFrom::Start(0)).map_err(|e| e.to_string())?;
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

/// Name of the OS default output device, if any.
pub fn default_output_name() -> Option<String> {
    use rodio::cpal::traits::{DeviceTrait, HostTrait};
    rodio::cpal::default_host()
        .default_output_device()
        .and_then(|d| d.name().ok())
}

/// Whether a device with this exact name still exists (removal check).
pub fn output_device_exists(name: &str) -> bool {
    use rodio::cpal::traits::{DeviceTrait, HostTrait};
    let host = rodio::cpal::default_host();
    host.devices()
        .map(|mut ds| ds.any(|d| d.name().map(|n| n == name).unwrap_or(false)))
        .unwrap_or(false)
}
