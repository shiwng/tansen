//! M7 end-to-end exercise (debug binary): the app-core playback `Engine`
//! drives the rodio-backed `PipelineFactory` over local files — play, pause,
//! resume, seek, volume — while printing every published state and the
//! `frames_played` counter.
//!
//! Usage:
//! ```sh
//! cargo run -p tansen --bin engine_demo -- [file.wav ...]
//! ```
//! With no arguments it picks four WAVs from the M4 stress fixtures.
//! Needs an audio device; not part of `cargo test`.

use app_core::playback_engine::{Engine, PipelineFactory, Transport};
use app_core::playback_types::{Command, EntryId, ErrorKind, Placement};
use playback::pipeline::AudioPipeline;
use playback::StreamCache;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The real `PipelineFactory` over the playback crate's rodio pipeline: one
/// audio thread + one sink; `prepare` swaps the sink's content, `release`
/// silences it, `transport` drives pause/resume/seek/volume. Lives in this
/// demo binary for M7; the production wiring migrates in M8+.
struct RodioFactory {
    resolve: HashMap<u64, String>,
    pipeline: Mutex<Option<AudioPipeline>>,
    cache: Option<Arc<StreamCache>>,
}

impl RodioFactory {
    fn new(resolve: HashMap<u64, String>, cache: Option<Arc<StreamCache>>) -> Self {
        Self { resolve, pipeline: Mutex::new(None), cache }
    }

    /// (frames_played, pos_ms) of the live pipeline, for the demo prints.
    fn frames(&self) -> (u64, u64) {
        match self.pipeline.lock() {
            Ok(g) => match g.as_ref() {
                Some(p) => (p.frames_played(), p.pos_ms()),
                None => (0, 0),
            },
            Err(_) => (0, 0),
        }
    }
}

impl PipelineFactory for RodioFactory {
    fn prepare(&self, entry: EntryId) -> Result<(), ErrorKind> {
        let path = self.resolve.get(&entry.0).ok_or(ErrorKind::Resolve)?.clone();
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
}

fn default_fixtures() -> Vec<String> {
    let dir = "/tmp/tansen-stress-lib";
    let mut files: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "wav"))
                .filter_map(|e| e.path().to_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files.into_iter().take(4).collect()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut files: Vec<String> = std::env::args().skip(1).collect();
    if files.is_empty() {
        files = default_fixtures();
    }
    if files.is_empty() {
        return Err("no wav files given and no /tmp/tansen-stress-lib fixtures".into());
    }
    println!("demo files: {files:?}");

    let resolve: HashMap<u64, String> =
        files.iter().enumerate().map(|(i, p)| (i as u64, p.clone())).collect();
    let factory = Arc::new(RodioFactory::new(resolve, None));

    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    rt.block_on(async move {
        let engine = Engine::start(factory.clone());
        let rx = engine.subscribe();
        let mut observer = rx.clone();
        tokio::spawn(async move {
            while observer.changed().await.is_ok() {
                let s = observer.borrow().clone();
                println!(
                    "state rev={} status={:?} entry={:?} rate={} pos_ms={}",
                    s.rev, s.status, s.entry, s.anchor.rate, s.anchor.pos_ms
                );
            }
        });

        let n = files.len() as u64;
        engine
            .send(Command::Enqueue {
                tracks: (0..n).map(EntryId).collect(),
                placement: Placement::Append,
            })
            .await;
        engine.send(Command::Play { entry: EntryId(0) }).await;
        tokio::time::sleep(Duration::from_secs(3)).await;
        let (f0, pos0) = factory.frames();
        println!("after 3 s: frames={f0} pos_ms={pos0}");

        engine.send(Command::Pause).await;
        tokio::time::sleep(Duration::from_secs(1)).await;
        let (f1, _) = factory.frames();
        println!(
            "after pause (1 s): frames={f1} (Δ={}; a playing pipeline gains ~44100/s)",
            f1.saturating_sub(f0)
        );

        engine.send(Command::Resume).await;
        engine.send(Command::Seek { ms: 1000 }).await;
        engine.send(Command::SetVolume(0.4)).await;
        tokio::time::sleep(Duration::from_secs(3)).await;
        let (f2, pos2) = factory.frames();
        println!("after resume+seek(1s)+vol(0.4): frames={f2} pos_ms={pos2} (seek reset the counter to ~44100)");

        let last = rx.borrow().clone();
        println!("final status={:?} rate={} volume={}", last.status, last.anchor.rate, last.volume);
    });
    Ok(())
}
