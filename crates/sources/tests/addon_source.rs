//! Addon adapter integration tests against a real local HTTP server (a
//! dependency-free `TcpListener` mock of the manifest/search/stream
//! protocol), plus the health distinctions the sources screen depends on.

use sources::addon::AddonSource;
use sources::{MusicSource, SourceHealth, SourceKind, StreamRequest};

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

/// A tiny HTTP/1.1 server answering a fixed route table. Runs on an OS
/// thread; returns the bound port.
fn mock_server(routes: &'static [(&'static str, u16, &'static str)]) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            handle(stream, routes);
        }
    });
    port
}

fn handle(mut stream: TcpStream, routes: &'static [(&'static str, u16, &'static str)]) {
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).unwrap_or(0);
    let request = String::from_utf8_lossy(&buf[..n]);
    let path = request.split(' ').nth(1).unwrap_or("").to_string();
    let (path_only, _) = path.split_once('?').unwrap_or((&path, ""));
    let entry = routes
        .iter()
        .find(|(p, _, _)| *p == path_only)
        .copied()
        .unwrap_or(("", 404, "{}"));
    let body = entry.2;
    let response = format!(
        "HTTP/1.1 {} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        entry.1,
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes());
}

const MANIFEST_OK: &str = r#"{"id":"mock","name":"Mock Addon","resources":["search","stream"]}"#;
const SEARCH_OK: &str = r#"{"tracks":[
  {"id":"t1","title":"Song One","artist":"Artist","album":"Album","duration":180.2,
   "artworkURL":"https://example.test/art/1.jpg","format":"FLAC","quality":"LOSSLESS",
   "bitrate":900,"sampleRate":44100,"bitDepth":16},
  {"id":"t2","title":"Song Two","artist":"Artist","format":"MP3","bitrate":320}
]}"#;
const STREAM_OK: &str = r#"{"url":"https://cdn.example.test/t1.flac","format":"FLAC","quality":"LOSSLESS","bitrate":900}"#;

#[tokio::test]
async fn health_search_and_stream_round_trip() {
    let port = mock_server(&[
        ("/manifest.json", 200, MANIFEST_OK),
        ("/search", 200, SEARCH_OK),
        ("/stream/t1", 200, STREAM_OK),
    ]);
    let source = AddonSource::new("mock", "Mock Addon", format!("http://127.0.0.1:{port}"));

    // Health: reachable and well-formed.
    assert_eq!(
        source.health().await.unwrap(),
        SourceHealth::Ok { detail: Some("Mock Addon".into()) }
    );

    // Search: rows carry this source's id and their advertised tier.
    let rows = source.search("song", 25, None).await.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].config_id, "mock");
    assert_eq!(rows[0].title, "Song One");
    assert_eq!(rows[0].duration_sec, Some(180));
    let fmt = rows[0].format.as_ref().unwrap();
    assert_eq!(fmt.is_lossless, Some(true));
    assert_eq!(fmt.kbps, Some(900));

    // Stream: the tiered request reaches the URL, the answer maps to a stream.
    let stream = source
        .stream("t1", &StreamRequest::Lossless)
        .await
        .unwrap()
        .expect("stream exists");
    assert_eq!(stream.url, "https://cdn.example.test/t1.flac");
    assert_eq!(stream.format.is_lossless, Some(true));
    assert!(source.kind() == SourceKind::Addon);
    assert_eq!(source.config_id(), "mock");
}

#[tokio::test]
async fn manifest_without_resources_is_rejected_not_unreachable() {
    let port = mock_server(&[("/manifest.json", 200, r#"{"id":"x","resources":["search"]}"#)]);
    let source = AddonSource::new("x", "Half Addon", format!("http://127.0.0.1:{port}"));
    match source.health().await.unwrap() {
        SourceHealth::Rejected { reason } => assert!(reason.contains("does not declare")),
        other => panic!("expected Rejected, got {other:?}"),
    }
}

#[tokio::test]
async fn down_server_is_unreachable() {
    // Port 1 on loopback is never listening; connect fails fast.
    let source = AddonSource::new("down", "Down Addon", "http://127.0.0.1:1");
    match source.health().await.unwrap() {
        SourceHealth::Unreachable { .. } => {}
        other => panic!("expected Unreachable, got {other:?}"),
    }
}

#[tokio::test]
async fn repeated_searches_serve_from_the_cache() {
    // Count hits server-side via an atomic; a second search within the TTL
    // must not reach the server again.
    use std::sync::atomic::{AtomicUsize, Ordering};
    let hits = std::sync::Arc::new(AtomicUsize::new(0));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits2 = hits.clone();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            hits2.fetch_add(1, Ordering::SeqCst);
            handle(stream, &[("/search", 200, SEARCH_OK)]);
        }
    });
    let source = AddonSource::new("cached", "Cached", format!("http://127.0.0.1:{port}"));
    let _ = source.search("a", 25, None).await.unwrap();
    let _ = source.search("a", 25, None).await.unwrap();
    assert_eq!(hits.load(Ordering::SeqCst), 1, "second search served from cache");
}
