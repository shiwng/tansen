//! Home recommendations: a cached, DB-backed listening profile blended with
//! real catalogue shelves and related-track expansion. No mock content.

use app_core::AppState;
use persistence::repo::{self, KeyRef, ListeningEvent};
use serde::Serialize;
use sources::yt::{DiscoveryArtist, DiscoveryHome, DiscoveryHomeItem, DiscoveryTrack};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(5 * 60);
const EVENT_WINDOW: i64 = 1_000;
const RECENT_LIMIT: usize = 24;
const MIX_LIMIT: usize = 20;
const SECTION_ITEM_LIMIT: usize = 20;

#[derive(Clone, Debug, Serialize)]
pub struct HomeItem {
    /// `song` / `video` play; `album` / `playlist` / `artist` navigate.
    pub kind: String,
    /// video id for songs, browse id for pages, `local:<rowid>` for library tracks.
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub thumbnail_url: Option<String>,
    pub duration_sec: Option<f64>,
    pub is_video: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct HomeSection {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    /// `tracks`, `square`, `video` or `circle`.
    pub layout: String,
    pub items: Vec<HomeItem>,
}

#[derive(Clone, Debug, Serialize)]
pub struct HomeFeed {
    /// True when at least one section comes from the user's own listening.
    pub personalized: bool,
    /// Distinct tracks started, for the honest header line.
    pub plays: i64,
    pub top_artists: Vec<String>,
    /// Large browse tiles above the shelves.
    pub featured: Vec<HomeItem>,
    pub sections: Vec<HomeSection>,
    pub catalogue_error: Option<String>,
}

struct Profile {
    events: Vec<ListeningEvent>,
    liked_tracks: HashSet<(String, String)>,
    liked_albums: HashSet<(String, String)>,
    followed: Vec<(KeyRef, String)>,
    dismissed: HashSet<(String, String)>,
    artists: Vec<ArtistScore>,
    tracks: Vec<AggTrack>,
    recent: Vec<ListeningEvent>,
    seed: Option<String>,
}

#[derive(Clone)]
struct ArtistScore {
    name: String,
    score: f64,
    plays: i64,
    key: Option<(String, String)>,
    local: bool,
}

#[derive(Clone)]
struct AggTrack {
    event: ListeningEvent,
    score: f64,
    plays: i64,
}

fn item_key(item: &HomeItem) -> (String, String) {
    let source = if item.id.starts_with("local:") {
        "local"
    } else {
        "youtube"
    };
    (source.to_string(), item.id.clone())
}

fn map_catalogue_item(item: &DiscoveryHomeItem) -> HomeItem {
    let (kind, id) = if let Some(video_id) = item.video_id.as_deref() {
        (
            if item.is_video { "video" } else { "song" },
            video_id.to_string(),
        )
    } else if let Some(browse) = item.browse_id.as_deref() {
        let kind = if browse.starts_with("UC") {
            "artist"
        } else if browse.starts_with("VL") {
            "playlist"
        } else {
            "album"
        };
        (kind, browse.to_string())
    } else {
        ("unknown", item.title.clone())
    };
    HomeItem {
        kind: kind.to_string(),
        id,
        title: item.title.clone(),
        subtitle: item.subtitle.clone(),
        thumbnail_url: item.thumbnail_url.clone(),
        duration_sec: None,
        is_video: item.is_video,
    }
}

fn map_related(t: &DiscoveryTrack) -> HomeItem {
    HomeItem {
        kind: if t.is_video { "video" } else { "song" }.to_string(),
        id: t.key.id.clone(),
        title: t.title.clone(),
        subtitle: t.artist.clone(),
        thumbnail_url: t.thumbnail_url.clone(),
        duration_sec: t.duration_sec.map(f64::from),
        is_video: t.is_video,
    }
}

fn event_item(e: &ListeningEvent) -> HomeItem {
    HomeItem {
        kind: "song".to_string(),
        id: if e.key_source == "local" {
            format!("local:{}", e.key_id)
        } else {
            e.key_id.clone()
        },
        title: e.title.clone(),
        subtitle: e.artist.clone(),
        thumbnail_url: e.artwork_url.clone(),
        duration_sec: if e.duration_ms > 0 {
            Some(e.duration_ms as f64 / 1000.0)
        } else {
            None
        },
        is_video: false,
    }
}

fn album_item(t: &DiscoveryTrack, album_id: &str, album: &str) -> HomeItem {
    HomeItem {
        kind: "album".to_string(),
        id: album_id.to_string(),
        title: album.to_string(),
        subtitle: format!("Album • {}", t.artist),
        thumbnail_url: t.thumbnail_url.clone(),
        duration_sec: None,
        is_video: false,
    }
}

fn artist_item(artist: &DiscoveryArtist, thumbnail: Option<String>) -> Option<HomeItem> {
    let id = artist.browse_id.clone()?;
    Some(HomeItem {
        kind: "artist".to_string(),
        id,
        title: artist.name.clone(),
        subtitle: "Artist".to_string(),
        thumbnail_url: thumbnail,
        duration_sec: None,
        is_video: false,
    })
}

fn catalogue_sections(
    home: Option<DiscoveryHome>,
    explore: Option<DiscoveryHome>,
    releases: Option<DiscoveryHome>,
    dismissed: &HashSet<(String, String)>,
) -> (Vec<HomeItem>, Vec<HomeSection>) {
    let mut sections = Vec::new();
    let mut featured = Vec::new();
    let mut seen_titles = HashSet::new();
    let mut seen_items = HashSet::new();
    let mut seen_featured = HashSet::new();

    let mut add = |mut source: DiscoveryHome, fallback: &str, per_source: usize| {
        for shelf in source.shelves.drain(..).take(per_source) {
            let title = if shelf.title.trim().is_empty() {
                fallback.to_string()
            } else {
                shelf.title.clone()
            };
            if shelf.items.is_empty() || !seen_titles.insert(title.to_lowercase()) {
                continue;
            }
            let items: Vec<HomeItem> = shelf
                .items
                .iter()
                .map(map_catalogue_item)
                .filter(|item| item.kind != "unknown" && !dismissed.contains(&item_key(item)))
                .take(SECTION_ITEM_LIMIT)
                .collect();
            if items.is_empty() {
                continue;
            }
            for item in &items {
                let label = item.title.trim().to_lowercase();
                if item.thumbnail_url.is_some()
                    && featured.len() < 10
                    && seen_items.insert(item_key(item))
                    && seen_featured.insert(label)
                {
                    featured.push(item.clone());
                }
            }
            sections.push(HomeSection {
                id: format!("catalogue-{}", sections.len()),
                title,
                subtitle: shelf.subtitle.clone(),
                layout: if shelf.layout == "video" {
                    "video".to_string()
                } else if shelf.layout == "list" {
                    "tracks".to_string()
                } else {
                    "square".to_string()
                },
                items,
            });
        }
    };

    add(
        home.unwrap_or(DiscoveryHome {
            shelves: vec![],
            continuation: None,
        }),
        "Made for You",
        5,
    );
    add(
        explore.unwrap_or(DiscoveryHome {
            shelves: vec![],
            continuation: None,
        }),
        "Trending Music",
        4,
    );
    add(
        releases.unwrap_or(DiscoveryHome {
            shelves: vec![],
            continuation: None,
        }),
        "New Releases",
        4,
    );
    sections.truncate(10);
    (featured, sections)
}

fn build_profile(
    events: Vec<ListeningEvent>,
    liked_tracks: Vec<KeyRef>,
    liked_albums: Vec<KeyRef>,
    followed: Vec<(KeyRef, String)>,
    dismissed: Vec<KeyRef>,
    now: i64,
) -> Profile {
    let liked_tracks: HashSet<(String, String)> = liked_tracks
        .into_iter()
        .map(|k| (k.key_source, k.key_id))
        .collect();
    let liked_albums: HashSet<(String, String)> = liked_albums
        .into_iter()
        .map(|k| (k.key_source, k.key_id))
        .collect();
    let dismissed: HashSet<(String, String)> = dismissed
        .into_iter()
        .map(|k| (k.key_source, k.key_id))
        .collect();

    let mut by_track: HashMap<(String, String), AggTrack> = HashMap::new();
    let mut artist_scores: HashMap<String, ArtistScore> = HashMap::new();
    for event in &events {
        let key = (event.key_source.clone(), event.key_id.clone());
        let age_days = ((now - event.started_at).max(0) as f64) / 86_400.0;
        // Two-week half-life: recent listening outranks older habits.
        let recency = 0.5f64.powf(age_days / 14.0).max(0.15);
        let engagement = if event.duration_ms > 0 {
            (event.played_ms as f64 / event.duration_ms as f64).clamp(0.0, 1.0)
        } else {
            0.6
        };
        // A short stop is a weak negative, never a dislike; one play is weak.
        let skip = event.duration_ms >= 60_000 && event.played_ms * 4 < event.duration_ms;
        let mut weight = (0.35 + engagement + if event.completed { 0.6 } else { 0.0 }) * recency;
        if skip {
            weight *= 0.3;
        }
        if liked_tracks.contains(&key) {
            weight += 4.0;
        }

        let entry = by_track.entry(key).or_insert_with(|| AggTrack {
            event: event.clone(),
            score: 0.0,
            plays: 0,
        });
        entry.score += weight;
        entry.plays += 1;
        if event.started_at >= entry.event.started_at {
            entry.event = event.clone();
        }

        let artist_key = event.artist.trim().to_lowercase();
        if !artist_key.is_empty() {
            let artist = artist_scores
                .entry(artist_key)
                .or_insert_with(|| ArtistScore {
                    name: event.artist.clone(),
                    score: 0.0,
                    plays: 0,
                    key: None,
                    local: event.key_source == "local",
                });
            artist.score += weight;
            artist.plays += 1;
            artist.local |= event.key_source == "local";
        }
    }

    for (key, name) in &followed {
        let artist_key = name.trim().to_lowercase();
        if artist_key.is_empty() {
            continue;
        }
        artist_scores
            .entry(artist_key)
            .and_modify(|a| {
                a.score += 6.0;
                a.key = Some((key.key_source.clone(), key.key_id.clone()));
            })
            .or_insert(ArtistScore {
                name: name.clone(),
                score: 6.0,
                plays: 0,
                key: Some((key.key_source.clone(), key.key_id.clone())),
                local: key.key_source == "local",
            });
    }

    let mut artists: Vec<ArtistScore> = artist_scores.into_values().collect();
    artists.sort_by(|a, b| b.score.total_cmp(&a.score));

    let mut tracks: Vec<AggTrack> = by_track.into_values().collect();
    tracks.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| b.event.started_at.cmp(&a.event.started_at))
    });

    let mut seen = HashSet::new();
    let recent: Vec<ListeningEvent> = events
        .iter()
        .filter(|e| {
            let key = (e.key_source.clone(), e.key_id.clone());
            seen.insert(key)
        })
        .take(RECENT_LIMIT)
        .cloned()
        .collect();

    let mut seed = liked_tracks
        .iter()
        .find(|(source, _)| source == "youtube")
        .map(|(_, id)| id.clone());
    if seed.is_none() {
        seed = tracks
            .iter()
            .find(|t| t.event.key_source == "youtube")
            .map(|t| t.event.key_id.clone());
    }
    if seed.is_none() {
        seed = events
            .iter()
            .find(|e| e.key_source == "youtube")
            .map(|e| e.key_id.clone());
    }

    Profile {
        events,
        liked_tracks,
        liked_albums,
        followed,
        dismissed,
        artists,
        tracks,
        recent,
        seed,
    }
}

/// The sidecar answers 502 while its upstream session warms up; one retry
/// turns a cold-start miss into a populated section.
fn retry<T>(mut f: impl FnMut() -> Result<T, String>) -> Result<T, String> {
    match f() {
        Ok(value) => Ok(value),
        Err(_) => {
            std::thread::sleep(Duration::from_millis(900));
            f()
        }
    }
}

fn related_tracks(
    yt: Arc<app_core::yt_handle::YtHandle>,
    video_id: String,
) -> Result<Vec<DiscoveryTrack>, String> {
    let key = sources::TrackKey::new(sources::SourceKind::YouTube, video_id)
        .map_err(|e| e.to_string())?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    rt.block_on(yt.source.related(&key))
        .map_err(|e| e.to_string())
}

fn personalized_sections(
    profile: &Profile,
    related: &[DiscoveryTrack],
    now: i64,
) -> Vec<HomeSection> {
    let mut sections = Vec::new();
    let liked_album_ids: HashSet<&str> = profile
        .liked_albums
        .iter()
        .filter(|(source, _)| source == "youtube")
        .map(|(_, id)| id.as_str())
        .collect();
    let followed_names: HashSet<String> = profile
        .followed
        .iter()
        .map(|(_, name)| name.trim().to_lowercase())
        .collect();
    let top_artists: HashSet<String> = profile
        .artists
        .iter()
        .filter(|a| a.plays >= 2 || a.score >= 4.0)
        .map(|a| a.name.trim().to_lowercase())
        .collect();

    let boost = |t: &DiscoveryTrack| -> f64 {
        let artist = t.artist.trim().to_lowercase();
        let mut score = 0.0;
        if top_artists.contains(&artist) {
            score += 3.0;
        }
        if followed_names.contains(&artist) {
            score += 4.0;
        }
        if profile
            .liked_tracks
            .contains(&("youtube".to_string(), t.key.id.clone()))
        {
            score += 4.0;
        }
        if t.album_id
            .as_deref()
            .is_some_and(|id| liked_album_ids.contains(id))
        {
            score += 3.0;
        }
        score
    };

    let mut ranked: Vec<(f64, usize, &DiscoveryTrack)> = related
        .iter()
        .enumerate()
        .filter(|(_, t)| {
            t.playable
                && !profile
                    .dismissed
                    .contains(&("youtube".to_string(), t.key.id.clone()))
        })
        .map(|(i, t)| (boost(t), i, t))
        .collect();
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

    if !profile.recent.is_empty() {
        sections.push(HomeSection {
            id: "recently-played".to_string(),
            title: "Recently Played".to_string(),
            subtitle: "From your listening history".to_string(),
            layout: "tracks".to_string(),
            items: profile
                .recent
                .iter()
                .map(event_item)
                .take(RECENT_LIMIT)
                .collect(),
        });
    }

    // Made for You: the tracks you return to plus real related rows, with a
    // per-artist cap so one favourite cannot fill the shelf.
    let mut mix: Vec<HomeItem> = Vec::new();
    let mut mix_ids = HashSet::new();
    let mut per_artist: HashMap<String, usize> = HashMap::new();
    for track in &profile.tracks {
        if profile
            .dismissed
            .contains(&(track.event.key_source.clone(), track.event.key_id.clone()))
        {
            continue;
        }
        let artist = track.event.artist.trim().to_lowercase();
        if per_artist.get(&artist).copied().unwrap_or(0) >= 2 {
            continue;
        }
        let item = event_item(&track.event);
        if mix_ids.insert(item.id.clone()) {
            *per_artist.entry(artist).or_insert(0) += 1;
            mix.push(item);
        }
        if mix.len() >= 8 {
            break;
        }
    }
    for (_, _, track) in &ranked {
        let artist = track.artist.trim().to_lowercase();
        if per_artist.get(&artist).copied().unwrap_or(0) >= 2 {
            continue;
        }
        let item = map_related(track);
        if mix_ids.insert(item.id.clone()) {
            *per_artist.entry(artist).or_insert(0) += 1;
            mix.push(item);
        }
        if mix.len() >= MIX_LIMIT {
            break;
        }
    }
    if mix.len() >= 3 {
        sections.push(HomeSection {
            id: "made-for-you".to_string(),
            title: "Made for You".to_string(),
            subtitle: if profile.events.is_empty() {
                String::new()
            } else {
                "Built from your plays, likes and skips".to_string()
            },
            layout: "tracks".to_string(),
            items: mix,
        });
    }

    // Favourite Artists: repeated listening or an explicit follow, linked to a
    // real catalogue page only when one is known.
    let related_artist_ids: HashMap<String, String> = related
        .iter()
        .flat_map(|t| t.artists.iter())
        .filter_map(|a| {
            a.browse_id
                .as_ref()
                .map(|id| (a.name.trim().to_lowercase(), id.clone()))
        })
        .collect();
    // Artist artwork comes from their real tracks: related rows first, then the
    // events' own artwork. No generated avatars.
    let mut artist_art: HashMap<String, String> = HashMap::new();
    for track in related {
        let Some(thumb) = track.thumbnail_url.as_deref() else {
            continue;
        };
        artist_art
            .entry(track.artist.trim().to_lowercase())
            .or_insert_with(|| thumb.to_string());
        for artist in &track.artists {
            artist_art
                .entry(artist.name.trim().to_lowercase())
                .or_insert_with(|| thumb.to_string());
        }
    }
    for event in &profile.recent {
        if let Some(thumb) = event.artwork_url.as_deref() {
            artist_art
                .entry(event.artist.trim().to_lowercase())
                .or_insert_with(|| thumb.to_string());
        }
    }
    let mut favourites: Vec<HomeItem> = Vec::new();
    let mut favourite_names = HashSet::new();
    for artist in &profile.artists {
        if favourites.len() >= 10 {
            break;
        }
        let followed = profile
            .followed
            .iter()
            .any(|(_, name)| name.trim().eq_ignore_ascii_case(&artist.name));
        if artist.plays < 2 && !followed {
            continue;
        }
        let lower = artist.name.trim().to_lowercase();
        if !favourite_names.insert(lower.clone()) {
            continue;
        }
        let id = artist
            .key
            .as_ref()
            .map(|(_, id)| id.clone())
            .or_else(|| related_artist_ids.get(&lower).cloned())
            .or_else(|| artist.local.then(|| artist.name.clone()));
        let Some(id) = id else { continue };
        favourites.push(HomeItem {
            kind: "artist".to_string(),
            id,
            title: artist.name.clone(),
            subtitle: if followed { "Following" } else { "Artist" }.to_string(),
            thumbnail_url: artist_art.get(&lower).cloned(),
            duration_sec: None,
            is_video: false,
        });
    }
    if !favourites.is_empty() {
        sections.push(HomeSection {
            id: "favourite-artists".to_string(),
            title: "Favourite Artists".to_string(),
            subtitle: "Artists you keep coming back to".to_string(),
            layout: "circle".to_string(),
            items: favourites,
        });
    }

    // Similar Artists: artists the catalogue returns around the strongest seed.
    let mut similar: Vec<HomeItem> = Vec::new();
    let mut similar_names = HashSet::new();
    for track in related {
        for artist in &track.artists {
            if similar.len() >= 10 {
                break;
            }
            let lower = artist.name.trim().to_lowercase();
            if top_artists.contains(&lower) || !similar_names.insert(lower.clone()) {
                continue;
            }
            if let Some(item) = artist_item(artist, artist_art.get(&lower).cloned()) {
                if !profile.dismissed.contains(&item_key(&item)) {
                    similar.push(item);
                }
            }
        }
    }
    if !similar.is_empty() {
        sections.push(HomeSection {
            id: "similar-artists".to_string(),
            title: "Similar Artists".to_string(),
            subtitle: "Close to what you already play".to_string(),
            layout: "circle".to_string(),
            items: similar,
        });
    }

    // Albums You May Like: real album ids carried by related rows.
    let mut albums: Vec<HomeItem> = Vec::new();
    let mut album_ids = HashSet::new();
    for (_, _, track) in &ranked {
        if albums.len() >= 12 {
            break;
        }
        let (Some(album_id), Some(album)) = (track.album_id.as_deref(), track.album.as_deref())
        else {
            continue;
        };
        if !album_ids.insert(album_id.to_string()) {
            continue;
        }
        let item = album_item(track, album_id, album);
        if !profile.dismissed.contains(&item_key(&item)) {
            albums.push(item);
        }
    }
    if !albums.is_empty() {
        sections.push(HomeSection {
            id: "albums-you-may-like".to_string(),
            title: "Albums You May Like".to_string(),
            subtitle: "Near the artists in your rotation".to_string(),
            layout: "square".to_string(),
            items: albums,
        });
    }

    let _ = now;
    sections
}

fn fingerprint(db: &persistence::Db) -> String {
    match repo::recommendation_counts(db) {
        Ok((events, tracks, albums, follows, dismissed)) => {
            format!("{events}:{tracks}:{albums}:{follows}:{dismissed}")
        }
        Err(_) => "0:0:0:0:0".to_string(),
    }
}

fn cache() -> &'static Mutex<Option<(String, Instant, HomeFeed)>> {
    static CACHE: OnceLock<Mutex<Option<(String, Instant, HomeFeed)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Builds (or returns the cached) home feed. `refresh` bypasses the cache.
pub async fn home_feed(state: &AppState, refresh: bool) -> Result<HomeFeed, String> {
    let print = fingerprint(&state.db);
    if !refresh {
        if let Ok(guard) = cache().lock() {
            if let Some((key, at, feed)) = guard.as_ref() {
                if key == &print && at.elapsed() < CACHE_TTL {
                    return Ok(feed.clone());
                }
            }
        }
    }

    let db = state.db_ro.clone();
    let profile = tauri::async_runtime::spawn_blocking(move || {
        let now = library::scanner::now_secs();
        let events = repo::listening_events(&db, EVENT_WINDOW).map_err(|e| e.to_string())?;
        let liked_tracks = repo::liked_tracks(&db).map_err(|e| e.to_string())?;
        let liked_albums = repo::liked_albums(&db).map_err(|e| e.to_string())?;
        let followed = repo::followed_artists(&db).map_err(|e| e.to_string())?;
        let dismissed = repo::dismissed_recommendations(&db).map_err(|e| e.to_string())?;
        Ok::<Profile, String>(build_profile(
            events,
            liked_tracks,
            liked_albums,
            followed,
            dismissed,
            now,
        ))
    })
    .await
    .map_err(|e| e.to_string())??;

    let mut errors = Vec::new();
    let yt = match state.yt() {
        Ok(yt) => Some(yt),
        Err(e) => {
            errors.push(e.to_string());
            None
        }
    };
    let (home, explore, releases, related) = if let Some(yt) = yt {
        let yt_home = yt.clone();
        let yt_explore = yt.clone();
        let yt_releases = yt.clone();
        let related_seed = profile.seed.clone();
        let yt_related = yt.clone();

        let home = tauri::async_runtime::spawn_blocking(move || {
            retry(|| yt_home.home().map_err(|e| e.to_string()))
        });
        let explore = tauri::async_runtime::spawn_blocking(move || {
            retry(|| yt_explore.explore("explore").map_err(|e| e.to_string()))
        });
        let releases = tauri::async_runtime::spawn_blocking(move || {
            retry(|| {
                yt_releases
                    .explore("new-releases")
                    .map_err(|e| e.to_string())
            })
        });
        let related = tauri::async_runtime::spawn_blocking(move || match related_seed {
            Some(id) => related_tracks(yt_related, id),
            None => Ok(vec![]),
        });

        let (home, explore, releases, related) = tokio::join!(home, explore, releases, related);
        let home = match home {
            Ok(Ok(v)) => Some(v),
            Ok(Err(e)) => {
                errors.push(e);
                None
            }
            Err(e) => {
                errors.push(e.to_string());
                None
            }
        };
        let explore = match explore {
            Ok(Ok(v)) => Some(v),
            Ok(Err(e)) => {
                errors.push(e);
                None
            }
            Err(e) => {
                errors.push(e.to_string());
                None
            }
        };
        let releases = match releases {
            Ok(Ok(v)) => Some(v),
            Ok(Err(e)) => {
                errors.push(e);
                None
            }
            Err(e) => {
                errors.push(e.to_string());
                None
            }
        };
        let related = match related {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => {
                errors.push(e);
                vec![]
            }
            Err(e) => {
                errors.push(e.to_string());
                vec![]
            }
        };
        (home, explore, releases, related)
    } else {
        (None, None, None, vec![])
    };

    let (featured, mut catalogue) = catalogue_sections(home, explore, releases, &profile.dismissed);
    let mut sections = personalized_sections(&profile, &related, library::scanner::now_secs());
    sections.append(&mut catalogue);

    // Never repeat an identical item within one section.
    for section in &mut sections {
        let mut seen = HashSet::new();
        section.items.retain(|item| seen.insert(item_key(item)));
    }

    let personalized = sections.iter().any(|s| {
        matches!(
            s.id.as_str(),
            "recently-played"
                | "made-for-you"
                | "favourite-artists"
                | "similar-artists"
                | "albums-you-may-like"
        )
    });
    let plays = profile.events.len() as i64;
    let top_artists = profile
        .artists
        .iter()
        .take(4)
        .map(|a| a.name.clone())
        .collect();
    let feed = HomeFeed {
        personalized,
        plays,
        top_artists,
        featured,
        sections,
        catalogue_error: if errors.is_empty() {
            None
        } else {
            Some(errors.join("; "))
        },
    };

    if let Ok(mut guard) = cache().lock() {
        *guard = Some((print, Instant::now(), feed.clone()));
    }
    Ok(feed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(
        key: &str,
        artist: &str,
        played_ms: i64,
        completed: bool,
        started_at: i64,
    ) -> ListeningEvent {
        ListeningEvent {
            id: 1,
            key_source: "youtube".to_string(),
            key_id: key.to_string(),
            title: key.to_string(),
            artist: artist.to_string(),
            album: Some("Album".to_string()),
            artwork_url: None,
            duration_ms: 200_000,
            played_ms,
            completed,
            started_at,
        }
    }

    #[test]
    fn one_play_is_weak_and_skips_are_not_dislikes() {
        let now = 1_000_000;
        let profile = build_profile(
            vec![
                event("a", "A", 200_000, true, now),
                event("b", "B", 10_000, false, now),
            ],
            vec![],
            vec![],
            vec![],
            vec![],
            now,
        );
        let a = profile.artists.iter().find(|x| x.name == "A").unwrap();
        let b = profile.artists.iter().find(|x| x.name == "B").unwrap();
        assert!(a.score > b.score);
        assert!(b.score > 0.0);
        assert_eq!(b.plays, 1);
    }

    #[test]
    fn liked_tracks_and_follows_raise_the_seed() {
        let now = 1_000_000;
        let profile = build_profile(
            vec![
                event("a", "A", 60_000, false, now),
                event("b", "B", 60_000, false, now),
            ],
            vec![KeyRef {
                key_source: "youtube".into(),
                key_id: "b".into(),
            }],
            vec![],
            vec![(
                KeyRef {
                    key_source: "youtube".into(),
                    key_id: "UCb".into(),
                },
                "B".into(),
            )],
            vec![],
            now,
        );
        assert_eq!(profile.artists[0].name, "B");
        assert_eq!(profile.seed.as_deref(), Some("b"));
    }

    #[test]
    fn repeated_plays_produce_personalized_sections() {
        let now = 1_000_000;
        let mut first = event("a", "A", 180_000, true, now - 100);
        first.album = Some("First".into());
        let mut second = event("a", "A", 180_000, true, now - 50);
        second.album = Some("First".into());
        let profile = build_profile(vec![first, second], vec![], vec![], vec![], vec![], now);
        let related = vec![
            DiscoveryTrack {
                key: sources::TrackKey::new(sources::SourceKind::YouTube, "b".to_string()).unwrap(),
                title: "B".into(),
                artist: "B".into(),
                album: Some("Second".into()),
                duration_sec: Some(200),
                thumbnail_url: None,
                artists: vec![DiscoveryArtist {
                    name: "A".into(),
                    browse_id: Some("UCa".into()),
                }],
                album_id: Some("MPREb_second".into()),
                is_video: false,
                explicit: false,
                playable: true,
            },
            DiscoveryTrack {
                key: sources::TrackKey::new(sources::SourceKind::YouTube, "c".to_string()).unwrap(),
                title: "C".into(),
                artist: "C".into(),
                album: Some("Third".into()),
                duration_sec: Some(200),
                thumbnail_url: None,
                artists: vec![DiscoveryArtist {
                    name: "C".into(),
                    browse_id: Some("UCc".into()),
                }],
                album_id: Some("MPREb_third".into()),
                is_video: false,
                explicit: false,
                playable: true,
            },
        ];
        let sections = personalized_sections(&profile, &related, now);
        let ids: Vec<&str> = sections.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&"recently-played"));
        assert!(ids.contains(&"made-for-you"));
        assert!(ids.contains(&"favourite-artists"));
        assert!(ids.contains(&"similar-artists"));
        assert!(ids.contains(&"albums-you-may-like"));
        let albums = sections
            .iter()
            .find(|s| s.id == "albums-you-may-like")
            .unwrap();
        assert_eq!(albums.items.len(), 2);
    }

    #[test]
    fn dismissed_items_are_filtered_from_catalogue() {
        let dismissed = HashSet::from([("youtube".to_string(), "vid".to_string())]);
        let shelf = sources::yt::DiscoveryShelf {
            title: "Test".into(),
            subtitle: String::new(),
            layout: "square".into(),
            more_browse_id: None,
            more_params: None,
            items: vec![sources::yt::DiscoveryHomeItem {
                title: "Hidden".into(),
                subtitle: String::new(),
                thumbnail_url: Some("https://img".into()),
                video_id: Some("vid".into()),
                browse_id: None,
                is_video: false,
            }],
        };
        let (featured, sections) = catalogue_sections(
            Some(DiscoveryHome {
                shelves: vec![shelf],
                continuation: None,
            }),
            None,
            None,
            &dismissed,
        );
        assert!(featured.is_empty());
        assert!(sections.is_empty());
    }
}
