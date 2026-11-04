//! SQLite persistence: schema migrations and typed repositories for
//! settings, library folders, tracks, playlists and play history.
//!
//! A single `Connection` guarded by a `Mutex` is deliberate: the database
//! work is short, and everything long-running (scanning, artwork decoding)
//! happens outside the lock in the library crate.

pub mod models;
pub mod repo;
pub mod views;

use rusqlite::{Connection, OpenFlags};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("db locked by another writer")]
    Poisoned,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, DbError>;

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    /// Opens (creating if needed) the database at `path` and runs migrations.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let db = Self { conn: Mutex::new(conn) };
        db.migrate()?;
        Ok(db)
    }

    /// Opens an additional read-only connection to the same WAL database
    /// (M11). Queries through it never contend with the writer for the
    /// write lock. Open it after `open` — migrations run on the writer.
    /// Writes through this handle fail at the SQLite level (READ_ONLY flag),
    /// which is the guard, not the type system.
    pub fn open_read_only(path: &Path) -> Result<Self> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn: Mutex::new(conn) };
        db.migrate()?;
        Ok(db)
    }

    pub fn lock(&self) -> Result<MutexGuard<'_, Connection>> {
        self.conn.lock().map_err(|_| DbError::Poisoned)
    }

    /// Migrations are an ordered list; `user_version` stores how many ran.
    fn migrate(&self) -> Result<()> {
        let mut conn = self.lock()?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
            let tx = conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", i as i64 + 1)?;
            tx.commit()?;
        }
        Ok(())
    }
}

const MIGRATIONS: &[&str] = &[
    // 1 — settings, folders, tracks, playlists, history.
    r#"
    CREATE TABLE settings (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );

    CREATE TABLE library_folders (
        id        INTEGER PRIMARY KEY,
        path      TEXT NOT NULL UNIQUE,
        added_at  INTEGER NOT NULL
    );

    CREATE TABLE tracks (
        id            INTEGER PRIMARY KEY,
        folder_id     INTEGER NOT NULL REFERENCES library_folders(id) ON DELETE CASCADE,
        path          TEXT NOT NULL UNIQUE,
        file_name     TEXT NOT NULL,
        title         TEXT NOT NULL,
        artist        TEXT NOT NULL DEFAULT '',
        album         TEXT NOT NULL DEFAULT '',
        album_artist  TEXT NOT NULL DEFAULT '',
        track_no      INTEGER,
        disc_no       INTEGER,
        year          INTEGER,
        genre         TEXT NOT NULL DEFAULT '',
        duration_sec  REAL NOT NULL DEFAULT 0,
        codec         TEXT NOT NULL DEFAULT '',
        bitrate_kbps  INTEGER,
        sample_rate   INTEGER,
        bit_depth     INTEGER,
        channels      INTEGER,
        art_path      TEXT,
        art_dirty     INTEGER NOT NULL DEFAULT 1,
        mtime_sec     INTEGER NOT NULL DEFAULT 0,
        size_bytes    INTEGER NOT NULL DEFAULT 0,
        missing       INTEGER NOT NULL DEFAULT 0,
        added_at      INTEGER NOT NULL,
        scanned_at    INTEGER NOT NULL
    );
    CREATE INDEX idx_tracks_album   ON tracks(album_artist, album, disc_no, track_no);
    CREATE INDEX idx_tracks_artist  ON tracks(artist);
    CREATE INDEX idx_tracks_title   ON tracks(title);

    CREATE TABLE playlists (
        id         INTEGER PRIMARY KEY,
        name       TEXT NOT NULL,
        created_at INTEGER NOT NULL
    );

    CREATE TABLE playlist_tracks (
        playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
        position    INTEGER NOT NULL,
        track_id    INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
        PRIMARY KEY (playlist_id, position)
    );

    CREATE TABLE play_history (
        id         INTEGER PRIMARY KEY,
        track_id   INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
        played_at  INTEGER NOT NULL
    );
    CREATE INDEX idx_history_time ON play_history(played_at);
    "#,
    // 2 — recoverable playback state (queue + position + repeat), one row.
    r#"
    CREATE TABLE playback_state (
        id           INTEGER PRIMARY KEY CHECK (id = 0),
        tracks_json  TEXT NOT NULL,
        index_num    INTEGER,
        position_sec REAL NOT NULL DEFAULT 0,
        repeat_mode  TEXT NOT NULL DEFAULT 'off',
        updated_at   INTEGER NOT NULL
    );
    "#,
    // 3 — seed built-in source config (YouTube + Local, both enabled) so a
    // fresh or pre-S2 database has a defined source roster. Forward-only.
    r#"
    INSERT OR IGNORE INTO settings(key, value) VALUES(
        'sources_config',
        '[{"config_id":"yt","kind":"youtube","name":"YouTube Music","base_url":"","enabled":true,"rank":0},{"config_id":"local","kind":"local","name":"Local Files","base_url":"","enabled":true,"rank":0}]'
    );
    "#,
    // 4 — remote tracks (S6): metadata for source-keyed queue entries. The
    // table is additive; local `tracks` semantics are untouched. Text
    // columns are bounded by the upsert (S6).
    r#"
    CREATE TABLE remote_tracks (
        id            INTEGER PRIMARY KEY,
        key_source    TEXT NOT NULL,
        key_id        TEXT NOT NULL,
        title         TEXT NOT NULL DEFAULT '',
        artist        TEXT NOT NULL DEFAULT '',
        album         TEXT,
        duration_sec  REAL NOT NULL DEFAULT 0,
        artwork_url   TEXT,
        UNIQUE(key_source, key_id)
    );
    "#,
];
