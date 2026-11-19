//! macOS Now Playing + remote commands via the MediaPlayer framework
//! (objc2 bindings). Replaces the souvlaki integration: same
//! features (metadata, elapsed/rate, transport commands), but wired to the
//! engine watch directly.
//!
//! Contract: the watch task updates Now Playing **only when the engine
//! publishes a new rev** — `elapsed` is the anchor position and `rate` the
//! anchor rate; MediaPlayer interpolates between publishes, so there are no
//! timers here. Remote commands map 1:1 onto [`Command`] and enter the
//! engine's bounded channel.

use app_core::playback_engine::Engine;
use app_core::playback_types::Command;
use app_core::playback_types::Status;
use app_core::AppState;
use core::ptr::NonNull;
use objc2::rc::autoreleasepool;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_foundation::{NSMutableDictionary, NSNotification, NSNumber, NSString};
use objc2_media_player::{
    MPChangePlaybackPositionCommandEvent, MPMediaItemPropertyAlbumTitle, MPMediaItemPropertyArtist,
    MPMediaItemPropertyPlaybackDuration, MPMediaItemPropertyTitle, MPNowPlayingInfoCenter,
    MPNowPlayingInfoPropertyElapsedPlaybackTime, MPNowPlayingInfoPropertyPlaybackRate,
    MPRemoteCommandCenter, MPRemoteCommandEvent, MPRemoteCommandHandlerStatus,
};
use std::sync::Arc;
use tokio::sync::watch;

/// `MPRemoteCommandHandlerStatusSuccess` (the binding does not generate the
/// constant).
const HANDLER_SUCCESS: MPRemoteCommandHandlerStatus = MPRemoteCommandHandlerStatus::Success;
/// Relative-seek step for the skip commands, matching the old key handler.
const SEEK_STEP_SEC: f64 = 10.0;

/// Attaches remote commands (main thread — Apple requires the shared
/// command center to be configured there) and spawns the watch task that
/// mirrors engine state into Now Playing.
pub fn start(engine: Engine, state: Arc<AppState>) -> Result<(), String> {
    let rx = engine.subscribe();
    unsafe { attach_remote_commands(engine.clone(), rx.clone()) };
    tauri::async_runtime::spawn(async move { watch_loop(state, rx).await });
    spawn_output_monitor(engine.clone());
    unsafe { observe_sleep_wake(engine) };
    tracing::info!("macOS Now Playing wired to the engine watch");
    Ok(())
}

/// System sleep pauses playback; wake rebuilds the output stream at the
/// anchor position — via NSWorkspace notifications on the shared center.
unsafe fn observe_sleep_wake(engine: Engine) {
    use objc2_app_kit::{NSWorkspaceDidWakeNotification, NSWorkspaceWillSleepNotification};

    let center = objc2_app_kit::NSWorkspace::sharedWorkspace().notificationCenter();

    let wake_sleep = engine.clone();
    let sleep_block = block2::RcBlock::new(move |_n: NonNull<NSNotification>| {
        wake_sleep.try_send(Command::Pause);
    });
    center.addObserverForName_object_queue_usingBlock(
        Some(&*NSWorkspaceWillSleepNotification),
        None,
        None,
        &sleep_block,
    );

    let wake_wake = engine.clone();
    let wake_block = block2::RcBlock::new(move |_n: NonNull<NSNotification>| {
        wake_wake.try_send(Command::RebuildOutput);
    });
    center.addObserverForName_object_queue_usingBlock(
        Some(&*NSWorkspaceDidWakeNotification),
        None,
        None,
        &wake_block,
    );
}

/// Polls the default output device + sleep state every 2 s and routes
/// transitions into the engine as Commands — `Pause` when the active device
/// was removed or the system sleeps, `RebuildOutput` on a device switch,
/// removal of a non-active device, and wake.
fn spawn_output_monitor(engine: Engine) {
    std::thread::Builder::new()
        .name("output-monitor".into())
        .spawn(move || {
            let mut last_name = playback::pipeline::default_output_name();
            loop {
                std::thread::sleep(std::time::Duration::from_secs(2));

                let current = playback::pipeline::default_output_name();
                if current != last_name {
                    tracing::info!(previous = ?last_name, current = ?current, "default output device changed; requesting rebuild");
                    // Removal (the old default vanished from the device list)
                    // pauses; a plain switch just rebuilds.
                    if let Some(old) = &last_name {
                        if !playback::pipeline::output_device_exists(old) {
                            engine.try_send(Command::Pause);
                        }
                    }
                    engine.try_send(Command::RebuildOutput);
                }
                last_name = current;
            }
        })
        .expect("output monitor thread");
}

struct TrackMeta {
    title: String,
    artist: String,
    album: String,
    duration_sec: f64,
}

/// Library lookup for Now Playing metadata (hash entries resolve to
/// nothing → empty metadata).
fn lookup_track(state: &AppState, id: u64) -> TrackMeta {
    match persistence::repo::get_track(&state.db_ro, id as i64) {
        Ok(Some(t)) if !t.missing => TrackMeta {
            title: t.title,
            artist: t.artist,
            album: t.album,
            duration_sec: t.duration_sec,
        },
        _ => TrackMeta {
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            duration_sec: 0.0,
        },
    }
}

/// Engine watch → Now Playing. Fires only on publishes (rev bumps); the
/// metadata comes from the library DB for the current entry.
async fn watch_loop(
    state: Arc<AppState>,
    mut rx: watch::Receiver<app_core::playback_types::PlaybackState>,
) {
    loop {
        let Ok(()) = rx.changed().await else { break };
        let st = rx.borrow_and_update().clone();
        autoreleasepool(|_| unsafe {
            let center = MPNowPlayingInfoCenter::defaultCenter();
            let Some(entry) = st.entry else {
                center.setNowPlayingInfo(None);
                return;
            };
            let meta = lookup_track(&state, entry.0);
            let dict = NSMutableDictionary::<NSString, AnyObject>::new();
            let title = NSString::from_str(&meta.title);
            let artist = NSString::from_str(&meta.artist);
            let album = NSString::from_str(&meta.album);
            dict.setObject_forKey(
                &*title,
                &ProtocolObject::from_ref(&*MPMediaItemPropertyTitle),
            );
            dict.setObject_forKey(
                &*artist,
                &ProtocolObject::from_ref(&*MPMediaItemPropertyArtist),
            );
            dict.setObject_forKey(
                &*album,
                &ProtocolObject::from_ref(&*MPMediaItemPropertyAlbumTitle),
            );
            if meta.duration_sec > 0.0 {
                let duration = NSNumber::numberWithDouble(meta.duration_sec);
                dict.setObject_forKey(
                    &*duration,
                    &ProtocolObject::from_ref(&*MPMediaItemPropertyPlaybackDuration),
                );
            }
            let elapsed = NSNumber::numberWithDouble(st.anchor.pos_ms as f64 / 1000.0);
            let rate = NSNumber::numberWithDouble(st.anchor.rate as f64);
            dict.setObject_forKey(
                &*elapsed,
                &ProtocolObject::from_ref(&*MPNowPlayingInfoPropertyElapsedPlaybackTime),
            );
            dict.setObject_forKey(
                &*rate,
                &ProtocolObject::from_ref(&*MPNowPlayingInfoPropertyPlaybackRate),
            );
            center.setNowPlayingInfo(Some(&dict));
        });
    }
}

/// Maps every remote command onto the engine's `Command`.
unsafe fn attach_remote_commands(
    engine: Engine,
    rx: watch::Receiver<app_core::playback_types::PlaybackState>,
) {
    let cc = MPRemoteCommandCenter::sharedCommandCenter();

    macro_rules! on {
        ($command:expr, $handler:expr) => {{
            let command = $command;
            let block = block2::RcBlock::new(
                move |_event: NonNull<MPRemoteCommandEvent>| -> MPRemoteCommandHandlerStatus {
                    $handler
                    HANDLER_SUCCESS
                },
            );
            command.addTargetWithHandler(&block);
        }};
    }

    let engine_toggle = engine.clone();
    let rx_toggle = rx.clone();
    on!(cc.togglePlayPauseCommand(), {
        let st = rx_toggle.borrow().clone();
        match st.status {
            Status::Playing => {
                let _ = engine_toggle.try_send(Command::Pause);
            }
            Status::Paused => {
                let _ = engine_toggle.try_send(Command::Resume);
            }
            _ => {
                if let Some(entry) = st.entry {
                    let _ = engine_toggle.try_send(Command::Play {
                        entry,
                        remote: None,
                    });
                }
            }
        }
    });

    let engine_play = engine.clone();
    let rx_play = rx.clone();
    on!(cc.playCommand(), {
        let st = rx_play.borrow().clone();
        if st.status == Status::Paused {
            let _ = engine_play.try_send(Command::Resume);
        } else if let Some(entry) = st.entry {
            let _ = engine_play.try_send(Command::Play {
                entry,
                remote: None,
            });
        }
    });

    let engine_pause = engine.clone();
    on!(cc.pauseCommand(), {
        let _ = engine_pause.try_send(Command::Pause);
    });

    let engine_next = engine.clone();
    on!(cc.nextTrackCommand(), {
        let _ = engine_next.try_send(Command::Next);
    });

    let engine_prev = engine.clone();
    on!(cc.previousTrackCommand(), {
        let _ = engine_prev.try_send(Command::Prev);
    });

    let engine_back = engine.clone();
    let rx_back = rx.clone();
    on!(cc.skipBackwardCommand(), {
        let pos = rx_back.borrow().anchor.pos_ms as f64 / 1000.0;
        let _ = engine_back.try_send(Command::Seek {
            ms: ((pos - SEEK_STEP_SEC).max(0.0) * 1000.0) as u64,
        });
    });

    let engine_fwd = engine.clone();
    let rx_fwd = rx.clone();
    on!(cc.skipForwardCommand(), {
        let pos = rx_fwd.borrow().anchor.pos_ms as f64 / 1000.0;
        let _ = engine_fwd.try_send(Command::Seek {
            ms: ((pos + SEEK_STEP_SEC) * 1000.0) as u64,
        });
    });

    // Scrubbing: the event carries the absolute target position.
    let engine_pos = engine.clone();
    let position_command = cc.changePlaybackPositionCommand();
    let block = block2::RcBlock::new(
        move |event: NonNull<MPRemoteCommandEvent>| -> MPRemoteCommandHandlerStatus {
            // SAFETY: the change-playback-position command only ever delivers
            // its own event subtype (Apple MediaPlayer contract).
            let event = unsafe {
                NonNull::new_unchecked(event.as_ptr() as *mut MPChangePlaybackPositionCommandEvent)
            };
            let target = unsafe { event.as_ref().positionTime() };
            let _ = engine_pos.try_send(Command::Seek {
                ms: (target.max(0.0) * 1000.0) as u64,
            });
            HANDLER_SUCCESS
        },
    );
    position_command.addTargetWithHandler(&*block);
}
