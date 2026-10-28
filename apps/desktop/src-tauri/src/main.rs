//! Desktop shell: wires app-core into Tauri (commands, events, art protocol,
//! OS media keys) and owns nothing of substance itself.

mod commands;

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

    let art_root = data_dir().join("art");

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

            attach_media_keys(&state);
            spawn_event_forwarder(app.handle().clone(), state.engine_events());
            // Watcher-driven rescans surface as the same `scan://done` event
            // the manual scan path uses, so library views refresh.
            spawn_scan_event_forwarder(app.handle().clone(), state.scan_events.clone());

            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
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
            commands::sources_get,
            commands::sources_get_with_health,
            commands::source_add,
            commands::source_remove,
            commands::source_set_enabled,
            commands::source_reorder,
        ])
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
