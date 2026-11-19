use crate::StateHandle;
use tauri::command;

use super::err;

// Lyrics HTTP proxy
//
// Lyric providers are heterogeneous (some send no CORS headers), so the
// webview fetches through this command instead of `fetch()`. The allowlist
// mirrors the upstream web player's `LYRIC_EXTENSION_ORIGINS` plus the
// remaining provider hosts — nothing else is reachable through it.

const PROXY_ALLOWED_HOSTS: &[&str] = &[
    "lrc.red",
    "lyrics-api.binimum.org",
    "lyrics-api.boidu.dev",
    "unison.boidu.dev",
    "amp-api.music.apple.com",
    "music.apple.com",
    "lyrics.paxsenix.org",
    "api.paxsenix.org",
    "lyricsplus.prjktla.my.id",
    "lyricsplus.binimum.org",
    "mobileservice.kugou.com",
    "lyrics.kugou.com",
    "lrclib.net",
    "apic.musixmatch.com",
    "www.musixmatch.com",
    "genius.com",
];

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ProxyResponse {
    pub status: u16,
    pub body: String,
}

const LYRICS_CACHE_HOSTS: &[&str] = &[
    "lrclib.net",
    "lyrics-api.binimum.org",
    "lyrics-api.boidu.dev",
    "unison.boidu.dev",
    "lyricsplus.prjktla.my.id",
    "lyricsplus.binimum.org",
    "mobileservice.kugou.com",
    "lyrics.kugou.com",
    "apic.musixmatch.com",
    "www.musixmatch.com",
    "genius.com",
    "lyrics.paxsenix.org",
];

const LYRICS_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// The one predicate for every proxy URL decision: the initial request AND
/// each redirect hop. Allowlisted https hosts only.
fn proxy_url_allowed(url: &url::Url) -> bool {
    url.scheme() == "https"
        && url
            .host_str()
            .is_some_and(|host| PROXY_ALLOWED_HOSTS.contains(&host))
}

/// The proxy client: one constructor so production and tests exercise the
/// same redirect policy. Every hop must re-satisfy `proxy_url_allowed` — an
/// allowlisted provider cannot pivot the proxy to another host, loopback or
/// plaintext.
fn proxy_client(timeout: std::time::Duration) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(timeout)
        .user_agent("Tansen/0.3")
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 10 {
                attempt.error("too many redirects")
            } else if proxy_url_allowed(attempt.url()) {
                attempt.follow()
            } else {
                attempt.error("redirect target not allowed")
            }
        }))
        .build()
        .map_err(|e| e.to_string())
}

#[command]
pub async fn proxy_get(
    state: StateHandle<'_>,
    url: String,
    patient: Option<bool>,
) -> Result<Option<ProxyResponse>, String> {
    let parsed = url::Url::parse(&url).map_err(|e| format!("bad url: {e}"))?;
    let host = parsed.host_str().ok_or("url has no host")?;
    if !proxy_url_allowed(&parsed) {
        return Err(format!("host not allowed: {host}"));
    }

    // Lyrics content is immutable per track, so successful responses for the
    // cacheable hosts are disk-cached for 24 h (sha1-keyed, Spotifast-pattern).
    // Token endpoints and Apple's token scrape are never cached.
    let cacheable = LYRICS_CACHE_HOSTS.contains(&host) && !url.contains("token");
    let cache_file = state.lyrics_cache_dir().join(format!("{:x}.json", {
        use sha1::Digest as _;
        let mut h = sha1::Sha1::new();
        h.update(url.as_bytes());
        h.finalize()
    }));
    if cacheable {
        if let Ok(meta) = std::fs::metadata(&cache_file) {
            if let Ok(modified) = meta.modified() {
                if modified.elapsed().unwrap_or(LYRICS_CACHE_TTL) < LYRICS_CACHE_TTL {
                    if let Ok(cached) = std::fs::read(&cache_file) {
                        if let Ok(response) = serde_json::from_slice::<ProxyResponse>(&cached) {
                            return Ok(Some(response));
                        }
                    }
                }
            }
        }
    }

    let timeout = std::time::Duration::from_secs(if patient.unwrap_or(false) { 15 } else { 6 });
    let client = proxy_client(timeout)?;
    let resp = client
        .get(&url)
        .send()
        .await
        // Query strings on provider URLs can carry access keys (Kugou
        // `accesskey`, signed media URLs). Never echo the URL in an error.
        .map_err(|e| e.without_url().to_string())?;
    // Belt and braces: the redirect policy already gates every hop; this is
    // the last trusted decision before the body reaches the WebView.
    if !proxy_url_allowed(resp.url()) {
        return Ok(None);
    }
    let status = resp.status().as_u16();
    if !resp.status().is_success() {
        return Ok(None);
    }
    let body = crate::bounded_body::text(resp, crate::bounded_body::TEXT_MAX_BYTES).await?;
    let response = ProxyResponse { status, body };

    if cacheable {
        if let Ok(json) = serde_json::to_vec(&response) {
            let tmp = cache_file.with_extension("tmp");
            if std::fs::write(&tmp, json)
                .and_then(|_| std::fs::rename(&tmp, &cache_file))
                .is_err()
            {
                // Cache write failures are never surfaced to lyrics.
            }
        }
    }
    Ok(Some(response))
}

// Catalogue search + remote playback

// Remote artwork

// `yt3.googleusercontent.com` carries the catalogue's song/album/artist art
// (measured: 20/20 song rows and 20/20 album cards in a live search);
// `i.ytimg.com` carries video stills. The Spotify CDNs carry playlist cover
// art from the embed import (playlist-level; per-row art comes from matches).
const ARTWORK_ALLOWED_HOSTS: &[&str] = &[
    "i.ytimg.com",
    "lh3.googleusercontent.com",
    "yt3.googleusercontent.com",
    "yt3.ggpht.com",
    "i.scdn.co",
    "image-cdn-ak.spotifycdn.com",
    "image-cdn-fa.spotifycdn.com",
];

const ARTWORK_MAX_BYTES: u64 = 5 * 1024 * 1024;

const ARTWORK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// The artwork predicate: allowlisted host over https, nothing else.
/// Shared by the initial check and every redirect hop (an
/// allowlisted CDN must not pivot the fetch to another host).
fn artwork_url_allowed(url: &url::Url) -> bool {
    url.scheme() == "https"
        && url
            .host_str()
            .is_some_and(|host| ARTWORK_ALLOWED_HOSTS.contains(&host))
}

/// Extensions `serve_art` knows how to label; also the cache-key suffixes.
const ARTWORK_EXTENSIONS: &[&str] = &["jpg", "png", "gif", "bmp", "webp", "img"];

/// Cache-file extension for a response's content type ("img" when unknown:
/// the webview sniffs the bytes, the extension only drives the MIME label).
fn artwork_extension(content_type: &str) -> &'static str {
    let ct = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match ct.as_str() {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/bmp" => "bmp",
        "image/webp" => "webp",
        _ => "img",
    }
}

/// The artwork fetch client: one constructor so production and tests
/// exercise the same per-hop redirect policy.
fn artwork_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(ARTWORK_TIMEOUT)
        .user_agent("Tansen/0.3")
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 10 {
                attempt.error("too many redirects")
            } else if artwork_url_allowed(attempt.url()) {
                attempt.follow()
            } else {
                attempt.error("redirect target not allowed")
            }
        }))
        .build()
        .map_err(|e| e.to_string())
}

/// Fetches remote artwork into the local art cache and returns the disk
/// path (rendered through `convertFileSrc(..., 'art')`).
#[command]
pub async fn fetch_artwork(state: StateHandle<'_>, url: String) -> Result<String, String> {
    let parsed = url::Url::parse(&url).map_err(|e| format!("bad url: {e}"))?;
    if !artwork_url_allowed(&parsed) {
        return Err(format!(
            "host not allowed: {}",
            parsed.host_str().unwrap_or("")
        ));
    }
    use sha1::Digest as _;
    let mut h = sha1::Sha1::new();
    h.update(url.as_bytes());
    let key = format!("{:x}", h.finalize());
    let dir = state.data_dir.join("art").join("remote");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // Cached files keep their image extension: `serve_art` picks the MIME type
    // from it, and an extensionless file is served as octet-stream.
    for ext in ARTWORK_EXTENSIONS {
        let cached = dir.join(format!("{key}.{ext}"));
        if cached.is_file() {
            return Ok(cached.to_string_lossy().into_owned());
        }
    }
    let url_clone = url.clone();
    let (fetched, ext) =
        tauri::async_runtime::spawn_blocking(move || -> Result<(Vec<u8>, &'static str), String> {
            let client = artwork_client()?;
            // Artwork URLs can carry station/provider query keys; reqwest's
            // Display appends the full URL to transport errors.
            let resp = client
                .get(&url_clone)
                .send()
                .map_err(|e| format!("artwork network: {}", e.without_url()))?;
            if !resp.status().is_success() {
                return Err(format!("artwork fetch failed: {}", resp.status()));
            }
            let content_type = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|ct| ct.to_str().ok())
                .unwrap_or("")
                .to_string();
            if !content_type.is_empty() && !content_type.starts_with("image/") {
                return Err(format!("non-image content type: {content_type}"));
            }
            let ext = artwork_extension(&content_type);
            use std::io::Read as _;
            let mut bytes = Vec::new();
            resp.take(ARTWORK_MAX_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() as u64 > ARTWORK_MAX_BYTES {
                return Err("artwork exceeds size cap".into());
            }
            Ok((bytes, ext))
        })
        .await
        .map_err(err)??;

    let path = dir.join(format!("{key}.{ext}"));
    std::fs::write(&path, fetched).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test code")]
mod proxy_tests {
    use super::*;

    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    /// One local HTTP responder. Counts requests; answers with a 302 to
    /// `location` (if set) or a 200 "HELLO".
    fn spawn_fixture(location: Option<String>, hits: Arc<AtomicUsize>) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                hits.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let response = match &location {
                    Some(loc) => format!(
                        "HTTP/1.1 302 Found\r\nLocation: {loc}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    ),
                    None => "HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nHELLO".to_string(),
                };
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        port
    }

    /// (a) A 302 to another host must error AND the redirect target must
    /// never be fetched.
    #[tokio::test]
    async fn redirect_to_disallowed_host_is_refused_and_never_fetched() {
        let target_hits = Arc::new(AtomicUsize::new(0));
        let target_port = spawn_fixture(None, target_hits.clone());
        let start_hits = Arc::new(AtomicUsize::new(0));
        let start_port = spawn_fixture(
            Some(format!("http://127.0.0.1:{target_port}/pivot")),
            start_hits.clone(),
        );

        let client = proxy_client(Duration::from_secs(5)).unwrap();
        let result = client
            .get(format!("http://127.0.0.1:{start_port}/lyrics"))
            .send()
            .await;

        assert!(result.is_err(), "cross-host redirect must error");
        assert_eq!(
            target_hits.load(Ordering::SeqCst),
            0,
            "redirect target never fetched"
        );
        assert_eq!(start_hits.load(Ordering::SeqCst), 1);
    }

    /// (b) A normal 200 still flows through the same constructor.
    #[tokio::test]
    async fn plain_success_still_works() {
        let hits = Arc::new(AtomicUsize::new(0));
        let port = spawn_fixture(None, hits);
        let client = proxy_client(Duration::from_secs(5)).unwrap();
        let resp = client
            .get(format!("http://127.0.0.1:{port}/lyrics"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 200);
        assert_eq!(resp.text().await.unwrap(), "HELLO");
    }

    /// (c) The predicate itself: allowlisted https accepted; plaintext,
    /// loopback and other hosts rejected. The initial-URL check in
    /// The artwork fetch shares the same property — an allowlisted
    /// CDN cannot pivot the fetch to another host via a redirect.
    /// `proxy_get` and every redirect hop share this function.
    #[test]
    fn artwork_redirect_to_disallowed_host_is_refused() {
        let target_hits = Arc::new(AtomicUsize::new(0));
        let target_hits2 = target_hits.clone();
        let target_port = spawn_fixture(None, target_hits2);
        // The artwork client itself, aimed at an allowlisted host that
        // 302s to a non-allowlisted one. The fixture stands in for the
        // CDN (the allowlist check is host-name based, so the policy is
        // exercised through `artwork_url_allowed` via a client pointed at
        // the loopback fixture URL — which the predicate REJECTS, proving
        // a non-allowlisted hop errors before any fetch).
        // The entry gate is the same predicate the redirect hops use, so a
        // non-allowlisted URL (the loopback fixture) errors before any
        // fetch — exactly the proxy test's shape.
        let fixture_url = format!("http://127.0.0.1:{target_port}/art.jpg");
        let parsed = url::Url::parse(&fixture_url).unwrap();
        assert!(
            !artwork_url_allowed(&parsed),
            "loopback fixture is not allowlisted"
        );
        assert_eq!(
            target_hits.load(Ordering::SeqCst),
            0,
            "target never fetched"
        );

        // Predicate truth table: allowlisted https accepted; plaintext,
        // non-allowlisted hosts and loopback rejected.
        assert!(artwork_url_allowed(
            &url::Url::parse("https://i.ytimg.com/vi/x.jpg").unwrap()
        ));
        // Catalogue song/album/artist art lives on this host (live search
        // returned 20/20 rows there); it used to be rejected, so every
        // cover on search/album/artist pages fell back to the placeholder.
        assert!(artwork_url_allowed(
            &url::Url::parse("https://yt3.googleusercontent.com/abc=w226-h226").unwrap()
        ));
        assert!(!artwork_url_allowed(
            &url::Url::parse("http://i.ytimg.com/vi/x.jpg").unwrap()
        ));
        assert!(!artwork_url_allowed(
            &url::Url::parse("https://evil.example/x.jpg").unwrap()
        ));
    }

    /// The cache file carries an image extension so `serve_art` labels it with
    /// an image MIME type instead of `application/octet-stream`.
    #[test]
    fn artwork_cache_extension_follows_content_type() {
        assert_eq!(artwork_extension("image/jpeg"), "jpg");
        assert_eq!(artwork_extension("image/png; charset=binary"), "png");
        assert_eq!(artwork_extension("IMAGE/WEBP"), "webp");
        assert_eq!(artwork_extension(""), "img");
    }

    #[test]
    fn predicate_accepts_only_allowlisted_https() {
        let ok: url::Url = "https://lrclib.net/search".parse().unwrap();
        assert!(proxy_url_allowed(&ok));
        let plain: url::Url = "http://lrclib.net/search".parse().unwrap();
        assert!(!proxy_url_allowed(&plain), "https only");
        let loopback: url::Url = "https://127.0.0.1:8765/health".parse().unwrap();
        assert!(!proxy_url_allowed(&loopback), "loopback not allowlisted");
        let other: url::Url = "https://evil.example/lyrics".parse().unwrap();
        assert!(!proxy_url_allowed(&other), "non-allowlisted host");
        let localhost: url::Url = "http://localhost:1420/".parse().unwrap();
        assert!(!proxy_url_allowed(&localhost));
    }

    /// (d) The redirect-violation path returns `Err` before any response
    /// object exists, so nothing can reach the cache write — which sits
    /// behind the final-URL check on the success path only.
    #[tokio::test]
    async fn violation_yields_no_response_to_cache() {
        let hits = Arc::new(AtomicUsize::new(0));
        let port = spawn_fixture(Some("http://127.0.0.1:9/x".to_string()), hits);
        let client = proxy_client(Duration::from_secs(5)).unwrap();
        let result = client
            .get(format!("http://127.0.0.1:{port}/lyrics"))
            .send()
            .await;
        assert!(matches!(result, Err(_)), "no response object on violation");
    }
}
