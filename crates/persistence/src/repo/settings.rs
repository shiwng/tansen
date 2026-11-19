use crate::{Db, Result};
use rusqlite::{params, OptionalExtension};

pub fn get_setting(db: &Db, key: &str) -> Result<Option<String>> {
    let conn = db.lock()?;
    Ok(conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |r| r.get(0),
        )
        .optional()?)
}

pub fn set_setting(db: &Db, key: &str, value: &str) -> Result<()> {
    let conn = db.lock()?;
    conn.execute(
        "INSERT INTO settings(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn list_settings(db: &Db) -> Result<Vec<(String, String)>> {
    let conn = db.lock()?;
    let mut stmt = conn.prepare("SELECT key, value FROM settings ORDER BY key")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn delete_setting(db: &Db, key: &str) -> Result<()> {
    let conn = db.lock()?;
    conn.execute("DELETE FROM settings WHERE key = ?1", params![key])?;
    Ok(())
}
