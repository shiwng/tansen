//! Listening-history recorder: one row per track that starts playing.
//!
//! Runs on a plain OS thread with `blocking_recv` — `AppState::open` happens
//! before Tauri's Tokio runtime exists, so this must not need one.

use persistence::repo;
use playback::EngineEvent;
use std::sync::Arc;
use tokio::sync::broadcast;

pub fn spawn_recorder(db: Arc<persistence::Db>, events: broadcast::Sender<EngineEvent>) {
    let mut rx = events.subscribe();
    std::thread::Builder::new()
        .name("history-recorder".into())
        .spawn(move || loop {
            match rx.blocking_recv() {
                Ok(EngineEvent::TrackChanged { track: Some(t) }) => {
                    // Catalogue rows have no library id to record (id = 0).
                    if t.id > 0 {
                        if let Err(e) = repo::record_play(&db, t.id, library::scanner::now_secs()) {
                            tracing::warn!(error = %e, "history insert failed");
                        }
                    }
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "history recorder lagged");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        })
        .expect("spawn history recorder");
}
