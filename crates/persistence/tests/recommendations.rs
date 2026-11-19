use persistence::{repo, Db};

#[test]
fn listening_events_finish_clamps_and_marks_completion() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("t.db")).unwrap();
    let id = repo::start_listening_event(
        &db,
        "youtube",
        "vid",
        "Title",
        "Artist",
        Some("Album"),
        Some("https://img"),
        200_000,
        10,
    )
    .unwrap();
    repo::finish_listening_event(&db, id, 500_000).unwrap();
    let rows = repo::listening_events(&db, 10).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].played_ms, 200_000);
    assert!(rows[0].completed);
}

#[test]
fn likes_follows_and_dismissals_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("t.db")).unwrap();
    repo::set_liked_track(&db, "youtube", "v", true, 1).unwrap();
    repo::set_liked_album(&db, "youtube", "MPREb_x", true, 1).unwrap();
    repo::set_followed_artist(&db, "youtube", "UCx", "Artist", true, 1).unwrap();
    repo::set_recommendation_dismissed(&db, "youtube", "gone", true, 1).unwrap();

    assert_eq!(repo::liked_tracks(&db).unwrap()[0].key_id, "v");
    assert_eq!(repo::liked_albums(&db).unwrap()[0].key_id, "MPREb_x");
    assert_eq!(repo::followed_artists(&db).unwrap()[0].1, "Artist");
    assert_eq!(
        repo::dismissed_recommendations(&db).unwrap()[0].key_id,
        "gone"
    );
    assert_eq!(repo::recommendation_counts(&db).unwrap(), (0, 1, 1, 1, 1));

    repo::set_liked_track(&db, "youtube", "v", false, 2).unwrap();
    repo::set_liked_album(&db, "youtube", "MPREb_x", false, 2).unwrap();
    repo::set_followed_artist(&db, "youtube", "UCx", "Artist", false, 2).unwrap();
    repo::set_recommendation_dismissed(&db, "youtube", "gone", false, 2).unwrap();
    assert!(repo::liked_tracks(&db).unwrap().is_empty());
    assert!(repo::liked_albums(&db).unwrap().is_empty());
    assert!(repo::followed_artists(&db).unwrap().is_empty());
    assert!(repo::dismissed_recommendations(&db).unwrap().is_empty());
}
