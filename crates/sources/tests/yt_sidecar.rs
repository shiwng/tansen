//! Sidecar round trip: launch the real Kotlin sidecar, search a known song,
//! resolve a progressive AAC stream, download a prefix, and decode five
//! seconds with the playback crate's probe. Asserts the decoded audio is not
//! digital silence — the closest a test gets to "it actually plays".
//!
//! Needs: the sidecar jar (`../BitChord/sidecar/build/libs/sidecar-all.jar`)
//! and network access to YouTube. Skipped by default so `cargo test` stays
//! hermetic; run explicitly with:
//!
//! ```sh
//! cargo test -p sources --test yt_sidecar -- --ignored --nocapture
//! ```

use sources::yt::{fetch_prefix, YtMusicSource};
use sources::{MusicSource, StreamRequest};
use std::path::PathBuf;

#[tokio::test(flavor = "current_thread")]
#[ignore = "needs the built sidecar jar + network access to YouTube"]
async fn sidecar_search_stream_decode_round_trip() {
    let jar = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../BitChord/sidecar/build/libs/sidecar-all.jar");
    if !jar.is_file() {
        panic!("sidecar jar missing at {} — build it with :sidecar:fatJar", jar.display());
    }

    // Launch supervises its own process; dropping the source kills the JVM.
    // Java: JAVA_HOME if the environment carries one, else this dev machine's
    // JDK 17 (the macOS /usr/bin/java stub exits 1 and is correctly refused).
    let java = std::env::var("JAVA_HOME").map(|h| format!("{h}/bin/java")).unwrap_or_else(|_| {
        "/opt/homebrew/opt/openjdk@17/libexec/openjdk.jdk/Contents/Home/bin/java".to_string()
    });
    let source = YtMusicSource::launch(Some(jar), Some(java)).expect("sidecar launches and reports healthy");
    assert!(source.version().is_some());

    // Search a recording that is certainly on YouTube Music.
    let rows = source.search("Tom Sawyer Rush", 10, None).await.expect("search succeeds");
    assert!(!rows.is_empty(), "search returned rows");
    let track = &rows[0];
    assert!(!track.track_id.is_empty());
    println!("search top hit: {} — {}", track.title, track.artist);

    // Resolve a progressive AAC stream through the prefer=aac gate.
    let stream = source
        .stream(&track.track_id, &StreamRequest::Best)
        .await
        .expect("stream resolves")
        .expect("stream exists for the search hit");
    assert_eq!(stream.format.codec, "AAC");
    assert!(stream.url.contains("googlevideo"), "a googlevideo URL: {}", stream.url);
    println!("resolved: itag-format {:?} at {} kbps", stream.format.kbps, stream.format.kbps.unwrap_or(0));

    // Download the whole file (4:37 @ 131 kbps ≈ 4.5 MB; MP4 demuxing wants
    // the real length) and decode it. The probe decodes real samples with
    // Symphonia — no device.
    // The engine's path: a seekable range reader straight over HTTP — no
    // download. The MP4 demuxer seeks for moov; the reader answers with
    // ranged fetches at line rate. The reader uses a blocking HTTP client,
    // which — exactly like in the engine — must be created and dropped off
    // the async runtime, so the decode runs on its own thread.
    let url = stream.url.clone();
    let rms = std::thread::scope(|scope| {
        scope
            .spawn(move || {
                let reader = playback::remote::HttpRangeReader::new(url, vec![])
                    .expect("range reader over the stream URL");
                playback::probe::decode_rms(reader, std::time::Duration::from_secs(5))
            })
            .join()
            .expect("decode thread")
    })
    .expect("5 seconds decode from ranged HTTP");
    println!("decoded RMS (ranged HTTP): {rms:.5}");
    assert!(rms > 0.005, "decoded audio is effectively silence (RMS {rms})");
}
