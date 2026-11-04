//! F2 real-provider verification (`#[ignore]`; live network + built jar + Java).
//!
//! ```sh
//! cargo test -p sources --test f2_live_providers -- --ignored --nocapture
//! ```
//!
//! Anonymous YouTube Music only (no account, no extraction changes): for 5
//! varied tracks — search → resolve ×10 (cold + 9 warm) → first audio frame
//! → seek → decode — then the killed-sidecar simulation (clean fast error,
//! supervisor recovery). Printed table carries codec/container/bitrate/
//! expiry/latencies. URLs are never printed (legal/secret hygiene).

use playback::probe::decode_rms;
use sources::yt::YtMusicSource;
use sources::{MusicSource, SourceKind, StreamRequest, TrackKey};
use std::io::Seek;
use std::time::{Duration, Instant};

const QUERIES: [&str; 5] = [
    "Tom Sawyer Rush",
    "Take On Me a-ha",
    "Fly Me To The Moon Frank Sinatra",
    "Bach Cello Suite Yo-Yo Ma",
    "Blinding Lights The Weeknd",
];

fn nearest_rank(sorted: &mut [f64], pct: f64) -> f64 {
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    match sorted.len() {
        0 => 0.0,
        1 => sorted[0],
        n => sorted[(((n as f64) * pct).ceil() as usize).clamp(1, n) - 1],
    }
}

#[tokio::test]
#[ignore = "live: needs the built sidecar jar, local Java and network"]
async fn five_tracks_search_resolve_play_verify() {
    let source = YtMusicSource::launch(None, None).expect("sidecar launches (jar + Java + network)");

    println!(
        "| query | codec | container | kbps | expiry_s | resolve cold | p50 | p95 | first-audio | seek-audio |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|");
    for q in QUERIES {
        let outcome: Result<String, String> = async {
            let rows = source.search(q, 5).await.map_err(|e| format!("search failed: {e}"))?;
            let Some(row) = rows.into_iter().next() else {
                return Err("no results".into());
            };
            let key = TrackKey::new(SourceKind::YouTube, row.track_id.clone())
                .map_err(|e| format!("track key: {e}"))?;

        // resolve ×10: call 1 cold (sidecar's first Innertube resolve for the
        // track), 2..10 warm (its own per-track caching).
        let mut lat: Vec<f64> = Vec::new();
        let mut codec = String::new();
        let mut container = String::new();
        let mut kbps = "None".to_string();
        let mut expiry = "None".to_string();
        let mut url_for_audio: Option<String> = None;
        for i in 0..10 {
            let t = Instant::now();
            let stream = source
                .stream(&key, &StreamRequest::Best)
                .await
                .map_err(|e| format!("resolve failed: {e}"))?
                .ok_or("no stream for track")?;
            lat.push(t.elapsed().as_secs_f64() * 1000.0);
            if i == 0 {
                codec = stream.format.codec.clone();
                container = stream.container.map(|c| format!("{c:?}")).unwrap_or_default();
                if let Some(k) = stream.format.kbps {
                    kbps = k.to_string();
                }
                if let Some(e) = stream.expiry_ms {
                    expiry = format!("{e}");
                }
            }
            if i == 9 {
                url_for_audio = Some(stream.url);
            }
        }
        let cold = lat[0];
        let p50 = nearest_rank(&mut lat.clone(), 0.50);
        let p95 = nearest_rank(&mut lat, 0.95);

        // First audio frame: ranged reader over the real googlevideo URL,
        // decode 0.25 s, RMS > 0. (The pipeline reads googlevideo directly —
        // the sidecar is a resolver, not a proxy; M3.)
        let first_audio_ms = {
            // The blocking client must be created/dropped off the async path
            // (same rule yt_measure and the engine follow).
            let url = url_for_audio.clone().unwrap();
            let joined = std::thread::scope(|scope| {
                scope
                    .spawn(move || -> Result<(f64, f32), String> {
                        let t = Instant::now();
                        let reader = playback::remote::HttpRangeReader::new(url, vec![])
                            .map_err(|e| format!("range reader: {e}"))?;
                        let rms = decode_rms(reader, Duration::from_millis(1000))
                            .map_err(|e| format!("decode: {e}"))?;
                        Ok((t.elapsed().as_secs_f64() * 1000.0, rms))
                    })
                    .join()
                    .expect("audio thread join cannot panic")
            });
            let (ms, rms) = joined?;
            if rms < 0.0005 { return Err(format!("first 1 s window near-silent (rms={rms:.6})")); }
            ms
        };
        // Seek: jump 1.5 MB into the stream, decode 0.25 s — audio again.
        let seek_audio_ms = {
            let url = url_for_audio.unwrap();
            let joined = std::thread::scope(|scope| {
                scope
                    .spawn(move || -> Result<(f64, f32), String> {
                        let t = Instant::now();
                        let mut reader = playback::remote::HttpRangeReader::new(url, vec![])
                            .map_err(|e| format!("range reader: {e}"))?;
                        use std::io::Seek as _;
                        std::io::Seek::seek(&mut reader, std::io::SeekFrom::Start(1_500_000))
                            .map_err(|e| format!("seek: {e}"))?;
                        let rms = decode_rms(reader, Duration::from_millis(1000))
                            .map_err(|e| format!("decode: {e}"))?;
                        Ok((t.elapsed().as_secs_f64() * 1000.0, rms))
                    })
                    .join()
                    .expect("seek thread join cannot panic")
            });
            let (ms, rms) = joined?;
            if rms < 0.0005 { return Err(format!("post-seek 1 s window near-silent (rms={rms:.6})")); }
            ms
        };

            Ok(format!(
                "| {q} | {codec} | {container} | {kbps} | {expiry} | {cold:.0} | {p50:.0} | {p95:.0} | {first_audio_ms:.0} | {seek_audio_ms:.0} |"
            ))
        }
        .await;
        match outcome {
            Ok(line) => println!("{line}"),
            Err(e) => println!("| {q} | FAILED: {e} | | | | | | | | |"),
        }
    }

    // ---- Simulations ----
    // (a) Sidecar killed mid-session: the CURRENT track is unaffected (the
    // pipeline streams googlevideo directly; the sidecar only resolves).
    // The NEXT resolve must fail clean and fast (no hang), then recover via
    // the M14 supervisor's backoff restart.
    let Some(pid) = source.pid() else {
        panic!("sidecar pid unknown");
    };
    libc_kill(pid);
    let t = Instant::now();
    let err = source.search("recovery probe", 3).await;
    let elapsed = t.elapsed();
    assert!(err.is_err(), "resolve right after kill fails");
    println!(
        "killed-sidecar: clean error in {:.0} ms ({}) — no hang",
        elapsed.as_secs_f64() * 1000.0,
        err.err().unwrap().to_string().split(" (").next().unwrap_or("")
    );
    // (b) Recovery: the supervisor restarts the JVM through its backoff.
    let mut recovered = None;
    for _ in 0..60 {
        tokio::time::sleep(Duration::from_millis(1000)).await;
        if let Ok(rows) = source.search("recovery probe", 3).await {
            recovered = Some(rows.len());
            break;
        }
    }
    assert!(recovered.is_some(), "supervisor restarts the sidecar");
    println!("killed-sidecar: supervisor recovered the sidecar (search answered {} rows)", recovered.unwrap());
}

/// SIGKILL via libc — the supervisor test's pattern (killed JVM is reaped by
/// the restart probe's try_wait).
fn libc_kill(pid: u32) {
    let rc = unsafe { kill_sys(pid as i32, 9) };
    assert_eq!(rc, 0, "kill succeeds");
}

extern "C" {
    #[link_name = "kill"]
    fn kill_sys(pid: i32, sig: i32) -> i32;
}
