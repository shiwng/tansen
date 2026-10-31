//! The rodio-backed pipeline (M7): the real-media implementation the
//! app-core `PipelineFactory` drives. One dedicated thread owns the
//! `OutputStream` (it is not `Send`); commands arrive over a std mpsc;
//! `frames_played` is a lock-free `AtomicU64` incremented by the source
//! wrapper on the feeder path — no locks, no allocations per sample.
//!
//! This is a parallel path: the existing `engine.rs` playback is untouched
//! and stays the production path until M10 removes it.

use crate::cache::StreamCache;
use crate::remote::HttpRangeReader;
use rodio::{Decoder, OutputStream, Sink, Source};
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

enum Request {
    /// Replace the sink's content with this path and start playing.
    /// The ack answers once the decoder is open (or not).
    Load { path: String, ack: mpsc::SyncSender<Result<(), String>> },
    Play,
    Pause,
    SeekMs(u64),
    Volume(f32),
    /// Drop all queued audio without touching the pipeline itself.
    Clear,
    Quit,
}

/// Handle to the audio thread. Cloneable is not needed: the factory owns one.
pub struct AudioPipeline {
    tx: Option<mpsc::Sender<Request>>,
    /// Interleaved sample *frames* delivered to the sink since the current
    /// load (or since the last seek). Lock-free; the audio thread writes,
    /// anyone may read.
    frames: Arc<AtomicU64>,
    /// Sample rate of the currently loaded source (for ms conversion).
    sample_rate: Arc<AtomicU64>,
    /// Duration of the current media (0 = unknown).
    duration_ms: Arc<AtomicU64>,
}

impl AudioPipeline {
    /// Spawns the audio thread. The output stream opens lazily inside the
    /// thread; a missing device surfaces on the first `load` as `Err`, which
    /// the factory maps to `ErrorKind::Device`.
    pub fn spawn(cache: Option<Arc<StreamCache>>) -> Result<Self, String> {
        let frames = Arc::new(AtomicU64::new(0));
        let sample_rate = Arc::new(AtomicU64::new(44_100));
        let duration_ms = Arc::new(AtomicU64::new(0));
        let (tx, rx) = mpsc::channel::<Request>();
        let (frames_t, rate_t, dur_t) = (frames.clone(), sample_rate.clone(), duration_ms.clone());
        std::thread::Builder::new()
            .name("pipeline-audio".into())
            .spawn(move || audio_thread(rx, cache, frames_t, rate_t, dur_t))
            .map_err(|e| format!("pipeline thread: {e}"))?;
        Ok(Self { tx: Some(tx), frames, sample_rate, duration_ms })
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

    /// Seek the sink; the frame counter jumps to the target so `pos_ms`
    /// stays truthful.
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
) {
    // The stream lives here forever: `OutputStream` is not `Send`.
    let mut slot: Option<OutputStream> = None;
    let mut sink: Option<Sink> = None;
    // Volume is sticky across loads: SetVolume before a Load still applies.
    let mut volume = 1.0f32;

    macro_rules! ensure_open {
        () => {
            if slot.is_none() {
                match crate::engine::open_output_stream() {
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
                let answer = (|| -> Result<(), String> {
                    let sink = sink.as_ref().ok_or_else(|| "no audio output device".to_string())?;
                    let source = open_source(&path, cache.as_ref())?;
                    sink.clear();
                    sample_rate.store(u64::from(source.sample_rate()), Ordering::Relaxed);
                    frames.store(0, Ordering::Relaxed);
                    let total = source
                        .total_duration()
                        .map(|d| d.as_millis().min(u128::from(u64::MAX)) as u64)
                        .unwrap_or(0);
                    duration_ms.store(total, Ordering::Relaxed);
                    let channels = source.channels().max(1);
                    sink.append(FrameCounted { inner: source, channels, samples: 0, frames: frames.clone() });
                    sink.set_volume(volume);
                    sink.play();
                    Ok(())
                })();
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
                if let Some(s) = &sink {
                    let _ = s.try_seek(Duration::from_millis(ms));
                }
                let rate = sample_rate.load(Ordering::Relaxed).max(1);
                frames.store(ms.saturating_mul(rate) / 1000, Ordering::Relaxed);
            }
            Request::Volume(v) => {
                volume = v.clamp(0.0, 1.0);
                if let Some(s) = &sink {
                    s.set_volume(volume);
                }
            }
            Request::Clear => {
                if let Some(s) = &sink {
                    s.clear();
                }
            }
            Request::Quit => break,
        }
    }
}

/// Same decode branches as the production engine's `append` (cache hit →
/// local file; http(s) → ranged reader; sidecar:// unresolved → error).
fn open_source(path: &str, cache: Option<&Arc<StreamCache>>) -> Result<Box<dyn Source<Item = f32> + Send>, String> {
    let source: Box<dyn Source<Item = f32> + Send> =
        if let Some(cached) = cache.and_then(|c| c.get(path)) {
            let file = File::open(cached).map_err(|e| e.to_string())?;
            Box::new(Decoder::new(BufReader::new(file)).map_err(|e| e.to_string())?)
        } else if path.starts_with("http://") || path.starts_with("https://") {
            let reader = HttpRangeReader::new(path.to_owned(), vec![]).map_err(|e| e.to_string())?;
            Box::new(Decoder::new(BufReader::new(reader)).map_err(|e| e.to_string())?)
        } else {
            let file = File::open(PathBuf::from(path)).map_err(|e| e.to_string())?;
            Box::new(Decoder::new(BufReader::new(file)).map_err(|e| e.to_string())?)
        };
    Ok(source)
}

/// Counts delivered interleaved sample frames. One `fetch_add` per frame
/// boundary; no locks, no allocations — feeder-thread safe.
struct FrameCounted {
    inner: Box<dyn Source<Item = f32> + Send>,
    channels: u16,
    samples: u64,
    frames: Arc<AtomicU64>,
}

impl Iterator for FrameCounted {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let sample = self.inner.next();
        if sample.is_some() {
            self.samples = self.samples.wrapping_add(1);
            if self.samples % u64::from(self.channels) == 0 {
                self.frames.fetch_add(1, Ordering::Relaxed);
            }
        }
        sample
    }
}

impl Source for FrameCounted {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }
    fn channels(&self) -> u16 {
        self.inner.channels()
    }
    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::*;

    struct FakeSource {
        left: usize,
    }
    impl Iterator for FakeSource {
        type Item = f32;
        fn next(&mut self) -> Option<f32> {
            if self.left == 0 {
                None
            } else {
                self.left -= 1;
                Some(0.0)
            }
        }
    }
    impl Source for FakeSource {
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
    fn counts_frames_not_samples() {
        let frames = Arc::new(AtomicU64::new(0));
        let mut counted = FrameCounted {
            inner: Box::new(FakeSource { left: 10 }),
            channels: 2,
            samples: 0,
            frames: frames.clone(),
        };
        while counted.next().is_some() {}
        // 10 interleaved samples at 2 channels = 5 frames.
        assert_eq!(frames.load(Ordering::Relaxed), 5);
    }
}
