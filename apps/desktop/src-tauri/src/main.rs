//! Desktop shell: wires app-core into Tauri (commands, events, art protocol,
//! OS media keys) and owns nothing of substance itself.

mod commands;
mod metrics;
mod rodio_factory;

use app_core::{AppState, MediaControlEvent, SeekDirection};

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use tauri::{Emitter, Manager};
use tracing_subscriber::EnvFilter;

pub type StateHandle<'a> = tauri::State<'a, Arc<AppState>>;

static DATA_DIR: OnceLock<PathBuf> = std::sync::OnceLock::new();

fn data_dir() -> PathBuf {
    DATA_DIR
        .get_or_init(app_core::state::data_dir_default)
        .clone()
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    tracing::info!(target: "bench", stage = "process-start", "bench: process start");

    // M2 instrumentation: per-command IPC counters, logged every 10 s.
    // Debug builds only — release keeps the no-op counter.
    #[cfg(debug_assertions)]
    std::thread::spawn(|| {
        tracing::info!(target: "metrics", "command counters active (10 s windows)");
        loop {
            std::thread::sleep(std::time::Duration::from_secs(10));
            metrics::cmd::log_window();
            tracing::info!(target: "audio-metrics", "{}", playback::metrics::audio::summary());
        }
    });

    let art_root = data_dir().join("art");

    app_core::state::migrate_legacy_data_dir();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .register_uri_scheme_protocol("art", move |_ctx, request| {
            serve_art(&art_root, request.uri().path())
        })
        .setup(|app| {
            let dir = data_dir();
            tracing::info!(dir = %dir.display(), "app data dir");
            let state = Arc::new(AppState::open(&dir).expect("app state init"));

            // M4 stress driver (env-gated, debug only): while playback runs,
            // hammer the backend the way heavy UI use would — repeated
            // library queries every 2 s, a rescan every 30 s, a catalogue
            // search every 30 s when the sidecar is available.
            #[cfg(debug_assertions)]
            if std::env::var("TANSEN_STRESS").as_deref() == Ok("1") {
                let stress_state = state.clone();
                let stress_lib = std::env::var("TANSEN_STRESS_LIB").ok();
                std::thread::spawn(move || {
                    if let Some(lib) = &stress_lib {
                        let _ = stress_state.add_folder(lib);
                        tracing::info!(target: "stress", lib = %lib, "stress library registered");
                    }
                    let mut round: u64 = 0;
                    loop {
                        std::thread::sleep(std::time::Duration::from_secs(2));
                        round += 1;
                        let _ = stress_state.tracks();
                        let _ = persistence::views::albums(&stress_state.db);
                        let _ = persistence::views::history(&stress_state.db, 50);
                        if round % 15 == 1 {
                            if stress_lib.is_some() {
                                let scan_state = stress_state.clone();
                                std::thread::spawn(move || {
                                    let _ = scan_state.scan(|_| {});
                                });
                                tracing::info!(target: "stress", round, "scan started");
                            }
                        }
                        if round % 15 == 8 {
                            match stress_state.yt() {
                                Ok(yt) => {
                                    let _ = yt.search(&format!("stress {round}"), 5);
                                }
                                Err(_) => {} // catalogue off: nothing to search
                            }
                        }
                    }
                });
            }

            // Packaged builds ship the sidecar jar as a bundle resource
            // (Tauri keeps the config-relative directory: Resources/resources/).
            let bundled = app
                .path()
                .resource_dir()
                .ok()
                .map(|d| d.join("resources").join("sidecar-all.jar"));
            if let Some(jar) = bundled.filter(|j| j.is_file()) {
                tracing::info!(jar = %jar.display(), "bundled sidecar jar");
                state.set_sidecar_hint(jar);
            }

            // Watcher-driven rescans surface as the same `scan://done` event
            // the manual scan path uses, so library views refresh.
            spawn_scan_event_forwarder(app.handle().clone(), state.scan_events.clone());

            // M10: the task-based playback engine is THE playback path. The
            // factory resolves entry ids against the live library (and the
            // bench sentinel through its env hook) and shares the stream disk
            // cache for http(s) sources.
            let resolve_state = state.clone();
            let resolve = Box::new(move |id: u64| -> Option<String> {
                if id == u64::MAX {
                    return std::env::var("TANSEN_BENCH_PLAY").ok();
                }
                resolve_state
                    .tracks()
                    .ok()?
                    .into_iter()
                    .find(|t| t.id as u64 == id && !t.missing)
                    .map(|t| t.path)
            });
            let engine = tauri::async_runtime::block_on(async {
                app_core::playback_engine::Engine::start(Arc::new(rodio_factory::RodioFactory::new(
                    resolve,
                    Some(state.stream_cache()),
                )))
            });

            // Every watch change → `playback_state` event to the frontend,
            // plus OS Now Playing metadata and one play_history row per local
            // track start (library lookups refuse hash entries).
            let mut engine_rx = engine.subscribe();
            let emitter = app.handle().clone();
            let media_state = state.clone();
            tauri::async_runtime::spawn(async move {
                let mut last_entry: Option<app_core::playback_types::EntryId> = None;
                while engine_rx.changed().await.is_ok() {
                    let st = engine_rx.borrow().clone();
                    if st.entry != last_entry {
                        last_entry = st.entry;
                        let track = st
                            .entry
                            .filter(|e| e.0 != 0)
                            .and_then(|e| persistence::repo::get_track(&media_state.db, e.0 as i64).ok().flatten())
                            .filter(|t| !t.missing)
                            .map(persistence::models::TrackRef::from);
                        media_state.media.update_track(track.as_ref());
                        if st.status != app_core::playback_types::Status::Idle {
                            if let Some(id) = st.entry.filter(|e| e.0 != 0) {
                                let _ = persistence::repo::record_play(
                                    &media_state.db,
                                    id.0 as i64,
                                    library::scanner::now_secs(),
                                );
                            }
                        }
                    }
                    media_state
                        .media
                        .update_playback(st.anchor.rate == 1, Some(st.anchor.pos_ms as f64 / 1000.0));
                    let _ = emitter.emit("playback_state", &st);
                }
            });

            // M1 bench hook (env-gated, never active in normal runs): start
            // playing the fixture for the 60 s "playing" measurement.
            if std::env::var("TANSEN_BENCH_PLAY").is_ok() {
                use app_core::playback_types::{Command, EntryId, Placement};
                tauri::async_runtime::block_on(async {
                    engine
                        .send(Command::Enqueue { tracks: vec![EntryId(u64::MAX)], placement: Placement::Append })
                        .await;
                    engine.send(Command::Play { entry: EntryId(u64::MAX) }).await;
                });
                tracing::info!(target: "bench", stage = "autoplay-started", "bench: playback started");
            }

            attach_media_keys(state.clone(), engine.clone());
            app.manage(state);
            app.manage(engine);
            tracing::info!("playback engine started");
            Ok(())
        })
        .on_page_load(|webview, payload| {
            tracing::info!(target: "bench", event = ?payload.event(), url = ?webview.url().ok().map(|u| u.to_string()), "page load");
        })
        .invoke_handler({
            // M2: one wrapper counts every command invocation (debug builds
            // only; no-op in release). The generated handler is pinned to a
            // boxed trait object so the wrapper closure can capture it.
            let handler: Box<
                dyn Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync,
            > = Box::new(tauri::generate_handler![
            commands::settings_get,
            commands::settings_set,
            commands::folder_add,
            commands::folder_remove,
            commands::folders_get,
            commands::tracks_get,
            commands::scan_start,
            commands::playlist_create,
            commands::playlists_get,
            commands::playlist_tracks_get,
            commands::playlist_tracks_set,
            commands::albums_get,
            commands::artists_get,
            commands::album_tracks_get,
            commands::artist_tracks_get,
            commands::playlist_add_tracks,
            commands::playlist_rename,
            commands::playlist_delete,
            commands::history_get,
            commands::proxy_get,
            commands::bench_marker,
            commands::engine_command,
            commands::yt_search,
            commands::sources_get,
            commands::sources_get_with_health,
            commands::source_add,
            commands::source_remove,
            commands::source_set_enabled,
            commands::source_reorder,
            ]);
            move |invoke| {
                metrics::cmd::hit(invoke.message.command());
                handler(invoke)
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// OS media keys: commands flow into the task-based engine; engine state
/// flows back via the same task that feeds the UI. Attached on the main
/// thread (souvlaki's macOS backend requires it). Relative seeks anchor on
/// the last published position — updates arrive per state change, not on a
/// timer, so long plays drift by the seconds since the last publish.
fn attach_media_keys(state: Arc<AppState>, engine: app_core::playback_engine::Engine) {
    use app_core::playback_types::{Command, Status};
    let state_for_keys = state.clone();
    state.media.attach(move |ev: MediaControlEvent| {
        let engine = engine.clone();
        let snap = engine.subscribe().borrow().clone();
        let current = snap.entry.filter(|e| e.0 != 0);
        let seek_to = |sec: f64| engine.try_send(Command::Seek { ms: (sec.max(0.0) * 1000.0) as u64 });
        tracing::debug!(?ev, "media key");
        match ev {
            MediaControlEvent::Play => {
                if snap.status == Status::Paused {
                    engine.try_send(Command::Resume);
                } else if let Some(e) = current {
                    engine.try_send(Command::Play { entry: e });
                }
            }
            MediaControlEvent::Pause => {
                engine.try_send(Command::Pause);
            }
            MediaControlEvent::Toggle => match snap.status {
                Status::Playing => {
                    engine.try_send(Command::Pause);
                }
                Status::Paused => {
                    engine.try_send(Command::Resume);
                }
                _ => {
                    if let Some(e) = current {
                        engine.try_send(Command::Play { entry: e });
                    }
                }
            },
            MediaControlEvent::Next => {
                engine.try_send(Command::Next);
            }
            MediaControlEvent::Previous => {
                engine.try_send(Command::Prev);
            }
            MediaControlEvent::Stop => {
                engine.try_send(Command::ClearQueue);
            }
            MediaControlEvent::Seek(dir) => {
                let delta = match dir {
                    SeekDirection::Forward => 10.0,
                    SeekDirection::Backward => -10.0,
                };
                seek_to(state_for_keys.media.position() + delta);
            }
            MediaControlEvent::SeekBy(dir, d) => {
                let delta = match dir {
                    SeekDirection::Forward => d.as_secs_f64(),
                    SeekDirection::Backward => -d.as_secs_f64(),
                };
                seek_to(state_for_keys.media.position() + delta);
            }
            MediaControlEvent::SetPosition(pos) => {
                seek_to(pos.0.as_secs_f64());
            }
            _ => {}
        }
    });
}

fn spawn_scan_event_forwarder(
    app: tauri::AppHandle,
    events: tokio::sync::broadcast::Sender<library::ScanSummary>,
) {
    let mut rx = events.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(summary) => {
                    let _ = app.emit("scan://done", summary);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
            }
        }
    });
}

/// `art://` protocol: serves cached artwork files so the webview can display
/// embedded art without opening the whole filesystem via the asset protocol.
fn serve_art(art_root: &std::path::Path, uri_path: &str) -> tauri::http::Response<Vec<u8>> {
    use tauri::http::Response;
    let path = uri_path.trim_start_matches('/');
    let decoded = percent_encoding::percent_decode_str(path).decode_utf8_lossy().into_owned();
    let file = PathBuf::from(&decoded);
    if !file.starts_with(art_root) || !file.is_file() {
        return Response::builder().status(404).body(Vec::new()).unwrap();
    }
    let mime = match file.extension().and_then(|e| e.to_str()) {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("bmp") => "image/bmp",
        _ => "application/octet-stream",
    };
    match std::fs::read(&file) {
        Ok(bytes) => Response::builder()
            .status(200)
            .header("Content-Type", mime)
            .header("Cache-Control", "max-age=86400")
            .body(bytes)
            .unwrap(),
        Err(_) => Response::builder().status(500).body(Vec::new()).unwrap(),
    }
}
