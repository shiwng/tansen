//! HTTP range reader: a `Read + Seek` over a remote file, fetching fixed-size
//! chunks with `Range` requests.
//!
//! Why ranges: googlevideo serves unbounded responses at a crawl (~15 kB/s,
//! measured upstream) while ranged requests run at line rate — the same
//! finding that produced the phone's `ChunkedDataSource`. The MP4 demuxer
//! seeks (it hunts for `moov`), so a seekable reader is what lets playback
//! start before the whole file is on disk.

use std::io::{Read, Seek, SeekFrom};
use std::time::Duration;

const CHUNK: u64 = 512 * 1024;
const TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, thiserror::Error)]
pub enum RangeError {
    #[error("http: {0}")]
    Http(String),
    #[error("server ignored range requests (status {0})")]
    NoRangeSupport(u16),
}

pub struct HttpRangeReader {
    url: String,
    client: reqwest::blocking::Client,
    /// Absolute position of the next byte `read` would return.
    pos: u64,
    /// Total length from the Content-Range of the probe, when known.
    len: Option<u64>,
    /// Fetched window; `buf_start` is its absolute start position.
    buf: Vec<u8>,
    buf_start: u64,
    headers: Vec<(String, String)>,
}

impl HttpRangeReader {
    /// Creates a blocking HTTP client internally — like rodio's decoders,
    /// this type must be constructed and dropped off the async runtime (the
    /// engine's audio thread qualifies).
    pub fn new(url: impl Into<String>, headers: Vec<(String, String)>) -> Result<Self, RangeError> {
        let mut reader = Self {
            url: url.into(),
            client: reqwest::blocking::Client::builder()
                .timeout(TIMEOUT)
                .user_agent("Tansen/0.3")
                .build()
                .map_err(|e| RangeError::Http(e.to_string()))?,
            pos: 0,
            len: None,
            buf: Vec::new(),
            buf_start: 0,
            headers,
        };
        reader.fetch_chunk(); // probe: first chunk + length
        Ok(reader)
    }

    fn fetch_chunk(&mut self) {
        let start = self.pos;
        let end = start + CHUNK - 1;
        let mut request = self.client.get(&self.url).header("Range", format!("bytes={start}-{end}"));
        for (name, value) in &self.headers {
            request = request.header(name, value);
        }
        let response = match request.send() {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(url = %self.url, error = %e, "range fetch failed");
                return;
            }
        };
        match response.status().as_u16() {
            206 => {
                if let Some(range) = response.headers().get("Content-Range").and_then(|v| v.to_str().ok()) {
                    // "bytes start-end/total"
                    if let Some(total) = range.rsplit('/').next().and_then(|t| t.parse::<u64>().ok()) {
                        self.len = Some(total);
                    }
                }
            }
            200 => {
                // Server ignored the range header. Not expected from
                // googlevideo; play it safe and refuse rather than pulling a
                // throttled unbounded body into the audio thread.
                tracing::warn!(url = %self.url, status = 200, "no range support");
                return; // empty buffer; read() hits EOF and the track errors
            }
            status => {
                tracing::warn!(url = %self.url, status, "range fetch refused");
                return;
            }
        }
        match response.bytes() {
            Ok(bytes) => {
                self.buf_start = start;
                self.buf = bytes.to_vec();
            }
            Err(e) => tracing::warn!(url = %self.url, error = %e, "range body read failed"),
        }
    }
}

impl Read for HttpRangeReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        // Past the end (or nothing fetched): EOF.
        if let Some(len) = self.len {
            if self.pos >= len {
                return Ok(0);
            }
        }
        // Ensure the buffer covers the current position.
        if self.buf.is_empty() || self.pos < self.buf_start || self.pos >= self.buf_start + self.buf.len() as u64 {
            self.fetch_chunk();
            if self.buf.is_empty() {
                return Ok(0); // fetch failed; treat as EOF so the track errors cleanly
            }
        }
        let offset = (self.pos - self.buf_start) as usize;
        let n = out.len().min(self.buf.len() - offset);
        out[..n].copy_from_slice(&self.buf[offset..offset + n]);
        self.pos += n as u64;
        if n == 0 {
            return Ok(0);
        }
        Ok(n)
    }
}

impl Seek for HttpRangeReader {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let target = match pos {
            SeekFrom::Start(n) => n as i64,
            SeekFrom::Current(n) => self.pos as i64 + n,
            SeekFrom::End(n) => match self.len {
                Some(len) => len as i64 + n,
                None => {
                    // Force one fetch to learn the length before seeking to End.
                    self.fetch_chunk();
                    match self.len {
                        Some(len) => len as i64 + n,
                        None => {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::Unsupported,
                                "seek from End without a known length",
                            ))
                        }
                    }
                }
            },
        };
        self.pos = target.max(0) as u64;
        // Position outside the buffer is fine — the next read fetches it.
        Ok(self.pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read as _;
    use std::net::TcpListener;

    /// A ranged HTTP server over a fixed byte blob (206 + Content-Range).
    fn range_server(data: &'static [u8]) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut buf = [0u8; 4096];
                let n = std::io::Read::read(&mut &stream, &mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]);
                let range = request
                    .lines()
                    .find(|l| l.to_ascii_lowercase().starts_with("range:"))
                    .and_then(|l| l.split("bytes=").nth(1))
                    .map(|s| s.trim().to_string());
                let (status, body, extra) = match range.as_deref().and_then(parse_range) {
                    Some((a, b)) if a < data.len() as u64 => {
                        let b = (b.min(data.len() as u64 - 1)) as usize;
                        let a = a as usize;
                        ("206 Partial Content", &data[a..=b], format!("Content-Range: bytes {}-{}/{}\r\n", a, b, data.len()))
                    }
                    _ => ("200 OK", data, String::new()),
                };
                let response = format!(
                    "HTTP/1.1 {}\r\nContent-Type: application/octet-stream\r\n{}Accept-Ranges: bytes\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    status,
                    extra,
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(body);
            }
        });
        port
    }

    fn parse_range(spec: &str) -> Option<(u64, u64)> {
        let (a, b) = spec.split_once('-')?;
        Some((a.parse().ok()?, b.parse().ok()?))
    }

    use std::io::Write as _;

    #[test]
    fn read_seek_and_length_across_chunks() {
        // 1.5 MiB of a recognizable pattern: more than one 512 KiB chunk.
        let data: Vec<u8> = (0..1_572_864u32).map(|i| (i % 251) as u8).collect();
        let boxed: &'static [u8] = Box::leak(data.into_boxed_slice());
        let port = range_server(boxed);
        let mut reader = HttpRangeReader::new(format!("http://127.0.0.1:{port}/audio.mp4"), vec![]).unwrap();

        assert_eq!(reader.len, Some(boxed.len() as u64));

        // Sequential read crosses the first chunk boundary.
        let mut out = Vec::new();
        reader.read_to_end(&mut out).unwrap();
        assert_eq!(out.len(), boxed.len());
        assert_eq!(out[600_000], boxed[600_000]);
        assert_eq!(*out.last().unwrap(), *boxed.last().unwrap());

        // Seek back into the middle of a previous chunk and read.
        reader.seek(SeekFrom::Start(700_000)).unwrap();
        let mut mid = [0u8; 16];
        reader.read_exact(&mut mid).unwrap();
        assert_eq!(&mid, &boxed[700_000..700_016]);

        // SeekFrom::End and Current.
        reader.seek(SeekFrom::End(-4)).unwrap();
        let mut tail = [0u8; 4];
        reader.read_exact(&mut tail).unwrap();
        assert_eq!(&tail, &boxed[boxed.len() - 4..]);
        reader.seek(SeekFrom::Current(-2)).unwrap();
        let mut two = [0u8; 2];
        reader.read_exact(&mut two).unwrap();
        assert_eq!(&two, &boxed[boxed.len() - 2..]);
    }

    /// The engine path end to end: decode audio served ONLY over range
    /// requests — no download, no device. Hermetic: the blob is the committed
    /// test WAV.
    #[test]
    fn decodes_audio_from_a_ranged_http_stream() {
        let wav = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/testmusic/Test Signal/test.wav"),
        )
        .expect("fixture");
        let wav: &'static [u8] = Box::leak(wav.into_boxed_slice());
        let port = range_server(wav);

        let reader = HttpRangeReader::new(format!("http://127.0.0.1:{port}/test.wav"), vec![]).unwrap();
        let rms = crate::probe::decode_rms(reader, Duration::from_secs(2)).expect("decodes");
        assert!(rms > 0.005, "decoded remote audio is silence (RMS {rms})");
    }
}
