//! storage 模块：tokenusage_app.db 存储与 Schema 定义（ADR-0031 / ADR-0033 / Wave 2）。
//!
//! 数据边界（架构整改 Wave 2）：
//! - 本地数据库仅作为应用自身偏好、多设备投影快照与可选可丢弃缓存（tokenusage_app.db）；
//! - 本机 AI 用量、会话、价格表与额度权威归属 Model Host（8766 窄接口与 usage.db）；
//! - 本地不再维护本机事件权威副本，严禁跨域直接读取 Model Host 的 usage.db 与 state.json。

use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

pub const SCHEMA_VERSION: i64 = 2;

pub const SCHEMA_V2: &str = r#"
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sync_devices (
    device_id TEXT PRIMARY KEY,
    device_name TEXT NOT NULL,
    last_synced_at TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'idle',
    payload_hash TEXT
);

CREATE TABLE IF NOT EXISTS remote_usage_snapshots (
    id TEXT PRIMARY KEY,
    device_id TEXT NOT NULL REFERENCES sync_devices(device_id),
    range_label TEXT NOT NULL,
    snapshot_json TEXT NOT NULL,
    generated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS projection_cache (
    cache_key TEXT PRIMARY KEY,
    cache_json TEXT NOT NULL,
    fetched_at TEXT NOT NULL,
    source_contract_version TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS app_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
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
        let mut conn =
            Connection::open(&db_path).map_err(|e| format!("open tokenusage.db: {e}"))?;

        conn.busy_timeout(Duration::from_millis(2000))
            .map_err(|e| e.to_string())?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| e.to_string())?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|e| e.to_string())?;

        let user_version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap_or(0);

        if user_version < SCHEMA_VERSION {
            // 事务化升级与安全备份：在修改旧库前生成 .bak 备份
            if user_version > 0 && db_path.exists() {
                let bak_path = data_dir.join("tokenusage.db.bak");
                let _ = std::fs::copy(&db_path, &bak_path);
            }

            let tx = conn
                .transaction()
                .map_err(|e| format!("begin migration tx: {e}"))?;

            // 1. 创建 V2 基础表
            tx.execute_batch(SCHEMA_V2)
                .map_err(|e| format!("exec schema v2: {e}"))?;

            // 2. 若旧版 sync_state 存在，保留合法设备记录迁移至 sync_devices
            let has_sync_state: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='sync_state')",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(false);
            if has_sync_state {
                let _ = tx.execute(
                    "INSERT OR IGNORE INTO sync_devices (device_id, device_name, last_synced_at, status, payload_hash)
                     SELECT device_id, device_name, last_synced_at, status, payload_hash FROM sync_state",
                    [],
                );
            }

            // 3. 删除旧权威表（彻底终结本地第二权威）
            // 顺序重要：先临时关闭外键或先删子表再删父表
            tx.execute_batch(
                "PRAGMA foreign_keys = OFF;
                 DROP TABLE IF EXISTS usage_records;
                 DROP TABLE IF EXISTS daily_aggregates;
                 DROP TABLE IF EXISTS sessions;
                 DROP TABLE IF EXISTS usage_sources;
                 DROP TABLE IF EXISTS limits_cache;
                 DROP TABLE IF EXISTS sync_state;
                 PRAGMA foreign_keys = ON;",
            )
            .map_err(|e| format!("drop legacy tables: {e}"))?;

            tx.pragma_update(None, "user_version", SCHEMA_VERSION)
                .map_err(|e| e.to_string())?;

            tx.commit()
                .map_err(|e| format!("commit migration tx: {e}"))?;
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
            .map(|w| w.clone())
            .unwrap_or_default()
    }

    pub fn with_read<F, R>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&Connection) -> Result<R, String>,
    {
        let conn = self
            .conn
            .lock()
            .map_err(|e| format!("lock poisoned: {e}"))?;
        f(&conn)
    }

    pub fn with_write<F, R>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&Connection) -> Result<R, String>,
    {
        let mut conn = self
            .conn
            .lock()
            .map_err(|e| format!("lock poisoned: {e}"))?;
        let tx = conn
            .transaction()
            .map_err(|e| format!("begin transaction: {e}"))?;
        let result = f(&tx)?;
        tx.commit()
            .map_err(|e| format!("commit transaction: {e}"))?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_tokenusage_db_v1_to_v2_migration_and_backup() {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("tokenusage.db");

        // 1. 初始化旧版 V1 数据库
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE settings (key TEXT PRIMARY KEY, value_json TEXT NOT NULL, updated_at TEXT NOT NULL);
                 CREATE TABLE sync_state (device_id TEXT PRIMARY KEY, device_name TEXT NOT NULL, last_synced_at TEXT NOT NULL, status TEXT NOT NULL, payload_hash TEXT);
                 CREATE TABLE usage_sources (id TEXT PRIMARY KEY, display_name TEXT NOT NULL);
                 CREATE TABLE sessions (session_id TEXT PRIMARY KEY);
                 CREATE TABLE daily_aggregates (date TEXT, source_id TEXT, model TEXT, PRIMARY KEY (date, source_id, model));
                 INSERT INTO settings (key, value_json, updated_at) VALUES ('theme', '\"dark\"', '12345');
                 INSERT INTO sync_state (device_id, device_name, last_synced_at, status, payload_hash) VALUES ('dev_1', 'My Mac', '2026-09-28', 'online', 'hash1');
                 PRAGMA user_version = 1;",
            ).unwrap();
        }

        // 2. 使用 Store::open 触发升级
        let store = Store::open(temp.path()).unwrap();

        // 3. 验证备份文件生成
        let bak_path = temp.path().join("tokenusage.db.bak");
        assert!(bak_path.exists(), "必须先备份旧库 tokenusage.db.bak");

        // 4. 验证 user_version 升至 2
        store
            .with_read(|conn| {
                let ver: i64 = conn
                    .query_row("PRAGMA user_version", [], |r| r.get(0))
                    .unwrap();
                assert_eq!(ver, 2);

                // 验证 settings 迁移保留
                let theme: String = conn
                    .query_row(
                        "SELECT value_json FROM settings WHERE key='theme'",
                        [],
                        |r| r.get(0),
                    )
                    .unwrap();
                assert_eq!(theme, "\"dark\"");

                // 验证 sync_devices 从旧 sync_state 迁移
                let dev_name: String = conn
                    .query_row(
                        "SELECT device_name FROM sync_devices WHERE device_id='dev_1'",
                        [],
                        |r| r.get(0),
                    )
                    .unwrap();
                assert_eq!(dev_name, "My Mac");

                // 验证旧权威表已被 DROP
                let legacy_tables = ["usage_sources", "sessions", "daily_aggregates", "sync_state"];
                for table in legacy_tables {
                    let exists: bool = conn
                        .query_row(
                            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                            [table],
                            |r| r.get(0),
                        )
                        .unwrap();
                    assert!(!exists, "旧权威表 {table} 必须被彻底删除");
                }

                // 完整性检查
                let check: String = conn
                    .query_row("PRAGMA integrity_check", [], |r| r.get(0))
                    .unwrap();
                assert_eq!(check, "ok");
                Ok(())
            })
            .unwrap();
    }
}
