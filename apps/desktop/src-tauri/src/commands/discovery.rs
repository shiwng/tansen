use crate::StateHandle;
use tauri::command;

use super::err;

#[command]
pub async fn yt_search(
    state: StateHandle<'_>,
    query: String,
) -> Result<Vec<app_core::yt_handle::CatalogRow>, String> {
    let yt = state.yt().map_err(err)?;
    tauri::async_runtime::spawn_blocking(move || yt.search(&query, 30))
        .await
        .map_err(err)?
}

// Discovery (YouTube sidecar; read-only, unauthenticated)

/// Typeahead rows for discovery. Query validated + capped at the IPC
/// boundary; the per-call deadline lives in the client (`DISCOVERY_TIMEOUT`).
#[command]
pub async fn discovery_suggest(
    state: StateHandle<'_>,
    query: String,
) -> Result<app_core::yt_handle::DiscoverySuggest, String> {
    let query = query.trim().to_string();
    if query.is_empty() {
        return Err("query is empty".into());
    }
    if query.len() > sources::search::SEARCH_MAX_QUERY {
        return Err(format!(
            "query too long (cap {})",
            sources::search::SEARCH_MAX_QUERY
        ));
    }
    let yt = state.yt().map_err(err)?;
    tauri::async_runtime::spawn_blocking(move || yt.suggest(&query))
        .await
        .map_err(err)?
}

/// Filtered anonymous YouTube catalogue search plus capped local-library rows.
#[command]
pub async fn discovery_search(
    state: StateHandle<'_>,
    query: String,
    filter: String,
    continuation: Option<String>,
    limit: usize,
) -> Result<
    (
        sources::yt::DiscoverySearchPage,
        Vec<sources::search::SearchRow>,
    ),
    String,
> {
    let query = query.trim().to_string();
    if query.is_empty() {
        return Err("query is empty".into());
    }
    if query.len() > sources::search::SEARCH_MAX_QUERY {
        return Err(format!(
            "query too long (cap {})",
            sources::search::SEARCH_MAX_QUERY
        ));
    }
    if !(1..=sources::search::SEARCH_LIMIT_CAP).contains(&limit) {
        return Err(format!(
            "limit out of range (1..={})",
            sources::search::SEARCH_LIMIT_CAP
        ));
    }
    if !matches!(
        filter.as_str(),
        "all" | "songs" | "videos" | "albums" | "artists" | "playlists"
    ) {
        return Err("invalid search filter".into());
    }
    if continuation.is_some()
        && !continuation
            .as_deref()
            .is_some_and(sources::yt::valid_continuation)
    {
        return Err("invalid continuation".into());
    }
    let yt = state.yt().map_err(err)?;
    let search_query = query.clone();
    let page = tauri::async_runtime::spawn_blocking(move || {
        yt.search_page(&search_query, &filter, continuation)
    })
    .await
    .map_err(err)??;
    let local = if app_core::state::local_files_enabled(&state.db) {
        persistence::repo::search_local(&state.db_ro, &query, limit)
            .map_err(err)?
            .into_iter()
            .filter_map(|t| {
                Some(sources::search::SearchRow {
                    key: sources::TrackKey::new(sources::SourceKind::Local, t.id.to_string())
                        .ok()?,
                    title: t.title,
                    artist: t.artist,
                    album: if t.album.is_empty() {
                        None
                    } else {
                        Some(t.album)
                    },
                    duration_sec: Some(t.duration_sec.round() as u32),
                    artwork_url: None,
                })
            })
            .collect()
    } else {
        Vec::new()
    };
    Ok((page, local))
}

/// Validates a browse id at the IPC boundary (the client re-checks).
fn discovery_id_checked(id: &str) -> Result<String, String> {
    if sources::yt::valid_browse_id(id) {
        Ok(id.to_string())
    } else {
        Err("invalid browse id (charset/length)".into())
    }
}

/// One album's first page.
#[command]
pub async fn discovery_album(
    state: StateHandle<'_>,
    id: String,
) -> Result<sources::yt::DiscoveryPage, String> {
    let id = discovery_id_checked(&id)?;
    let yt = state.yt().map_err(err)?;
    tauri::async_runtime::spawn_blocking(move || yt.album(&id))
        .await
        .map_err(err)?
}

/// One artist's page.
#[command]
pub async fn discovery_artist(
    state: StateHandle<'_>,
    id: String,
) -> Result<sources::yt::DiscoveryPage, String> {
    let id = discovery_id_checked(&id)?;
    let yt = state.yt().map_err(err)?;
    tauri::async_runtime::spawn_blocking(move || yt.artist(&id))
        .await
        .map_err(err)?
}

/// One playlist's first page.
#[command]
pub async fn discovery_playlist(
    state: StateHandle<'_>,
    id: String,
) -> Result<sources::yt::DiscoveryPage, String> {
    let id = discovery_id_checked(&id)?;
    let yt = state.yt().map_err(err)?;
    tauri::async_runtime::spawn_blocking(move || yt.playlist(&id))
        .await
        .map_err(err)?
}

#[command]
pub async fn discovery_continuation(
    state: StateHandle<'_>,
    token: String,
) -> Result<sources::yt::DiscoveryPage, String> {
    if !sources::yt::valid_continuation(&token) {
        return Err("invalid continuation".into());
    }
    let yt = state.yt().map_err(err)?;
    tauri::async_runtime::spawn_blocking(move || yt.browse_continuation(&token))
        .await
        .map_err(err)?
}

#[command]
pub async fn discovery_home(state: StateHandle<'_>) -> Result<sources::yt::DiscoveryHome, String> {
    let yt = state.yt().map_err(err)?;
    tauri::async_runtime::spawn_blocking(move || yt.home())
        .await
        .map_err(err)?
}

#[command]
pub async fn discovery_home_more(
    state: StateHandle<'_>,
    continuation: String,
) -> Result<sources::yt::DiscoveryHome, String> {
    if !sources::yt::valid_continuation(&continuation) {
        return Err("invalid continuation".into());
    }
    let yt = state.yt().map_err(err)?;
    tauri::async_runtime::spawn_blocking(move || yt.home_continuation(&continuation))
        .await
        .map_err(err)?
}

#[command]
pub async fn discovery_explore(
    state: StateHandle<'_>,
    section: String,
) -> Result<sources::yt::DiscoveryHome, String> {
    if !matches!(section.as_str(), "explore" | "new-releases") {
        return Err("invalid explore section".into());
    }
    let yt = state.yt().map_err(err)?;
    tauri::async_runtime::spawn_blocking(move || yt.explore(&section))
        .await
        .map_err(err)?
}

#[command]
pub async fn discovery_radio(
    state: StateHandle<'_>,
    video_id: String,
) -> Result<Vec<sources::yt::DiscoveryTrack>, String> {
    if !sources::yt::valid_browse_id(&video_id) {
        return Err("invalid video id".into());
    }
    let yt = state.yt().map_err(err)?;
    let key = sources::TrackKey::new(sources::SourceKind::YouTube, video_id).map_err(err)?;
    tauri::async_runtime::spawn_blocking(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(err)?;
        rt.block_on(yt.source.related(&key)).map_err(err)
    })
    .await
    .map_err(err)?
}

/// Multi-source search. Validation: trimmed, ≤ SEARCH_MAX_QUERY,
/// limit in 1..=SEARCH_LIMIT_CAP. Stale searches are cancelled by token:
/// when a newer request arrives, the older in-flight result is dropped.
static SEARCH_TOKEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[command]
pub async fn search_tracks(
    state: StateHandle<'_>,
    query: String,
    limit: usize,
) -> Result<
    (
        Vec<sources::search::SearchRow>,
        Vec<sources::search::SourceOutcome>,
    ),
    String,
> {
    let query = query.trim().to_string();
    if query.is_empty() {
        return Err("query is empty".into());
    }
    if query.len() > sources::search::SEARCH_MAX_QUERY {
        return Err(format!(
            "query too long (cap {})",
            sources::search::SEARCH_MAX_QUERY
        ));
    }
    if !(1..=sources::search::SEARCH_LIMIT_CAP).contains(&limit) {
        return Err(format!(
            "limit out of range (1..={})",
            sources::search::SEARCH_LIMIT_CAP
        ));
    }
    let token = SEARCH_TOKEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    let db = state.db.clone();
    let sources_vec = tauri::async_runtime::spawn_blocking(move || {
        sources::registry::build_active(&db).map_err(|e| e.to_string())
    })
    .await
    .map_err(err)??;

    // Unified search — local library rows join the remote fan-out
    // through the injected read-only seam (rank-ordered with the rest).
    let local_on = app_core::state::local_files_enabled(&state.db);
    let db_ro = state.db_ro.clone();
    let local: sources::search::LocalSearch = std::sync::Arc::new(move |q, cap| {
        if !local_on {
            return Vec::new();
        }
        persistence::repo::search_local(&db_ro, q, cap)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|t| {
                Some(sources::search::SearchRow {
                    key: sources::TrackKey::new(sources::SourceKind::Local, t.id.to_string())
                        .ok()?,
                    title: t.title,
                    artist: t.artist,
                    album: if t.album.is_empty() {
                        None
                    } else {
                        Some(t.album)
                    },
                    duration_sec: Some(t.duration_sec.round() as u32),
                    artwork_url: None,
                })
            })
            .collect()
    });

    let (rows, statuses) =
        sources::search::search_all_with_local(&sources_vec, Some(local), &query, limit).await;

    // Stale: a newer search arrived while this one ran → drop results.
    if SEARCH_TOKEN.load(std::sync::atomic::Ordering::Relaxed) != token {
        return Ok((vec![], vec![]));
    }
    Ok((rows, statuses))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test code")]
mod discovery_tests {
    use super::*;

    /// Conformance (G3): the stale-search cancellation condition — a
    /// superseded token no longer matches the current one.
    #[test]
    fn stale_search_token_superseded() {
        let first = SEARCH_TOKEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        let second = SEARCH_TOKEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        assert_ne!(
            SEARCH_TOKEN.load(std::sync::atomic::Ordering::Relaxed),
            first,
            "first search is stale"
        );
        assert_eq!(
            SEARCH_TOKEN.load(std::sync::atomic::Ordering::Relaxed),
            second
        );
    }
}
