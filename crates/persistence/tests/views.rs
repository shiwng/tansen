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
    }
}

#[test]
fn playback_state_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("t.db")).unwrap();

    assert!(views::load_playback_state(&db).unwrap().is_none());

    let state = PlaybackState {
        tracks: vec![ref_with(1, "A", "X", "AL", "X"), ref_with(2, "B", "X", "AL", "X")],
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
        &PlaybackState { tracks: vec![], index: None, position_sec: 0.0, repeat: "off".into() },
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
