//! M12 timing measurement (`#[ignore]`): create 100 000 real library files,
//! insert their rows, and measure the read paths the library search uses
//! (`list_tracks` — the data behind `tracks_get`) plus the scanner's
//! stat-compare rescan. No criterion (not a dependency) — plain `Instant`,
//! medians over repeats.
//!
//! ```sh
//! cargo test -p library --test scan_timing -- --ignored --nocapture
//! ```

use library::Scanner;
use persistence::{models::Track, repo, Db};
use std::time::{Duration, Instant};

fn synthetic_track(i: usize, path: &str) -> Track {
    Track {
        id: 0,
        folder_id: 1,
        path: path.into(),
        file_name: format!("track-{:05}.flac", i),
        title: format!("Track {:05}", i),
        artist: format!("Artist {:03}", i / 500),
        album: format!("Album {:03}", i / 20),
        album_artist: format!("Artist {:03}", i / 500),
        track_no: Some((i % 20) as i64 + 1),
        disc_no: None,
        year: Some(2020 + (i % 6) as i64),
        genre: "Synthetic".into(),
        duration_sec: 180.0 + (i % 120) as f64,
        codec: "FLAC".into(),
        bitrate_kbps: Some(1000),
        sample_rate: Some(44_100),
        bit_depth: Some(16),
        channels: Some(2),
        art_path: None,
        missing: false,
    }
}

fn median(v: &mut [Duration]) -> Duration {
    v.sort();
    v[v.len() / 2]
}

#[test]
#[ignore = "timing measurement; run with --ignored --nocapture"]
fn search_read_paths_on_100k_tracks() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("t.db")).unwrap();
    let folder = repo::add_folder(&db, dir.path().to_string_lossy().as_ref(), 0).unwrap();

    // 100k real files (1 byte each — the scanner walks and stats them; only
    // the tags are synthetic). Rows carry their REAL stat values so the
    // rescan below measures the true stat-compare skip.
    let root = dir.path().join("lib");
    let t0 = Instant::now();
    for chunk_start in (0..100_000).step_by(200) {
        let mut items: Vec<repo::TrackWrite> = Vec::with_capacity(200);
        for i in chunk_start..chunk_start + 200 {
            let album_dir = root.join(format!("album{}", i / 20));
            std::fs::create_dir_all(&album_dir).unwrap();
            let path = album_dir.join(format!("track-{:05}.flac", i));
            std::fs::write(&path, b"x").unwrap();
            let meta = std::fs::metadata(&path).unwrap();
            let mtime = meta
                .modified()
                .unwrap()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64;
            items.push(repo::TrackWrite {
                track: synthetic_track(i, &path.to_string_lossy()),
                folder_id: folder,
                mtime,
                size: meta.len() as i64,
                art_path: None,
            });
        }
        repo::upsert_batch(&db, &items, 0).unwrap();
    }
    println!(
        "create + insert 100k real files (batched, 500 tx of 200): {:?}",
        t0.elapsed()
    );

    assert_eq!(repo::count(&db, "tracks").unwrap(), 100_000);

    // The search page's data path: tracks_get -> list_tracks (full library).
    let mut list_runs: Vec<Duration> = Vec::new();
    for _ in 0..5 {
        let t = Instant::now();
        let all = repo::list_tracks(&db).unwrap();
        list_runs.push(t.elapsed());
        assert_eq!(all.len(), 100_000);
    }
    println!(
        "list_tracks (100k rows): median {:?} of 5",
        median(&mut list_runs)
    );

    // The M11 queue-rows query at scale (1000 ids spread across the library).
    let ids: Vec<i64> = (1..=100_000).step_by(100).collect();
    let mut by_ids_runs: Vec<Duration> = Vec::new();
    for _ in 0..5 {
        let t = Instant::now();
        let rows = repo::tracks_by_ids(&db, &ids).unwrap();
        by_ids_runs.push(t.elapsed());
        assert_eq!(rows.len(), 1000);
    }
    println!(
        "tracks_by_ids (1000 of 100k): median {:?} of 5",
        median(&mut by_ids_runs)
    );

    // Scanner stat-compare cost over the whole library: every file unchanged,
    // so a rescan parses nothing — the number the skip buys.
    let scanner = Scanner::new(dir.path().join("art"));
    let mut scan_runs: Vec<Duration> = Vec::new();
    for _ in 0..3 {
        let t = Instant::now();
        let summary = scanner
            .scan(&db, &[(folder, root.to_string_lossy().into())], |_| {})
            .unwrap();
        scan_runs.push(t.elapsed());
        assert_eq!(summary.unchanged, 100_000, "every row skipped by stat compare");
        assert_eq!(summary.added, 0);
        assert_eq!(summary.errors, 0);
    }
    println!(
        "full rescan, 100k unchanged (stat compare only): median {:?} of 3",
        median(&mut scan_runs)
    );
}
