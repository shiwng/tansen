//! Restart persistence: the queue saved by the persister comes back through
//! `restore_playback`, paused, at the saved index, with dead files skipped.

use persistence::views::{self, PlaybackState};
use persistence::Db;

#[test]
fn queue_survives_an_app_restart() {
    // Skip gracefully where there is no audio device (Engine::spawn fails).
    let dir = tempfile::tempdir().unwrap();
    let db = std::sync::Arc::new(Db::open(&dir.path().join("bitchord.db")).unwrap());

    // Two real files in the test's own temp dir — the shared fixture directory
    // must stay exactly one file, or the scanner test's counts drift.
    let fixture_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/testmusic/Test Signal");
    let music = dir.path().join("music");
    std::fs::create_dir_all(&music).unwrap();
    let a = music.join("test.wav");
    let b = music.join("test2.wav");
    std::fs::copy(fixture_dir.join("test.wav"), &a).unwrap();
    std::fs::copy(fixture_dir.join("test.wav"), &b).unwrap();
    assert!(a.exists() && b.exists());

    let tracks = vec![
        persistence::models::TrackRef {
            id: 1,
            title: "A".into(),
            artist: "X".into(),
            album: "AL".into(),
            duration_sec: 2.0,
            art_path: None,
            path: a.to_string_lossy().into_owned(),
        },
        persistence::models::TrackRef {
            id: 2,
            title: "B".into(),
            artist: "X".into(),
            album: "AL".into(),
            duration_sec: 2.0,
            art_path: None,
            path: b.to_string_lossy().into_owned(),
        },
        persistence::models::TrackRef {
            id: 3,
            title: "Gone".into(),
            artist: "X".into(),
            album: "AL".into(),
            duration_sec: 2.0,
            art_path: None,
            path: "/definitely/not/there.flac".into(),
        },
    ];
    views::save_playback_state(
        &db,
        &PlaybackState { tracks, index: Some(1), position_sec: 1.0, repeat: "all".into() },
        0,
    )
    .unwrap();

    let state = app_core::AppState::open(dir.path()).expect("app state");
    let loaded = persistence::views::load_playback_state(&db).unwrap();
    eprintln!("loaded: {:?}", loaded.as_ref().map(|s| (s.tracks.len(), s.index)));
    eprintln!("a exists: {}, b exists: {}", a.exists(), b.exists());
    assert!(state.restore_playback().unwrap(), "a saved queue was restored");

    // The engine applies the restore command asynchronously; wait for it.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let snap = loop {
        let snap = state.queue_snapshot();
        if !snap.tracks.is_empty() || std::time::Instant::now() > deadline {
            break snap;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    assert_eq!(snap.tracks.len(), 2, "the dead file is dropped");
    assert_eq!(snap.tracks[0].title, "A");
    assert_eq!(snap.tracks[1].title, "B");
    assert_eq!(snap.index, Some(1), "restore lands on the saved track");
    assert!(!snap.playing, "restore comes back paused");
    assert_eq!(snap.repeat, playback::Repeat::All);

    // A second restore over an app whose queue is playing must not double-load:
    // the persister now re-saved the restored queue; restore again is idempotent.
    assert!(state.restore_playback().unwrap());
    assert_eq!(state.queue_snapshot().tracks.len(), 2);
}

#[test]
fn restore_without_state_is_a_noop() {
    let dir = tempfile::tempdir().unwrap();
    let db = std::sync::Arc::new(Db::open(&dir.path().join("t.db")).unwrap());
    let state = app_core::AppState::open(dir.path()).expect("app state");
    assert!(!state.restore_playback().unwrap());
    assert!(state.queue_snapshot().tracks.is_empty());
}
