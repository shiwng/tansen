//! Desktop shell: wires app-core into Tauri (commands, events, art protocol,
//! OS media keys) and owns nothing of substance itself.

mod commands;
mod metrics;

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
            let _ = state.restore_playback();

            // M1 bench hook (env-gated, never active in normal runs): start
            // playing a local file for the 60 s "playing" measurement.
            if let Ok(path) = std::env::var("TANSEN_BENCH_PLAY") {
                use playback::QueueOp;
                let track = persistence::models::TrackRef {
                    id: 0,
                    title: "Bench".into(),
                    artist: String::new(),
                    album: String::new(),
                    duration_sec: 0.0,
                    art_path: None,
                    path,
                    video_id: None,
                };
                state.engine.send(playback::Command::Apply(QueueOp::Load {
                    tracks: vec![track],
                    index: 0,
                    play: true,
                }));
                tracing::info!(target: "bench", stage = "autoplay-started", "bench: playback started");
            }

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

            attach_media_keys(&state);
            spawn_event_forwarder(app.handle().clone(), state.engine_events());
            // Catalogue rows ride placeholders until this loop swaps in real
            // stream URLs, then prefetches the next track into the disk cache.
            spawn_resolve_ahead(app.handle().clone(), state.engine_events(), state.engine_tx(), state.stream_cache());
            // Watcher-driven rescans surface as the same `scan://done` event
            // the manual scan path uses, so library views refresh.
            spawn_scan_event_forwarder(app.handle().clone(), state.scan_events.clone());

            app.manage(state);
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
            commands::player_load,
            commands::player_toggle,
            commands::player_pause,
            commands::player_resume,
            commands::player_stop,
            commands::player_next,
            commands::player_previous,
            commands::player_seek,
            commands::player_volume,
            commands::player_repeat,
            commands::queue_snapshot,
            commands::queue_insert,
            commands::queue_remove,
            commands::queue_jump,
            commands::queue_move,
            commands::queue_clear,
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
            commands::yt_search,
            commands::player_play_remote,
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

/// OS media keys: commands flow into the engine, engine state flows back via
/// the same forwarder that feeds the UI. Attached on the main thread
/// (souvlaki's macOS backend requires it).
fn attach_media_keys(state: &Arc<AppState>) {
    let state_for_handler = state.clone();
    state.media.attach(move |ev: MediaControlEvent| {
        let st = state_for_handler.clone();
        tracing::debug!(?ev, "media key");
        match ev {
            MediaControlEvent::Play => st.resume(),
            MediaControlEvent::Pause => st.pause(),
            MediaControlEvent::Toggle => st.toggle_play(),
            MediaControlEvent::Next => st.next(),
            MediaControlEvent::Previous => st.previous(),
            MediaControlEvent::Stop => st.stop(),
            MediaControlEvent::Seek(dir) => {
                let delta = match dir {
                    SeekDirection::Forward => 10.0,
                    SeekDirection::Backward => -10.0,
                };
                st.seek((st.media.position() + delta).max(0.0));
            }
            MediaControlEvent::SeekBy(dir, d) => {
                let delta = match dir {
                    SeekDirection::Forward => d.as_secs_f64(),
                    SeekDirection::Backward => -d.as_secs_f64(),
                };
                st.seek((st.media.position() + delta).max(0.0));
            }
            MediaControlEvent::SetPosition(pos) => st.seek(pos.0.as_secs_f64()),
            _ => {}
        }
    });
}

/// Engine events → webview + OS media controls. Position updates reach the OS
/// at 2 Hz (throttled inside `Media`), the UI at full rate.
fn spawn_event_forwarder(
    app: tauri::AppHandle,
    events: tokio::sync::broadcast::Sender<playback::EngineEvent>,
) {
    let mut rx = events.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    if let Some(st) = app.try_state::<Arc<AppState>>().map(|s| s.inner().clone()) {
                        match &ev {
                            playback::EngineEvent::TrackChanged { track } => {
                                st.media.update_track(track.as_ref());
                                if track.is_some() {
                                    st.media.set_playing(true);
                                }
                            }
                            playback::EngineEvent::Position { sec, playing, .. } => {
                                st.media.update_playback(*playing, Some(*sec));
                            }
                            playback::EngineEvent::QueueChanged { snapshot } => {
                                st.media.set_playing(snapshot.playing);
                            }
                            _ => {}
                        }
                    }
                    let _ = app.emit("player://event", &ev);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "event forwarder lagged");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

fn spawn_resolve_ahead(
    app: tauri::AppHandle,
    events: tokio::sync::broadcast::Sender<playback::EngineEvent>,
    engine_tx: std::sync::mpsc::Sender<playback::Command>,
    stream_cache: Arc<playback::StreamCache>,
) {
    tauri::async_runtime::spawn_blocking(move || {
        // Cloned out so the closure owns the state for 'static. The resolver
        // reads the cached sidecar handle when the catalogue flag is on; with
        // the flag off the loop stays idle because rows never get placeholders.
        let state = app.state::<Arc<AppState>>().inner().clone();
        app_core::remote::spawn_resolve_ahead(
            events,
            engine_tx,
            Arc::new(move |video_id: &str| {
                let yt = state.yt().map_err(|e| e.to_string())?;
                yt.stream_blocking(video_id).map(|s| s.url)
            }),
            stream_cache,
        );
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
