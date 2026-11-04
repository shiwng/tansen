//! F1 conformance, group 1: registry defaults, enable/disable, order
//! persistence across a full reopen, and a disabled source resolving as
//! `SourceError::Disabled` for its queued tracks. No network: fake sources.

use sources::registry::{self, SourceConfig};
use sources::{MusicSource, SourceError, SourceHealth, SourceKind, StreamRequest, TrackKey};

#[derive(Default)]
struct Noop;
#[async_trait::async_trait]
impl MusicSource for Noop {
    fn config_id(&self) -> &str { "noop" }
    fn kind(&self) -> SourceKind { SourceKind::Addon }
    fn display_name(&self) -> &str { "Noop" }
    async fn health(&self) -> Result<SourceHealth, SourceError> { Ok(SourceHealth::Ok { detail: None }) }
    async fn search(&self, _q: &str, _l: usize) -> Result<Vec<sources::CatalogTrack>, SourceError> { Ok(vec![]) }
    async fn stream(&self, _k: &TrackKey, _r: &StreamRequest) -> Result<Option<sources::SourceStream>, SourceError> {
        Err(SourceError::Unsupported)
    }
}

fn config(id: &str, kind: SourceKind, enabled: bool) -> SourceConfig {
    SourceConfig {
        config_id: id.into(),
        kind,
        name: format!("S {id}"),
        base_url: "https://example.test".into(),
        enabled,
        rank: 0,
        username: None,
        allow_http: false,
    }
}

#[test]
fn defaults_seeded_and_order_persists_across_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    {
        let db = persistence::Db::open(&path).unwrap();
        // Defaults: migration 3 seeds yt + local.
        let seeded = registry::load_configs(&db).unwrap();
        assert_eq!(seeded.len(), 2, "built-ins seeded");
        registry::add_config(&db, config("a", SourceKind::Addon, true)).unwrap();
        registry::add_config(&db, config("b", SourceKind::Addon, true)).unwrap();
        let all: Vec<String> = registry::load_configs(&db).unwrap().into_iter().map(|c| c.config_id).collect();
        let mut reversed = all.clone();
        reversed.reverse();
        registry::set_order(&db, &reversed).unwrap();
        registry::set_enabled(&db, "b", false).unwrap();
    }
    // Reopen: order (rank) and the disable survive the process.
    let db = persistence::Db::open(&path).unwrap();
    let configs = registry::load_configs(&db).unwrap();
    let rank = |id: &str| configs.iter().find(|c| c.config_id == id).unwrap().rank;
    let enabled = |id: &str| configs.iter().find(|c| c.config_id == id).unwrap().enabled;
    assert!(rank("b") < rank("a"), "order persisted across reopen");
    assert!(!enabled("b"), "disable persisted across reopen");
    // build_active excludes the disabled source.
    let active = registry::build_active(&db).unwrap();
    assert!(active.iter().all(|s| s.config_id() != "b"), "disabled excluded from active");
}

#[tokio::test]
async fn disabled_source_yields_disabled_for_queued_tracks() {
    let dir = tempfile::tempdir().unwrap();
    let db = persistence::Db::open(&dir.path().join("t.db")).unwrap();
    registry::add_config(&db, config("off", SourceKind::Addon, false)).unwrap();
    assert!(!registry::is_enabled(&db, "off"));
    // The shell's lookup consults is_enabled — a disabled config resolves
    // to None there, and the resolver surfaces exactly SourceError::Disabled.
    let db2 = std::sync::Arc::new(db);
    let noop: std::sync::Arc<dyn MusicSource> = std::sync::Arc::new(Noop);
    let lookup: sources::resolver::SourceLookup = {
        let db = db2.clone();
        let noop = noop.clone();
        std::sync::Arc::new(move |kind| {
            // Any addon config is "off" here; a real lookup would map the
            // kind to its config. Disabled is decided by is_enabled.
            let _ = kind;
            registry::is_enabled(&db, "off")
                .then(|| noop.clone())
        })
    };
    let resolver = sources::resolver::Resolver::new(lookup);
    let key = TrackKey { source: SourceKind::Addon, id: "t".into() };
    let err = resolver.resolve(&key, &StreamRequest::Best).await.unwrap_err();
    assert!(matches!(err, SourceError::Disabled), "{err:?}");
}
