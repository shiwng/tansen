//! Audio engine: owns the output stream and the rodio sink, applies queue
//! effects, and drives gapless track advance.
//!
//! The engine runs on a dedicated OS thread because `rodio::OutputStream` is
//! not `Send` and dropping it stops audio. Commands arrive over an mpsc
//! channel; state changes go out on a tokio broadcast channel that the app
//! layer forwards to the frontend.
//!
//! Device handling follows the Spotifast pattern (reimplemented, not copied):
//! the engine starts even when no output device is available, retries the
//! device on demand, and reports honest errors instead of dying. While no
//! device is open the engine is idle-cheap and never burns through the queue.

use crate::cache::StreamCache;
use crate::queue::{Queue, QueueOp, QueueSnapshot};
use crate::remote::HttpRangeReader;
use persistence::models::TrackRef;
use rodio::{Decoder, OutputStream, OutputStreamBuilder, Sink, Source};
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

/// Opens the default output. Debug builds route cpal's error callback into
/// the M4 reliability counters; release uses rodio's default behavior.
pub(crate) fn open_output_stream() -> Result<OutputStream, rodio::StreamError> {
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

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EngineEvent {
    TrackChanged { track: Option<TrackRef> },
    Position { sec: f64, duration: f64, playing: bool },
    QueueChanged { #[serde(flatten)] snapshot: QueueSnapshot },
    Volume { level: f64 },
    /// Playback stopped because the queue ran out.
    Finished,
    /// A track failed to open or decode; the queue moved past it.
    TrackError { track: TrackRef, message: String },
}

#[derive(Debug)]
pub enum Command {
    Apply(QueueOp),
    TogglePlay,
    Pause,
    Resume,
    Stop,
    Seek(f64),
    SetVolume(f64),
    /// Replaces a queued track's source path — how remote tracks get their
    /// real stream URL after the resolve-ahead task fetches one.
    PatchUrl { index: usize, url: String },
    /// Try to open the audio output again after it was unavailable.
    RetryOutput,
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("audio output: {0}")]
    Output(String),
}

/// How close to the end the next track is decoded and appended, for gapless
/// advance. rodio mixes queued sources without a seam, so the earlier this
/// lands the safer the transition.
const PRELOAD_AHEAD_SEC: f64 = 2.0;

struct Worker {
    queue: Queue,
    /// Handle to the open output. The `OutputStream` itself lives beside this
    /// in `STREAM_SLOT` on the audio thread — it is not `Send`, so it cannot
    /// be part of `Worker` (the worker is created off-thread and moved in).
    output: Option<Sink>,
    cache: Option<Arc<StreamCache>>,
    events: tokio::sync::broadcast::Sender<EngineEvent>,
    volume: f64,
    /// Queue index appended ahead of time (gapless), `SOME_SENTINEL` after a
    /// failed append so we do not retry every tick.
    preloaded: Option<usize>,
    /// Position at the previous tick — a *drop* in sink position is the only
    /// reliable signal that a preloaded track became current.
    last_pos: f64,
    /// Set after manual loads so the empty-sink check does not read them as
    /// "track finished".
    manual_until: Instant,
    playing: bool,
    duration_hint: f64,
    /// A queue row selected for playback whose stream URL is still a
    /// `sidecar://` placeholder. The resolve-ahead service patches it; the
    /// tick notices and loads. Bounded by `PLACEHOLDER_WAIT`.
    pending: Option<PendingLoad>,
}

#[derive(Debug, Clone, Copy)]
struct PendingLoad {
    index: usize,
    play: bool,
    since: Instant,
}

/// How long to hold a transition while the resolve-ahead fetches a stream
/// URL. Past this, the row errors and the queue advances (a dead sidecar must
/// not wedge the player).
const PLACEHOLDER_WAIT: Duration = Duration::from_secs(10);

/// Tick cadence: fast while audible, slow otherwise. The old fixed 100 ms
/// loop woke ten times a second even with nothing loaded.
const TICK_ACTIVE_MS: u64 = 100;
const TICK_IDLE_MS: u64 = 500;

impl Worker {
    fn is_placeholder(path: &str) -> bool {
        path.starts_with("sidecar://")
    }

    fn sink(&self) -> Option<&Sink> {
        self.output.as_ref()
    }

    /// Opens the default output if not already open. False when the device
    /// still is not available. `stream_slot` keeps the `OutputStream` alive;
    /// it lives on the audio thread because `OutputStream` is not `Send`.
    fn ensure_output(&mut self, stream_slot: &mut Option<OutputStream>) -> bool {
        if self.output.is_some() {
            return true;
        }
        match open_output_stream() {
            Ok(stream) => {
                let sink = Sink::connect_new(stream.mixer());
                sink.pause();
                *stream_slot = Some(stream);
                self.output = Some(sink);
                tracing::info!("audio output opened");
                true
            }
            Err(e) => {
                tracing::warn!(error = %e, "no audio output device");
                false
            }
        }
    }
}

impl Worker {
    fn emit(&self, ev: EngineEvent) {
        let _ = self.events.send(ev);
    }

    fn emit_queue(&self) {
        let audible = self.playing && self.sink().map(|s| !s.is_paused()).unwrap_or(false);
        let snap = self.queue.snapshot(audible);
        self.emit(EngineEvent::QueueChanged { snapshot: snap });
    }

    fn emit_position(&mut self) {
        let Some(sink) = self.sink() else { return };
        let pos = sink.get_pos().as_secs_f64();
        let playing = self.playing && !sink.is_paused();
        self.emit(EngineEvent::Position {
            sec: pos,
            duration: self.duration_hint,
            playing,
        });
    }

    fn clear_sink(&mut self) {
        if let Some(sink) = self.sink() {
            sink.clear();
        }
        self.preloaded = None;
        self.last_pos = 0.0;
        self.manual_until = Instant::now() + Duration::from_millis(250);
    }

    /// Opens and appends `track`'s decoder to the sink without clearing.
    /// Cached remote tracks play straight from disk; other http(s) paths
    /// stream through the range reader; `sidecar://` placeholders that were
    /// never patched (a manual next before the resolve-ahead finished) error
    /// and skip.
    fn append(&mut self, track: &TrackRef) -> Result<(), String> {
        let sink = self
            .output
            .as_ref()
            .ok_or_else(|| "no audio output device available".to_string())?;
        // Local and remote decoders are different concrete types; the sink
        // takes any Source, so each branch erases to the same trait object.
        let source: Box<dyn Source<Item = f32> + Send> =
            if let Some(path) = self
                .cache
                .as_ref()
                .and_then(|c| c.get(&track.path))
            {
                let file = File::open(&path).map_err(|e| e.to_string())?;
                Box::new(Decoder::new(BufReader::new(file)).map_err(|e| e.to_string())?)
            } else if track.path.starts_with("http://") || track.path.starts_with("https://") {
                let reader =
                    HttpRangeReader::new(track.path.clone(), vec![]).map_err(|e| e.to_string())?;
                Box::new(Decoder::new(BufReader::new(reader)).map_err(|e| e.to_string())?)
            } else if track.path.starts_with("sidecar://") {
                return Err("stream not resolved yet".into());
            } else {
                let path: PathBuf = track.path.clone().into();
                let file = File::open(&path).map_err(|e| e.to_string())?;
                Box::new(Decoder::new(BufReader::new(file)).map_err(|e| e.to_string())?)
            };
        if let Some(d) = source.total_duration() {
            self.duration_hint = d.as_secs_f64();
        } else if track.duration_sec > 0.0 {
            self.duration_hint = track.duration_sec;
        }
        // M4: debug builds time every decode read and honor the stall env
        // hook; release passes the decoder through untouched.
        sink.append(crate::metrics::audio::wrap_source(source));
        Ok(())
    }

    fn load(&mut self, track: &TrackRef, play: bool) {
        self.clear_sink();
        if let Err(e) = self.append(track) {
            tracing::warn!(track = %track.path, error = %e, "failed to open track");
            self.emit(EngineEvent::TrackError { track: track.clone(), message: e.clone() });
            // Degraded engine (no output device): stay on the row so the queue
            // is not burned error-by-error; RetryOutput re-opens the device.
            if self.output.is_none() {
                return;
            }
            self.advance_past_current();
            return;
        }
        if play {
            if let Some(sink) = self.sink() {
                sink.play();
            }
        } else if let Some(sink) = self.sink() {
            sink.pause();
        }
        self.playing = play;
        self.emit(EngineEvent::TrackChanged { track: Some(track.clone()) });
        self.emit_queue();
        self.emit_position();
    }

    /// Current track is unusable: move on without counting it as Next.
    fn advance_past_current(&mut self) {
        self.clear_sink();
        let effect = self.queue.apply(QueueOp::Next);
        match effect.load {
            Some((t, _)) if !Self::is_placeholder(&t.path) => self.load(&t, true),
            Some(_) => {
                // Placeholder next: hold a pending load (silence now, sound
                // when the patch lands) instead of skipping the track.
                if let Some(index) = self.queue.current_index() {
                    self.pending = Some(PendingLoad { index, play: true, since: Instant::now() });
                }
                self.emit_queue();
            }
            None => {
                self.playing = false;
                self.emit(EngineEvent::TrackChanged { track: None });
                self.emit(EngineEvent::Finished);
                self.emit_queue();
            }
        }
    }

    fn tick(&mut self) {
        // A transition waiting on a stream URL outranks everything else: pick
        // up the patch, or give up after the bound.
        if let Some(p) = self.pending {
            match self.queue.track_at(p.index) {
                Some(track) if !Self::is_placeholder(&track.path) => {
                    let track = track.clone();
                    self.pending = None;
                    self.load(&track, p.play);
                    return;
                }
                Some(_) => {
                    if p.since.elapsed() > PLACEHOLDER_WAIT {
                        let track = self.queue.track_at(p.index).cloned();
                        self.pending = None;
                        if let Some(track) = track {
                            tracing::warn!(track = %track.path, "placeholder wait timed out");
                            self.emit(EngineEvent::TrackError {
                                track,
                                message: "stream did not resolve in time".into(),
                            });
                        }
                        self.advance_past_current();
                    }
                    // Still waiting: hold here. Old audio keeps playing (manual
                    // next) or stays silent (natural end); no position/preload
                    // work makes sense for a transition in flight.
                    return;
                }
                None => self.pending = None, // row removed under us
            }
        }

        // Degraded engine: nothing audible can happen. RetryOutput re-opens
        // the device; until then stay quiet and cheap.
        let (pos, empty, paused) = match self.sink() {
            Some(s) => (s.get_pos().as_secs_f64(), s.empty(), s.is_paused()),
            None => return,
        };

        if empty {
            if Instant::now() >= self.manual_until {
                self.on_track_finished();
            }
            self.last_pos = 0.0;
            return;
        }

        // A preloaded next track becoming audible shows up as the sink
        // position dropping back near zero.
        if let Some(pre) = self.preloaded {
            if pos + 0.05 < self.last_pos && pos < 0.2 {
                self.preloaded = None;
                self.manual_until = Instant::now() + Duration::from_millis(250);
                self.queue.apply(QueueOp::Jump(pre));
                let track = self.queue.current().cloned();
                self.emit(EngineEvent::TrackChanged { track });
                self.emit_queue();
                self.last_pos = pos;
                return;
            }
        }
        self.last_pos = pos;

        if self.playing && !paused {
            self.emit_position();

            let remaining = (self.duration_hint - pos).max(0.0);
            if self.preloaded.is_none()
                && remaining > 0.0
                && remaining < PRELOAD_AHEAD_SEC
                && self.queue.repeat() != crate::queue::Repeat::One
            {
                if let Some(next) = self.queue.next_track().cloned() {
                    match self.append(&next) {
                        Ok(()) => {
                            self.preloaded = self
                                .queue
                                .snapshot(false)
                                .tracks
                                .iter()
                                .position(|t| t.path == next.path && t.id == next.id);
                        }
                        Err(e) => {
                            tracing::warn!(track = %next.path, error = %e, "gapless preload failed");
                            self.preloaded = Some(usize::MAX);
                        }
                    }
                }
            }
        }
    }

    fn on_track_finished(&mut self) {
        self.playing = false;
        let effect = self.queue.apply(QueueOp::Next);
        if effect.load.is_some() {
            self.apply_effect(effect);
        } else {
            self.clear_sink();
            self.emit(EngineEvent::TrackChanged { track: None });
            self.emit(EngineEvent::Finished);
            self.emit_queue();
        }
    }
}

pub struct Engine {
    tx: Sender<Command>,
    pub events: tokio::sync::broadcast::Sender<EngineEvent>,
    snapshot: Arc<RwLock<QueueSnapshot>>,
    volume: Arc<Mutex<f64>>,
}

impl Engine {
    /// Spawns the audio worker. Never fails: when no output device is
    /// available the engine starts degraded and retries on demand, so the app
    /// (library, settings, lyrics) still works and playback reports honestly.
    ///
    /// `cpal::Stream` (inside rodio's `OutputStream`) is not `Send`, so the
    /// stream is created and kept on the audio thread itself; the caller only
    /// ever holds channels and the `Send`+`Sync` `Sink`.
    pub fn spawn(cache: Option<Arc<StreamCache>>) -> Self {
        let (init_tx, init_rx) = std::sync::mpsc::channel::<Result<(), String>>();
        let (tx, rx) = std::sync::mpsc::channel::<Command>();
        let (events, _) = tokio::sync::broadcast::channel(128);
        let snapshot = Arc::new(RwLock::new(QueueSnapshot {
            tracks: vec![],
            index: None,
            repeat: crate::queue::Repeat::Off,
            playing: false,
            at_end: true,
        }));
        let volume = Arc::new(Mutex::new(1.0));

        let mut worker = Worker {
            queue: Queue::default(),
            output: None,
            cache,
            events: events.clone(),
            volume: 1.0,
            preloaded: None,
            last_pos: 0.0,
            manual_until: Instant::now(),
            playing: false,
            duration_hint: 0.0,
            pending: None,
        };
        let snap_out = snapshot.clone();
        let vol_out = volume.clone();
        std::thread::Builder::new()
            .name("audio-engine".into())
            .spawn(move || {
                // The output stream must outlive everything in this loop, and
                // it cannot cross threads — so it lives in this slot here.
                let mut stream_slot: Option<OutputStream> = None;
                match open_output_stream() {
                    Ok(stream) => {
                        let sink = Sink::connect_new(stream.mixer());
                        sink.pause(); // start silent; a Load command starts sound
                        worker.output = Some(sink);
                        stream_slot = Some(stream);
                    }
                    Err(e) => {
                        // Degraded start: Command::RetryOutput re-opens.
                        tracing::warn!(error = %e, "starting without audio output");
                    }
                }
                let _ = init_tx.send(Ok(()));

                loop {
                    // Fast ticks while audible; slow ticks otherwise.
                    let audible = worker.playing
                        && worker.sink().map(|s| !s.is_paused()).unwrap_or(false);
                    let timeout = Duration::from_millis(if audible {
                        TICK_ACTIVE_MS
                    } else {
                        TICK_IDLE_MS
                    });
                    match rx.recv_timeout(timeout) {
                        Ok(cmd) => worker.handle(cmd, &mut stream_slot),
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => worker.tick(),
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                    let audible = worker.playing
                        && worker.sink().map(|s| !s.is_paused()).unwrap_or(false);
                    *snap_out.write().unwrap() = worker.queue.snapshot(audible);
                    *vol_out.lock().unwrap() = worker.volume;
                }
                tracing::info!("audio engine stopped");
            })
            .expect("audio thread spawn");

        init_rx
            .recv()
            .map_err(|_| EngineError::Output("audio thread died during init".into()))
            .and_then(|r| r.map_err(EngineError::Output))
            .expect("audio engine init");

        Self { tx, events, snapshot, volume }
    }

    pub fn send(&self, cmd: Command) {
        let _ = self.tx.send(cmd);
    }

    /// A handle for services that push commands from their own threads
    /// (resolve-ahead). Commands are applied on the audio thread.
    pub fn command_tx(&self) -> Sender<Command> {
        self.tx.clone()
    }

    /// Cheap synchronous view for command handlers.
    pub fn snapshot(&self) -> QueueSnapshot {
        self.snapshot.read().unwrap().clone()
    }

    pub fn volume(&self) -> f64 {
        *self.volume.lock().unwrap()
    }
}

impl Worker {
    /// Route a queue effect's load through the placeholder logic: rows whose
    /// stream URL has not been resolved yet hold a pending load (current audio
    /// keeps playing, or silence for natural ends) until the resolve-ahead
    /// patch lands — bounded, then a clean error + advance.
    fn apply_effect(&mut self, effect: crate::queue::QueueEffect) {
        match effect.load {
            Some((t, play)) if Self::is_placeholder(&t.path) => {
                if let Some(index) = self.queue.current_index() {
                    if !play {
                        if let Some(sink) = self.sink() {
                            sink.pause();
                        }
                        self.playing = false;
                    }
                    self.pending = Some(PendingLoad { index, play, since: Instant::now() });
                    self.emit_queue();
                }
            }
            Some((t, play)) => self.load(&t, play),
            None if effect.changed => self.emit_queue(),
            None => {}
        }
    }

    fn handle(&mut self, cmd: Command, stream_slot: &mut Option<OutputStream>) {
        match cmd {
            Command::Apply(op) => {
                let effect = self.queue.apply(op);
                self.apply_effect(effect);
            }
            Command::TogglePlay => {
                if self.queue.current().is_none() {
                    return;
                }
                if self.output.is_none() && !self.ensure_output(stream_slot) {
                    return;
                }
                if let Some(sink) = self.sink() {
                    if sink.is_paused() {
                        sink.play();
                        self.playing = true;
                    } else {
                        sink.pause();
                        self.playing = false;
                    }
                }
                self.emit_queue();
                self.emit_position();
            }
            Command::Pause => {
                if let Some(sink) = self.sink() {
                    sink.pause();
                }
                self.playing = false;
                self.emit_queue();
                self.emit_position();
            }
            Command::Resume => {
                if self.queue.current().is_some() {
                    if self.output.is_none() && !self.ensure_output(stream_slot) {
                        return;
                    }
                    if let Some(sink) = self.sink() {
                        sink.play();
                    }
                    self.playing = true;
                    self.emit_queue();
                    self.emit_position();
                }
            }
            Command::Stop => {
                self.clear_sink();
                self.playing = false;
                self.queue.apply(QueueOp::Clear);
                self.emit(EngineEvent::TrackChanged { track: None });
                self.emit_queue();
            }
            Command::Seek(sec) => {
                let target = Duration::from_secs_f64(sec.max(0.0));
                if self.output.is_none() {
                    return;
                }
                let preloaded = self.preloaded.is_some();
                if preloaded {
                    // A pending append would receive the seeked position of
                    // the *old* track; start over from the current one.
                    if let Some(cur) = self.queue.current().cloned() {
                        self.clear_sink();
                        if self.append(&cur).is_ok() {
                            if let Some(sink) = self.sink() {
                                let _ = sink.try_seek(target);
                            }
                        }
                    }
                } else if let Some(sink) = self.sink() {
                    let _ = sink.try_seek(target);
                }
                self.emit_position();
            }
            Command::SetVolume(v) => {
                let v = v.clamp(0.0, 1.0);
                if let Some(sink) = self.sink() {
                    sink.set_volume(v as f32);
                }
                self.volume = v;
                self.emit(EngineEvent::Volume { level: v });
            }
            Command::PatchUrl { index, url } => {
                self.queue.patch_path(index, &url);
            }
            Command::RetryOutput => {
                if self.ensure_output(stream_slot) {
                    self.emit_queue();
                }
            }
        }
    }
}
