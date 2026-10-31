//! The real `PipelineFactory` over the playback crate's rodio pipeline (M7):
//! one audio thread + one sink; `prepare` swaps the sink's content,
//! `release` silences it, `transport` drives pause/resume/seek/volume.
//! Shared by the app shell (M9 wiring) and the `engine_demo` debug binary.

use app_core::playback_engine::{PipelineFactory, Transport};
use app_core::playback_types::{EntryId, ErrorKind};
use playback::pipeline::AudioPipeline;
use playback::StreamCache;
use std::sync::{Arc, Mutex};

pub struct RodioFactory {
    /// EntryId → media path/URL. Library lookups happen live (scans add
    /// files); the M1 bench sentinel resolves through the env hook.
    resolve: Box<dyn Fn(u64) -> Option<String> + Send + Sync>,
    pipeline: Mutex<Option<AudioPipeline>>,
    cache: Option<Arc<StreamCache>>,
}

impl RodioFactory {
    pub fn new(
        resolve: Box<dyn Fn(u64) -> Option<String> + Send + Sync>,
        cache: Option<Arc<StreamCache>>,
    ) -> Self {
        Self { resolve, pipeline: Mutex::new(None), cache }
    }

    /// (frames_played, pos_ms) of the live pipeline, for observability.
    pub fn frames(&self) -> (u64, u64) {
        match self.pipeline.lock() {
            Ok(guard) => match guard.as_ref() {
                Some(p) => (p.frames_played(), p.pos_ms()),
                None => (0, 0),
            },
            Err(_) => (0, 0),
        }
    }
}

impl PipelineFactory for RodioFactory {
    fn prepare(&self, entry: EntryId) -> Result<(), ErrorKind> {
        let path = (self.resolve)(entry.0).ok_or(ErrorKind::Resolve)?;
        let mut guard = self.pipeline.lock().map_err(|_| ErrorKind::Device)?;
        if guard.is_none() {
            *guard = Some(AudioPipeline::spawn(self.cache.clone()).map_err(|_| ErrorKind::Device)?);
        }
        let pipeline = guard.as_ref().ok_or(ErrorKind::Device)?;
        pipeline.load(&path).map_err(|e| {
            if e.contains("no audio output device") {
                ErrorKind::Device
            } else {
                ErrorKind::Decode
            }
        })
    }

    fn release(&self, _entry: EntryId) {
        if let Ok(guard) = self.pipeline.lock() {
            if let Some(p) = guard.as_ref() {
                p.clear();
            }
        }
    }

    fn transport(&self, action: Transport) {
        if let Ok(guard) = self.pipeline.lock() {
            if let Some(p) = guard.as_ref() {
                match action {
                    Transport::Play => p.play(),
                    Transport::Pause => p.pause(),
                    Transport::Seek { ms } => p.seek_ms(ms),
                    Transport::Volume(v) => p.set_volume(v),
                }
            }
        }
    }

    fn position_ms(&self) -> Option<u64> {
        let guard = self.pipeline.lock().ok()?;
        guard.as_ref().map(|p| p.pos_ms())
    }

    fn duration_ms(&self) -> Option<u64> {
        let guard = self.pipeline.lock().ok()?;
        guard.as_ref().and_then(|p| p.duration_ms())
    }
}
