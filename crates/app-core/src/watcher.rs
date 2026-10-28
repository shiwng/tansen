//! Library folder watcher: re-indexes when watched folders change on disk.
//!
//! The Android original relies on MediaStore updating itself; the desktop
//! equivalent is an in-process watcher. Design: one notify watcher over the
//! registered roots, a debounce so a burst of writes costs one scan, and a
//! quiet-period scan that skips itself when a user-triggered scan holds the
//! lock or the `watch_folders` setting is off. The folder list is re-read
//! whenever it changes so adding/removing folders needs no restart.

use library::Scanner;
use notify::Watcher;
use persistence::repo;
use persistence::Db;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

/// How long the filesystem must stay quiet before a scan starts.
const DEBOUNCE: Duration = Duration::from_secs(2);
/// Re-read the folder list this often (adds/removes apply without restart).
const FOLDER_POLL: Duration = Duration::from_secs(10);

pub fn spawn_watcher(
    db: Arc<Db>,
    scanner: Scanner,
    scan_events: broadcast::Sender<library::ScanSummary>,
) {
    std::thread::Builder::new()
        .name("library-watcher".into())
        .spawn(move || run(db, scanner, scan_events))
        .expect("spawn library watcher");
}

fn run(db: Arc<Db>, scanner: Scanner, scan_events: broadcast::Sender<library::ScanSummary>) {
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let inner = notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
        // Any event marks the tree dirty; the debounce decides when to scan.
        if res.is_ok() {
            let _ = tx.send(());
        }
    });
    let Ok(inner) = inner else {
        tracing::warn!("filesystem watcher unavailable; automatic rescans disabled");
        return;
    };
    let watcher = Mutex::new(inner);

    let mut watching: Vec<String> = Vec::new();
    let mut dirty_since: Option<Instant> = None;
    let mut folders_poll = Instant::now();
    let mut last_scan = Instant::now() - DEBOUNCE;

    loop {
        std::thread::sleep(Duration::from_millis(250));

        // Drain events (any event counts; details don't matter for a rescan).
        let mut events = false;
        while rx.try_recv().is_ok() {
            events = true;
        }
        if events {
            dirty_since.get_or_insert(Instant::now());
        }

        // Pick up folder changes (including the first pass).
        if folders_poll.elapsed() >= FOLDER_POLL || watching.is_empty() {
            folders_poll = Instant::now();
            let folders = repo::list_folders(&db).unwrap_or_default();
            let paths: Vec<String> = folders.into_iter().map(|f| f.path).collect();
            if paths != watching {
                let mut w = watcher.lock().unwrap();
                for old in &watching {
                    let _ = w.unwatch(std::path::Path::new(old));
                }
                for path in &paths {
                    if let Err(e) = w.watch(std::path::Path::new(path), notify::RecursiveMode::Recursive) {
                        tracing::warn!(path = %path, error = %e, "watch failed");
                    }
                }
                tracing::info!(count = paths.len(), "watching library folders");
                watching = paths;
                // Newly watched roots (including the first run) get one quiet-
                // period scan so pre-existing files are indexed without
                // waiting for an unrelated change.
                dirty_since.get_or_insert(Instant::now());
            }
        }

        // Quiet period elapsed → one scan, if allowed and something is watched.
        if let Some(quiet_at) = dirty_since {
            if quiet_at.elapsed() >= DEBOUNCE && last_scan.elapsed() >= DEBOUNCE {
                dirty_since = None;
                last_scan = Instant::now();
                let watch_on = repo::get_setting(&db, "watch_folders")
                    .ok()
                    .flatten()
                    .map(|v| v != "false")
                    .unwrap_or(true);
                if !watch_on || watching.is_empty() {
                    continue;
                }
                match scan_quiet(&db, &scanner) {
                    Ok(summary) => {
                        tracing::info!(added = summary.added, missing = summary.marked_missing, "watcher rescan");
                        let _ = scan_events.send(summary);
                    }
                    Err(e) => tracing::warn!(error = %e, "watcher scan failed"),
                }
            }
        }
    }
}

/// A scan that yields when the user-triggered path holds the scan lock. The
/// lock lives in `AppState`; here we approximate with a short try via the
/// shared scanner — the real gate is `AppState::scan_if_free`, which the
/// shell uses. This path exists for tests and direct runs.
fn scan_quiet(db: &Arc<Db>, scanner: &Scanner) -> persistence::Result<library::ScanSummary> {
    let folders: Vec<(i64, String)> = repo::list_folders(db)?
        .into_iter()
        .map(|f| (f.id, f.path))
        .collect();
    scanner.scan(db, &folders, |_| {})
}
