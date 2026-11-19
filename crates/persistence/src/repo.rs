//! Typed queries over the schema in `lib.rs`.
//!
//! Split by domain: settings and library state, track metadata, playlists
//! and remote membership, the persistent download queue, and recommendation
//! feedback.

pub mod downloads;
pub mod playlists;
pub mod recommendations;
pub mod settings;
pub mod tracks;

pub use downloads::*;
pub use playlists::*;
pub use recommendations::*;
pub use settings::*;
pub use tracks::*;

use crate::{Db, Result};

/// Bound on remote metadata text columns. Oversized values are truncated,
/// never rejected — the UI should not break on a hostile title.
pub const REMOTE_MAX_STR: usize = 512;
pub const REMOTE_MAX_KEY: usize = 1024;

fn clip(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// Row count for one table. The table name comes from call sites only,
/// never user input.
pub fn count(db: &Db, table: &str) -> Result<i64> {
    let conn = db.lock()?;
    let sql = format!("SELECT COUNT(*) FROM {table}");
    conn.query_row(&sql, [], |r| r.get(0)).map_err(Into::into)
}
