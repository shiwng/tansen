use crate::StateHandle;
use tauri::command;

use super::err;

// Sources (addon protocol first)

#[derive(serde::Serialize)]
pub struct SourceWithHealth {
    #[serde(flatten)]
    pub config: sources::registry::SourceConfig,
    pub health: sources::SourceHealth,
}

/// The configured source roster (no live health).
#[command]
pub fn sources_get(state: StateHandle<'_>) -> Result<Vec<sources::registry::SourceConfig>, String> {
    sources::registry::load_configs(&state.db).map_err(err)
}

/// Full-order replacement for the source roster: `ids` must name every
/// known source exactly once, bounded.
#[command]
pub fn source_reorder(state: StateHandle<'_>, ids: Vec<String>) -> Result<(), String> {
    sources::registry::set_order(&state.db, &ids)
}

/// The one sanctioned read-only stream-info query — facts about
/// the last successful resolution for an entry. Not part of PlaybackState.
#[command]
pub fn stream_info_get(
    factory: tauri::State<'_, std::sync::Arc<crate::rodio_factory::RodioFactory>>,
    entry: u64,
) -> Result<Option<crate::rodio_factory::StreamInfo>, String> {
    Ok(factory.stream_info(entry))
}

/// All configured sources with a live health read each, checked concurrently.
/// Health probes are kind-aware: addons speak the addon protocol, Subsonic
/// servers speak ping.
#[command]
pub async fn sources_get_with_health(
    state: StateHandle<'_>,
) -> Result<Vec<SourceWithHealth>, String> {
    let db = state.db.clone();
    let configs = tauri::async_runtime::spawn_blocking(move || {
        sources::registry::load_configs(&db).map_err(err)
    })
    .await
    .map_err(err)??;
    let mut set = tokio::task::JoinSet::new();
    let mut skipped = Vec::new();
    for config in configs {
        let source = match config.kind {
            sources::SourceKind::Subsonic => {
                let username = config.username.clone().unwrap_or_default();
                match sources::subsonic::SubsonicSource::new(
                    config.config_id.clone(),
                    config.name.clone(),
                    &config.base_url,
                    &username,
                    config.allow_http,
                ) {
                    Ok(s) => std::sync::Arc::new(s) as std::sync::Arc<dyn sources::MusicSource>,
                    Err(e) => {
                        skipped.push(SourceWithHealth {
                            config,
                            health: sources::SourceHealth::Rejected {
                                reason: e.to_string(),
                            },
                        });
                        continue;
                    }
                }
            }
            _ => match sources::addon::AddonSource::new(
                config.config_id.clone(),
                config.name.clone(),
                config.base_url.clone(),
                config.allow_http,
            ) {
                Ok(a) => std::sync::Arc::new(a) as std::sync::Arc<dyn sources::MusicSource>,
                Err(e) => {
                    skipped.push(SourceWithHealth {
                        config: config.clone(),
                        health: sources::SourceHealth::Rejected {
                            reason: e.to_string(),
                        },
                    });
                    continue;
                }
            },
        };
        set.spawn(async move {
            let health = match source.health().await {
                Ok(h) => h,
                Err(e) => sources::SourceHealth::Unreachable {
                    reason: e.to_string(),
                },
            };
            SourceWithHealth { config, health }
        });
    }
    let mut out = skipped;
    while let Some(result) = set.join_next().await {
        out.push(result.map_err(err)?);
    }
    out.sort_by_key(|s| (s.config.kind.rank(), s.config.rank));
    Ok(out)
}

/// Adds a Subsonic/OpenSubsonic source. The password goes into the macOS
/// Keychain FIRST; only on success is the (password-free) config row saved to
/// the settings DB. Never in SQLite/logs/events/frontend state.
#[command]
pub fn subsonic_add(
    state: StateHandle<'_>,
    base_url: String,
    username: String,
    password: String,
    allow_http: bool,
) -> Result<sources::registry::SourceConfig, String> {
    if username.trim().is_empty() {
        return Err("username is empty".into());
    }
    if password.is_empty() {
        return Err("password is empty".into());
    }
    sources::subsonic::validate_base_url(&base_url, allow_http).map_err(|e| e.to_string())?;
    let trimmed = base_url.trim_end_matches('/').to_string();
    let config_id = format!("subsonic:{trimmed}");
    // Server-bound key: the credential belongs to (username, server), so
    // two same-username servers cannot overwrite each other's password.
    // The pre-fix username-only item is deleted after a successful store
    // so stale shared credentials cannot be adopted later.
    let account = sources::keychain::credential_account(&username, &trimmed);
    sources::keychain::store_subsonic_password(&account, &password).map_err(|e| e.to_string())?;
    let _ = sources::keychain::delete_subsonic_password(&username);
    let name = url::Url::parse(&trimmed)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .map(|h| format!("Subsonic ({h})"))
        .unwrap_or_else(|| "Subsonic".into());
    let config = sources::registry::SourceConfig {
        config_id,
        kind: sources::SourceKind::Subsonic,
        name,
        base_url: trimmed,
        enabled: true,
        rank: 0,
        username: Some(username),
        allow_http,
    };
    sources::registry::upsert_config(&state.db, config.clone()).map_err(err)?;
    Ok(config)
}

#[derive(serde::Deserialize)]
pub struct NewSource {
    pub base_url: String,
}

/// Adds an addon source from a base URL: fetches the manifest to validate the
/// protocol and derive a display name.
#[command]
pub async fn source_add(
    state: StateHandle<'_>,
    new: NewSource,
) -> Result<sources::registry::SourceConfig, String> {
    let base = new.base_url.trim_end_matches('/').to_string();
    if !base.starts_with("http://") && !base.starts_with("https://") {
        return Err("Source URL must start with http:// or https://".into());
    }
    // HTTPS-only at registration — the per-addon http opt-in needs a
    // settings field (decision required), so http addons cannot be added
    // from the UI today.
    let source = sources::addon::AddonSource::new(base.clone(), base.clone(), base.clone(), false)
        .map_err(|e| format!("Source rejected: {e}"))?;
    let manifest = source
        .probe_manifest()
        .await
        .map_err(|e| format!("Source did not answer with a valid manifest: {e}"))?;
    let name = manifest.name.clone().unwrap_or_else(|| manifest.id.clone());
    let config = sources::registry::SourceConfig {
        config_id: format!("addon:{}", manifest.id),
        kind: sources::SourceKind::Addon,
        name,
        base_url: base,
        enabled: true,
        rank: 0,
        username: None,
        allow_http: false,
    };
    sources::registry::add_config(&state.db, config.clone()).map_err(err)?;
    Ok(config)
}

#[command]
pub fn source_remove(state: StateHandle<'_>, config_id: String) -> Result<(), String> {
    // Revoke the credential with the row: without this the Keychain item
    // outlived the source forever and was silently adopted by any later
    // same-key registration. Best-effort — the row removal succeeds even
    // if the platform rejects the delete.
    if let Ok(configs) = sources::registry::load_configs(&state.db) {
        if let Some(config) = configs.iter().find(|c| c.config_id == config_id) {
            if let Some(username) = &config.username {
                let _ = sources::keychain::delete_subsonic_password(
                    &sources::keychain::credential_account(username, &config.base_url),
                );
                let _ = sources::keychain::delete_subsonic_password(username); // pre-fix key
            }
        }
    }
    sources::registry::remove_config(&state.db, &config_id).map_err(err)
}

#[command]
pub fn source_set_enabled(
    state: StateHandle<'_>,
    config_id: String,
    enabled: bool,
) -> Result<(), String> {
    sources::registry::set_enabled(&state.db, &config_id, enabled).map_err(err)
}
