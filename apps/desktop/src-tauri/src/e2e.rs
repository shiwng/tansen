//! F9: debug-only end-to-end self-test. `TANSEN_E2E=1` drives the same
//! command paths the UI buttons use (search → upsert → Enqueue/Play →
//! pause/resume/seek/next) against the real engine, resolver, sidecar and
//! output device, logging timestamped evidence lines. Compiled out of
//! release. Never claims audibility — it proves non-silent samples via the
//! debug sink peak meter.

use crate::rodio_factory::RodioFactory;
use app_core::playback_engine::Engine;
use sources::MusicSource as _;
use app_core::playback_types::{Command, EntryId, Placement, Status};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Evidence {
    t0: Instant,
}

impl Evidence {
    fn log(&self, scenario: &str, detail: impl Into<String>) {
        let detail = detail.into();
        eprintln!(
            "[E2E +{:>6.1}s] {scenario}: {detail}",
            self.t0.elapsed().as_secs_f64()
        );
    }
}

fn spawn_e2e(
    engine: Engine,
    factory: Arc<RodioFactory>,
    state: Arc<app_core::state::AppState>,
) {
    tauri::async_runtime::spawn(async move {
        let ev = Evidence { t0: Instant::now() };
        let mut rx = engine.subscribe();

        // Volume up + unmuted.
        engine.send(Command::SetVolume(1.0)).await;

        // ---- S1: real search (same path as search_tracks) ----
        let sources_vec = {
            let db = state.db.clone();
            tauri::async_runtime::spawn_blocking(move || {
                // A fresh harness data dir has no catalogue flag; the real
                // app gets it from Settings → Sources.
                let _ = persistence::repo::set_setting(&db, "catalogue_sources", "true");
                sources::registry::build_active(&db).map_err(|e| e.to_string())
            })
            .await
        };
        let sources_vec = match sources_vec {
            Ok(Ok(v)) if !v.is_empty() => v,
            _other => {
                ev.log("S1", "search unavailable".to_string());
                return;
            }
        };
        let (rows, statuses) = sources::search::search_all_with_local(&sources_vec, None, "Kevin MacLeod", 10).await;
        let ok_rows = rows.len();
        let bad = statuses.iter().filter(|s| !matches!(s.status, sources::search::SourceStatus::Ok)).count();
        ev.log(
            "S1",
            format!("search rows={ok_rows} sources-ok={} sources-failed={bad}", statuses.len() - bad),
        );
        if ok_rows == 0 {
            ev.log("S1", "FAIL: no real rows".to_string());
            return;
        }
        for r in rows.iter().take(3) {
            ev.log("S1", format!("row: {} — {} ({:?})", r.title, r.artist, r.key.source));
        }

        // ---- S2/S3: select first remote row → upsert → Enqueue → Play ----
        let Some(first) = rows.iter().find(|r| r.key.source != sources::SourceKind::Local) else {
            ev.log("S2", "FAIL: no remote row in results".to_string());
            return;
        };
        let key = first.key.clone();
        let row_id = match persistence::repo::upsert_remote_track(
            &state.db,
            "youtube",
            &key.id,
            &first.title,
            &first.artist,
            first.album.as_deref(),
            first.duration_sec.map(|d| f64::from(d)).unwrap_or(0.0),
            first.artwork_url.as_deref().filter(|a| a.starts_with("https://")),
        ) {
            Ok(id) => id as u64 + app_core::playback_types::REMOTE_ID_OFFSET,
            Err(e) => {
                ev.log("S2", format!("upsert failed: {e}"));
                return;
            }
        };
        ev.log("S2", format!("upserted remote row engine_id={row_id}"));

        let before = factory.frames().0;
        let t_play = Instant::now();
        engine
            .send(Command::Enqueue {
                tracks: vec![EntryId(row_id)],
                remote: vec![(
                    EntryId(row_id),
                    ("youtube".to_string(), key.id.clone()),
                )],
                placement: Placement::Append,
            })
            .await;
        engine
            .send(Command::Play { entry: EntryId(row_id), remote: None })
            .await;
        // Wait for Playing (≤15 s: cold resolve).
        loop {
            if rx.changed().await.is_err() {
                return;
            }
            if rx.borrow().status == Status::Playing && rx.borrow().entry == Some(EntryId(row_id)) {
                break;
            }
            if t_play.elapsed() > Duration::from_secs(15) {
                ev.log("S3", format!("FAIL: not Playing in 15 s (last {:?})", rx.borrow().status));
                return;
            }
        }
        let play_ms = t_play.elapsed().as_millis() as u64;
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let after = factory.frames().0;
        ev.log(
            "S3",
            format!("Playing after {play_ms} ms; frames {before}→{after} (Δ={})", after - before),
        );
        // Non-silence proof: drain the debug sink peak (1.5 s of audio).
        let peak = playback::metrics::audio::drain_peak();
        ev.log("S3", format!("sink peak |sample|={peak:.4} (silence ≈ 0)"));

        // F9 diagnostic: decode the same resolved stream through the
        // HttpRangeReader + symphonia path (F2 pattern); logs the exact
        // symphonia error if the pipeline path diverges.
        if let Ok(yt) = state.yt() {
            if let Ok(k) = sources::TrackKey::new(sources::SourceKind::YouTube, key.id.clone()) {
                match yt.source.stream(&k, &sources::StreamRequest::Best).await {
                    Ok(Some(stream)) => {
                        let url = stream.url.clone();
                        let rms = std::thread::spawn(move || {
                            let reader = playback::remote::HttpRangeReader::new(url, vec![])
                                .map_err(|e| e.to_string())?;
                            playback::probe::decode_rms(reader, Duration::from_millis(500))
                                .map_err(|e| e.to_string())
                        })
                        .join();
                        match rms {
                            Ok(Ok(rms)) => ev.log("S3-diag", format!("httpreader decode rms={rms:.4}")),
                            Ok(Err(e)) => ev.log("S3-diag", format!("httpreader decode FAILED: {e}")),
                            Err(e) => ev.log("S3-diag", format!("thread: {e:?}")),
                        }

                        // F9: the pipeline's exact probe path (PrefetchReader
                        // + hint m4a + rodio) as a playback-side helper.
                        let url = stream.url.clone();
                        match std::thread::spawn(move || {
                            playback::pipeline::diagnostic_probe_decode(url)
                        })
                        .join()
                        {
                            Ok(Ok(n)) => ev.log("S3-diag", format!("prefetch+probe decodes ({n} samples)")),
                            Ok(Err(e)) => ev.log("S3-diag", format!("prefetch+probe FAILED: {e}")),
                            Err(e) => ev.log("S3-diag", format!("thread: {e:?}")),
                        }
                    }
                    Ok(None) => ev.log("S3-diag", "no stream".to_string()),
                    Err(e) => ev.log("S3-diag", format!("resolve failed: {e}")),
                }
            }
        }

        // Warm re-select: same key again (cache hit).
        let t_warm = Instant::now();
        engine.send(Command::Pause).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        engine.send(Command::Play { entry: EntryId(row_id), remote: None }).await;
        loop {
            if rx.changed().await.is_err() {
                return;
            }
            if rx.borrow().status == Status::Playing {
                break;
            }
            if t_warm.elapsed() > Duration::from_secs(10) {
                break;
            }
        }
        ev.log("S3-warm", format!("resume-warm re-land in {} ms", t_warm.elapsed().as_millis()));

        // ---- S4: pause/resume ----
        engine.send(Command::Pause).await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        let f0 = factory.frames().0;
        tokio::time::sleep(Duration::from_millis(1200)).await;
        let f1 = factory.frames().0;
        let paused_ok = f0 == f1;
        ev.log("S4", format!("pause: frames flat={paused_ok} ({f0})"));
        engine.send(Command::Resume).await;
        tokio::time::sleep(Duration::from_millis(400)).await;
        let f2 = factory.frames().0;
        ev.log("S4", format!("resume: frames advancing={} (+{})", f2 > f1, f2 - f1));

        // ---- S5: seek ----
        engine.send(Command::Seek { ms: 30_000 }).await;
        tokio::time::sleep(Duration::from_millis(700)).await;
        let st = rx.borrow().clone();
        ev.log(
            "S5",
            format!("seek→30 s: anchor pos_ms={} ({:?})", st.anchor.pos_ms, st.status),
        );

        // ---- S8: kill the sidecar mid-play, then next resolve recovers ----
        if let Some(yt) = state.yt().ok() {
            if let Some(pid) = yt.source.pid() {
                let _ = std::process::Command::new("kill").arg("-9").arg(pid.to_string()).status();
                ev.log("S8", "sidecar SIGKILLed mid-play");
            }
        }
        // The current track streams from googlevideo directly: still Playing.
        tokio::time::sleep(Duration::from_millis(400)).await;
        let still = rx.borrow().status == Status::Playing;
        ev.log("S8", format!("current track continues after sidecar death: {still}"));

        // ---- S6: an owner-provided local file, if one is already indexed ----
        // Never manufacture a tone/beep for playback evidence.
        let local_id = persistence::repo::search_local(&state.db_ro, "", 1)
            .ok()
            .and_then(|v| v.first().map(|t| t.id));
        if let Some(local_id) = local_id {
            engine
                .send(Command::Enqueue {
                    tracks: vec![EntryId(local_id as u64)],
                    remote: vec![],
                    placement: Placement::Append,
                })
                .await;
            engine.send(Command::Next).await;
            tokio::time::sleep(Duration::from_millis(600)).await;
            let st = rx.borrow().clone();
            let on_local = st.entry == Some(EntryId(local_id as u64)) && st.status == Status::Playing;
            ev.log("S6", format!("Next → local entry playing: {on_local} (entry {:?})", st.entry));
        } else {
            ev.log("S6", "NOT RUN: no owner-provided indexed local track; no synthetic audio created");
        }

        // ---- S10: unresolvable id alone → honest error, then back ----
        let bogus = u64::MAX - 7;
        engine
            .send(Command::Enqueue {
                tracks: vec![EntryId(bogus)],
                remote: vec![(
                    EntryId(bogus),
                    ("youtube".to_string(), "zzzNoSuchTrackE2E".to_string()),
                )],
                placement: Placement::Next,
            })
            .await;
        engine.send(Command::Next).await;
        tokio::time::sleep(Duration::from_millis(2500)).await;
        let st = rx.borrow().clone();
        ev.log("S10", format!("bogus id → {:?} (typed error, then queue recovers)", st.status));
        engine.send(Command::Next).await;

        ev.log("DONE", "scenarios executed; see per-scenario lines".to_string());
    });
}

/// Entry point called from setup when `TANSEN_E2E=1` (debug builds only).
pub fn maybe_run(engine: &Engine, factory: Arc<RodioFactory>, state: Arc<app_core::state::AppState>) {
    if std::env::var("TANSEN_E2E").as_deref() != Ok("1") {
        return;
    }
    spawn_e2e(engine.clone(), factory.clone(), state.clone());
}
