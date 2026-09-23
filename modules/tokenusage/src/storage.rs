//! storage 模块：tokenusage.db 连接管理与 Schema 定义（ADR-0031 / 技术 06）。
//!
//! 独占写入，SQLite WAL + 外键 + busy_timeout(2000ms)。
//! 锁纪律：非重入 Mutex<Connection> 仅在 with_read/with_write 内短持有。

use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

pub const SCHEMA_VERSION: i64 = 1;

pub const SCHEMA_V1: &str = r#"
CREATE TABLE IF NOT EXISTS usage_sources (
    id TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    category TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    custom_scan_path TEXT,
    last_scanned_at TEXT,
    last_cursor TEXT,
    status TEXT NOT NULL DEFAULT 'ok',
    error_message TEXT,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sessions (
    session_id TEXT PRIMARY KEY,
    source_id TEXT NOT NULL REFERENCES usage_sources(id),
    title TEXT NOT NULL DEFAULT '',
    project_path TEXT NOT NULL DEFAULT '',
    started_at TEXT NOT NULL,
    last_used_at TEXT NOT NULL,
    total_input_tokens INTEGER NOT NULL DEFAULT 0,
    total_output_tokens INTEGER NOT NULL DEFAULT 0,
    total_cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    total_cache_write_tokens INTEGER NOT NULL DEFAULT 0,
    total_reasoning_tokens INTEGER NOT NULL DEFAULT 0,
    total_cost_micros INTEGER NOT NULL DEFAULT 0,
    message_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_sessions_source_time ON sessions(source_id, last_used_at DESC);
CREATE INDEX IF NOT EXISTS idx_sessions_time ON sessions(last_used_at DESC);

CREATE TABLE IF NOT EXISTS usage_records (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    record_hash TEXT UNIQUE,
    source_id TEXT NOT NULL REFERENCES usage_sources(id),
    session_id TEXT REFERENCES sessions(session_id),
    turn_id TEXT,
    model TEXT NOT NULL DEFAULT 'unknown',
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cache_write_tokens INTEGER NOT NULL DEFAULT 0,
    reasoning_tokens INTEGER NOT NULL DEFAULT 0,
    cost_micros INTEGER NOT NULL DEFAULT 0,
    recorded_at TEXT NOT NULL,
    raw_metadata TEXT
);
CREATE INDEX IF NOT EXISTS idx_records_source_model ON usage_records(source_id, model, recorded_at DESC);
CREATE INDEX IF NOT EXISTS idx_records_time ON usage_records(recorded_at DESC);

CREATE TABLE IF NOT EXISTS daily_aggregates (
    date TEXT NOT NULL,
    source_id TEXT NOT NULL,
    model TEXT NOT NULL,
    total_tokens INTEGER NOT NULL DEFAULT 0,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cost_micros INTEGER NOT NULL DEFAULT 0,
    session_count INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (date, source_id, model)
);
CREATE INDEX IF NOT EXISTS idx_daily_date ON daily_aggregates(date);

CREATE TABLE IF NOT EXISTS limits_cache (
    provider_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    window_kind TEXT NOT NULL,
    label TEXT NOT NULL DEFAULT '',
    used_percent REAL,
    remaining_percent REAL,
    used_units REAL,
    total_units REAL,
    unit_type TEXT NOT NULL DEFAULT 'percent',
    resets_at TEXT,
    fetched_at TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'ok',
    PRIMARY KEY (provider_id, account_id, window_kind)
);

CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sync_state (
    device_id TEXT PRIMARY KEY,
    device_name TEXT NOT NULL,
    last_synced_at TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'idle',
    payload_hash TEXT
);
"#;

pub struct Store {
    conn: Mutex<Connection>,
    data_dir: PathBuf,
    writer_version: Mutex<String>,
}

impl Store {
    pub fn open(data_dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(data_dir).map_err(|e| format!("create data dir: {e}"))?;
        let db_path = data_dir.join("tokenusage.db");
        let conn = Connection::open(&db_path).map_err(|e| format!("open tokenusage.db: {e}"))?;

        conn.busy_timeout(Duration::from_millis(2000))
            .map_err(|e| e.to_string())?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| e.to_string())?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|e| e.to_string())?;

        // 检查并执行初始 migration
        let user_version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap_or(0);

        if user_version == 0 {
            conn.execute_batch(SCHEMA_V1)
                .map_err(|e| format!("exec schema v1: {e}"))?;
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)
                .map_err(|e| e.to_string())?;
        }

        Ok(Self {
            conn: Mutex::new(conn),
            data_dir: data_dir.to_path_buf(),
            writer_version: Mutex::new(String::new()),
        })
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn set_writer_version(&self, ver: &str) {
        if let Ok(mut w) = self.writer_version.lock() {
            *w = ver.to_string();
        }
    }

    pub fn writer_version(&self) -> String {
        self.writer_version
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    pub fn with_read<F, R>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&Connection) -> Result<R, String>,
    {
        let guard = self.conn.lock().map_err(|e| format!("store lock: {e}"))?;
        let res = f(&*guard);
        drop(guard);
        res
    }

    pub fn with_write<F, R>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&Connection) -> Result<R, String>,
    {
        let guard = self.conn.lock().map_err(|e| format!("store lock: {e}"))?;
        let res = f(&*guard);
        drop(guard);
        res
    }
}
