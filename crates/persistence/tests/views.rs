//! Integration tests for grouped views and playback-state persistence.

use persistence::models::TrackRef;
use persistence::views::{self, PlaybackState};
use persistence::{repo, Db};

fn ref_with(id: i64, title: &str, artist: &str, album: &str, _album_artist: &str) -> TrackRef {
    TrackRef {
        id,
        title: title.into(),
        artist: artist.into(),
        album: album.into(),
        duration_sec: 100.0,
        art_path: None,
        path: format!("/t/{id}.flac"),
        video_id: None,
        key_source: None,
        artwork_url: None,
    }
}

#[test]
fn playback_state_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("t.db")).unwrap();

    assert!(views::load_playback_state(&db).unwrap().is_none());

    let state = PlaybackState {
        tracks: vec![
            ref_with(1, "A", "X", "AL", "X"),
            ref_with(2, "B", "X", "AL", "X"),
        ],
        index: Some(1),
        position_sec: 42.5,
        repeat: "all".into(),
    };
    views::save_playback_state(&db, &state, 1000).unwrap();
    let loaded = views::load_playback_state(&db).unwrap().unwrap();
    assert_eq!(loaded.index, Some(1));
    assert_eq!(loaded.position_sec, 42.5);
    assert_eq!(loaded.repeat, "all");
    assert_eq!(loaded.tracks.len(), 2);
    assert_eq!(loaded.tracks[1].title, "B");

    // Overwrite keeps a single row.
    views::save_playback_state(
        &db,
        &PlaybackState {
            tracks: vec![],
            index: None,
            position_sec: 0.0,
            repeat: "off".into(),
        },
        2000,
    )
    .unwrap();
    let loaded = views::load_playback_state(&db).unwrap().unwrap();
    assert!(loaded.tracks.is_empty());
}

#[test]
fn albums_group_and_order() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("t.db")).unwrap();
    repo::add_folder(&db, "/t", 0).unwrap();

    // Two albums; tracks inserted through the real upsert so ordering rules
    // are exercised against the actual schema.
    let mk = |path: &str, title: &str, artist: &str, album: &str, aa: &str, no: Option<i64>| {
        let t = persistence::models::Track {
            id: 0,
            folder_id: 1,
            path: path.into(),
            file_name: path.into(),
            title: title.into(),
            artist: artist.into(),
            album: album.into(),
            album_artist: aa.into(),
            track_no: no,
            disc_no: None,
            year: Some(2020),
            genre: "".into(),
            duration_sec: 60.0,
            codec: "FLAC".into(),
            bitrate_kbps: None,
            sample_rate: None,
            bit_depth: None,
            channels: None,
            art_path: None,
            missing: false,
        };
        repo::upsert_track(&db, &t, 1, 0, 0, 0).unwrap();
    };

    mk("/t/b1.flac", "B1", "Singer", "Album B", "Singer", Some(1));
    mk("/t/b2.flac", "B2", "Singer", "Album B", "Singer", Some(2));
    mk("/t/a1.flac", "A1", "Other", "Album A", "", Some(1)); // compilation: empty album_artist

    let albums = views::albums(&db).unwrap();
    assert_eq!(albums.len(), 2, "two distinct albums");
    // Albums with a known album_artist sort first; unknown last.
    assert_eq!(albums[0].album, "Album B");
    assert_eq!(albums[0].album_artist, "Singer");
    assert_eq!(albums[0].track_count, 2);
    assert_eq!(albums[0].duration_sec, 120.0);
    assert_eq!(albums[1].album, "Album A");
    assert_eq!(albums[1].album_artist, "Unknown Artist");
    assert_eq!(albums[1].track_count, 1);

    let tracks = views::album_tracks(&db, "Singer||Album B").unwrap();
    assert_eq!(tracks.len(), 2);
    assert_eq!(tracks[0].title, "B1");
    assert_eq!(tracks[1].title, "B2");

    let artists = views::artists(&db).unwrap();
    assert_eq!(artists.len(), 2, "Singer + Other via album_artist fallback");
    assert_eq!(artists[0].name, "Other");
    assert_eq!(artists[0].track_count, 1);
    assert_eq!(artists[1].name, "Singer");
    assert_eq!(artists[1].track_count, 2);
    assert_eq!(artists[1].album_count, 1);
}

// read-only connections + the queue-rows query

#[test]
fn tracks_by_ids_returns_rows_in_input_order_skipping_missing() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("t.db")).unwrap();

    let folder = repo::add_folder(&db, "/t", 0).unwrap();
    let mk = |id_hint: i64, path: &str, title: &str| {
        let t = persistence::models::Track {
            id: 0,
            folder_id: folder,
            path: path.into(),
            file_name: path.into(),
            title: title.into(),
            artist: "A".into(),
            album: "AL".into(),
            album_artist: "A".into(),
            track_no: None,
            disc_no: None,
            year: None,
            genre: "".into(),
            duration_sec: 60.0,
            codec: "FLAC".into(),
            bitrate_kbps: None,
            sample_rate: None,
            bit_depth: None,
            channels: None,
            art_path: None,
            missing: false,
        };
        repo::upsert_track(&db, &t, folder, 0, 0, 0).unwrap();
        let _ = id_hint;
    };
    mk(0, "/t/one.flac", "One");
    mk(0, "/t/two.flac", "Two");
    mk(0, "/t/three.flac", "Three");

    let by_title: std::collections::HashMap<String, i64> = repo::list_tracks(&db)
        .unwrap()
        .into_iter()
        .map(|t| (t.title.clone(), t.id))
        .collect();
    let one = by_title["One"];
    let three = by_title["Three"];
    // Input order 3,1,3(dup),999(missing) → rows for 3 then 1, no dup, no 999.
    let query = [three, one, three, 999_999];
    let rows = repo::tracks_by_ids(&db, &query).unwrap();
    assert_eq!(rows.len(), 2, "missing id skipped, duplicates collapse");
    assert_eq!(rows[0].title, "Three", "input order preserved");
    assert_eq!(rows[1].title, "One");
}

#[test]
fn read_only_connection_reads_and_refuses_writes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    let writer = Db::open(&path).unwrap();
    let folder = repo::add_folder(&writer, "/t", 0).unwrap();
    let t = persistence::models::Track {
        id: 0,
        folder_id: folder,
        path: "/t/x.flac".into(),
        file_name: "/t/x.flac".into(),
        title: "X".into(),
        artist: "A".into(),
        album: "AL".into(),
        album_artist: "A".into(),
        track_no: None,
        disc_no: None,
        year: None,
        genre: "".into(),
        duration_sec: 60.0,
        codec: "FLAC".into(),
        bitrate_kbps: None,
        sample_rate: None,
        bit_depth: None,
        channels: None,
        art_path: None,
        missing: false,
    };
    repo::upsert_track(&writer, &t, folder, 0, 0, 0).unwrap();

    let reader = Db::open_read_only(&path).unwrap();
    // Reads work on the read-only handle.
    let rows = repo::tracks_by_ids(&reader, &[1]).unwrap();
    assert_eq!(rows.len(), 1);
    // Writes fail at the SQLite level.
    let mut wrong = t.clone();
    wrong.id = 0;
    wrong.path = "/t/y.flac".into();
    assert!(
        repo::upsert_track(&reader, &wrong, folder, 0, 0, 0).is_err(),
        "read-only refuses writes"
    );
}

// migration 3 seed + persistence across restart

#[test]
fn fresh_db_is_seeded_with_builtin_sources() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("t.db")).unwrap();
    let cfg = repo::get_setting(&db, "sources_config")
        .unwrap()
        .expect("seeded");
    assert!(
        cfg.contains("\"config_id\":\"yt\"") && cfg.contains("\"config_id\":\"local\""),
        "{cfg}"
    );
    assert!(cfg.contains("\"enabled\":true"));
}

#[test]
fn upgrade_path_seeds_when_sources_config_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    {
        let db = Db::open(&path).unwrap();
        // Simulate a pre-v2 database: no seed and neither remote
        // table, so the next open reruns migration 3 (seed) plus the
        // additive table migrations after it.
        let conn = db.lock().unwrap();
        conn.execute("DELETE FROM settings WHERE key = 'sources_config'", [])
            .unwrap();
        conn.execute("DROP TABLE remote_tracks", []).unwrap();
        conn.execute("DROP TABLE IF EXISTS playlist_remote_tracks", [])
            .unwrap();
        conn.execute("DROP TABLE IF EXISTS track_loudness", [])
            .unwrap();
        conn.execute("DROP TABLE IF EXISTS downloads", []).unwrap();
        // Migration 9 tables too.
        conn.execute("DROP TABLE IF EXISTS listening_events", [])
            .unwrap();
        conn.execute("DROP TABLE IF EXISTS liked_tracks", [])
            .unwrap();
        conn.execute("DROP TABLE IF EXISTS liked_albums", [])
            .unwrap();
        conn.execute("DROP TABLE IF EXISTS followed_artists", [])
            .unwrap();
        conn.execute("DROP TABLE IF EXISTS dismissed_recommendations", [])
            .unwrap();
        // Undo migration 6 as well, or its rerun hits an existing column.
        conn.execute("ALTER TABLE playlists DROP COLUMN favourite", [])
            .unwrap();
        // Migration 10 adds the duration index; a v2 rerun would collide.
        conn.execute("DROP INDEX IF EXISTS idx_tracks_duration", [])
            .unwrap();
        conn.pragma_update(None, "user_version", 2).unwrap();
    }
    let db = Db::open(&path).unwrap(); // upgrade: migration 3 reruns
    assert!(
        repo::get_setting(&db, "sources_config").unwrap().is_some(),
        "re-seeded on upgrade"
    );
}

#[test]
fn settings_persist_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    {
        let db = Db::open(&path).unwrap();
        repo::set_setting(&db, "audio_quality", "lossless").unwrap();
    }
    let db = Db::open(&path).unwrap();
    assert_eq!(
        repo::get_setting(&db, "audio_quality").unwrap().as_deref(),
        Some("lossless")
    );
}

// remote track metadata round-trip

#[test]
fn remote_track_round_trip_and_clipping() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("t.db")).unwrap();
    let id = repo::upsert_remote_track(
        &db,
        "youtube",
        "vid123",
        "T",
        "A",
        Some("AL"),
        178.0,
        Some("https://i.ytimg.com/x.jpg"),
    )
    .unwrap();
    let rows = repo::remote_tracks_by_ids(&db, &[id]).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title, "T");
    assert_eq!(rows[0].key_id, "vid123");
    // Same key re-upserts, id preserved.
    let id2 =
        repo::upsert_remote_track(&db, "youtube", "vid123", "T2", "A", None, 1.0, None).unwrap();
    assert_eq!(id, id2);
    let rows = repo::remote_tracks_by_ids(&db, &[id]).unwrap();
    assert_eq!(rows[0].title, "T2");
    // Oversized title is clipped.
    let long = "x".repeat(2000);
    let id3 =
        repo::upsert_remote_track(&db, "youtube", "long", long.as_str(), "A", None, 1.0, None)
            .unwrap();
    let rows = repo::remote_tracks_by_ids(&db, &[id3]).unwrap();
    assert!(rows[0].title.len() <= repo::REMOTE_MAX_STR);
}

#[test]
fn queue_rows_query_returns_both_kinds_in_input_order() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("t.db")).unwrap();

    let folder = repo::add_folder(&db, "/t", 0).unwrap();
    let mk = |path: &str, title: &str| {
        let t = persistence::models::Track {
            id: 0,
            folder_id: folder,
            path: path.into(),
            file_name: path.into(),
            title: title.into(),
            artist: "L".into(),
            album: "Local AL".into(),
            album_artist: "L".into(),
            track_no: None,
            disc_no: None,
            year: None,
            genre: "".into(),
            duration_sec: 60.0,
            codec: "FLAC".into(),
            bitrate_kbps: None,
            sample_rate: None,
            bit_depth: None,
            channels: None,
            art_path: None,
            missing: false,
        };
        repo::upsert_track(&db, &t, folder, 0, 0, 0).unwrap();
    };
    mk("/t/a.flac", "Local A");
    mk("/t/b.flac", "Local B");
    let local_ids: Vec<i64> = repo::list_tracks(&db)
        .unwrap()
        .into_iter()
        .map(|t| t.id)
        .collect();

    let remote_a = repo::upsert_remote_track(
        &db,
        "youtube",
        "vidA",
        "Remote A",
        "RA",
        Some("R AL"),
        200.0,
        Some("https://i.ytimg.com/x.jpg"),
    )
    .unwrap();
    let remote_b =
        repo::upsert_remote_track(&db, "addon", "vidB", "Remote B", "RB", None, 10.0, None)
            .unwrap();

    // Local ids [b, a] (input order preserved), remote ids [a, b] — the
    // combined result keeps each group's input order and tags kinds.
    let mut query = local_ids.clone();
    query.reverse();
    let rows = repo::queue_rows_by_ids(&db, &query, &[remote_a, remote_b]).unwrap();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0].kind, "local");
    assert_eq!(rows[0].title, "Local B");
    assert_eq!(rows[1].title, "Local A");
    assert_eq!(rows[2].kind, "remote");
    assert_eq!(rows[2].title, "Remote A");
    assert_eq!(rows[2].key_source.as_deref(), Some("youtube"));
    assert_eq!(rows[2].video_id.as_deref(), Some("vidA"));
    assert_eq!(
        rows[2].artwork_url.as_deref(),
        Some("https://i.ytimg.com/x.jpg")
    );
    assert_eq!(rows[3].kind, "remote");
    assert_eq!(rows[3].title, "Remote B");
    assert_eq!(rows[3].album, None);
    // Unknown ids on either side are skipped.
    let rows = repo::queue_rows_by_ids(&db, &[999_999], &[999_999]).unwrap();
    assert!(rows.is_empty());
}

/// "Recently Added" is library additions, not playback: newest `added_at`
/// first, and a rescan of an existing file must not move it.
#[test]
fn recently_added_orders_by_added_at_not_rescan() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("t.db")).unwrap();
    let folder = repo::add_folder(&db, "/music", 0).unwrap();

    let track = |path: &str, title: &str| persistence::models::Track {
        id: 0,
        folder_id: folder,
        path: path.into(),
        file_name: format!("{title}.flac"),
        title: title.into(),
        artist: "A".into(),
        album: "AL".into(),
        album_artist: "A".into(),
        track_no: None,
        disc_no: None,
        year: None,
        genre: String::new(),
        duration_sec: 100.0,
        codec: "flac".into(),
        bitrate_kbps: None,
        sample_rate: None,
        bit_depth: None,
        channels: None,
        art_path: None,
        missing: false,
    };

    repo::upsert_track(&db, &track("/m/old.flac", "Old"), folder, 1, 1, 100).unwrap();
    repo::upsert_track(&db, &track("/m/new.flac", "New"), folder, 1, 1, 200).unwrap();
    // A later scan re-writes the older file; its added_at must survive.
    repo::upsert_track(&db, &track("/m/old.flac", "Old"), folder, 2, 2, 300).unwrap();

    let recent = repo::recently_added(&db, 10).unwrap();
    let titles: Vec<_> = recent.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(titles, ["New", "Old"]);
}
