//! Desktop shell: wires app-core into Tauri (commands, events, art protocol,
//! OS media keys) and owns nothing of substance itself.


/// S13 bench helper: the first search row's key, or a placeholder when the
/// search came back empty.
#[cfg(debug_assertions)]
fn row_key_of(rows: &[sources::search::SearchRow]) -> sources::TrackKey {
    rows.first()
        .map(|r| r.key.clone())
        .unwrap_or(sources::TrackKey {
            source: sources::SourceKind::YouTube,
            id: "zzzNoSuchTrack00000".into(),
        })
}

mod autoplay;
mod commands;
#[cfg(target_os = "macos")]
mod platform_macos;
// F9 self-test driver: debug-only (it calls the cfg-gated sink peak meter).
#[cfg(debug_assertions)]
mod e2e;
mod metrics;
mod rodio_factory;

use app_core::AppState;

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use tauri::{Emitter, Manager};
use tracing_subscriber::EnvFilter;

pub type StateHandle<'a> = tauri::State<'a, Arc<AppState>>;

static DATA_DIR: OnceLock<PathBuf> = std::sync::OnceLock::new();

fn data_dir() -> PathBuf {
    // Debug harnesses (E2E, stress, bench) must never touch the real
    // library — they polluted it once (31 /private/tmp rows). Scratch dir
    // instead, debug only.
    #[cfg(debug_assertions)]
    if std::env::var_os("TANSEN_E2E").is_some()
        || std::env::var_os("TANSEN_STRESS").is_some()
        || std::env::var_os("TANSEN_BENCH_PLAY").is_some()
    {
        return std::env::temp_dir().join("tansen-harness-data");
    }
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
            sources::metrics::source::log_window();
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
                state.set_sidecar_hint(jar.clone());
                let stored_jar = persistence::repo::get_setting(&state.db, "sidecar_jar").ok().flatten();
                if stored_jar.as_deref().map_or(true, |p| stale_bundled_path(p, &jar)) {
                    let _ = persistence::repo::set_setting(
                        &state.db,
                        "sidecar_jar",
                        &jar.to_string_lossy(),
                    );
                }
            }
            // F7: the bundled jlink JRE makes YouTube work on a clean Mac.
            let bundled_java = app
                .path()
                .resource_dir()
                .ok()
                .map(|d| d.join("resources").join("jre").join("bin").join("java"));
            if let Some(java) = bundled_java.filter(|j| j.is_file()) {
                tracing::info!(java = %java.display(), "bundled sidecar JRE");
                state.set_java_hint(java.clone());
                // Stamp the path so registry::build_active (which reads the
                // setting directly) finds the bundled runtime too. A real
                // user-chosen java_path is never overwritten; another build's
                // bundled JRE is stale and is.
                let stored_java = persistence::repo::get_setting(&state.db, "java_path").ok().flatten();
                if stored_java.as_deref().map_or(true, |p| stale_bundled_path(p, &java)) {
                    let _ = persistence::repo::set_setting(
                        &state.db,
                        "java_path",
                        &java.to_string_lossy(),
                    );
                }
            }

            // Watcher-driven rescans surface as the same `scan://done` event
            // the manual scan path uses, so library views refresh.
            spawn_scan_event_forwarder(app.handle().clone(), state.scan_events.clone());

            // M10: the task-based playback engine is THE playback path. The
            // factory resolves entry ids against the live library (and the
            // bench sentinel through its env hook) and shares the stream disk
            // cache for http(s) sources.
            let resolve_state = state.clone();
            let resolve = Box::new(move |id: u64| -> Option<sources::ResolvedMedia> {
                if id == u64::MAX {
                    return std::env::var("TANSEN_BENCH_PLAY")
                        .ok()
                        .map(|url| sources::ResolvedMedia {
                            url,
                            headers: vec![],
                            expiry: None,
                            length: None,
                            mime: None,
                            format: None,
                            container: None,
                            source: sources::SourceKind::Local,
                            below_request: false,
                        });
                }
                resolve_state
                    .tracks()
                    .ok()?
                    .into_iter()
                    .find(|t| t.id as u64 == id && !t.missing)
                    .map(|t| sources::ResolvedMedia {
                        url: t.path,
                        headers: vec![],
                        expiry: None,
                        length: None,
                        mime: None,
                        format: None,
                        container: None,
                        source: sources::SourceKind::Local,
                        below_request: false,
                    })
            });
            // S3: the source resolver — wired into the factory so keyed
            // (source) entries resolve through it. The lookup is catalogue-
            // gated: disabled/unavailable source kinds return None → the
            // resolver answers SourceError::Disabled.
            let resolver_state = state.clone();
            let source_lookup: std::sync::Arc<
                dyn Fn(sources::SourceKind) -> Option<std::sync::Arc<dyn sources::MusicSource>>
                    + Send
                    + Sync,
            > = std::sync::Arc::new(move |kind| {
                if kind != sources::SourceKind::YouTube {
                    return None;
                }
                let yt = resolver_state.yt().ok()?;
                Some(yt.source.clone())
            });
            // S7: fallback ladder — other ENABLED sources in rank order
            // (addons by user rank, then the memoized sidecar handle). Built
            // lazily per fallback trigger; the sidecar is never launched
            // eagerly here.
            let fallback_state = state.clone();
            let fallbacks: std::sync::Arc<
                dyn Fn() -> Vec<std::sync::Arc<dyn sources::MusicSource>> + Send + Sync,
            > = std::sync::Arc::new(move || {
                let mut out: Vec<std::sync::Arc<dyn sources::MusicSource>> = Vec::new();
                if let Ok(configs) = sources::registry::load_configs(&fallback_state.db) {
                    let mut addons: Vec<_> = configs
                        .into_iter()
                        .filter(|c| c.enabled && c.kind == sources::SourceKind::Addon)
                        .collect();
                    addons.sort_by_key(|c| c.rank);
                    for c in addons {
                        // F3: an addon whose base URL fails policy is skipped
                        // (a bad entry must not break the ladder).
                        if let Ok(a) = sources::addon::AddonSource::new(
                            c.config_id,
                            c.name,
                            c.base_url,
                            c.allow_http,
                        ) {
                            out.push(std::sync::Arc::new(a) as std::sync::Arc<dyn sources::MusicSource>);
                        }
                    }
                }
                if let Ok(yt) = fallback_state.yt() {
                    out.push(yt.source.clone());
                }
                out
            });
            // S7: target metadata for fallback matching comes from the
            // remote-track table (written by the single writer before the
            // command was sent).
            let meta_state = state.clone();
            let meta_lookup: std::sync::Arc<
                dyn Fn(&sources::TrackKey) -> Option<sources::matcher::TrackMeta> + Send + Sync,
            > = std::sync::Arc::new(move |key| {
                let kind_str = match key.source {
                    sources::SourceKind::YouTube => "youtube",
                    sources::SourceKind::Addon => "addon",
                    sources::SourceKind::Subsonic => "subsonic",
                    sources::SourceKind::Local => "local",
                };
                persistence::repo::get_remote_track_by_key(&meta_state.db, kind_str, &key.id)
                    .ok()
                    .flatten()
                    .map(|r| sources::matcher::TrackMeta {
                        title: r.title,
                        artist: r.artist,
                        duration_sec: Some(r.duration_sec),
                    })
            });
            // S12: prefer a local library file for Lossless/Best before any
            // remote resolution. Read-only: db_ro only, and the file must
            // exist and be readable or resolution falls through to remote.
            let db_ro_for_local = state.db_ro.clone();
            let local_match = Arc::new(
                move |meta: &sources::matcher::TrackMeta| -> Option<sources::ResolvedMedia> {
                    let tracks = persistence::repo::list_tracks(&db_ro_for_local).ok()?;
                    let hit = tracks.into_iter().find(|t| {
                        sources::matcher::matches(
                            meta,
                            &sources::matcher::TrackMeta {
                                title: t.title.clone(),
                                artist: t.artist.clone(),
                                duration_sec: Some(t.duration_sec),
                            },
                        )
                    })?;
                    std::fs::File::open(&hit.path).ok()?;
                    Some(sources::ResolvedMedia {
                        url: hit.path.clone(),
                        headers: vec![],
                        expiry: None,
                        length: None,
                        mime: None,
                        format: Some(sources::StreamFormat {
                            codec: hit.codec.clone(),
                            is_lossless: sources::StreamFormat::is_lossless(&hit.codec),
                            kbps: None,
                            is_dolby_atmos: false,
                            sample_rate: None,
                            bit_depth: None,
                            kbps_inferred: false,
                        }),
                        container: None,
                        source: sources::SourceKind::Local,
                        below_request: false,
                    })
                },
            );
            let resolver = std::sync::Arc::new(
                sources::resolver::Resolver::with_fallbacks(source_lookup, fallbacks, meta_lookup)
                    .with_local_match(local_match),
            );

            // F5: the stored quality ceiling feeds the factory at startup
            // (default Best). quality_set updates it live for future resolves.
            let startup_quality = persistence::repo::get_setting(&state.db, "audio_quality")
                .ok()
                .flatten()
                .and_then(|raw| serde_json::from_str(&format!("\"{raw}\"")).ok())
                .unwrap_or(sources::AudioQuality::Best)
                .to_stream_request();
            let factory = Arc::new(rodio_factory::RodioFactory::with_resolver(
                resolve,
                Some(resolver),
                Some(state.stream_cache()),
                startup_quality,
            ));
            let engine = tauri::async_runtime::block_on(async {
                app_core::playback_engine::Engine::start(factory.clone() as Arc<_>)
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
                        // One play_history row per LOCAL track start (S6:
                        // remote rows are source-keyed, not library rows —
                        // recording them would violate the play_history FK).
                        if st.status != app_core::playback_types::Status::Idle {
                            if let Some(id) = st.entry.filter(|e| e.0 != 0) {
                                let is_local = persistence::repo::get_track(
                                    &media_state.db,
                                    id.0 as i64,
                                )
                                .ok()
                                .flatten()
                                .is_some();
                                if is_local {
                                    let _ = persistence::repo::record_play(
                                        &media_state.db,
                                        id.0 as i64,
                                        library::scanner::now_secs(),
                                    );
                                }
                            }
                        }
                    }
                    let _ = emitter.emit("playback_state", &st);
                }
            });

            // M1 bench hook (env-gated, never active in normal runs): start
            // playing the fixture for the 60 s "playing" measurement.
            if std::env::var("TANSEN_BENCH_PLAY").is_ok() {
                use app_core::playback_types::{Command, EntryId, Placement};
                tauri::async_runtime::block_on(async {
                    engine
                        .send(Command::Enqueue { tracks: vec![EntryId(u64::MAX)], remote: vec![], placement: Placement::Append })
                        .await;
                    engine.send(Command::Play { entry: EntryId(u64::MAX), remote: None }).await;
                });
                tracing::info!(target: "bench", stage = "autoplay-started", "bench: playback started");
                // F0: user-facing seek latency (debug builds only). Waits
                // for Playing, sends Seek{5000}, and logs the wall time
                // until the engine publishes the seeked position.
                #[cfg(debug_assertions)]
                {
                    let mut rx = engine.subscribe();
                    let seek_engine = engine.clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = tokio::time::timeout(std::time::Duration::from_secs(30), async {
                            loop {
                                if rx.changed().await.is_err() { return; }
                                if rx.borrow().status == app_core::playback_types::Status::Playing {
                                    break;
                                }
                            }
                        });
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        let t0 = std::time::Instant::now();
                        seek_engine.send(app_core::playback_types::Command::Seek { ms: 5000 }).await;
                        let _ = tokio::time::timeout(std::time::Duration::from_secs(10), async {
                            loop {
                                if rx.changed().await.is_err() { return; }
                                if rx.borrow().anchor.pos_ms >= 4000 { break; }
                            }
                        });
                        tracing::info!(target: "bench", stage = "seek", latency_ms = t0.elapsed().as_millis() as u64, "seek published");
                    });
                }
            }

            // S11: autoplay — app-crate task, event-driven only. The flag
            // mirrors the stored setting (default off); the command channel
            // feeds the task's queue mirror.
            let autoplay_on = Arc::new(std::sync::atomic::AtomicBool::new(
                persistence::repo::get_setting(&state.db, autoplay::AUTOPLAY_SETTING)
                    .ok()
                    .flatten()
                    .as_deref()
                    == Some("true"),
            ));
            let (autoplay_tx, autoplay_rx) =
                tokio::sync::mpsc::unbounded_channel::<app_core::playback_types::Command>();
            let yt_source_for_related = state.clone();
            let related = Arc::new(move |kind: String, key_id: String| {
                let state = yt_source_for_related.clone();
                Box::pin(async move {
                    if kind != "youtube" {
                        return Ok(vec![]);
                    }
                    let Ok(key) = sources::TrackKey::new(sources::SourceKind::YouTube, key_id)
                    else {
                        return Ok(vec![]);
                    };
                    match state.yt() {
                        Ok(yt) => yt.source.related(&key).await.map_err(|e| e.to_string()),
                        // Catalogue off: nothing to add, never an error.
                        Err(_) => Ok(vec![]),
                    }
                }) as std::pin::Pin<
                    Box<dyn std::future::Future<Output = Result<Vec<sources::yt::DiscoveryTrack>, String>> + Send>,
                >
            });
            let engine_for_enqueue = engine.clone();
            let state_for_enqueue = state.clone();
            let emitter_for_autoplay = app.handle().clone();
            let enqueue = Arc::new(move |rows: Vec<sources::yt::DiscoveryTrack>| {
                let engine = engine_for_enqueue.clone();
                let state = state_for_enqueue.clone();
                let emitter = emitter_for_autoplay.clone();
                Box::pin(async move {
                    use app_core::playback_types::{Command, EntryId, Placement};
                    let mut minted: Vec<(EntryId, String)> = Vec::new();
                    let mut payload: Vec<autoplay::AutoplayRow> = Vec::new();
                    for row in rows {
                        let duration = row.duration_sec.map(|d| d as f64).unwrap_or(0.0);
                        let artwork = row
                            .thumbnail_url
                            .clone()
                            .filter(|a| a.starts_with("https://"));
                        // The DB row id is the EntryId (same identity path
                        // the frontend's own enqueues use).
                        let upsert = tauri::async_runtime::spawn_blocking({
                            let state = state.clone();
                            let row = row.clone();
                            let artwork = artwork.clone();
                            move || {
                                persistence::repo::upsert_remote_track(
                                    &state.db,
                                    "youtube",
                                    &row.key.id,
                                    &row.title,
                                    &row.artist,
                                    row.album.as_deref(),
                                    duration,
                                    artwork.as_deref(),
                                )
                            }
                        })
                        .await;
                        let Ok(Ok(row_id)) = upsert else { continue };
                        let engine_id = row_id as u64 + app_core::playback_types::REMOTE_ID_OFFSET;
                        minted.push((EntryId(engine_id), row.key.id.clone()));
                        payload.push(autoplay::AutoplayRow {
                            engine_id,
                            video_id: row.key.id,
                            title: row.title,
                            artist: row.artist,
                            album: row.album,
                            duration_sec: row.duration_sec.map(|d| d as f64),
                            thumbnail_url: artwork,
                        });
                    }
                    if !minted.is_empty() {
                        let remote = minted
                            .iter()
                            .map(|(id, vid)| (*id, ("youtube".to_string(), vid.clone())))
                            .collect();
                        engine
                            .send(Command::Enqueue {
                                tracks: minted.iter().map(|(id, _)| *id).collect(),
                                remote,
                                placement: Placement::Append,
                            })
                            .await;
                        // The frontend's queue mirror only refreshes ids it
                        // knows — announce the appended rows.
                        let _ = emitter.emit("autoplay_enqueued", &payload);
                    }
                    minted
                }) as std::pin::Pin<
                    Box<dyn std::future::Future<Output = Vec<(app_core::playback_types::EntryId, String)>> + Send>,
                >
            });
            tauri::async_runtime::spawn(autoplay::run_autoplay(
                engine.subscribe(),
                autoplay_rx,
                autoplay::AutoplayDeps {
                    enabled: autoplay_on.clone(),
                    related,
                    enqueue,
                },
            ));
            app.manage(autoplay::AutoplayCmds(autoplay_tx));
            app.manage(autoplay::AutoplayFlag(autoplay_on));

            #[cfg(target_os = "macos")]
            platform_macos::start(engine.clone(), state.clone())
                .unwrap_or_else(|e| tracing::warn!(error = %e, "macOS Now Playing unavailable"));
            app.manage(state.clone());
            app.manage(engine.clone());
            app.manage(factory.clone());

            // F9 Phase 4: debug-only self-test driver (TANSEN_E2E=1). The
            // same command paths the UI uses; compiled out of release.
            #[cfg(debug_assertions)]
            e2e::maybe_run(&engine, factory.clone(), state.clone());
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
            commands::queue_tracks_get,
            commands::remote_track_upsert,
            commands::yt_search,
            commands::search_tracks,
            commands::fetch_artwork,
            commands::discovery_suggest,
            commands::discovery_search,
            commands::discovery_album,
            commands::discovery_artist,
            commands::discovery_playlist,
            commands::discovery_continuation,
            commands::discovery_home,
            commands::discovery_home_more,
            commands::discovery_explore,
            commands::discovery_radio,
            commands::sources_get,
            commands::sources_get_with_health,
            commands::source_reorder,
            commands::stream_info_get,
            commands::autoplay_get,
            commands::autoplay_set,
            commands::quality_get,
            commands::quality_set,
            commands::source_add,
            commands::subsonic_add,
            commands::source_remove,
            commands::source_set_enabled,
            ]);
            move |invoke| {
                metrics::cmd::hit(invoke.message.command());
                handler(invoke)
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|handle, event| {
            // The sidecar JVM is a child process: exiting must take it along,
            // or every relaunch leaves an orphaned listener behind.
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = handle.try_state::<Arc<AppState>>() {
                    state.shutdown_sidecar();
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
/// Whether a stored sidecar/JRE path is another build's bundled resource (a
/// dev `target/` dir or a trashed `.app`): those self-heal to the running
/// bundle. A genuine user-chosen JDK or jar outside `resources/` is kept.
fn stale_bundled_path(stored: &str, bundled: &std::path::Path) -> bool {
    let path = std::path::Path::new(stored);
    if path == bundled {
        return false;
    }
    !path.is_file() || path.components().any(|c| c.as_os_str() == "resources")
}

fn serve_art(art_root: &std::path::Path, uri_path: &str) -> tauri::http::Response<Vec<u8>> {
    use tauri::http::Response;
    let path = uri_path.trim_start_matches('/');
    let decoded = percent_encoding::percent_decode_str(path).decode_utf8_lossy().into_owned();
    let file = PathBuf::from(&decoded);
    // Containment is judged on canonical paths: `Path::starts_with` alone
    // is a lexical component-prefix match, so an art-root-prefixed path
    // with embedded `..` passed the old check while the OS resolved the
    // traversal at read time. Canonicalizing both sides (which also
    // resolves symlinks) closes both routes.
    let (Ok(real_file), Ok(real_root)) = (file.canonicalize(), art_root.canonicalize()) else {
        return Response::builder().status(404).body(Vec::new()).unwrap();
    };
    if !real_file.starts_with(&real_root) || !real_file.is_file() {
        return Response::builder().status(404).body(Vec::new()).unwrap();
    }
    let mime = match real_file.extension().and_then(|e| e.to_str()) {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("bmp") => "image/bmp",
        Some("webp") => "image/webp",
        _ => "application/octet-stream",
    };
    match std::fs::read(&real_file) {
        Ok(bytes) => Response::builder()
            .status(200)
            .header("Content-Type", mime)
            .header("Cache-Control", "max-age=86400")
            .body(bytes)
            .unwrap(),
        Err(_) => Response::builder().status(500).body(Vec::new()).unwrap(),
    }
}

#[cfg(test)]
mod path_tests {
    use super::stale_bundled_path;
    use std::path::Path;

    #[test]
    fn stale_build_paths_self_heal_but_user_choices_do_not() {
        let bundled = Path::new("/Applications/Tansen.app/Contents/Resources/resources/sidecar-all.jar");
        // A dev build's or a trashed app's own resource is stale.
        assert!(stale_bundled_path(
            "/Users/x/Tansen/target/debug/resources/sidecar-all.jar",
            bundled
        ));
        assert!(stale_bundled_path(
            "/Users/x/.Trash/Tansen.app/Contents/Resources/resources/jre/bin/java",
            bundled
        ));
        // Missing file is stale; the bundled path itself and a user JDK are not.
        assert!(stale_bundled_path("/nonexistent/java", bundled));
        assert!(!stale_bundled_path(bundled.to_str().unwrap(), bundled));
        assert!(!stale_bundled_path("/opt/homebrew/opt/openjdk/bin/java", bundled));
    }
}

#[cfg(test)]
mod art_tests {
    use super::serve_art;
    use std::fs;
    use std::path::PathBuf;

    /// Root with one real cached artwork; returns (root, inner file path).
    fn setup() -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("tansen-art-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let inner = root.join("abc.png");
        fs::write(&inner, b"\x89PNG fake").unwrap();
        (root, inner)
    }

    /// Regression (audit run-1: serve_art.lexical-containment-traversal):
    /// the containment check was a lexical starts_with on the decoded
    /// path, so an art-root-prefixed path with embedded `..` (raw or
    /// percent-encoded, exactly what convertFileSrc produces) passed the
    /// check while the OS resolved the traversal at read time. Both sides
    /// are canonicalized now: out-of-root files are 404, in-root files
    /// still serve.
    #[test]
    fn traversal_out_of_root_is_refused_and_inner_files_still_serve() {
        let (root, inner) = setup();
        let outside = root.parent().unwrap().join("tansen-art-test-outside.txt");
        fs::write(&outside, b"secret").unwrap();

        // Mirror production: convertFileSrc emits "/" + the whole
        // percent-encoded absolute path (its leading slash included).
        fn uri_of(abs: &std::path::Path) -> String {
            format!(
                "/{}",
                percent_encoding::percent_encode(
                    abs.display().to_string().as_bytes(),
                    percent_encoding::NON_ALPHANUMERIC,
                )
            )
        }

        // Legitimate in-root request still serves.
        let ok = serve_art(&root, &uri_of(&inner));
        assert_eq!(ok.status(), 200);
        assert_eq!(ok.body(), b"\x89PNG fake");

        // Raw `..` traversal out of the root (uri adds the leading slash,
        // so the path's own leading slash is dropped here).
        let raw = root.display().to_string();
        let traversal = format!("/{}/../tansen-art-test-outside.txt", &raw[1..]);
        let refused = serve_art(&root, &traversal);
        assert_eq!(refused.status(), 404, "raw .. traversal must not serve");
        assert!(refused.body().is_empty());

        // The encoded form the webview actually produces.
        let encoded = uri_of(&root.join("../tansen-art-test-outside.txt"));
        let refused = serve_art(&root, &encoded);
        assert_eq!(refused.status(), 404, "percent-encoded .. traversal must not serve");

        // A symlink inside the root pointing outside must not serve either.
        #[cfg(unix)]
        {
            let link = root.join("link.png");
            std::os::unix::fs::symlink(&outside, &link).unwrap();
            let via_symlink = serve_art(&root, &uri_of(&link));
            assert_eq!(via_symlink.status(), 404, "symlink escape must not serve");
        }

        let _ = fs::remove_file(&outside);
        let _ = fs::remove_dir_all(&root);
    }
}
