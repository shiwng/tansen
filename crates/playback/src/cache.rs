//! Disk cache for streamed tracks.
//!
//! Spotifast-style idea, own implementation: fully downloaded stream files
//! are kept in a bounded cache directory. The engine plays a cached copy
//! straight from disk (instant, gapless-friendly replay) and the resolve-ahead
//! service fills the cache for the next track while the current one plays.
//!
//! Layout: `<dir>/<sha1(url)>.audio` + `<sha1(url)>.json` (url + size).
//! Complete files only — partial downloads live as `.part` and rename on
//! finish. Eviction is LRU by mtime once the byte limit is exceeded.

use sha1::{Digest, Sha1};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("http: {0}")]
    Http(String),
}

pub struct StreamCache {
    dir: PathBuf,
    limit_bytes: u64,
}

impl StreamCache {
    pub fn new(dir: PathBuf, limit_bytes: u64) -> Self {
        let _ = fs::create_dir_all(&dir);
        Self { dir, limit_bytes }
    }

    fn key(url: &str) -> String {
        let mut h = Sha1::new();
        h.update(url.as_bytes());
        format!("{:x}", h.finalize())
    }

    fn file_for(&self, url: &str) -> PathBuf {
        self.dir.join(format!("{}.audio", Self::key(url)))
    }

    /// Complete cached copy for `url`, if any.
    pub fn get(&self, url: &str) -> Option<PathBuf> {
        let path = self.file_for(url);
        if path.is_file() {
            Some(path)
        } else {
            None
        }
    }

    /// Whether a download for `url` is already in flight or complete.
    pub fn has_or_pending(&self, url: &str) -> bool {
        self.get(url).is_some() || self.part_for(url).exists()
    }

    fn part_for(&self, url: &str) -> PathBuf {
        self.dir.join(format!("{}.part", Self::key(url)))
    }

    /// Downloads `url` into the cache if not already complete. Blocking —
    /// run it on a background thread (the resolve-ahead service does).
    pub fn fetch(&self, url: &str) -> Result<PathBuf, CacheError> {
        let final_path = self.file_for(url);
        if final_path.is_file() {
            return Ok(final_path);
        }
        let part = self.part_for(url);
        // A stale .part from an interrupted run restarts from scratch.
        let _ = fs::remove_file(&part);

        let client = reqwest::blocking::Client::builder()
            .timeout(DOWNLOAD_TIMEOUT)
            .user_agent("Tansen/0.3")
            .build()
            .map_err(|e| CacheError::Http(e.to_string()))?;
        let mut file = fs::File::create(&part)?;
        // blocking::Response implements Read; stream straight to disk.
        let mut response = client
            .get(url)
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|e| CacheError::Http(e.to_string()))?;
        std::io::copy(&mut response, &mut file)
            .map_err(|e| {
                let _ = fs::remove_file(&part);
                CacheError::Http(e.to_string())
            })?;
        file.flush()?;
        drop(file);
        fs::rename(&part, &final_path)?;
        self.evict(&final_path);
        Ok(final_path)
    }

    /// LRU eviction: delete oldest cached files until under the limit.
    /// The file just written is never evicted.
    fn evict(&self, keep: &Path) {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return;
        };
        let mut files: Vec<(PathBuf, u64, std::time::SystemTime)> = entries
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "audio"))
            .filter_map(|e| {
                let meta = e.metadata().ok()?;
                Some((e.path(), meta.len(), meta.modified().ok()?))
            })
            .collect();
        let total: u64 = files.iter().map(|(_, len, _)| len).sum();
        if total <= self.limit_bytes {
            return;
        }
        files.sort_by_key(|(_, _, m)| *m);
        let mut total = total;
        for (path, len, _) in files {
            if total <= self.limit_bytes {
                break;
            }
            if path == keep {
                continue;
            }
            if fs::remove_file(&path).is_ok() {
                total = total.saturating_sub(len);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn server(data: &'static [u8], hits: Arc<AtomicUsize>) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                hits.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0u8; 2048];
                let _ = std::io::Read::read(&mut stream, &mut buf);
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    data.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(data);
            }
        });
        port
    }

    #[test]
    fn fetch_caches_and_serves_replays_from_disk() {
        let data: &'static [u8] = Box::leak(vec![7u8; 256 * 1024].into_boxed_slice());
        let hits = Arc::new(AtomicUsize::new(0));
        let port = server(data, hits.clone());

        let dir = tempfile::tempdir().unwrap();
        let cache = StreamCache::new(dir.path().join("cache"), u64::MAX);
        let url = format!("http://127.0.0.1:{port}/track.m4a");

        assert!(cache.get(&url).is_none());

        let first = cache.fetch(&url).unwrap();
        assert_eq!(first, cache.get(&url).unwrap());
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        let mut disk = Vec::new();
        fs::File::open(&first).unwrap().read_to_end(&mut disk).unwrap();
        assert_eq!(&disk, data);

        // Second fetch: no HTTP hit, served from the cache.
        cache.fetch(&url).unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn eviction_respects_the_byte_limit() {
        let dir = tempfile::tempdir().unwrap();
        let cache = StreamCache::new(dir.path().join("cache"), 1000);
        let a = cache.file_for("https://a/one");
        let b = cache.file_for("https://b/two");
        fs::write(&a, vec![0u8; 600]).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(&b, vec![0u8; 600]).unwrap();

        // `keep` models the file just written: the newest one survives.
        cache.evict(&b);
        assert!(b.exists(), "newest file kept");
        assert!(!a.exists(), "older file evicted");
    }
}
