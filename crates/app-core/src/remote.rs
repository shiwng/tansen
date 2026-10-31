//! Remote-track support: resolving sidecar placeholders ahead of playback.
//!
//! Queue rows for catalogue tracks carry a placeholder path
//! (`sidecar://track/{videoId}/stream?prefer=aac`) so the engine can queue
//! them instantly. A dedicated thread watches engine events and replaces each
//! upcoming placeholder with a real stream URL (`PatchUrl`) — before the
//! engine's gapless preload ever needs it. The audio thread never resolves.

use playback::Command;
use playback::EngineEvent;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

/// How many tracks ahead of the current one to keep resolved.
const AHEAD: usize = 2;
/// Stream URLs from the sidecar live ~6 h; a short cache keeps repeat plays
/// from re-resolving without risking a dead URL.
const CACHE_TTL: Duration = Duration::from_secs(60 * 60);

pub fn is_placeholder(path: &str) -> bool {
    path.starts_with("sidecar://")
}

pub fn placeholder_for(video_id: &str) -> String {
    format!("sidecar://track/{video_id}/stream?prefer=aac")
}

pub fn video_id_of(path: &str) -> Option<&str> {
    path.strip_prefix("sidecar://track/")?.split('/').next()
}

/// Spawns the resolve-ahead loop. `resolve` turns a video id into a stream
/// URL (blocking, on this thread — never the audio thread).
pub fn spawn_resolve_ahead(
    events: broadcast::Sender<EngineEvent>,
    engine_tx: std::sync::mpsc::Sender<Command>,
    resolve: Arc<dyn Fn(&str) -> Result<String, String> + Send + Sync>,
    stream_cache: Arc<playback::StreamCache>,
) {
    let mut rx = events.subscribe();
    std::thread::Builder::new()
        .name("resolve-ahead".into())
        .spawn(move || {
            let mut url_cache: HashMap<String, (Instant, String)> = HashMap::new();
            let mut patching: std::collections::HashSet<usize> = std::collections::HashSet::new();
            loop {
                // QueueChanged carries the whole queue + index; other events
                // are not actionable here. Repeats are made harmless by the
                // cache and by checking what is still a placeholder.
                let snapshot = match rx.blocking_recv() {
                    Ok(EngineEvent::QueueChanged { snapshot }) => snapshot,
                    Ok(_) => continue,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                };

                url_cache.retain(|_, (at, _)| at.elapsed() < CACHE_TTL);
                let start = snapshot.index.unwrap_or(0);
                for (offset, track) in snapshot.tracks.iter().enumerate().skip(start).take(AHEAD + 1) {
                    let Some(video_id) = video_id_of(&track.path) else { continue };
                    if !is_placeholder(&track.path) {
                        continue;
                    }
                    if let Some((_, url)) = url_cache.get(video_id) {
                        let _ = engine_tx.send(Command::PatchUrl { index: offset, url: url.clone() });
                        continue;
                    }
                    match resolve(video_id) {
                        Ok(url) => {
                            url_cache.insert(video_id.to_string(), (Instant::now(), url.clone()));
                            let _ = engine_tx.send(Command::PatchUrl { index: offset, url: url.clone() });
                            // Spotifast-pattern prefetch: the immediate next
                            // track downloads fully into the disk cache while
                            // the current one plays, so its replay (and
                            // gapless preload) comes from disk.
                            if offset == start + 1 && stream_cache.get(&url).is_none() {
                                if let Err(e) = stream_cache.fetch(&url) {
                                    tracing::warn!(error = %e, "prefetch failed");
                                }
                            }
                        }
                        Err(e) => {
                            // Leave the placeholder; the engine's preload
                            // attempt will fail cleanly and the natural
                            // advance will retry.
                            tracing::warn!(video_id, error = %e, "resolve-ahead failed");
                        }
                    }
                }
                patching.clear();
            }
        })
        .expect("spawn resolve-ahead");
}
