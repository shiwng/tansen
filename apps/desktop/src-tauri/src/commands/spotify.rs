use tauri::command;

use super::err;

// Spotify playlist import
//
// No-auth import of PUBLIC playlists through Spotify's embed page — the same
// document Spotify serves for its embeddable player. The Web API now demands
// a user token for /v1/playlists (401 without one), and the embed exposes at
// most the first 100 tracks, so a playlist at that cap is REFUSED rather
// than imported incomplete. No Spotify audio is touched: rows carry metadata
// only, and playback resolves through Tansen's own sources.

const SPOTIFY_EMBED_URL: &str = "https://open.spotify.com/embed/playlist/";

/// Rows the embed document shows before it truncates.
const SPOTIFY_EMBED_CAP: usize = 100;

/// Ceiling for one import, so a pathological playlist cannot blow up the app.
pub(crate) const SPOTIFY_IMPORT_MAX: usize = 5000;

// The web player's own GraphQL, used for the rows the embed hides. The embed
// page hands out an anonymous bearer; the persisted-query hash rotates with
// Spotify's schema, and a 400 falls back to the embed's first 100 rows.
const SPOTIFY_PATHFINDER_URL: &str = "https://api-partner.spotify.com/pathfinder/v2/query";

const SPOTIFY_PLAYLIST_HASH: &str =
    "a65e12194ed5fc443a1cdebed5fabe33ca5b07b987185d63c72483867ad13cb4";

// The UA the embed fetch was verified with (a bare client label also 200s;
// this is the conservative one).
const SPOTIFY_EMBED_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0 Safari/537.36";

/// One imported Spotify track: metadata only, never audio.
#[derive(Debug, serde::Serialize)]
pub struct SpotifyImportTrack {
    /// Spotify track id (the `spotify:track:` uri's tail).
    pub id: String,
    pub title: String,
    /// Comma-joined artist names, as the embed shows them.
    pub artist: String,
    pub duration_sec: f64,
    /// False when Spotify itself marks the row unplayable (market, removal).
    pub playable: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct SpotifyImportPlaylist {
    pub name: String,
    pub artwork_url: Option<String>,
    /// Non-music rows skipped while parsing (podcast episodes, local files,
    /// empty titles) — reported so the summary can be honest about them.
    pub skipped: usize,
    /// True only when Spotify's longer list could not be read and the import
    /// is the embed's first 100 rows.
    pub truncated: bool,
    pub tracks: Vec<SpotifyImportTrack>,
}

/// The 22-character base62 id a Spotify playlist URL/URI carries.
fn spotify_playlist_id_valid(id: &str) -> bool {
    id.len() == 22 && id.chars().all(|c| c.is_ascii_alphanumeric())
}

#[command]
pub async fn spotify_playlist_fetch(playlist_id: String) -> Result<SpotifyImportPlaylist, String> {
    let playlist_id = playlist_id.trim();
    if !spotify_playlist_id_valid(playlist_id) {
        return Err("that is not a Spotify playlist id".into());
    }
    let url = format!("{SPOTIFY_EMBED_URL}{playlist_id}");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent(SPOTIFY_EMBED_UA)
        .build()
        .map_err(err)?;
    // Rate limits: Spotify answers 429 with Retry-After; honor it once so a
    // burst of imports doesn't fail outright, then report honestly.
    let resp = client.get(&url).send().await.map_err(err)?;
    let resp = if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let wait = resp
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok())
            .unwrap_or(2)
            .clamp(1, 10);
        tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
        client.get(&url).send().await.map_err(err)?
    } else {
        resp
    };
    let status = resp.status();
    let html = crate::bounded_body::text(resp, crate::bounded_body::TEXT_MAX_BYTES)
        .await
        .map_err(err)?;
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(
            "Spotify is rate-limiting this connection — wait a minute and try again".into(),
        );
    }
    if !status.is_success() {
        return Err(format!("Spotify embed fetch failed: {status}"));
    }
    let page = parse_spotify_embed_page(&html)?;
    let mut playlist = SpotifyImportPlaylist {
        name: page.name,
        artwork_url: page.artwork_url,
        skipped: page.skipped,
        truncated: page.truncated,
        tracks: page.tracks,
    };
    // The embed hides everything past its 100 rows; the web player's graph has
    // the rest, under the anonymous bearer the same page hands us. Any failure
    // there keeps the embed rows (reported as truncated) instead of the import.
    if let Some(token) = page.access_token {
        if let Ok((tracks, skipped)) = spotify_pathfinder_tracks(&client, &token, playlist_id).await
        {
            if !tracks.is_empty() {
                playlist.tracks = tracks;
                playlist.skipped = skipped;
                playlist.truncated = false;
            }
        }
    }
    Ok(playlist)
}

/// Everything one embed document yields.
pub(crate) struct SpotifyEmbedPage {
    pub name: String,
    pub artwork_url: Option<String>,
    /// Anonymous bearer for `SPOTIFY_PATHFINDER_URL`; absent if Spotify stops
    /// shipping it in the page settings.
    pub access_token: Option<String>,
    pub skipped: usize,
    /// The embed's list is capped, so `>= SPOTIFY_EMBED_CAP` means rows are
    /// missing until the graph call replaces them.
    pub truncated: bool,
    pub tracks: Vec<SpotifyImportTrack>,
}

/// Fetches every page of the playlist through Spotify's own paginated graph.
/// Rows are the same shape the embed shows, just not cut at 100.
async fn spotify_pathfinder_tracks(
    client: &reqwest::Client,
    token: &str,
    playlist_id: &str,
) -> Result<(Vec<SpotifyImportTrack>, usize), String> {
    let uri = format!("spotify:playlist:{playlist_id}");
    let mut tracks = Vec::new();
    let mut skipped = 0usize;
    let mut offset = 0usize;
    while offset < SPOTIFY_IMPORT_MAX {
        let body = serde_json::json!({
            "operationName": "fetchPlaylistContents",
            "variables": {
                "uri": uri,
                "offset": offset,
                "limit": SPOTIFY_EMBED_CAP,
            },
            "extensions": {
                "persistedQuery": { "version": 1, "sha256Hash": SPOTIFY_PLAYLIST_HASH }
            },
        });
        let resp = client
            .post(SPOTIFY_PATHFINDER_URL)
            .bearer_auth(token)
            .header("app-platform", "WebPlayer")
            .json(&body)
            .send()
            .await
            .map_err(err)?;
        if !resp.status().is_success() {
            return Err(format!("Spotify playlist paging failed: {}", resp.status()));
        }
        let body = crate::bounded_body::text(resp, crate::bounded_body::TEXT_MAX_BYTES)
            .await
            .map_err(err)?;
        let json: serde_json::Value = serde_json::from_str(&body).map_err(err)?;
        let content = &json["data"]["playlistV2"]["content"];
        let items = content["items"]
            .as_array()
            .ok_or("could not read a playlist page from Spotify")?;
        if items.is_empty() {
            break;
        }
        for item in items {
            match pathfinder_track(item) {
                Some(track) => tracks.push(track),
                None => skipped += 1,
            }
        }
        offset += items.len();
        let total = content["totalCount"].as_u64().unwrap_or(0) as usize;
        if total == 0 || offset >= total {
            break;
        }
    }
    tracks.truncate(SPOTIFY_IMPORT_MAX);
    Ok((tracks, skipped))
}

/// One `fetchPlaylistContents` item → an import row; `None` for episodes,
/// local files and rows without a title.
fn pathfinder_track(item: &serde_json::Value) -> Option<SpotifyImportTrack> {
    let data = &item["itemV2"]["data"];
    let id = data["uri"]
        .as_str()?
        .strip_prefix("spotify:track:")?
        .to_string();
    let title = data["name"].as_str()?.trim().to_string();
    if title.is_empty() {
        return None;
    }
    let artist = data["artists"]["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|a| a["profile"]["name"].as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    Some(SpotifyImportTrack {
        id,
        title,
        artist,
        duration_sec: data["trackDuration"]["totalMilliseconds"]
            .as_f64()
            .unwrap_or(0.0)
            / 1000.0,
        playable: data["playability"]["playable"].as_bool().unwrap_or(false),
    })
}

/// Parses the embed document: one `__NEXT_DATA__` JSON island holds the
/// playlist entity (name, cover art, ordered track list). Pure so tests run
/// without network.
pub(crate) fn parse_spotify_embed_page(html: &str) -> Result<SpotifyEmbedPage, String> {
    const ISLAND_OPEN: &str = "<script id=\"__NEXT_DATA__\" type=\"application/json\">";
    let start = html
        .find(ISLAND_OPEN)
        .ok_or("Spotify returned no playlist data — is the playlist public?")?;
    let rest = &html[start + ISLAND_OPEN.len()..];
    let end = rest
        .find("</script>")
        .ok_or("Spotify playlist data is truncated")?;
    let data: serde_json::Value = serde_json::from_str(&rest[..end])
        .map_err(|e| format!("could not parse Spotify playlist data: {e}"))?;

    // A missing/invalid id lands on an error page ({ status, title, ... }).
    if data["props"]["pageProps"].get("status").is_some() {
        return Err("playlist not found or not public".into());
    }
    let page_props = &data["props"]["pageProps"];
    let access_token = page_props["state"]["settings"]["session"]["accessToken"]
        .as_str()
        .map(str::to_string);
    let entity = &page_props["state"]["data"]["entity"];
    let name = entity["name"].as_str().unwrap_or("").trim().to_string();
    if name.is_empty() {
        return Err("could not read the playlist from Spotify's data (page format changed)".into());
    }
    // Cover art sources come in several sizes; take the widest. Editorial
    // playlists leave `coverArt` null and carry the image in visualIdentity.
    let artwork_url = [
        &entity["coverArt"]["sources"],
        &entity["visualIdentity"]["image"],
    ]
    .iter()
    .find_map(|sources| {
        sources.as_array().and_then(|sources| {
            sources
                .iter()
                .filter_map(|s| {
                    s["url"]
                        .as_str()
                        .map(|u| (s["width"].as_u64().unwrap_or(0), u))
                })
                .max_by_key(|(w, _)| *w)
                .map(|(_, u)| u.to_string())
        })
    });
    let raw = entity["trackList"]
        .as_array()
        .ok_or("could not read the track list from Spotify's data (page format changed)")?;
    let truncated = raw.len() >= SPOTIFY_EMBED_CAP;
    let mut tracks = Vec::with_capacity(raw.len());
    let mut skipped = 0usize;
    for row in raw {
        let Some(id) = row["uri"]
            .as_str()
            .and_then(|u| u.strip_prefix("spotify:track:"))
        else {
            skipped += 1; // podcast episode or local file
            continue;
        };
        let title = row["title"].as_str().unwrap_or("").trim().to_string();
        if title.is_empty() {
            skipped += 1;
            continue;
        }
        tracks.push(SpotifyImportTrack {
            id: id.to_string(),
            title,
            artist: row["subtitle"].as_str().unwrap_or("").trim().to_string(),
            duration_sec: row["duration"].as_f64().unwrap_or(0.0) / 1000.0,
            playable: row["playabilityReason"].as_str() == Some("PLAYABLE"),
        });
    }
    if tracks.is_empty() {
        return Err("this playlist has no importable music tracks".into());
    }
    Ok(SpotifyEmbedPage {
        name,
        artwork_url,
        access_token,
        skipped,
        truncated,
        tracks,
    })
}

/// The embed rows alone, without the graph top-up (what tests and the
/// fallback path see).
pub(crate) fn parse_spotify_embed(html: &str) -> Result<SpotifyImportPlaylist, String> {
    let page = parse_spotify_embed_page(html)?;
    Ok(SpotifyImportPlaylist {
        name: page.name,
        artwork_url: page.artwork_url,
        skipped: page.skipped,
        truncated: page.truncated,
        tracks: page.tracks,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test code")]
mod spotify_import_tests {
    use super::*;

    fn island(json: &str) -> String {
        format!("<html><head></head><body><script id=\"__NEXT_DATA__\" type=\"application/json\">{json}</script></body></html>")
    }

    fn entity_json(tracks: &[serde_json::Value]) -> serde_json::Value {
        serde_json::json!({
            "props": { "pageProps": { "state": { "data": { "entity": {
                "name": "Test Playlist",
                "coverArt": { "sources": [
                    { "url": "https://i.scdn.co/image/small", "width": 640 },
                    { "url": "https://i.scdn.co/image/large", "width": 1000 }
                ] },
                "trackList": tracks
            } } } } }
        })
    }

    fn track(uri: &str, title: &str, playable: bool) -> serde_json::Value {
        serde_json::json!({
            "uri": uri, "title": title, "subtitle": "An Artist",
            "duration": 223_069u64,
            "playabilityReason": if playable { "PLAYABLE" } else { "NOT_AVAILABLE_MARKET" }
        })
    }

    #[test]
    fn parses_name_art_and_track_rows() {
        let page = island(
            &entity_json(&[
                track("spotify:track:7bxaFZ1O3cHkgLKMsdC3xR", "Loser", true),
                track("spotify:track:0000000000000000000000", "Gone", false),
            ])
            .to_string(),
        );
        let p = parse_spotify_embed(&page).unwrap();
        assert_eq!(p.name, "Test Playlist");
        assert_eq!(
            p.artwork_url.as_deref(),
            Some("https://i.scdn.co/image/large")
        );
        assert_eq!(p.skipped, 0);
        assert_eq!(p.tracks.len(), 2);
        assert_eq!(p.tracks[0].id, "7bxaFZ1O3cHkgLKMsdC3xR");
        assert_eq!(p.tracks[0].artist, "An Artist");
        assert!((p.tracks[0].duration_sec - 223.069).abs() < 1e-9);
        assert!(p.tracks[0].playable);
        assert!(!p.tracks[1].playable);
    }

    #[test]
    fn skips_non_music_and_empty_rows() {
        let page = island(
            &entity_json(&[
                track("spotify:episode:abc", "An Episode", true),
                track("spotify:track:7bxaFZ1O3cHkgLKMsdC3xR", "", true),
                track("spotify:track:0000000000000000000000", "Real Song", true),
            ])
            .to_string(),
        );
        let p = parse_spotify_embed(&page).unwrap();
        assert_eq!(p.tracks.len(), 1);
        assert_eq!(p.skipped, 2);
    }

    #[test]
    fn a_full_embed_page_is_marked_truncated_not_refused() {
        let tracks: Vec<serde_json::Value> = (0..SPOTIFY_EMBED_CAP)
            .map(|i| {
                track(
                    "spotify:track:0000000000000000000000",
                    &format!("S{i}"),
                    true,
                )
            })
            .collect();
        let page = island(&entity_json(&tracks).to_string());
        let p = parse_spotify_embed(&page).unwrap();
        assert_eq!(p.tracks.len(), SPOTIFY_EMBED_CAP);
        assert!(p.truncated);
    }

    #[test]
    fn reads_the_anonymous_bearer_from_the_page_settings() {
        let mut value: serde_json::Value = serde_json::from_str(
            &entity_json(&[track("spotify:track:7bxaFZ1O3cHkgLKMsdC3xR", "Loser", true)])
                .to_string(),
        )
        .unwrap();
        value["props"]["pageProps"]["state"]["settings"] =
            serde_json::json!({ "session": { "accessToken": "anon-token" } });
        let page = parse_spotify_embed_page(&island(&value.to_string())).unwrap();
        assert_eq!(page.access_token.as_deref(), Some("anon-token"));
    }

    #[test]
    fn maps_a_pathfinder_item_and_skips_episodes() {
        let item = serde_json::json!({
            "itemV2": { "data": {
                "__typename": "Track", "uri": "spotify:track:7bxaFZ1O3cHkgLKMsdC3xR",
                "name": "Loser",
                "artists": { "items": [
                    { "profile": { "name": "Beck" } },
                    { "profile": { "name": "Guest" } }
                ] },
                "trackDuration": { "totalMilliseconds": 223_069u64 },
                "playability": { "playable": true, "reason": "PLAYABLE" }
            } }
        });
        let t = pathfinder_track(&item).unwrap();
        assert_eq!(t.id, "7bxaFZ1O3cHkgLKMsdC3xR");
        assert_eq!(t.artist, "Beck, Guest");
        assert!((t.duration_sec - 223.069).abs() < 1e-9);
        assert!(t.playable);
        let episode = serde_json::json!({
            "itemV2": { "data": { "uri": "spotify:episode:x", "name": "An Episode" } }
        });
        assert!(pathfinder_track(&episode).is_none());
    }

    #[test]
    fn error_pages_and_garbage_fail_with_clear_messages() {
        // Not-found page shape.
        let nf = island(r#"{"props":{"pageProps":{"status":404,"title":"Page Not Found"}}}"#);
        assert!(parse_spotify_embed(&nf)
            .unwrap_err()
            .contains("not found or not public"));
        // No data island at all.
        assert!(parse_spotify_embed("<html></html>")
            .unwrap_err()
            .contains("no playlist data"));
        // Entity without a name.
        let empty =
            island(r#"{"props":{"pageProps":{"state":{"data":{"entity":{"trackList":[]}}}}}}"#);
        assert!(parse_spotify_embed(&empty)
            .unwrap_err()
            .contains("could not read the playlist"));
    }

    #[test]
    fn playlist_ids_are_strictly_22_base62() {
        assert!(spotify_playlist_id_valid("4hSEJgIGVwZP350BO6MvSk"));
        assert!(
            !spotify_playlist_id_valid("4hSEJgIGVwZP350BO6MvS"),
            "21 chars"
        );
        assert!(
            !spotify_playlist_id_valid("4hSEJgIGVwZP350BO6MvSk!"),
            "non-base62"
        );
        assert!(!spotify_playlist_id_valid("../../etc/passwd"));
        assert!(!spotify_playlist_id_valid(""));
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test code")]
mod spotify_live_tests {
    use super::*;

    /// Real-network check against Spotify's live embed (skipped in CI runs:
    /// `cargo test -p tansen -- --ignored`). Uses Spotify's own editorial
    /// 50-track playlist; asserts the full pipeline: fetch → parse → rows.
    #[tokio::test]
    #[ignore = "requires network; run explicitly"]
    async fn live_embed_fetch_parses_a_real_playlist() {
        let playlist = spotify_playlist_fetch("37i9dQZF1DX10zKzsJ2jva".into())
            .await
            .expect("live fetch should succeed");
        assert_eq!(playlist.name, "Viva Latino");
        assert!(playlist
            .artwork_url
            .as_deref()
            .unwrap_or("")
            .starts_with("https://"));
        assert!(
            (1..=SPOTIFY_IMPORT_MAX).contains(&playlist.tracks.len()),
            "served tracks: {}",
            playlist.tracks.len()
        );
        assert!(
            !playlist.truncated,
            "graph top-up should give the whole list"
        );
        let first = &playlist.tracks[0];
        assert!(!first.id.is_empty());
        assert!(!first.title.is_empty());
        assert!(first.duration_sec > 0.0);
    }

    /// A playlist longer than the embed's 100 rows: the graph top-up must
    /// return the rest, not a truncated import (the reported bug).
    #[tokio::test]
    #[ignore = "requires network; run explicitly"]
    async fn live_long_playlist_imports_past_the_embed_cap() {
        let playlist = spotify_playlist_fetch("37i9dQZF1DX4UtSsGT1Sbe".into())
            .await
            .expect("live fetch should succeed");
        assert_eq!(playlist.name, "All Out 80s");
        assert!(!playlist.truncated, "graph top-up should reach the end");
        assert!(
            playlist.tracks.len() > SPOTIFY_EMBED_CAP,
            "served tracks: {}",
            playlist.tracks.len()
        );
        assert!(playlist
            .tracks
            .iter()
            .all(|t| !t.id.is_empty() && t.duration_sec > 0.0));
    }

    /// A garbage id must fail at the boundary without any network call.
    #[tokio::test]
    async fn invalid_id_fails_fast() {
        let e = spotify_playlist_fetch("../../etc/passwd".into())
            .await
            .unwrap_err();
        assert!(e.contains("not a Spotify playlist id"));
    }
}
