//! Watcher integration: a change under a watched folder lands in the index.

use persistence::repo;
use std::time::Duration;

fn fixture() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/testmusic/Test Signal/test.wav")
}

#[test]
fn watcher_picks_up_new_files_and_respects_the_setting() {
    let src = fixture();
    assert!(src.exists(), "missing fixture {}", src.display());

    let dir = tempfile::tempdir().unwrap();
    let db = std::sync::Arc::new(persistence::Db::open(&dir.path().join("t.db")).unwrap());
    let scanner = library::Scanner::new(dir.path().join("art"));
    let (scan_tx, _scan_rx) = tokio::sync::broadcast::channel(8);

    // A library folder with one file in it.
    let music = dir.path().join("music");
    std::fs::create_dir_all(&music).unwrap();
    std::fs::copy(&src, music.join("a.wav")).unwrap();
    repo::add_folder(&db, &music.to_string_lossy(), 0).unwrap();

    // The watcher's debounce is 2 s; allow FSEvents + debounce + scan.
    app_core::watcher::spawn_watcher(db.clone(), scanner.clone(), scan_tx.clone());
    std::thread::sleep(Duration::from_secs(8));
    assert_eq!(repo::count(&db, "tracks").unwrap(), 1, "watcher indexed the file");

    // A second file lands → another scan adds it.
    std::fs::copy(&src, music.join("b.wav")).unwrap();
    std::thread::sleep(Duration::from_secs(8));
    assert_eq!(repo::count(&db, "tracks").unwrap(), 2, "watcher indexed the added file");

    // Setting off → no more scans (the file we add now must not appear).
    repo::set_setting(&db, "watch_folders", "false").unwrap();
    std::fs::copy(&src, music.join("c.wav")).unwrap();
    std::thread::sleep(Duration::from_secs(8));
    assert_eq!(repo::count(&db, "tracks").unwrap(), 2, "watch_folders=false stops rescans");
}
