//! Tauri command surface. Thin: each command calls an app-core service and
//! maps errors to strings for the JS bridge.

mod discovery;
mod library;
mod playlists;
mod proxy;
mod settings;
mod sources;
mod spotify;

pub use discovery::*;
pub use library::*;
pub use playlists::*;
pub use proxy::*;
pub use settings::*;
pub use sources::*;
pub use spotify::*;

use tauri::command;

pub(crate) fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Forwards one playback `Command` into the task-based engine channel.
#[command]
pub async fn engine_command(
    engine: tauri::State<'_, app_core::playback_engine::Engine>,
    autoplay_cmds: tauri::State<'_, crate::autoplay::AutoplayCmds>,
    cmd: app_core::playback_types::Command,
) -> Result<(), String> {
    #[cfg(debug_assertions)]
    tracing::info!(target: "play-path", ?cmd, "engine command");
    // The autoplay task's queue mirror observes every forwarded
    // command (best-effort; never blocks the command path).
    let _ = autoplay_cmds.0.send(cmd.clone());
    // send only fails when the engine task is gone (app shutdown) — nothing
    // actionable for the caller.
    engine.send(cmd).await;
    Ok(())
}
