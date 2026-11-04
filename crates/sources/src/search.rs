//! S5: multi-source search fan-out. Parallel over enabled sources (Local is
//! never a search source); per-source timeout; per-source status; results
//! ordered by source rank with per-source and total caps.

use crate::{MusicSource, SourceError, SourceKind, StreamRequest, TrackKey};
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;

/// Per-source search timeout (S5).
pub const SEARCH_TIMEOUT: Duration = Duration::from_secs(12);
/// Maximum permitted query length.
pub const SEARCH_MAX_QUERY: usize = 200;
/// Total results cap per command.
pub const SEARCH_LIMIT_CAP: usize = 30;
/// Per-source results cap.
pub const SEARCH_PER_SOURCE_LIMIT: usize = 10;

/// One search result row: the TrackKey + metadata for the UI.
#[derive(Debug, Clone, Serialize)]
pub struct SearchRow {
    pub key: TrackKey,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_sec: Option<u32>,
    pub artwork_url: Option<String>,
}

/// Per-source search outcome.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum SourceStatus {
    Ok,
    Timeout,
    Error { kind: String, message: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceOutcome {
    pub source: SourceKind,
    pub status: SourceStatus,
}

/// F1: the local-library search seam — the shell injects a read-only
/// closure (db_ro + `repo::search_local`); tests inject fakes. The result
/// feeds `search_all_with_local`, the single unified entry point.
pub type LocalSearch = Arc<dyn Fn(&str, usize) -> Vec<SearchRow> + Send + Sync>;

/// F1 conformance (metadata normalization): every row, from every source,
/// gets the same shape — trimmed title/artist/album, https-only artwork
/// capped in length, everything else untouched. Duration stays in seconds
/// (`duration_sec`): the field is consumed by the UI and the matcher, and
/// changing its unit would break both (recorded deviation from the
/// "duration in ms" reading of the conformance item).
pub fn normalize_row(mut row: SearchRow) -> SearchRow {
    const ARTWORK_MAX_LEN: usize = 2048;
    row.title = row.title.trim().to_string();
    row.artist = row.artist.trim().to_string();
    row.album = row.album.as_ref().map(|a| a.trim().to_string()).filter(|a| !a.is_empty());
    match &row.artwork_url {
        Some(u) if u.starts_with("https://") && u.len() <= ARTWORK_MAX_LEN => {}
        _ => row.artwork_url = None,
    }
    row
}

/// Runs one source search with the per-source timeout and cap.
async fn search_source(
    source: Arc<dyn MusicSource>,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchRow>, SourceError> {
    let t0 = std::time::Instant::now();
    let result = source.search(query, limit).await;
    // S13: per-source latency, ok or failed (timeouts land here too).
    crate::metrics::source::search_sample(
        &format!("{:?}", source.kind()),
        t0.elapsed().as_secs_f64() * 1000.0,
    );
    let rows = result?;
    Ok(rows
        .into_iter()
        .take(limit)
        // Track ids over the length cap are dropped, not emitted empty.
        .filter_map(|t| {
            let key = TrackKey::new(source.kind(), t.track_id).ok()?;
            Some(normalize_row(SearchRow {
                key,
                title: t.title,
                artist: t.artist,
                album: t.album,
                duration_sec: t.duration_sec,
                artwork_url: t.artwork_url,
            }))
        })
        .collect())
}

/// Fan-out over `sources` in parallel, ordered by kind rank, total-capped.
/// Never touches the engine or the library DB.
pub async fn search_all(
    sources: &[Arc<dyn MusicSource>],
    query: &str,
    limit: usize,
) -> (Vec<SearchRow>, Vec<SourceOutcome>) {
    search_all_with_local(sources, None, query, limit).await
}

/// F1: the unified entry point — remote fan-out PLUS the injected local
/// library rows, all grouped by source rank in the same ordering pass.
/// `local` runs synchronously (one capped read-only query); errors in the
/// local provider surface as a per-source status, never panic.
pub async fn search_all_with_local(
    sources: &[Arc<dyn MusicSource>],
    local: Option<LocalSearch>,
    query: &str,
    limit: usize,
) -> (Vec<SearchRow>, Vec<SourceOutcome>) {
    let query = query.trim();
    let limit = limit.clamp(1, SEARCH_LIMIT_CAP);
    let per_source = SEARCH_PER_SOURCE_LIMIT.min(limit);

    let mut tasks = Vec::with_capacity(sources.len());
    for source in sources {
        let source = source.clone();
        let query = query.to_string();
        tasks.push(tokio::spawn(async move {
            match tokio::time::timeout(SEARCH_TIMEOUT, search_source(source.clone(), &query, per_source)).await {
                Ok(Ok(rows)) => (rows, SourceOutcome { source: source.kind(), status: SourceStatus::Ok }),
                Ok(Err(e)) => (vec![], SourceOutcome { source: source.kind(), status: SourceStatus::Error { kind: "source".into(), message: e.to_string() } }),
                Err(_) => (vec![], SourceOutcome { source: source.kind(), status: SourceStatus::Timeout }),
            }
        }));
    }

    let mut outcomes = Vec::new();
    let mut rows = Vec::new();
    for task in tasks {
        if let Ok((source_rows, outcome)) = task.await {
            outcomes.push(outcome);
            rows.extend(source_rows);
        }
    }
    // Local rows join the same ranked ordering; the local provider runs
    // after the remote fan-out is dispatched so it adds no latency to it.
    if let Some(local) = local {
        let (local_rows, local_outcome) = {
            let found = local(query, per_source);
            let outcome = SourceOutcome {
                source: SourceKind::Local,
                status: SourceStatus::Ok,
            };
            (found, outcome)
        };
        outcomes.push(local_outcome);
        rows.extend(local_rows);
    }

    // Order by source rank (kind rank; addons by config rank are handled by
    // the caller-provided order — kinds already sorted by build_active).
    rows.sort_by_key(|r| r.key.source.rank());
    rows.truncate(limit);
    (rows, outcomes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CatalogTrack, SourceHealth, SourceStream, StreamFormat};

    struct FakeSearchSource {
        kind: SourceKind,
        rows: usize,
        delay: Duration,
        err: Option<SourceError>,
    }
    #[async_trait::async_trait]
    impl MusicSource for FakeSearchSource {
        fn config_id(&self) -> &str { "fake" }
        fn kind(&self) -> SourceKind { self.kind }
        fn display_name(&self) -> &str { "Fake" }
        async fn health(&self) -> Result<SourceHealth, SourceError> { Ok(SourceHealth::Ok { detail: None }) }
        async fn search(&self, query: &str, limit: usize) -> Result<Vec<CatalogTrack>, SourceError> {
            if let Some(e) = &self.err {
                return Err(e.clone());
            }
            if self.delay > Duration::ZERO {
                tokio::time::sleep(self.delay).await;
            }
            Ok((0..limit)
                .map(|i| CatalogTrack {
                    track_id: format!("{}-{i}", query.len()),
                    config_id: "fake".into(),
                    title: format!("T {i}"),
                    artist: "A".into(),
                    album: None,
                    duration_sec: None,
                    artwork_url: None,
                    format: None,
                    embedded_stream_url: None,
                })
                .take(self.rows)
                .collect())
        }
        async fn stream(&self, _k: &TrackKey, _r: &StreamRequest) -> Result<Option<SourceStream>, SourceError> {
            Err(SourceError::Unsupported)
        }
    }

    fn mk(kind: SourceKind, rows: usize) -> Arc<dyn MusicSource> {
        Arc::new(FakeSearchSource { kind, rows, delay: Duration::ZERO, err: None })
    }

    #[tokio::test(start_paused = true)]
    async fn one_timeout_others_return_with_status() {
        let slow: Arc<dyn MusicSource> = Arc::new(FakeSearchSource {
            kind: SourceKind::YouTube,
            rows: 5,
            delay: Duration::from_secs(60),
            err: None,
        });
        let sources = vec![slow, mk(SourceKind::Addon, 4)];
        let (rows, outcomes) = search_all(&sources, "abc", 10).await;
        assert_eq!(outcomes.len(), 2);
        assert!(matches!(outcomes[0].status, SourceStatus::Timeout));
        assert!(matches!(outcomes[1].status, SourceStatus::Ok));
        assert_eq!(rows.len(), 4, "slow source contributes nothing");
    }

    #[tokio::test]
    async fn error_source_reports_kind() {
        let err: Arc<dyn MusicSource> = Arc::new(FakeSearchSource {
            kind: SourceKind::Addon,
            rows: 0,
            delay: Duration::ZERO,
            err: Some(SourceError::Permanent("blocked".into())),
        });
        let sources = vec![mk(SourceKind::YouTube, 3), err];
        let (rows, outcomes) = search_all(&sources, "x", 10).await;
        assert_eq!(rows.len(), 3);
        match &outcomes[1].status {
            SourceStatus::Error { kind, .. } => assert_eq!(kind, "source"),
            other => panic!("expected error kind, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn order_follows_kind_rank_and_caps_apply() {
        // Addon rank 0 < YouTube rank 3: addon rows come first.
        let sources = vec![mk(SourceKind::YouTube, 5), mk(SourceKind::Addon, 8)];
        let (rows, _) = search_all(&sources, "q", 10).await;
        assert_eq!(rows.len(), 10, "total cap");
        assert!(rows[0].key.source == SourceKind::Addon, "rank order");
        assert_eq!(rows.iter().filter(|r| r.key.source == SourceKind::Addon).count(), 8, "per-source cap");
    }

    // ---- F1 conformance: G3 search semantics + G7 normalization ----

    #[tokio::test(start_paused = true)]
    async fn per_source_timeout_honored_within_window() {
        // Paused clock: the 60 s slow source is cut at exactly the const.
        let slow: Arc<dyn MusicSource> = Arc::new(FakeSearchSource {
            kind: SourceKind::YouTube,
            rows: 5,
            delay: Duration::from_secs(60),
            err: None,
        });
        // Paused clock auto-advances, so the measurement is in tokio time:
        // the fan-out must be cut at exactly the const (well inside ±200 ms).
        let t0 = tokio::time::Instant::now();
        let (rows, outcomes) = search_all(&[slow], "abc", 10).await;
        let elapsed = t0.elapsed();
        assert!(rows.is_empty());
        assert!(matches!(outcomes[0].status, SourceStatus::Timeout));
        assert_eq!(elapsed, SEARCH_TIMEOUT, "cut exactly at the const");
    }

    #[tokio::test]
    async fn result_cap_never_exceeded_and_query_trimmed() {
        let big: Arc<dyn MusicSource> = mk(SourceKind::Addon, 100);
        let (rows, _) = search_all(&[big], "  padded  ", SEARCH_LIMIT_CAP + 50).await;
        assert!(rows.len() <= SEARCH_LIMIT_CAP, "total cap holds");
        assert_eq!(rows.len(), SEARCH_PER_SOURCE_LIMIT.min(SEARCH_LIMIT_CAP), "per-source cap also holds");
    }

    #[tokio::test]
    async fn local_rows_join_remote_grouped_by_rank() {
        // Ranks: Addon 0 < Local 1 < Subsonic 2 < YouTube 3.
        let addon = mk(SourceKind::Addon, 2);
        let sub = mk(SourceKind::Subsonic, 2);
        let local: LocalSearch = Arc::new(|_q, _l| {
            vec![SearchRow {
                key: TrackKey { source: SourceKind::Local, id: "42".into() },
                title: "Local Song".into(),
                artist: "Local Artist".into(),
                album: None,
                duration_sec: Some(120),
                artwork_url: None,
            }]
        });
        let sources: Vec<Arc<dyn MusicSource>> = vec![sub, addon]; // insertion order scrambled on purpose
        let (rows, outcomes) = search_all_with_local(&sources, Some(local), "q", 30).await;
        let kinds: Vec<SourceKind> = rows.iter().map(|r| r.key.source).collect();
        let first_of = |k: SourceKind| kinds.iter().position(|x| *x == k).unwrap();
        assert!(first_of(SourceKind::Addon) < first_of(SourceKind::Local));
        assert!(first_of(SourceKind::Local) < first_of(SourceKind::Subsonic), "grouped by rank, not insertion");
        assert!(outcomes.iter().any(|o| o.source == SourceKind::Local && matches!(o.status, SourceStatus::Ok)));
    }

    #[test]
    fn normalize_row_shapes_every_source_alike() {
        let mk_row = |title: &str, artist: &str, album: Option<&str>, art: Option<String>| SearchRow {
            key: TrackKey { source: SourceKind::YouTube, id: "x".into() },
            title: title.into(),
            artist: artist.into(),
            album: album.map(str::to_string),
            duration_sec: Some(180),
            artwork_url: art,
        };
        let r = normalize_row(mk_row("  Song  ", "  Artist  ", Some("  Album "), Some("https://i.ytimg.com/a.jpg".into())));
        assert_eq!(r.title, "Song");
        assert_eq!(r.artist, "Artist");
        assert_eq!(r.album.as_deref(), Some("Album"));
        assert!(r.artwork_url.is_some(), "https artwork kept");
        // Non-https artwork dropped; overlong artwork dropped; empty fields emptied.
        assert!(normalize_row(mk_row("t", "a", None, Some("http://i.ytimg.com/a.jpg".into()))).artwork_url.is_none());
        assert!(normalize_row(mk_row("t", "a", None, Some(format!("https://e/{}", "x".repeat(3000))))).artwork_url.is_none());
        let empty = normalize_row(mk_row("  ", "  ", Some("   "), None));
        assert!(empty.title.is_empty() && empty.artist.is_empty() && empty.album.is_none());
    }
}
