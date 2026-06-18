use rusqlite::Connection;
use std::path::Path;

use crate::Result;

/// Initialize the SQLite database with WAL mode and foreign keys
pub fn init_db(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA foreign_keys=ON;
         PRAGMA busy_timeout=5000;",
    )?;
    // TODO: Create tables in Loop 3
    Ok(conn)
}
