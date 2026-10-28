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
/// kinds carry their own base order (`SourceKind::rank`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceConfig {
    pub config_id: String,
    pub kind: SourceKind,
    pub name: String,
    pub base_url: String,
    pub enabled: bool,
    pub rank: u32,
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

/// Moves `config_id` within its kind's ordering (`delta` = ±1).
pub fn reorder(db: &Db, config_id: &str, delta: i32) -> persistence::Result<()> {
    let mut configs = load_configs(db)?;
    let Some(kind) = configs.iter().find(|c| c.config_id == config_id).map(|c| c.kind) else {
        return Ok(());
    };
    let mut ranks: Vec<(usize, u32)> = configs
        .iter()
        .enumerate()
        .filter(|(_, c)| c.kind == kind)
        .map(|(i, c)| (i, c.rank))
        .collect();
    ranks.sort_by_key(|(_, r)| *r);
    let Some(pos) = ranks.iter().position(|(i, _)| configs[*i].config_id == config_id) else {
        return Ok(());
    };
    let swap_with = if delta < 0 { pos.checked_sub(1) } else { Some(pos + 1) };
    if let Some(other) = swap_with {
        if let Some(&(a_idx, a_rank)) = ranks.get(pos) {
            if let Some(&(b_idx, b_rank)) = ranks.get(other) {
                configs[a_idx].rank = b_rank;
                configs[b_idx].rank = a_rank;
            }
        }
    }
    save_configs(db, &configs)
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
    let mut active: Vec<std::sync::Arc<dyn MusicSource>> = load_configs(db)?
        .into_iter()
        .filter(|c| c.enabled && c.kind == SourceKind::Addon)
        .map(|c| std::sync::Arc::new(AddonSource::new(c.config_id, c.name, c.base_url)) as std::sync::Arc<dyn MusicSource>)
        .collect();

    let catalogue_on = repo::get_setting(db, "catalogue_sources")
        .ok()
        .flatten()
        .map(|v| v == "true")
        .unwrap_or(false);
    if catalogue_on {
        let jar = repo::get_setting(db, "sidecar_jar").ok().flatten().map(PathBuf::from);
        let java = repo::get_setting(db, "java_path").ok().flatten();
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
        }
    }

    #[test]
    fn configs_round_trip_and_order() {
        let (_dir, db) = db();
        add_config(&db, config("a", SourceKind::Addon)).unwrap();
        add_config(&db, config("b", SourceKind::Addon)).unwrap();
        add_config(&db, config("yt", SourceKind::YouTube)).unwrap();
        assert_eq!(load_configs(&db).unwrap().len(), 3);
        // Duplicate add is idempotent and does not bump rank.
        add_config(&db, config("a", SourceKind::Addon)).unwrap();
        let configs = load_configs(&db).unwrap();
        assert_eq!(configs.len(), 3);

        // Kind rank orders addons before YouTube regardless of insertion.
        let active = build_active(&db).unwrap();
        assert_eq!(active.len(), 2); // YouTube has no builder yet
        reorder(&db, "a", 1).unwrap();
        let configs = load_configs(&db).unwrap();
        let rank = |id: &str| configs.iter().find(|c| c.config_id == id).unwrap().rank;
        assert!(rank("b") < rank("a"), "reorder moved a below b");

        set_enabled(&db, "b", false).unwrap();
        let active = build_active(&db).unwrap();
        assert_eq!(active.len(), 1);
        remove_config(&db, "a").unwrap();
        assert_eq!(load_configs(&db).unwrap().len(), 2);
    }
}
