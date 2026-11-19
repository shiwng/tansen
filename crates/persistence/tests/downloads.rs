//! Download queue persistence: insertion, duplicate lookup, progress, restart
//! recovery and finished-row cleanup.

use persistence::{repo, Db};

fn db() -> (tempfile::TempDir, Db) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("t.db")).unwrap();
    (dir, db)
}

#[test]
fn download_rows_round_trip_and_restart() {
    let (_dir, db) = db();
    let id = repo::insert_download(
        &db,
        "track",
        "subsonic",
        "42",
        "Song",
        "Artist",
        Some("Album"),
        180.0,
        None,
        "original",
        1_000,
    )
    .unwrap();

    let found = repo::find_download(&db, "subsonic", "42", "original")
        .unwrap()
        .unwrap();
    assert_eq!(found.id, id);
    assert_eq!(found.status, "queued");
    assert_eq!(found.title, "Song");

    repo::set_download_status(&db, id, "downloading", None, 1_001).unwrap();
    repo::set_download_progress(&db, id, 512, 4096, 1_002).unwrap();
    // A crash mid-transfer is requeued on the next launch, keeping bytes.
    repo::requeue_interrupted_downloads(&db, 1_003).unwrap();
    let resumed = repo::get_download(&db, id).unwrap().unwrap();
    assert_eq!(resumed.status, "queued");
    assert_eq!(resumed.progress, 512);

    repo::finish_download(&db, id, "/tmp/song.flac", 4096, 1_004).unwrap();
    let done = repo::get_download(&db, id).unwrap().unwrap();
    assert_eq!(done.status, "complete");
    assert_eq!(done.path.as_deref(), Some("/tmp/song.flac"));

    let paths = repo::clear_finished_downloads(&db).unwrap();
    assert_eq!(paths, vec!["/tmp/song.flac".to_string()]);
    assert!(repo::get_download(&db, id).unwrap().is_none());
}

#[test]
fn reset_and_delete_are_idempotent() {
    let (_dir, db) = db();
    let id = repo::insert_download(
        &db, "track", "addon", "abc", "T", "A", None, 0.0, None, "high", 10,
    )
    .unwrap();
    repo::set_download_status(&db, id, "failed", Some("network"), 11).unwrap();
    repo::reset_download(&db, id, 12).unwrap();
    let row = repo::get_download(&db, id).unwrap().unwrap();
    assert_eq!(row.status, "queued");
    assert_eq!(row.error, None);
    repo::delete_download(&db, id).unwrap();
    repo::delete_download(&db, id).unwrap();
    assert!(repo::list_downloads(&db).unwrap().is_empty());
}
