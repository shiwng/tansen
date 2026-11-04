//! The source registry: persisted configs, ordered; instances built on
//! demand. Upstream `SourceRegistry.kt`, minus the parts that don't exist
//! yet (QuickJS modules, JioSaavn, YouTube).

use crate::addon::AddonSource;
use crate::{MusicSource, SourceKind};
use persistence::repo;
use persistence::Db;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// One stored source. `rank` orders user-added sources among themselves;
/// kinds carry their own base order (`SourceKind::rank`). Credentials are
/// NEVER stored here: only the username (non-secret) and the LAN/http
/// opt-in flag live in the config; the password lives in the macOS
/// Keychain (S9).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceConfig {
    pub config_id: String,
    pub kind: SourceKind,
    pub name: String,
    pub base_url: String,
    pub enabled: bool,
    pub rank: u32,
    /// Subsonic (S9): the account username (not secret).
    #[serde(default)]
    pub username: Option<String>,
    /// Subsonic (S9): allow http to a loopback/private host.
    #[serde(default)]
    pub allow_http: bool,
}

const SETTINGS_KEY: &str = "sources_config";

pub fn load_configs(db: &Db) -> persistence::Result<Vec<SourceConfig>> {
    let Some(json) = repo::get_setting(db, SETTINGS_KEY)? else {
        return Ok(vec![]);
    };
    Ok(serde_json::from_str(&json).unwrap_or_default())
}

pub fn save_configs(db: &Db, configs: &[SourceConfig]) -> persistence::Result<()> {
    let json = serde_json::to_string(configs).map_err(|e| {
        persistence::DbError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    })?;
    repo::set_setting(db, SETTINGS_KEY, &json)
}

pub fn add_config(db: &Db, mut config: SourceConfig) -> persistence::Result<()> {
    let mut configs = load_configs(db)?;
    if configs.iter().any(|c| c.config_id == config.config_id) {
        return Ok(()); // idempotent re-add
    }
    config.rank = configs.iter().filter(|c| c.kind == config.kind).map(|c| c.rank).max().unwrap_or(0) + 1;
    configs.push(config);
    save_configs(db, &configs)
}

/// Adds or fully replaces one config (S9: re-saving a Subsonic source with
/// changed username/allow_http flags).
pub fn upsert_config(db: &Db, config: SourceConfig) -> persistence::Result<()> {
    let mut configs = load_configs(db)?;
    if let Some(existing) = configs.iter_mut().find(|c| c.config_id == config.config_id) {
        *existing = config;
    } else {
        configs.push(config);
    }
    save_configs(db, &configs)
}

pub fn remove_config(db: &Db, config_id: &str) -> persistence::Result<()> {
    let mut configs = load_configs(db)?;
    configs.retain(|c| c.config_id != config_id);
    save_configs(db, &configs)
}

pub fn set_enabled(db: &Db, config_id: &str, enabled: bool) -> persistence::Result<()> {
    let mut configs = load_configs(db)?;
    if let Some(c) = configs.iter_mut().find(|c| c.config_id == config_id) {
        c.enabled = enabled;
    }
    save_configs(db, &configs)
}

/// Maximum ids accepted in one `set_order` call (S2).
pub const MAX_ORDER_IDS: usize = 64;

/// Rejects unknown ids, duplicates, and wrong-length orders.
pub fn validate_order(ids: &[String], known: &[SourceConfig]) -> Result<(), String> {
    if ids.len() > MAX_ORDER_IDS {
        return Err(format!("too many ids: {} (cap {MAX_ORDER_IDS})", ids.len()));
    }
    let known: std::collections::HashSet<&str> =
        known.iter().map(|c| c.config_id.as_str()).collect();
    let mut seen = std::collections::HashSet::new();
    for id in ids {
        if !known.contains(id.as_str()) {
            return Err(format!("unknown source id: {id}"));
        }
        if !seen.insert(id.as_str()) {
            return Err(format!("duplicate source id: {id}"));
        }
    }
    if ids.len() != known.len() {
        return Err(format!(
            "order must name every source (got {}, need {})",
            ids.len(),
            known.len()
        ));
    }
    Ok(())
}

/// Sets the full order of the known configs (S2). `ids` must name every
/// known source exactly once; the order is stored as `rank`.
pub fn set_order(db: &Db, ids: &[String]) -> Result<(), String> {
    let mut configs = load_configs(db).map_err(|e| e.to_string())?;
    validate_order(ids, &configs)?;
    for (pos, id) in ids.iter().enumerate() {
        if let Some(c) = configs.iter_mut().find(|c| c.config_id == *id) {
            c.rank = pos as u32;
        }
    }
    save_configs(db, &configs).map_err(|e| e.to_string())
}

/// Whether a config is present and enabled (S2: disabled sources must not
/// resolve — propagated as `SourceError::Disabled` by the resolver).
pub fn is_enabled(db: &Db, config_id: &str) -> bool {
    load_configs(db)
        .ok()
        .and_then(|cs| cs.iter().find(|c| c.config_id == config_id).cloned())
        .map(|c| c.enabled)
        .unwrap_or(false)
}

/// Builds the enabled sources in playback order: kind rank first (addons
/// before YouTube), then the user's per-kind ordering.
///
/// The YouTube source participates only when the catalogue flag is on
/// (`catalogue_sources` = "true"; local sources stay the default) and the
/// sidecar jar + a JVM are discoverable. A failed launch leaves the source
/// out rather than failing the library — a missing sidecar must not break
/// local playback.
pub fn build_active(db: &Db) -> persistence::Result<Vec<std::sync::Arc<dyn MusicSource>>> {
    let mut active: Vec<std::sync::Arc<dyn MusicSource>> = Vec::new();
    for c in load_configs(db)? {
        if !c.enabled || c.kind == SourceKind::Local {
            continue;
        }
        match c.kind {
            SourceKind::Addon => {
                active.push(
                    match AddonSource::new(c.config_id, c.name, c.base_url, c.allow_http) {
                        Ok(a) => std::sync::Arc::new(a) as std::sync::Arc<dyn MusicSource>,
                        Err(e) => {
                            tracing::warn!(error = %e, "addon source skipped (invalid base url)");
                            continue;
                        }
                    }
                );
            }
            SourceKind::Subsonic => {
                let username = c.username.clone().unwrap_or_default();
                let cid = c.config_id.clone();
                match crate::subsonic::SubsonicSource::new(
                    c.config_id,
                    c.name,
                    &c.base_url,
                    &username,
                    c.allow_http,
                ) {
                    Ok(s) => active.push(std::sync::Arc::new(s) as std::sync::Arc<dyn MusicSource>),
                    Err(e) => tracing::warn!(error = %e, config = cid, "Subsonic source skipped"),
                }
            }
            SourceKind::YouTube | SourceKind::Local => {}
        }
    }

    let catalogue_on = repo::get_setting(db, "catalogue_sources")
        .ok()
        .flatten()
        .map(|v| v == "true")
        .unwrap_or(false);
    if catalogue_on {
        let jar = repo::get_setting(db, "sidecar_jar")
            .ok()
            .flatten()
            .map(PathBuf::from)
            .filter(|p| p.is_file());
        let java = repo::get_setting(db, "java_path")
            .ok()
            .flatten()
            .filter(|p| PathBuf::from(p).is_file());
        match crate::yt::YtMusicSource::launch(jar, java) {
            Ok(yt) => active.push(std::sync::Arc::new(yt)),
            Err(e) => tracing::warn!(error = %e, "YouTube source unavailable; catalogue stays local-only"),
        }
    }
    Ok(active)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("t.db")).unwrap();
        (dir, db)
    }

    fn config(id: &str, kind: SourceKind) -> SourceConfig {
        SourceConfig {
            config_id: id.into(),
            kind,
            name: format!("Source {id}"),
            base_url: "https://example.test".into(),
            enabled: true,
            rank: 0,
            username: None,
            allow_http: false,
        }
    }

    #[test]
    fn configs_round_trip_and_order() {
        let (_dir, db) = db();
        // Migration 3 seeds yt + local; the test adds two addons.
        let seeded = load_configs(&db).unwrap();
        assert_eq!(seeded.len(), 2, "built-ins seeded");
        add_config(&db, config("a", SourceKind::Addon)).unwrap();
        add_config(&db, config("b", SourceKind::Addon)).unwrap();
        assert_eq!(load_configs(&db).unwrap().len(), 4);
        // Duplicate add is idempotent and does not bump rank.
        add_config(&db, config("a", SourceKind::Addon)).unwrap();
        let configs = load_configs(&db).unwrap();
        assert_eq!(configs.len(), 4);

        // Kind rank orders addons before YouTube regardless of insertion.
        let active = build_active(&db).unwrap();
        assert_eq!(active.len(), 2); // YouTube has no builder yet
        // set_order must name every known source exactly once (seeded yt +
        // local + a + b).
        let all: Vec<String> = load_configs(&db)
            .unwrap()
            .iter()
            .map(|c| c.config_id.clone())
            .collect();
        let mut reversed = all.clone();
        reversed.reverse();
        set_order(&db, &reversed).unwrap();
        let configs = load_configs(&db).unwrap();
        let rank = |id: &str| configs.iter().find(|c| c.config_id == id).unwrap().rank;
        assert!(rank("b") < rank("a"), "order applied");
        // Rejections: unknown id, duplicate, wrong length.
        assert!(validate_order(&["nope".into()], &configs).is_err());
        let dup = vec![all[0].clone(), all[0].clone()];
        assert!(validate_order(&dup, &configs).is_err());
        assert!(validate_order(&all[..all.len() - 1], &configs).is_err());

        set_enabled(&db, "b", false).unwrap();
        let active = build_active(&db).unwrap();
        assert_eq!(active.len(), 1);
        remove_config(&db, "a").unwrap();
        assert_eq!(load_configs(&db).unwrap().len(), 3);
    }
}

#[cfg(test)]
mod order_validation_tests {
    use super::*;

    fn known() -> Vec<SourceConfig> {
        (0..4)
            .map(|i| SourceConfig {
                config_id: format!("s{i}"),
                kind: SourceKind::Addon,
                name: format!("S{i}"),
                base_url: "https://example.test".into(),
                enabled: true,
                rank: i as u32,
                username: None,
                allow_http: false,
            })
            .collect()
    }

    #[test]
    fn oversize_order_is_rejected() {
        let big: Vec<String> = (0..=MAX_ORDER_IDS).map(|i| format!("s{i}")).collect();
        assert!(validate_order(&big, &known()).is_err(), "oversize rejected");
    }

    #[test]
    fn unknown_duplicate_wrong_length_rejected() {
        let k = known();
        assert!(validate_order(&["nope".into()], &k).is_err());
        let dup = vec!["s0".into(), "s0".into(), "s1".into(), "s2".into()];
        assert!(validate_order(&dup, &k).is_err());
        let short: Vec<String> = k.iter().take(3).map(|c| c.config_id.clone()).collect();
        assert!(validate_order(&short, &k).is_err());
        let ok: Vec<String> = k.iter().map(|c| c.config_id.clone()).collect();
        assert!(validate_order(&ok, &k).is_ok());
    }
}
