//! storage 模块：fund.db 连接管理（实施 B1）。
//!
//! 仅 Fund 模块独占写入；SQLite WAL + 外键 + busy_timeout；schema 版本权威是
//! `PRAGMA user_version`（契约 §4.1：业务 DB 的 schema version 是实际数据
//! 版本权威，migration journal 负责跨步骤恢复）。
//!
//! 锁纪律（AGENTS）：非重入 `Mutex<Connection>` 只在 `with_read/with_write`
//! 内短持有；业务函数接收 `&Connection`，不再加锁，杜绝嵌套锁死锁。

use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

/// 当前业务 schema 版本。
pub const SCHEMA_VERSION: i64 = 2;

pub struct Store {
    conn: Mutex<Connection>,
    data_dir: std::path::PathBuf,
    /// 用户 DB writer 版本 = Natives Product Version（计划 §27.2），
    /// 由 Runtime 经 ModuleContext 注入；空值时写 "unknown"。
    writer_version: Mutex<String>,
}

/// schema v1：账户、基金主档、交易流水、持仓投影、净值、导入收据、元数据。
/// 金额/净值/份额均为定点 raw（i64）；精度语义见 crate::fixed。
pub const SCHEMA_V1: &str = r#"
CREATE TABLE IF NOT EXISTS accounts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    note TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS funds (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    code TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL DEFAULT '',
    fund_type TEXT NOT NULL DEFAULT '',
    latest_nav_raw INTEGER,
    latest_nav_date TEXT,
    updated_at TEXT NOT NULL
);

-- positions 是 transactions+期初快照的只读回放投影（审计 B6），不双权威。
CREATE TABLE IF NOT EXISTS positions (
    account_id INTEGER NOT NULL REFERENCES accounts(id),
    fund_id INTEGER NOT NULL REFERENCES funds(id),
    quantity_raw INTEGER NOT NULL DEFAULT 0,
    cost_raw INTEGER NOT NULL DEFAULT 0,
    realized_raw INTEGER NOT NULL DEFAULT 0,
    as_of TEXT NOT NULL DEFAULT '',
    updated_at TEXT NOT NULL,
    PRIMARY KEY (account_id, fund_id)
);

-- 同日同账户同基金允许多笔合法交易；手工幂等 request_id 唯一；
-- 导入幂等 (account_id, external_id) 唯一（external_id 非空时）。
CREATE TABLE IF NOT EXISTS transactions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id INTEGER NOT NULL REFERENCES accounts(id),
    fund_id INTEGER NOT NULL REFERENCES funds(id),
    type TEXT NOT NULL CHECK (type IN ('BUY','SELL')),
    state TEXT NOT NULL DEFAULT 'confirmed' CHECK (state IN ('confirmed','pending')),
    quantity_raw INTEGER NOT NULL,
    price_raw INTEGER NOT NULL,
    amount_raw INTEGER NOT NULL,
    fee_raw INTEGER NOT NULL DEFAULT 0,
    trade_date TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT 'manual',
    request_id TEXT UNIQUE,
    external_id TEXT,
    batch_id TEXT,
    line_no INTEGER,
    payload_hash TEXT,
    created_at TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_tx_external
    ON transactions(account_id, external_id) WHERE external_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_tx_replay
    ON transactions(account_id, fund_id, trade_date, id);
CREATE INDEX IF NOT EXISTS idx_tx_date ON transactions(trade_date);

-- 期初持仓快照：仅用于尚无该账户/基金记账历史的建账（实施方案 §7.3）；
-- 不捏造真实买入交易；cost_raw 为 NULL 表示成本未知（不得默认为零）。
CREATE TABLE IF NOT EXISTS opening_snapshots (
    account_id INTEGER NOT NULL REFERENCES accounts(id),
    fund_id INTEGER NOT NULL REFERENCES funds(id),
    as_of TEXT NOT NULL,
    quantity_raw INTEGER NOT NULL,
    cost_raw INTEGER,
    derived_cost INTEGER NOT NULL DEFAULT 0,
    nav_raw INTEGER,
    nav_date TEXT,
    created_at TEXT NOT NULL,
    PRIMARY KEY (account_id, fund_id)
);

CREATE TABLE IF NOT EXISTS fund_nav (
    fund_id INTEGER NOT NULL REFERENCES funds(id),
    nav_date TEXT NOT NULL,
    unit_nav_raw INTEGER NOT NULL,
    accumulated_raw INTEGER,
    daily_growth_raw INTEGER,
    source TEXT NOT NULL,
    fetched_at TEXT NOT NULL,
    PRIMARY KEY (fund_id, nav_date)
);
CREATE INDEX IF NOT EXISTS idx_nav_recent ON fund_nav(fund_id, nav_date DESC);

CREATE TABLE IF NOT EXISTS import_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_id TEXT NOT NULL,
    template_version INTEGER NOT NULL,
    file_sha256 TEXT NOT NULL,
    params_hash TEXT NOT NULL,
    state TEXT NOT NULL,
    result_json TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    UNIQUE(source_id, template_version, file_sha256, params_hash)
);

CREATE TABLE IF NOT EXISTS app_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- 自选分组
CREATE TABLE IF NOT EXISTS watchlists (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);

-- 自选标的列表
CREATE TABLE IF NOT EXISTS watchlist_items (
    watchlist_id TEXT NOT NULL,
    symbol TEXT NOT NULL,
    name TEXT NOT NULL,
    asset_type TEXT NOT NULL DEFAULT 'stock',
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    PRIMARY KEY (watchlist_id, symbol)
);

-- Canvas 原生划线图形持久化
CREATE TABLE IF NOT EXISTS chart_drawings (
    id TEXT PRIMARY KEY,
    symbol TEXT NOT NULL,
    period TEXT NOT NULL,
    tool_type TEXT NOT NULL,
    points_json TEXT NOT NULL,
    options_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_drawings_symbol ON chart_drawings(symbol, period);
"#;

impl Store {
    /// 打开（或创建）data_dir/fund.db，应用 PRAGMA，并确保 schema 版本。
    /// 迁移由 migration 模块执行；这里只允许在版本已就绪时进入业务。
    pub fn open(data_dir: &Path) -> rusqlite::Result<Self> {
        std::fs::create_dir_all(data_dir).map_err(|_| {
            rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CANTOPEN),
                Some("cannot create data dir".into()),
            )
        })?;
        let path = data_dir.join("fund.db");
        let conn = Connection::open(&path)?;
        conn.busy_timeout(std::time::Duration::from_millis(2000))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let store = Self {
            conn: Mutex::new(conn),
            data_dir: data_dir.to_path_buf(),
            writer_version: Mutex::new(String::new()),
        };
        if store.schema_version()? == 0 {
            store.with_write(|conn| -> rusqlite::Result<()> {
                conn.execute_batch(SCHEMA_V1)?;
                conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
                Ok(())
            })?;
        }
        // 自动对齐/修复 chart_drawings 字段（兼容 v2 早期版本缺少 tool_type 等列）
        let _ = store.with_write(|conn: &Connection| -> rusqlite::Result<()> {
            let mut pragma = conn.prepare("PRAGMA table_info(chart_drawings)")?;
            let columns: Vec<String> = pragma
                .query_map([], |row| row.get(1))?
                .filter_map(|r| r.ok())
                .collect();
            if !columns.is_empty() {
                if !columns.iter().any(|c| c == "tool_type") {
                    let _ = conn.execute("ALTER TABLE chart_drawings ADD COLUMN tool_type TEXT NOT NULL DEFAULT 'trendline'", []);
                    if columns.iter().any(|c| c == "tool") {
                        let _ = conn.execute("UPDATE chart_drawings SET tool_type = tool", []);
                    }
                }
                if !columns.iter().any(|c| c == "options_json") {
                    let _ = conn.execute("ALTER TABLE chart_drawings ADD COLUMN options_json TEXT NOT NULL DEFAULT '{}'", []);
                }
                if !columns.iter().any(|c| c == "updated_at") {
                    let _ = conn.execute("ALTER TABLE chart_drawings ADD COLUMN updated_at TEXT NOT NULL DEFAULT ''", []);
                    if columns.iter().any(|c| c == "created_at") {
                        let _ = conn.execute("UPDATE chart_drawings SET updated_at = created_at", []);
                    }
                }
            }
            Ok(())
        });
        Ok(store)
    }

    /// 内存库（测试/健康检查用），直接建到当前版本。
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        let store = Self {
            conn: Mutex::new(conn),
            data_dir: std::path::PathBuf::new(),
            writer_version: Mutex::new(String::new()),
        };
        store.with_write(|conn| -> rusqlite::Result<()> {
            conn.execute_batch(SCHEMA_V1)?;
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            Ok(())
        })?;
        Ok(store)
    }

    pub fn schema_version(&self) -> rusqlite::Result<i64> {
        let guard = self.conn.lock().expect("storage mutex poisoned");
        guard.query_row("PRAGMA user_version", [], |row| row.get(0))
    }

    /// 只读访问：短持锁执行，不嵌套加锁。
    pub fn with_read<T, E>(&self, f: impl FnOnce(&Connection) -> Result<T, E>) -> Result<T, E> {
        let guard = self.conn.lock().expect("storage mutex poisoned");
        f(&guard)
    }

    /// 注入产品版本作为用户 DB writer 版本（计划 §27.2：禁止用模块 Cargo Version）。
    pub fn set_writer_version(&self, version: &str) {
        *self.writer_version.lock().expect("storage mutex poisoned") = version.to_string();
    }

    /// 写事务访问：BEGIN IMMEDIATE…COMMIT/ROLLBACK，短持锁，不嵌套加锁。
    /// 泛型错误：业务层可返回自己的错误类型，rusqlite 错误经 `From` 转换。
    pub fn with_write<T, E>(&self, f: impl FnOnce(&Connection) -> Result<T, E>) -> Result<T, E>
    where
        E: From<rusqlite::Error>,
    {
        let mut guard = self.conn.lock().expect("storage mutex poisoned");
        let tx = guard.transaction()?;
        match f(&tx) {
            Ok(value) => {
                let writer = self
                    .writer_version
                    .lock()
                    .expect("storage mutex poisoned")
                    .clone();
                let writer = if writer.is_empty() {
                    "unknown".to_string()
                } else {
                    writer
                };
                Self::mark_write_committed(&tx, &writer)?;
                tx.commit()?;
                if !self.data_dir.as_os_str().is_empty() {
                    let _ = crate::migration::mark_journal_new_writes(&self.data_dir);
                }
                Ok(value)
            }
            Err(error) => {
                let _ = tx.rollback();
                Err(error)
            }
        }
    }

    /// 在业务事务内记录新写入与 writer 版本标记（与数据原子提交）。
    pub fn mark_write_committed(conn: &Connection, app_version: &str) -> rusqlite::Result<()> {
        conn.execute(
            "INSERT INTO app_meta(key, value) VALUES('has_committed_new_writes', 'true')
             ON CONFLICT(key) DO UPDATE SET value='true'",
            [],
        )?;
        conn.execute(
            "INSERT INTO app_meta(key, value) VALUES('last_data_writer_version', ?1)
             ON CONFLICT(key) DO UPDATE SET value=?1",
            [app_version],
        )?;
        Ok(())
    }

    pub fn has_committed_new_writes(conn: &Connection) -> bool {
        conn.query_row(
            "SELECT value FROM app_meta WHERE key = 'has_committed_new_writes'",
            [],
            |r| r.get::<_, String>(0),
        )
        .map(|v| v == "true")
        .unwrap_or(false)
    }

    pub fn last_data_writer_version(conn: &Connection) -> Option<String> {
        conn.query_row(
            "SELECT value FROM app_meta WHERE key = 'last_data_writer_version'",
            [],
            |r| r.get::<_, String>(0),
        )
        .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_creates_schema_v1() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
        store
            .with_write(|conn| -> rusqlite::Result<()> {
                conn.execute(
                    "INSERT INTO accounts(name, created_at, updated_at) VALUES ('默认账户','2026-01-01','2026-01-01')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let count: i64 = store
            .with_read(|conn| conn.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn foreign_keys_enforced() {
        let store = Store::open_in_memory().unwrap();
        let err = store.with_write(|conn| -> rusqlite::Result<()> {
            conn.execute(
                "INSERT INTO transactions(account_id, fund_id, type, quantity_raw, price_raw, amount_raw, trade_date, created_at)
                 VALUES (999, 999, 'BUY', 1, 1, 1, '2026-01-01', '2026-01-01')",
                [],
            )
            .map(|_| ())
        });
        assert!(err.is_err(), "FK should reject orphan transaction");
    }
}
