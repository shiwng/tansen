//! Audio engine: owns the output stream and the rodio sink, applies queue
//! effects, and drives gapless track advance.
//!
//! The engine runs on a dedicated OS thread because `rodio::OutputStream` is
//! not `Send` and dropping it stops audio. Commands arrive over an mpsc
//! channel; state changes go out on a tokio broadcast channel that the app
//! layer forwards to the frontend.

use crate::queue::{Queue, QueueOp, QueueSnapshot};
use crate::remote::HttpRangeReader;
use persistence::models::TrackRef;
use rodio::{Decoder, OutputStreamBuilder, Sink, Source};
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

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
    sink: Sink,
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
}

impl Worker {
    fn emit(&self, ev: EngineEvent) {
        let _ = self.events.send(ev);
    }

    fn emit_queue(&self) {
        let snap = self.queue.snapshot(self.playing && !self.sink.is_paused());
        self.emit(EngineEvent::QueueChanged { snapshot: snap });
    }

    fn emit_position(&self) {
        let pos = self.sink.get_pos().as_secs_f64();
        self.emit(EngineEvent::Position {
            sec: pos,
            duration: self.duration_hint,
            playing: self.playing && !self.sink.is_paused(),
        });
    }

    fn clear_sink(&mut self) {
        self.sink.clear();
        self.preloaded = None;
        self.last_pos = 0.0;
        self.manual_until = Instant::now() + Duration::from_millis(250);
    }

    /// Opens and appends `track`'s decoder to the sink without clearing.
    /// Local files open straight from disk; http(s) paths stream through the
    /// range reader; `sidecar://` placeholders that were never patched (a
    /// manual next before the resolve-ahead finished) error and skip.
    fn append(&mut self, track: &TrackRef) -> Result<(), String> {
        // Local and remote decoders are different concrete types; the sink
        // takes any Source, so each branch erases to the same trait object.
        let source: Box<dyn Source<Item = f32> + Send> =
            if track.path.starts_with("http://") || track.path.starts_with("https://") {
                let reader = HttpRangeReader::new(track.path.clone(), vec![]).map_err(|e| e.to_string())?;
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
        self.sink.append(source);
        Ok(())
    }

    fn load(&mut self, track: &TrackRef, play: bool) {
        self.clear_sink();
        if let Err(e) = self.append(track) {
            tracing::warn!(track = %track.path, error = %e, "failed to open track");
            self.emit(EngineEvent::TrackError { track: track.clone(), message: e });
            self.advance_past_current();
            return;
        }
        if play {
            self.sink.play();
        } else {
            self.sink.pause();
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
            Some((t, _)) => self.load(&t, true),
            None => {
                self.playing = false;
                self.emit(EngineEvent::TrackChanged { track: None });
                self.emit(EngineEvent::Finished);
                self.emit_queue();
            }
        }
    }

    fn tick(&mut self) {
        let pos = self.sink.get_pos().as_secs_f64();
        let empty = self.sink.empty();

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

        if self.playing && !self.sink.is_paused() {
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
        match effect.load {
            Some((t, _)) => self.load(&t, true),
            None => {
                self.clear_sink();
                self.emit(EngineEvent::TrackChanged { track: None });
                self.emit(EngineEvent::Finished);
                self.emit_queue();
            }
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
    /// Spawns the audio worker. Fails only if no output device exists.
    ///
    /// `cpal::Stream` (inside rodio's `OutputStream`) is not `Send`, so the
    /// stream is created and kept on the audio thread itself; the caller only
    /// ever holds channels and the `Send`+`Sync` `Sink`.
    pub fn spawn() -> Result<Self, EngineError> {
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
            sink: Sink::new().0, // replaced once the stream is open on-thread
            events: events.clone(),
            volume: 1.0,
            preloaded: None,
            last_pos: 0.0,
            manual_until: Instant::now(),
            playing: false,
            duration_hint: 0.0,
        };
        let snap_out = snapshot.clone();
        let vol_out = volume.clone();
        std::thread::Builder::new()
            .name("audio-engine".into())
            .spawn(move || {
                // The output stream must outlive everything in this loop.
                let stream = match OutputStreamBuilder::open_default_stream() {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = init_tx.send(Err(e.to_string()));
                        return;
                    }
                };
                worker.sink = Sink::connect_new(stream.mixer());
                worker.sink.pause(); // start silent; a Load command starts sound
                let _ = init_tx.send(Ok(()));

                loop {
                    match rx.recv_timeout(Duration::from_millis(100)) {
                        Ok(cmd) => worker.handle(cmd),
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => worker.tick(),
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                    *snap_out.write().unwrap() =
                        worker.queue.snapshot(worker.playing && !worker.sink.is_paused());
                    *vol_out.lock().unwrap() = worker.volume;
                }
                tracing::info!("audio engine stopped");
            })
            .map_err(|e| EngineError::Output(e.to_string()))?;

        init_rx
            .recv()
            .map_err(|_| EngineError::Output("audio thread died during init".into()))?
            .map_err(EngineError::Output)?;

        Ok(Self { tx, events, snapshot, volume })
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
    fn handle(&mut self, cmd: Command) {
        match cmd {
            Command::Apply(op) => {
                let effect = self.queue.apply(op);
                match effect.load {
                    Some((t, play)) => self.load(&t, play),
                    None if effect.changed => self.emit_queue(),
                    None => {}
                }
            }
            Command::TogglePlay => {
                if self.queue.current().is_none() {
                    return;
                }
                if self.sink.is_paused() {
                    self.sink.play();
                    self.playing = true;
                } else {
                    self.sink.pause();
                    self.playing = false;
                }
                self.emit_queue();
                self.emit_position();
            }
            Command::Pause => {
                self.sink.pause();
                self.playing = false;
                self.emit_queue();
                self.emit_position();
            }
            Command::Resume => {
                if self.queue.current().is_some() {
                    self.sink.play();
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
                if self.preloaded.is_some() {
                    // A pending append would receive the seeked position of
                    // the *old* track; start over from the current one.
                    if let Some(cur) = self.queue.current().cloned() {
                        self.clear_sink();
                        if self.append(&cur).is_ok() {
                            let _ = self.sink.try_seek(target);
                        }
                    }
                } else {
                    let _ = self.sink.try_seek(target);
                }
                self.emit_position();
            }
            Command::SetVolume(v) => {
                let v = v.clamp(0.0, 1.0);
                self.sink.set_volume(v as f32);
                self.volume = v;
                self.emit(EngineEvent::Volume { level: v });
            }
            Command::PatchUrl { index, url } => {
                self.queue.patch_path(index, &url);
            }
        }
    }
}
