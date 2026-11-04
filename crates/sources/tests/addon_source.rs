//! Addon adapter integration tests against a real local HTTP server (a
//! dependency-free `TcpListener` mock of the manifest/search/stream
//! protocol), plus the health distinctions the sources screen depends on.

use sources::addon::AddonSource;
use sources::{MusicSource, SourceHealth, SourceKind, StreamRequest, TrackKey};

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

/// A tiny HTTP/1.1 server answering a fixed route table. Runs on an OS
/// thread; returns the bound port.
fn mock_server(routes: &'static [(&'static str, u16, &'static str)]) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
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

/// M13: the default `resolve` on the catalogue trait maps a `stream`
/// answer into a `ResolvedMedia`; the addon protocol carries no MIME or
/// expiry, so those stay unknown while url/headers flow through.
#[tokio::test]
async fn resolve_maps_stream_to_resolved_media() {
    let port = mock_server(&[("/stream/t1", 200, STREAM_OK)]);
    let source = AddonSource::new("mock", "Mock Addon", &format!("http://127.0.0.1:{port}"), true).unwrap();
    let media = source.resolve(&TrackKey { source: SourceKind::Addon, id: "t1".into() }, &StreamRequest::Best).await.unwrap().expect("resolved");
    assert_eq!(media.url, "https://cdn.example.test/t1.flac");
    assert!(media.headers.is_empty());
    assert_eq!(media.expiry, None);
    assert_eq!(media.length, None);
    assert_eq!(media.mime, None);
}

#[tokio::test]
async fn health_search_and_stream_round_trip() {
    let port = mock_server(&[
        ("/manifest.json", 200, MANIFEST_OK),
        ("/search", 200, SEARCH_OK),
        ("/stream/t1", 200, STREAM_OK),
    ]);
    let source = AddonSource::new("mock", "Mock Addon", &format!("http://127.0.0.1:{port}"), true).unwrap();

    // Health: reachable and well-formed.
    assert_eq!(
        source.health().await.unwrap(),
        SourceHealth::Ok { detail: Some("Mock Addon".into()) }
    );

    // Search: rows carry this source's id and their advertised tier.
    let rows = source.search("song", 25).await.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].config_id, "mock");
    assert_eq!(rows[0].title, "Song One");
    assert_eq!(rows[0].duration_sec, Some(180));
    let fmt = rows[0].format.as_ref().unwrap();
    assert_eq!(fmt.is_lossless, Some(true));
    assert_eq!(fmt.kbps, Some(900));

    // Stream: the tiered request reaches the URL, the answer maps to a stream.
    let stream = source
        .stream(&TrackKey { source: SourceKind::Addon, id: "t1".into() }, &StreamRequest::Lossless)
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
    let source = AddonSource::new("x", "Half Addon", &format!("http://127.0.0.1:{port}"), true).unwrap();
    match source.health().await.unwrap() {
        SourceHealth::Rejected { reason } => assert!(reason.contains("does not declare")),
        other => panic!("expected Rejected, got {other:?}"),
    }
}

#[tokio::test]
async fn down_server_is_unreachable() {
    // Port 1 on loopback is never listening; connect fails fast.
    let source = AddonSource::new("down", "Down Addon", "http://127.0.0.1:1", true).unwrap();
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
        for mut stream in listener.incoming().flatten() {
            hits2.fetch_add(1, Ordering::SeqCst);
            handle(stream, &[("/search", 200, SEARCH_OK)]);
        }
    });
    let source = AddonSource::new("cached", "Cached", &format!("http://127.0.0.1:{port}"), true).unwrap();
    let _ = source.search("a", 25).await.unwrap();
    let _ = source.search("a", 25).await.unwrap();
    assert_eq!(hits.load(Ordering::SeqCst), 1, "second search served from cache");
}

// ---- F3 contract tests (dynamic mock: per-request handler) ----------------

use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::sync::Arc as StdArc;

/// A dynamic mock: the handler receives the raw request line and answers
/// with (status, headers, body). Runs on an OS thread; returns the port.
fn mock_dynamic<F>(handler: F) -> u16
where
    F: Fn(&str) -> (u16, Vec<(String, String)>, String) + Send + 'static,
{
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).into_owned();
            let (status, headers, body) = handler(&req);
            let mut resp = format!("HTTP/1.1 {status} S\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
            for (k, v) in headers {
                resp.push_str(&format!("{k}: {v}\r\n"));
            }
            resp.push_str("\r\n");
            resp.push_str(&body);
            let _ = stream.write_all(resp.as_bytes());
        }
    });
    port
}

fn manifest_route(body: &'static str) -> impl Fn(&str) -> (u16, Vec<(String, String)>, String) {
    move |req| {
        if req.contains("/manifest.json") {
            (200, vec![], body.to_string())
        } else {
            (404, vec![], "{}".into())
        }
    }
}

const STREAM_FULL: &str = r#"{"url":"https://cdn.example.test/t1.flac","codec":"FLAC","container":"mp4","manifest":"none","quality":"LOSSLESS","bitrate":900,"sampleRate":44100,"bitDepth":16,
  "headers":{"Authorization":"Bearer xyz","X-Drop-Me":"nope"}}"#;

#[tokio::test]
async fn stream_contract_fields_quality_params_and_header_allowlist() {
    let seen: StdArc<AtomicUsize> = StdArc::new(AtomicUsize::new(0));
    let seen2 = seen.clone();
    let port = mock_dynamic(move |req| {
        seen2.fetch_add(1, AtomicOrdering::SeqCst);
        if req.contains("/manifest.json") {
            (200, vec![], MANIFEST_OK.into())
        } else if req.contains("/stream/t1?quality=") {
            assert!(req.contains("quality=HIGH") || req.contains("quality=LOSSLESS") || req.contains("quality=LOW"), "contract quality values: {req}");
            (200, vec![], STREAM_FULL.into())
        } else {
            (404, vec![], "{}".into())
        }
    });
    let source = AddonSource::new("mock", "Mock Addon", &format!("http://127.0.0.1:{port}"), true).unwrap();
    let k = TrackKey { source: SourceKind::Addon, id: "t1".into() };
    let s = source.stream(&k, &StreamRequest::Best).await.unwrap().unwrap();
    assert_eq!(s.format.codec, "FLAC");
    assert_eq!(s.format.is_lossless, Some(true), "lossless by codec");
    assert_eq!(s.format.sample_rate, Some(44100));
    assert_eq!(s.format.bit_depth, Some(16));
    assert_eq!(s.format.kbps, Some(900));
    // Header allowlist: Authorization kept, X-Drop-Me dropped.
    assert!(s.headers.iter().any(|(k, _)| k == "Authorization"));
    assert!(!s.headers.iter().any(|(k, _)| k == "X-Drop-Me"));
    // Tier mapping: Lossless => LOSSLESS, Capped => LOW (cache-busted by key).
    let _ = source.stream(&k, &StreamRequest::Lossless).await.unwrap().unwrap();
    let _ = source.stream(&k, &StreamRequest::Capped { max_kbps: 64 }).await.unwrap().unwrap();
    assert!(seen.load(AtomicOrdering::SeqCst) >= 3);
}

#[tokio::test]
async fn encrypted_stream_refused() {
    let body = r#"{"encrypted":true}"#;
    let port = mock_dynamic(move |req| {
        if req.contains("/manifest.json") {
            (200, vec![], MANIFEST_OK.into())
        } else {
            (200, vec![], body.into())
        }
    });
    let source = AddonSource::new("mock", "M", &format!("http://127.0.0.1:{port}"), true).unwrap();
    let k = TrackKey { source: SourceKind::Addon, id: "t1".into() };
    let err = source.stream(&k, &StreamRequest::Best).await.unwrap_err();
    assert!(matches!(err, sources::SourceError::Encrypted), "{err:?}");
}

#[tokio::test]
async fn hls_and_dash_manifests_refused_as_unsupported() {
    for m in ["hls", "dash"] {
        let body = format!(r#"{{"url":"https://cdn.example.test/x.m3u8","manifest":"{m}"}}"#);
        let port = mock_dynamic(move |req| {
            if req.contains("/manifest.json") {
                (200, vec![], MANIFEST_OK.into())
            } else {
                (200, vec![], body.clone())
            }
        });
        let source = AddonSource::new("mock", "M", &format!("http://127.0.0.1:{port}"), true).unwrap();
        let k = TrackKey { source: SourceKind::Addon, id: "t1".into() };
        let err = source.stream(&k, &StreamRequest::Best).await.unwrap_err();
        assert!(matches!(err, sources::SourceError::Unsupported), "{m}: {err:?}");
    }
}

#[tokio::test]
async fn in_band_error_surfaces_permanent() {
    let port = mock_dynamic(|req| {
        if req.contains("/manifest.json") {
            (200, vec![], MANIFEST_OK.into())
        } else {
            (200, vec![], r#"{"error":"no such track"}"#.into())
        }
    });
    let source = AddonSource::new("mock", "M", &format!("http://127.0.0.1:{port}"), true).unwrap();
    let k = TrackKey { source: SourceKind::Addon, id: "t1".into() };
    let err = source.stream(&k, &StreamRequest::Best).await.unwrap_err();
    assert!(matches!(err, sources::SourceError::Permanent(ref m) if m.contains("no such track")), "{err:?}");
}

#[tokio::test]
async fn redirect_to_other_host_rejected_and_never_fetched() {
    let hits = StdArc::new(AtomicUsize::new(0));
    let hits2 = hits.clone();
    let target_port = mock_dynamic(move |_| {
        hits2.fetch_add(1, AtomicOrdering::SeqCst);
        (200, vec![], "{}".into())
    });
    let port = mock_dynamic(move |req| {
        if req.contains("/manifest.json") {
            (200, vec![], MANIFEST_OK.into())
        } else {
            (302, vec![("Location".into(), format!("http://localhost:{target_port}/evil"))], String::new())
        }
    });
    let source = AddonSource::new("mock", "M", &format!("http://127.0.0.1:{port}"), true).unwrap();
    let k = TrackKey { source: SourceKind::Addon, id: "t1".into() };
    assert!(source.stream(&k, &StreamRequest::Best).await.is_err());
    assert_eq!(hits.load(AtomicOrdering::SeqCst), 0, "redirect target never fetched");
}

#[tokio::test]
async fn oversize_response_rejected() {
    let big = "x".repeat(4 * 1024 * 1024 + 64);
    let port = mock_dynamic(move |req| {
        if req.contains("/manifest.json") {
            (200, vec![], MANIFEST_OK.into())
        } else {
            (200, vec![], big.clone())
        }
    });
    let source = AddonSource::new("mock", "M", &format!("http://127.0.0.1:{port}"), true).unwrap();
    let k = TrackKey { source: SourceKind::Addon, id: "t1".into() };
    let err = source.stream(&k, &StreamRequest::Best).await.unwrap_err();
    assert!(matches!(err, sources::SourceError::Permanent(ref m) if m.contains("too large")), "{err:?}");
}

#[tokio::test]
#[ignore = "slow: sleeps 13 s to cross the 12 s client timeout"]
async fn timeout_is_clean_not_hanging() {
    let port = mock_dynamic(move |req| {
        if req.contains("/manifest.json") {
            (200, vec![], MANIFEST_OK.into())
        } else {
            std::thread::sleep(std::time::Duration::from_secs(13));
            (200, vec![], "{}".into())
        }
    });
    let source = AddonSource::new("mock", "M", &format!("http://127.0.0.1:{port}"), true).unwrap();
    let k = TrackKey { source: SourceKind::Addon, id: "t1".into() };
    let t = std::time::Instant::now();
    assert!(source.stream(&k, &StreamRequest::Best).await.is_err());
    assert!(t.elapsed() < std::time::Duration::from_secs(14), "bounded by the client timeout");
}

#[tokio::test]
async fn bad_json_manifest_and_stream_are_protocol_errors() {
    let port = mock_dynamic(|_| (200, vec![], "{not json".into()));
    let source = AddonSource::new("mock", "M", &format!("http://127.0.0.1:{port}"), true).unwrap();
    assert!(matches!(
        source.health().await,
        Err(sources::SourceError::Protocol(_)) | Ok(sources::SourceHealth::Rejected { .. })
    ));
    let k = TrackKey { source: SourceKind::Addon, id: "t1".into() };
    assert!(source.stream(&k, &StreamRequest::Best).await.is_err());
}

#[tokio::test]
async fn id_and_query_caps_enforced_before_any_request() {
    // Down address: the caps must fire before any connection attempt.
    let source = AddonSource::new("down", "D", "http://127.0.0.1:1", true).unwrap();
    let long_id = "a".repeat(129);
    let k = TrackKey { source: SourceKind::Addon, id: long_id };
    let err = source.stream(&k, &StreamRequest::Best).await.unwrap_err();
    assert!(matches!(err, sources::SourceError::Permanent(ref m) if m.contains("out of bounds")), "{err:?}");
    let long_query = "q".repeat(201);
    let err = source.search(&long_query, 5).await.unwrap_err();
    assert!(matches!(err, sources::SourceError::Permanent(ref m) if m.contains("query too long")), "{err:?}");
}

#[tokio::test]
async fn addon_failure_falls_back_to_next_source() {
    // Pinned addon on a 500-server; a healthy fallback answers.
    let fail_port = mock_dynamic(|_| (500, vec![], "{}".into()));
    let pinned = AddonSource::new("pinned", "P", &format!("http://127.0.0.1:{fail_port}"), true).unwrap();
    let fallback = AddonSource::new("fb", "F", &format!("http://127.0.0.1:1"), true).unwrap();
    // The fallback must answer, so serve it from a working mock.
    let ok_port = mock_dynamic(move |req| {
        if req.contains("/manifest.json") {
            (200, vec![], MANIFEST_OK.into())
        } else if req.contains("/stream/") {
            (200, vec![], STREAM_OK.into())
        } else {
            (200, vec![], SEARCH_OK.into())
        }
    });
    let ok_addon = AddonSource::new("fbok", "F", &format!("http://127.0.0.1:{ok_port}"), true).unwrap();
    let _ = fallback; // the failing pinned is the one the resolver pins

    let pinned_dyn: StdArc<dyn sources::MusicSource> = StdArc::new(pinned);
    let pinned_for_lookup = pinned_dyn.clone();
    let lookup: sources::resolver::SourceLookup = StdArc::new(move |kind| {
        (kind == SourceKind::Addon).then(|| pinned_for_lookup.clone())
    });
    let fb: StdArc<dyn sources::MusicSource> = StdArc::new(ok_addon);
    let fb2 = fb.clone();
    let provider: sources::resolver::FallbackProvider = StdArc::new(move || vec![fb2.clone()]);
    // Meta matching the SEARCH_OK row so the ladder's matcher can land.
    let meta: sources::resolver::MetaLookup = StdArc::new(|_| {
        Some(sources::matcher::TrackMeta {
            title: "Song One".into(),
            artist: "Artist".into(),
            duration_sec: Some(180.2),
        })
    });
    let resolver = sources::resolver::Resolver::with_fallbacks(lookup, provider, meta);
    let k = TrackKey { source: SourceKind::Addon, id: "t1".into() };
    // resolve_blocking builds its own runtime — plain thread, as the engine does.
    let media = std::thread::spawn(move || resolver.resolve_blocking(&k, &StreamRequest::Best))
        .join()
        .unwrap()
        .expect("fallback hit");
    assert_eq!(media.source, SourceKind::Addon, "the healthy fallback answered");
}
