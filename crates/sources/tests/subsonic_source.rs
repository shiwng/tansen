//! Subsonic/OpenSubsonic adapter integration tests against a real local
//! HTTP server (dependency-free `TcpListener` mock): search, stream + lossless
//! detection, auth failure, redirect-to-other-host rejection, oversize
//! rejection, DRM/encrypted rejection, source-failure isolation, and the
//! Resolver fallback integration.

use sources::subsonic::{FixedSecretProvider, SubsonicSource};
use sources::{MusicSource, SourceError, SourceHealth, SourceKind, StreamRequest, TrackKey};

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

/// A tiny HTTP/1.1 mock: handler receives the raw request, returns
/// (status, headers, body). Runs on an OS thread; returns the port.
fn spawn_mock<F>(handler: F) -> u16
where
    F: Fn(&str) -> (u16, Vec<(String, String)>, String) + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).into_owned();
            let (status, headers, body) = handler(&req);
            let mut resp = format!(
                "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n",
                body.len()
            );
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

fn subsonic(port: u16, account: &str) -> SubsonicSource {
    SubsonicSource::new_with_secrets(
        "subsonic:t",
        "Test Subsonic",
        &format!("http://127.0.0.1:{port}"),
        account,
        true, // loopback http allowed for tests
        Arc::new(FixedSecretProvider("hunter2".into())),
    )
    .expect("valid base url")
}

fn key(id: &str) -> TrackKey {
    TrackKey {
        source: SourceKind::Subsonic,
        id: id.into(),
    }
}

const PING_OK: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1"}}"#;
const PING_FAILED: &str = r#"{"subsonic-response":{"status":"failed","error":{"code":40,"message":"Wrong username or password"}}}"#;
const SEARCH_OK: &str = r#"{"subsonic-response":{"status":"ok","searchResult3":{"song":[
  {"id":"s1","title":"Song One","artist":"Artist","album":"Album","duration":180,"coverArt":"al-1"},
  {"id":"s2","title":"Other","artist":"Artist","duration":200,"isVideo":true}
]}}}"#;

fn params_of(req: &str) -> std::collections::HashMap<String, String> {
    let path = req.split(' ').nth(1).unwrap_or("");
    let (_, query) = path.split_once('?').unwrap_or(("", ""));
    query
        .split('&')
        .filter_map(|kv| {
            kv.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
        })
        .collect()
}

#[tokio::test]
async fn search_round_trip_uses_token_auth_and_skips_videos() {
    let port = spawn_mock(|req| {
        let p = params_of(req);
        assert_eq!(p.get("u").map(String::as_str), Some("alice"));
        let t = p.get("t").expect("token").clone();
        assert_eq!(t.len(), 32, "md5 hex token");
        assert!(p.get("s").is_some(), "salt present");
        assert_eq!(p.get("v").map(String::as_str), Some("1.16.1"));
        assert_eq!(p.get("f").map(String::as_str), Some("json"));
        (
            200,
            vec![("Content-Type".into(), "application/json".into())],
            SEARCH_OK.into(),
        )
    });
    let src = subsonic(port, "alice");
    let rows = src.search("song", 10).await.unwrap();
    assert_eq!(rows.len(), 1, "video row skipped");
    assert_eq!(rows[0].track_id, "s1");
    assert_eq!(rows[0].title, "Song One");
    assert_eq!(rows[0].artist, "Artist");
    assert_eq!(rows[0].album.as_deref(), Some("Album"));
    assert_eq!(rows[0].duration_sec, Some(180));
    assert!(rows[0]
        .artwork_url
        .as_deref()
        .unwrap_or("")
        .contains("getCoverArt"));
}

#[tokio::test]
async fn stream_sniffs_flac_and_returns_auth_url() {
    let port = spawn_mock(|req| {
        if req.contains("/rest/stream?") {
            let p = params_of(req);
            assert_eq!(p.get("id").map(String::as_str), Some("s1"));
            assert_eq!(p.get("format").map(String::as_str), Some("raw"));
            assert_eq!(p.get("u").map(String::as_str), Some("alice"));
            (
                200,
                vec![("Content-Type".into(), "audio/flac".into())],
                String::new(),
            )
        } else {
            (404, vec![], String::new())
        }
    });
    let src = subsonic(port, "alice");
    let stream = src
        .stream(&key("s1"), &StreamRequest::Lossless)
        .await
        .unwrap()
        .expect("stream");
    assert_eq!(stream.format.codec, "flac");
    assert_eq!(stream.format.is_lossless, Some(true));
    assert!(stream.url.contains("rest/stream?"));
    assert!(stream.url.contains("id=s1"));
    assert!(stream.url.contains("u=alice"));
    assert!(stream.url.contains("t="), "token embedded in stream url");
}

#[tokio::test]
async fn ping_ok_and_auth_failure_map_to_health() {
    let ok_port = spawn_mock(|_| (200, vec![], PING_OK.into()));
    let health = subsonic(ok_port, "alice").health().await.unwrap();
    assert!(matches!(health, SourceHealth::Ok { .. }));

    let bad_port = spawn_mock(|_| (200, vec![], PING_FAILED.into()));
    let health = subsonic(bad_port, "alice").health().await.unwrap();
    assert!(
        matches!(health, SourceHealth::Rejected { .. }),
        "{health:?}"
    );

    // search on a failing auth surfaces the server's error message.
    let err = subsonic(bad_port, "alice")
        .search("x", 5)
        .await
        .unwrap_err();
    assert!(matches!(err, SourceError::Permanent(m) if m.contains("Wrong username")));
}

#[tokio::test]
async fn redirect_to_another_host_is_rejected_and_never_fetched() {
    let hits = Arc::new(AtomicUsize::new(0));
    let hits2 = hits.clone();
    let target_port = spawn_mock(move |_| {
        hits2.fetch_add(1, Ordering::SeqCst);
        (200, vec![], PING_OK.into())
    });
    let source_port = spawn_mock(move |_| {
        (
            302,
            vec![(
                "Location".into(),
                format!("http://localhost:{target_port}/evil"),
            )],
            String::new(),
        )
    });
    let src = subsonic(source_port, "alice");
    assert!(src.search("x", 5).await.is_err());
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "redirect target never fetched"
    );
}

#[tokio::test]
async fn oversize_search_response_is_rejected() {
    let port = spawn_mock(|_| {
        let big = "x".repeat(4 * 1024 * 1024 + 1);
        (
            200,
            vec![("Content-Type".into(), "application/json".into())],
            big,
        )
    });
    let src = subsonic(port, "alice");
    let err = src.search("x", 5).await.unwrap_err();
    assert!(
        matches!(err, SourceError::Permanent(ref m) if m.contains("too large")),
        "{err:?}"
    );
}

#[tokio::test]
async fn drm_and_encrypted_streams_are_rejected() {
    let drm_port = spawn_mock(|req| {
        if req.contains("/rest/stream?") {
            (
                200,
                vec![("x-subsonic-drm".into(), "true".into())],
                String::new(),
            )
        } else {
            (404, vec![], String::new())
        }
    });
    let err = subsonic(drm_port, "alice")
        .stream(&key("s1"), &StreamRequest::Best)
        .await
        .unwrap_err();
    assert!(
        matches!(err, SourceError::Permanent(ref m) if m.contains("drm")),
        "{err:?}"
    );

    let enc_port = spawn_mock(|req| {
        if req.contains("/rest/stream?") {
            (
                200,
                vec![("Content-Type".into(), "application/x-encrypted".into())],
                String::new(),
            )
        } else {
            (404, vec![], String::new())
        }
    });
    let err = subsonic(enc_port, "alice")
        .stream(&key("s1"), &StreamRequest::Best)
        .await
        .unwrap_err();
    assert!(
        matches!(err, SourceError::Permanent(ref m) if m.contains("encrypted")),
        "{err:?}"
    );
}

#[tokio::test]
async fn subsonic_failure_does_not_affect_other_sources() {
    let down_port = spawn_mock(|_| (200, vec![], PING_FAILED.into()));
    let sub = subsonic(down_port, "alice");
    // A healthy addon speaking its own protocol (manifest + search).
    let ok_port = spawn_mock(|req| {
        if req.contains("/manifest.json") {
            (
                200,
                vec![("Content-Type".into(), "application/json".into())],
                r#"{"id":"mock","name":"Mock Addon","resources":["search","stream"]}"#.into(),
            )
        } else if req.contains("/search?") {
            (
                200,
                vec![("Content-Type".into(), "application/json".into())],
                r#"{"tracks":[{"id":"t1","title":"Song One","artist":"Artist"}]}"#.into(),
            )
        } else {
            (404, vec![], String::new())
        }
    });
    let addon = sources::addon::AddonSource::new(
        "addon-t",
        "Addon",
        format!("http://127.0.0.1:{ok_port}"),
        true,
    )
    .unwrap();
    let sources: Vec<Arc<dyn sources::MusicSource>> = vec![Arc::new(sub), Arc::new(addon)];
    let (rows, outcomes) = sources::search::search_all(&sources, "song", 10).await;
    // The failing Subsonic source contributes nothing but is reported; the
    // healthy addon still contributes rows.
    assert!(outcomes
        .iter()
        .any(|o| matches!(o.status, sources::search::SourceStatus::Error { .. })));
    assert!(!rows.is_empty(), "healthy source unaffected");
}

// integration: resolver fallback onto Subsonic

/// A minimal failing fake pinned source (mirrors the resolver test fake).
struct PinnedFail;
#[async_trait::async_trait]
impl sources::MusicSource for PinnedFail {
    fn config_id(&self) -> &str {
        "pinned"
    }
    fn kind(&self) -> SourceKind {
        SourceKind::YouTube
    }
    fn display_name(&self) -> &str {
        "Pinned"
    }
    async fn health(&self) -> Result<SourceHealth, SourceError> {
        Ok(SourceHealth::Ok { detail: None })
    }
    async fn search(&self, _q: &str, _l: usize) -> Result<Vec<sources::CatalogTrack>, SourceError> {
        Ok(vec![])
    }
    async fn stream(
        &self,
        _k: &TrackKey,
        _r: &StreamRequest,
    ) -> Result<Option<sources::SourceStream>, SourceError> {
        Err(SourceError::Unsupported)
    }
    async fn resolve(
        &self,
        _k: &TrackKey,
        _r: &StreamRequest,
    ) -> Result<Option<sources::ResolvedMedia>, SourceError> {
        Err(SourceError::Permanent("nope".into()))
    }
}

#[tokio::test]
async fn resolver_fallbackbacks_onto_subsonic_when_pinned_fails() {
    use sources::matcher::TrackMeta;
    let port = spawn_mock(|req| {
        if req.contains("/rest/search3?") {
            (
                200,
                vec![("Content-Type".into(), "application/json".into())],
                SEARCH_OK.into(),
            )
        } else if req.contains("/rest/stream?") {
            (
                200,
                vec![("Content-Type".into(), "audio/flac".into())],
                String::new(),
            )
        } else {
            (404, vec![], String::new())
        }
    });
    let sub = Arc::new(subsonic(port, "alice")) as Arc<dyn sources::MusicSource>;
    let pinned = Arc::new(PinnedFail) as Arc<dyn sources::MusicSource>;

    let pinned_for_lookup = pinned.clone();
    let lookup: sources::resolver::SourceLookup =
        Arc::new(move |kind| (kind == SourceKind::YouTube).then(|| pinned_for_lookup.clone()));
    let sub_for_provider = sub.clone();
    let provider: sources::resolver::FallbackProvider =
        Arc::new(move || vec![sub_for_provider.clone()]);
    let meta: sources::resolver::MetaLookup = Arc::new(|_| {
        Some(TrackMeta {
            title: "Song One".into(),
            artist: "Artist".into(),
            duration_sec: Some(180.0),
        })
    });
    let resolver = sources::resolver::Resolver::with_fallbacks(lookup, provider, meta);

    // `resolve_blocking` builds its own runtime, so it must run off the test
    // runtime — exactly how the engine/factory call it (plain thread). The
    // pinned key is YouTube-sourced (the pinned source owns it); the ladder
    // then lands on Subsonic.
    let k = TrackKey {
        source: SourceKind::YouTube,
        id: "s1".into(),
    };
    let media = std::thread::spawn(move || resolver.resolve_blocking(&k, &StreamRequest::Best))
        .join()
        .unwrap()
        .expect("fallback hit");
    assert_eq!(media.source, SourceKind::Subsonic, "source recorded");
    let url = media.url.clone();
    assert!(url.contains("rest/stream?"), "{url}");
}

#[test]
fn download_url_uses_the_original_file_or_a_server_side_transcode() {
    let src = subsonic(1, "alice");

    let original = src.download_url("s1", "high").unwrap();
    assert!(original.path().ends_with("/rest/download"), "{original}");
    let p = params_of(&format!("GET {original} HTTP/1.1"));
    assert_eq!(p.get("id").map(String::as_str), Some("s1"));
    assert_eq!(p.get("u").map(String::as_str), Some("alice"));
    assert_eq!(
        p.get("t").map(String::len),
        Some(32),
        "fresh token per call"
    );
    assert_eq!(p.get("s").map(String::len), Some(16));

    let low = src.download_url("s1", "low").unwrap();
    assert!(low.path().ends_with("/rest/stream"), "{low}");
    let p = params_of(&format!("GET {low} HTTP/1.1"));
    assert_eq!(p.get("format").map(String::as_str), Some("mp3"));
    assert_eq!(p.get("maxBitRate").map(String::as_str), Some("64"));

    let medium = src.download_url("s1", "medium").unwrap();
    let p = params_of(&format!("GET {medium} HTTP/1.1"));
    assert_eq!(p.get("maxBitRate").map(String::as_str), Some("128"));
}
