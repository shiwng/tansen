//! Recoverable playback state: observes engine events on a plain thread and
//! persists the queue snapshot so a restart resumes where the user left off.

use persistence::models::TrackRef;
use persistence::views::{self, PlaybackState};
use persistence::Db;
use playback::EngineEvent;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

pub fn spawn_persister(db: Arc<Db>, events: broadcast::Sender<EngineEvent>) {
    let mut rx = events.subscribe();
    std::thread::Builder::new()
        .name("playback-state".into())
        .spawn(move || {
            // Latest known state, updated by events; flushed to SQLite on
            // structural changes and when the position has moved enough.
            let mut tracks: Vec<TrackRef> = Vec::new();
            let mut index: Option<usize> = None;
            let mut repeat = "off".to_string();
            let mut position = 0.0f64;
            let mut dirty = false;
            let mut last_flush = Instant::now();
            let mut last_flushed_pos = 0.0f64;

            // Polling (not awaiting) because this thread must run before the
            // Tauri runtime exists; 50 ms keeps up with the 10 Hz event rate.
            loop {
                match rx.try_recv() {
                    Ok(ev) => match ev {
                        EngineEvent::QueueChanged { snapshot } => {
                            tracks = snapshot.tracks;
                            index = snapshot.index;
                            repeat = format!("{:?}", snapshot.repeat).to_lowercase();
                            dirty = true;
                        }
                        EngineEvent::TrackChanged { track: Some(t) } => {
                            // A preloaded gapless advance can start the next
                            // track without a new queue snapshot; re-derive
                            // the index by identity.
                            if let Some(i) = tracks.iter().position(|x| x.id == t.id && x.path == t.path) {
                                index = Some(i);
                            }
                            dirty = true;
                        }
                        EngineEvent::TrackChanged { track: None } => dirty = true,
                        EngineEvent::Position { sec, .. } => {
                            position = sec;
                            dirty = true;
                        }
                        _ => {}
                    },
                    Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::TryRecvError::Closed) => break,
                    Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {}
                }

                let due = dirty
                    && (last_flush.elapsed() >= Duration::from_millis(800)
                        || (position - last_flushed_pos).abs() >= 2.0);
                if due {
                    let state = PlaybackState {
                        tracks: tracks.clone(),
                        index,
                        position_sec: position,
                        repeat: repeat.clone(),
                    };
                    if let Err(e) = views::save_playback_state(&db, &state, library::scanner::now_secs()) {
                        tracing::warn!(error = %e, "playback state save failed");
                    } else {
                        dirty = false;
                        last_flush = Instant::now();
                        last_flushed_pos = position;
                    }
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        })
        .expect("spawn playback-state persister");
}
