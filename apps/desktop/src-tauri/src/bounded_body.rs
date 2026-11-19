//! Size-capped response bodies for commands that must read a whole body.
//!
//! `Response::text()` / `Response::bytes()` buffer without a limit, so a
//! hostile or broken server could grow memory until the app is killed. The
//! helpers check `Content-Length` when present and enforce the same cap while
//! accumulating chunks, because a chunked response can omit or lie about it.

/// Default cap for text endpoints (lyrics proxy, Spotify pages, radio JSON).
pub(crate) const TEXT_MAX_BYTES: u64 = 8 * 1024 * 1024;

pub(crate) async fn bytes(response: reqwest::Response, cap: u64) -> Result<Vec<u8>, String> {
    if let Some(len) = response.content_length() {
        if len > cap {
            return Err(format!("response exceeds size cap ({len} > {cap} bytes)"));
        }
    }
    let mut response = response;
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("response body: {}", e.without_url()))?
    {
        body.extend_from_slice(&chunk);
        if body.len() as u64 > cap {
            return Err("response exceeds size cap".into());
        }
    }
    Ok(body)
}

/// Reads a capped body and decodes it lossily as UTF-8. Lossy decoding matches
/// `Response::text()`'s replacement-character behavior while keeping the cap.
pub(crate) async fn text(response: reqwest::Response, cap: u64) -> Result<String, String> {
    let body = bytes(response, cap).await?;
    Ok(String::from_utf8_lossy(&body).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn text_cap_constant_is_sane() {
        assert!(TEXT_MAX_BYTES >= 1024 * 1024);
    }

    fn serve_once(response: &'static [u8], body: &'static [u8]) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut probe = [0u8; 1024];
            let _ = stream.read(&mut probe);
            stream.write_all(response).unwrap();
            stream.write_all(body).unwrap();
            let _ = stream.flush();
        });
        format!("http://{addr}/")
    }

    #[tokio::test]
    async fn content_length_over_cap_is_refused_before_reading() {
        let url = serve_once(
            b"HTTP/1.1 200 OK\r\nContent-Length: 64\r\nConnection: close\r\n\r\n",
            &[b'x'; 64],
        );
        let response = reqwest::get(url).await.unwrap();
        let error = bytes(response, 16).await.unwrap_err();
        assert!(error.contains("size cap"), "{error}");
    }

    #[tokio::test]
    async fn chunked_body_over_cap_is_refused_while_accumulating() {
        // Sixteen 4-byte chunks = 64 bytes with no Content-Length; the cap is
        // enforced on the accumulated length, not on the header.
        let url = serve_once(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
            b"4\r\nxxxx\r\n4\r\nxxxx\r\n4\r\nxxxx\r\n4\r\nxxxx\r\n4\r\nxxxx\r\n4\r\nxxxx\r\n4\r\nxxxx\r\n4\r\nxxxx\r\n4\r\nxxxx\r\n4\r\nxxxx\r\n4\r\nxxxx\r\n4\r\nxxxx\r\n4\r\nxxxx\r\n4\r\nxxxx\r\n4\r\nxxxx\r\n4\r\nxxxx\r\n0\r\n\r\n",
        );
        let response = reqwest::get(url).await.unwrap();
        let error = bytes(response, 16).await.unwrap_err();
        assert!(error.contains("size cap"), "{error}");
    }
}
