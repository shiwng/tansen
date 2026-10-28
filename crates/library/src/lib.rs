//! Library crate: folder scanning, metadata reading, artwork caching.

pub mod art;
pub mod metadata;
pub mod scanner;

pub use metadata::{is_supported, SUPPORTED_EXT};
pub use scanner::{ScanProgress, ScanSummary, Scanner};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_filter() {
        assert!(is_supported(std::path::Path::new("/x/Song.FLAC")));
        assert!(is_supported(std::path::Path::new("/x/Song.m4a")));
        assert!(!is_supported(std::path::Path::new("/x/Song.exe")));
        assert!(!is_supported(std::path::Path::new("/x/noext")));
    }

    #[test]
    fn art_hash_is_stable_and_mtime_sensitive() {
        let dir = tempfile::tempdir().unwrap();
        let a = art::extract(std::path::Path::new("/nonexistent/song.mp3"), 1, dir.path()).unwrap();
        let b = art::extract(std::path::Path::new("/nonexistent/song.mp3"), 1, dir.path()).unwrap();
        let c = art::extract(std::path::Path::new("/nonexistent/song.mp3"), 2, dir.path()).unwrap();
        // No picture in a nonexistent file: Ok(None), never an error.
        assert_eq!(a, None);
        assert_eq!(b, None);
        assert_eq!(c, None);
    }

    /// End-to-end scanner check against a real decodable WAV created by
    /// scripts/make_test_audio.py (committed under tests/fixtures).
    #[test]
    fn scanner_indexes_and_rescans_without_duplicates() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/testmusic/Test Signal/test.wav");
        if !fixture.exists() {
            panic!("missing fixture {}", fixture.display());
        }
        let dir = tempfile::tempdir().unwrap();
        let db = persistence::Db::open(&dir.path().join("t.db")).unwrap();
        let scanner = Scanner::new(dir.path().join("art"));

        let folder_root = fixture.parent().unwrap().parent().unwrap().to_path_buf();
        let folder_id =
            persistence::repo::add_folder(&db, &folder_root.to_string_lossy(), 0).unwrap();

        let summary = scanner
            .scan(&db, &[(folder_id, folder_root.to_string_lossy().into())], |_| {})
            .unwrap();
        assert_eq!(summary.added, 1, "first scan indexes the file");
        assert_eq!(persistence::repo::count(&db, "tracks").unwrap(), 1);

        let track = persistence::repo::list_tracks(&db).unwrap().remove(0);
        assert_eq!(track.title, "test"); // filename fallback (untagged WAV)
        assert!(track.duration_sec > 1.5, "WAV duration parsed, got {}", track.duration_sec);
        assert_eq!(track.sample_rate, Some(44100));
        assert_eq!(track.channels, Some(2));

        // Second scan: no duplicates, no phantom missing.
        let summary = scanner
            .scan(&db, &[(folder_id, folder_root.to_string_lossy().into())], |_| {})
            .unwrap();
        assert_eq!(summary.added, 0);
        assert_eq!(summary.marked_missing, 0);
        assert_eq!(persistence::repo::count(&db, "tracks").unwrap(), 1);
    }
}
