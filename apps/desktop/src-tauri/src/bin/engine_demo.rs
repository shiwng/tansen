//! End-to-end exercise (debug binary): the app-core playback `Engine`
//! drives the rodio-backed `PipelineFactory` over local files — play, pause,
//! resume, seek, volume — while printing every published state and the
//! `frames_played` counter.
//!
//! Usage:
//! ```sh
//! cargo run -p tansen --bin engine_demo -- [file.wav ...]
//! ```
//! With no arguments it looks for four WAVs in /tmp/tansen-demo-lib.
//! Needs an audio device; not part of `cargo test`.

#[path = "../rodio_factory.rs"]
mod rodio_factory;

use app_core::playback_engine::Engine;
use app_core::playback_types::{Command, EntryId, Placement};
use rodio_factory::RodioFactory;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

fn default_fixtures() -> Vec<String> {
    let paths = ["/tmp/tansen-demo-a.wav", "/tmp/tansen-demo-b.wav"];
    for path in paths {
        if std::path::Path::new(path).is_file() {
            continue;
        }
        let rate = 44_100;
        write_wav(path, rate, rate * 30);
    }
    paths.iter().map(|p| p.to_string()).collect()
}

/// Minimal mono 16-bit WAV writer (std only; hound is not a dependency).
fn write_wav(path: &str, sample_rate: usize, samples: usize) {
    use std::io::Write;
    let data_len = samples * 2;
    let mut f = std::fs::File::create(path).expect("demo fixture create");
    let mut buf = Vec::with_capacity(44 + data_len);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
    buf.extend_from_slice(b"WAVEfmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes()); // PCM
    buf.extend_from_slice(&1u16.to_le_bytes()); // mono
    buf.extend_from_slice(&(sample_rate as u32).to_le_bytes());
    buf.extend_from_slice(&((sample_rate * 2) as u32).to_le_bytes());
    buf.extend_from_slice(&2u16.to_le_bytes());
    buf.extend_from_slice(&16u16.to_le_bytes());
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&(data_len as u32).to_le_bytes());
    for i in 0..samples {
        let v = (9000.0
            * (2.0 * std::f64::consts::PI * 220.0 * i as f64 / sample_rate as f64).sin())
            as i16;
        buf.extend_from_slice(&v.to_le_bytes());
    }
    let _ = f.write_all(&buf);
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut files: Vec<String> = std::env::args().skip(1).collect();
    if files.is_empty() {
        files = default_fixtures();
    }
    if files.is_empty() {
        return Err("no wav files given and no /tmp/tansen-demo-lib fixtures".into());
    }
    println!("demo files: {files:?}");

    let resolve: HashMap<u64, String> = files
        .iter()
        .enumerate()
        .map(|(i, p)| (i as u64, p.clone()))
        .collect();
    let factory = Arc::new(RodioFactory::new(
        Box::new(move |id: u64| {
            resolve.get(&id).cloned().map(|url| sources::ResolvedMedia {
                url,
                headers: vec![],
                expiry: None,
                length: None,
                mime: None,
                format: None,
                container: None,
                source: sources::SourceKind::Local,
                below_request: false,
            })
        }),
        None,
    ));

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
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
                remote: vec![],
                placement: Placement::Append,
            })
            .await;
        engine.send(Command::Play { entry: EntryId(0), remote: None }).await;
        tokio::time::sleep(Duration::from_secs(2)).await;
        let (f0, pos0) = factory.frames();
        println!("after 2 s: frames={f0} pos_ms={pos0}");

        engine.send(Command::Pause).await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        let (f1, _) = factory.frames();
        println!(
            "after pause (0.5 s): frames={f1} (Δ={}; a playing pipeline gains ~44100/s)",
            f1.saturating_sub(f0)
        );

        engine.send(Command::Resume).await;
        engine.send(Command::Seek { ms: 1000 }).await;
        engine.send(Command::SetVolume(0.4)).await;
        tokio::time::sleep(Duration::from_secs(3)).await;
        let (f2, pos2) = factory.frames();
        println!("after resume+seek(1s)+vol(0.4): frames={f2} pos_ms={pos2} (seek reset the counter to ~44100)");

        // Auto-advance demo: seek near the 30 s end; the deadline fires,
        // the frames confirm, and the engine walks into entry 1.
        engine.send(Command::Seek { ms: 29_000 }).await;
        tokio::time::sleep(Duration::from_millis(2500)).await;
        let last = rx.borrow().clone();
        println!(
            "final status={:?} entry={:?} rate={} (auto-advance expected to entry 1)",
            last.status, last.entry, last.anchor.rate
        );
    });
    Ok(())
}
