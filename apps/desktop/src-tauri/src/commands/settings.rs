use crate::StateHandle;
use tauri::command;

use super::err;

// Settings

/// The only settings keys the webview may write. Everything else —
/// notably the spawn-config keys `sidecar_jar`/`java_path` — is
/// native-only by design; a webview-supplied value for one of those would
/// feed an attacker-chosen executable into the sidecar spawn chain.
const SETTINGS_KEY_ALLOWLIST: [&str; 2] = ["watch_folders", "catalogue_sources"];

pub fn settings_key_allowed(key: &str) -> bool {
    SETTINGS_KEY_ALLOWLIST.contains(&key)
}

#[command]
pub fn settings_get(state: StateHandle<'_>) -> Result<Vec<(String, String)>, String> {
    state.settings().map_err(err)
}

#[command]
pub fn settings_set(state: StateHandle<'_>, key: String, value: String) -> Result<(), String> {
    if !settings_key_allowed(&key) {
        return Err(format!("settings key not writable from the UI: {key}"));
    }
    state.set_setting(&key, &value).map_err(err)
}

// Local files source

/// The real local-files source switch. Off stops indexing/watching and drops
/// the local source out of search; existing library rows are kept.
#[command]
pub fn local_files_get(state: StateHandle<'_>) -> Result<bool, String> {
    Ok(app_core::state::local_files_enabled(&state.db))
}

#[command]
pub fn local_files_set(state: StateHandle<'_>, enabled: bool) -> Result<(), String> {
    persistence::repo::set_setting(
        &state.db,
        app_core::state::LOCAL_FILES_SETTING,
        if enabled { "true" } else { "false" },
    )
    .map_err(err)
}

/// The stored audio-quality setting (strict enum).
#[command]
pub fn quality_get(state: StateHandle<'_>) -> Result<sources::AudioQuality, String> {
    let raw = persistence::repo::get_setting(&state.db, "audio_quality")
        .map_err(err)?
        .unwrap_or_else(|| "best".into());
    serde_json::from_str(&format!("\"{raw}\"")).map_err(|e| format!("bad stored quality: {e}"))
}

/// Sets the audio-quality ceiling; strict enum, unknown values rejected.
#[command]
pub fn quality_set(
    state: StateHandle<'_>,
    factory: tauri::State<'_, std::sync::Arc<crate::rodio_factory::RodioFactory>>,
    quality: sources::AudioQuality,
) -> Result<(), String> {
    let raw = serde_json::to_string(&quality).map_err(|e| e.to_string())?;
    let raw = raw.trim_matches('"').to_string();
    persistence::repo::set_setting(&state.db, "audio_quality", &raw).map_err(err)?;
    // Applies to future resolves; the playing track keeps its media.
    factory.set_quality(quality.to_stream_request());
    Ok(())
}

// Autoplay setting

/// The stored autoplay setting (default off).
#[command]
pub fn autoplay_get(state: StateHandle<'_>) -> Result<bool, String> {
    Ok(
        persistence::repo::get_setting(&state.db, crate::autoplay::AUTOPLAY_SETTING)
            .map_err(err)?
            .as_deref()
            == Some("true"),
    )
}

/// Sets autoplay: the stored setting AND the task's flag, together.
#[command]
pub fn autoplay_set(
    state: StateHandle<'_>,
    flag: tauri::State<'_, crate::autoplay::AutoplayFlag>,
    on: bool,
) -> Result<(), String> {
    persistence::repo::set_setting(
        &state.db,
        crate::autoplay::AUTOPLAY_SETTING,
        if on { "true" } else { "false" },
    )
    .map_err(err)?;
    flag.0.store(on, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

// Audio settings (output precision, DSP, gapless/crossfade)

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioSettingsView {
    #[serde(flatten)]
    pub settings: crate::audio_settings::AudioSettings,
    #[serde(flatten)]
    pub output: crate::rodio_factory::OutputCapabilitiesReport,
}

#[command]
pub fn audio_settings_get(
    state: StateHandle<'_>,
    factory: tauri::State<'_, std::sync::Arc<crate::rodio_factory::RodioFactory>>,
) -> Result<AudioSettingsView, String> {
    Ok(AudioSettingsView {
        settings: crate::audio_settings::AudioSettings::load(&state.db),
        output: factory.output_capabilities(),
    })
}

/// Patch one or more audio preferences. Persists before applying; an output
/// depth change rebuilds the stream at the current position (a short,
/// audible gap — the only honest way to switch hardware formats).
#[command]
pub fn audio_settings_set(
    state: StateHandle<'_>,
    factory: tauri::State<'_, std::sync::Arc<crate::rodio_factory::RodioFactory>>,
    engine: tauri::State<'_, app_core::playback_engine::Engine>,
    patch: crate::audio_settings::AudioSettingsPatch,
) -> Result<AudioSettingsView, String> {
    let mut settings = crate::audio_settings::AudioSettings::load(&state.db);
    let loudness_changed = patch.loudness.is_some();
    let reopen = patch.clone().apply(&mut settings);
    settings.persist(&state.db).map_err(err)?;
    factory.set_output_bits(settings.output_bits);
    let curve = if settings.crossfade_curve == "equal-power" {
        playback::pipeline::FADE_CURVE_EQUAL_POWER
    } else {
        playback::pipeline::FADE_CURVE_LINEAR
    };
    factory.configure_playback(
        settings.effective_crossfade_ms(),
        curve,
        settings.gapless,
        settings.skip_silence,
        settings.skip_threshold_db,
        settings.skip_min_ms,
    );
    factory.set_eq(settings.eq_enabled, &settings.eq_gains);
    if loudness_changed {
        factory.set_loudness_enabled(settings.loudness);
    }
    if reopen {
        // Engine-owned: rebuild at the anchor and keep the play state.
        engine.try_send(app_core::playback_types::Command::RebuildOutput);
    }
    Ok(AudioSettingsView {
        settings,
        output: factory.output_capabilities(),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test code")]
mod settings_tests {
    use super::*;

    /// Regression (settings-spawn-chain.sidecar_jar+java_path.unvalidated-spawn):
    /// settings_set used to accept any key, so webview content could write
    /// the spawn-config keys `sidecar_jar`/`java_path` (fed into `java -jar`)
    /// and defeat the startup self-heal. The IPC surface is now allowlisted
    /// to the two keys the UI actually writes.
    #[test]
    fn settings_keys_are_allowlisted_to_ui_writable_ones() {
        assert!(settings_key_allowed("watch_folders"));
        assert!(settings_key_allowed("catalogue_sources"));
        for key in [
            "sidecar_jar",
            "java_path",
            "sources_config",
            "audio_quality",
            "autoplay",
            "sidecar_jar ",
            "",
            "SIDECAR_JAR",
        ] {
            assert!(!settings_key_allowed(key), "key {key:?} must be refused");
        }
    }
}
