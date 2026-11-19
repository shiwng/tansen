//! Sidecar measurements (`#[ignore]`; needs the sidecar jar + network).
//!
//! Measures the app's own spawn path (`YtMusicSource::launch`: free port →
//! jar discovery → JVM spawn → /health poll):
//!   1. spawn → ready (launch returns only once /health answers ok)
//!   2. sidecar JVM RSS (`ps -o rss=`), at ready and after the resolve run
//!   3. resolve latency of `/track/{id}/stream?prefer=aac` over 30 calls —
//!      nearest-rank p50/p95; call 1 is cold (first Innertube resolve)
//!   4. first-play latency: cold resolve → ranged HTTP reader open (MP4 moov
//!      over the wire) → first decoded audio (0.25 s window, RMS > 0)
//!
//! Scope: sidecar + engine-equivalent media path only. Tauri IPC, queue
//! wiring and UI are not included.
//!
//! ```sh
//! cargo test -p sources --test yt_measure -- --ignored --nocapture
//! ```

use sources::yt::YtMusicSource;
use sources::SourceKind;
use sources::{MusicSource, StreamRequest, TrackKey};
use std::process::Command;
use std::time::{Duration, Instant};

fn sidecar_pid() -> Option<u32> {
    let out = Command::new("pgrep")
        .args(["-f", "sidecar-all.jar"])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()?
        .trim()
        .parse()
        .ok()
}

fn rss_kib(pid: u32) -> Option<u64> {
    let out = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// Nearest-rank percentile over a pre-sorted slice.
fn nearest_rank(sorted: &[f64], pct: f64) -> f64 {
    let rank = ((pct / 100.0) * sorted.len() as f64).ceil().max(1.0) as usize;
    sorted[rank - 1]
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "needs the sidecar jar + network access to YouTube; measurement only"]
async fn sidecar_measurements() {
    // Java: JAVA_HOME if set, else this dev machine's JDK 17 (the macOS
    // /usr/bin/java stub exits 1 and is refused by the app's own discovery).
    let java = std::env::var("JAVA_HOME")
        .map(|h| format!("{h}/bin/java"))
        .unwrap_or_else(|_| {
            "/opt/homebrew/opt/openjdk@17/libexec/openjdk.jdk/Contents/Home/bin/java".to_string()
        });

    // 1. spawn → ready, via the app's real path (jar discovery finds the dev
    //    jar the same way the running app does).
    let t0 = Instant::now();
    let source =
        YtMusicSource::launch(None, Some(java)).expect("sidecar launches and reports healthy");
    let ready_ms = t0.elapsed().as_millis();
    println!("spawn→ready: {ready_ms} ms");
    assert!(source.version().is_some());

    let pid = sidecar_pid().expect("sidecar JVM pid found");
    let rss_ready = rss_kib(pid).expect("rss readable");
    println!(
        "sidecar pid {pid}: RSS at ready {} KiB ({} MiB)",
        rss_ready,
        rss_ready / 1024
    );

    // One search to pin a track; resolve latency is measured against it.
    let rows = source
        .search("Tom Sawyer Rush", 5)
        .await
        .expect("search succeeds");
    let track = &rows[0];
    println!(
        "track: {} — {} ({})",
        track.title, track.artist, track.track_id
    );

    // 2. resolve latency ×30. Call 1 is cold (sidecar's first resolve for
    //    this id; later ones may hit its internal caches).
    let mut lat: Vec<f64> = Vec::with_capacity(30);
    for i in 0..30 {
        let t = Instant::now();
        let s = source
            .stream(
                &TrackKey {
                    source: SourceKind::YouTube,
                    id: track.track_id.clone(),
                },
                &StreamRequest::Best,
            )
            .await
            .expect("stream resolves")
            .expect("stream present");
        lat.push(t.elapsed().as_secs_f64() * 1000.0);
        if i == 0 {
            assert!(s.url.contains("googlevideo"), "expected a googlevideo URL");
        }
    }
    lat.sort_by(|a, b| a.total_cmp(b));
    println!(
        "resolve ×30 ms: cold={:.0} p50={:.0} p95={:.0} min={:.0} max={:.0}",
        lat[0],
        nearest_rank(&lat, 50.0),
        nearest_rank(&lat, 95.0),
        lat[0],
        lat[lat.len() - 1]
    );

    let rss_after = rss_kib(pid).expect("rss readable");
    println!(
        "sidecar RSS after resolve run: {} KiB ({} MiB)",
        rss_after,
        rss_after / 1024
    );

    // 3. first-play latency: cold resolve → reader open (moov over ranged
    //    HTTP) → first decoded audio. The reader/decode run on a plain
    //    thread: reqwest's blocking client must not be created or dropped
    //    inside the tokio runtime — same rule the engine follows.
    let t = Instant::now();
    let s = source
        .stream(
            &TrackKey {
                source: SourceKind::YouTube,
                id: track.track_id.clone(),
            },
            &StreamRequest::Best,
        )
        .await
        .expect("stream resolves")
        .expect("stream present");
    let resolve_ms = t.elapsed().as_secs_f64() * 1000.0;
    let url = s.url.clone();
    let (open_ms, rms) = std::thread::scope(|scope| {
        scope
            .spawn(move || {
                let t = Instant::now();
                let reader =
                    playback::remote::HttpRangeReader::new(url, vec![]).expect("range reader");
                let open_ms = t.elapsed().as_secs_f64() * 1000.0;
                let rms = playback::probe::decode_rms(reader, Duration::from_millis(250))
                    .expect("decode");
                (open_ms, rms)
            })
            .join()
            .expect("decode thread")
    });
    println!(
        "first-play: resolve {resolve_ms:.0} ms + reader open {open_ms:.0} ms + first 0.25 s decode = {:.0} ms total, RMS {rms:.4}",
        resolve_ms + open_ms
    );
    assert!(
        rms > 0.005,
        "decoded audio is effectively silence (RMS {rms})"
    );
}
