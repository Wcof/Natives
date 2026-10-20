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

/// 当前业务 schema 版本（v3 人工资产/币种，v4 目标/配置，v5 投资首页账本，
/// v6 交易现金待核对/理由归档，v7 首页自定义资金分组，v27 自定义分类说明）。
pub const SCHEMA_VERSION: i64 = 28;

pub struct Store {
    conn: Mutex<Connection>,
    pub(crate) information_cache: crate::information::ListCache,
    data_dir: std::path::PathBuf,
    /// 用户 DB writer 版本 = Natives Product Version（计划 §27.2），
    /// 由 Runtime 经 ModuleContext 注入；空值时写 "unknown"。
    writer_version: Mutex<String>,
}

/// 新库初始化 DDL：一次性建齐当前 `SCHEMA_VERSION`（v4）的全部表与索引，
/// 因此全新安装的库直接落在 v4，不经历 v1→v2→v3→v4 阶梯。
/// 覆盖：账户、基金主档、交易流水、持仓投影、期初快照、净值、导入收据、
/// 人工资产（v3）、基金币种／分类（v3）、投资目标（v4）、配置目标（v4）、
/// 元数据、自选与画线（v2）。
/// 新增表必须同时加进这里和对应版本的 migration SQL，否则新库缺表。
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

-- v3（P1）：人工资产当前快照与基金币种／分类补充信息（字段规则见
-- migration::SCHEMA_V3_SQL；fresh init 与顺序迁移共用同一套约束）。
CREATE TABLE IF NOT EXISTS manual_assets (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id INTEGER NOT NULL REFERENCES accounts(id),
    product_type TEXT NOT NULL CHECK (product_type IN ('cash','stock','etf','gold')),
    instrument_key TEXT NOT NULL,
    name TEXT NOT NULL,
    risk_class TEXT NOT NULL DEFAULT 'unknown'
        CHECK (risk_class IN ('equity','gold','fixed_income','cash','other','unknown')),
    market_value_minor INTEGER NOT NULL CHECK (market_value_minor >= 0),
    valued_at TEXT NOT NULL,
    legacy_overlap_distinct INTEGER NOT NULL DEFAULT 0 CHECK (legacy_overlap_distinct IN (0,1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (account_id, instrument_key)
);

CREATE TABLE IF NOT EXISTS fund_asset_metadata (
    fund_id INTEGER PRIMARY KEY REFERENCES funds(id),
    nav_currency TEXT NOT NULL DEFAULT 'unknown' CHECK (nav_currency IN ('unknown','CNY','unsupported')),
    risk_class TEXT NOT NULL DEFAULT 'unknown'
        CHECK (risk_class IN ('equity','gold','fixed_income','cash','other','unknown')),
    updated_at TEXT NOT NULL
);

-- v4（P2）：唯一投资资产目标与 5 类配置目标。
CREATE TABLE IF NOT EXISTS investment_goals (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    target_minor INTEGER NOT NULL CHECK (target_minor > 0),
    target_date TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS allocation_targets (
    risk_class TEXT PRIMARY KEY
        CHECK (risk_class IN ('equity','gold','fixed_income','cash','other')),
    target_tenths INTEGER NOT NULL CHECK (target_tenths BETWEEN 0 AND 1000),
    lower_tenths INTEGER NOT NULL CHECK (lower_tenths BETWEEN 0 AND 1000),
    upper_tenths INTEGER NOT NULL CHECK (upper_tenths BETWEEN 0 AND 1000),
    updated_at TEXT NOT NULL
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

-- v5（投资首页，方案 02/03）：资金事件账本、现金预留/占用、多财务目标、
-- 规则与事件、决策记录、信息源与主线。字段语义与 migration::SCHEMA_V5_SQL 一致。
CREATE TABLE IF NOT EXISTS cash_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id INTEGER NOT NULL REFERENCES accounts(id),
    event_type TEXT NOT NULL CHECK (event_type IN
        ('opening','external_in','external_out','internal_transfer',
         'dividend_cash','dividend_reinvest','fee','reconciliation','correction')),
    amount_minor INTEGER NOT NULL,
    occurred_at TEXT NOT NULL,
    idempotency_key TEXT UNIQUE,
    related_type TEXT NOT NULL DEFAULT '',
    related_id TEXT NOT NULL DEFAULT '',
    note TEXT NOT NULL DEFAULT '',
    corrected_by INTEGER REFERENCES cash_events(id),
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_cash_events_account
    ON cash_events(account_id, occurred_at, id);

CREATE TABLE IF NOT EXISTS cash_reserves (
    account_id INTEGER PRIMARY KEY REFERENCES accounts(id),
    state TEXT NOT NULL CHECK (state IN ('unconfigured','configured')),
    amount_minor INTEGER NOT NULL DEFAULT 0 CHECK (amount_minor >= 0),
    purpose TEXT NOT NULL DEFAULT '',
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS cash_reservations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id INTEGER NOT NULL REFERENCES accounts(id),
    symbol TEXT NOT NULL DEFAULT '',
    amount_minor INTEGER NOT NULL CHECK (amount_minor > 0),
    status TEXT NOT NULL DEFAULT 'held'
        CHECK (status IN ('held','released','converted','review')),
    expected_date TEXT NOT NULL DEFAULT '',
    tx_ref TEXT NOT NULL DEFAULT '',
    note TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_cash_reservations_account
    ON cash_reservations(account_id, status);

CREATE TABLE IF NOT EXISTS finance_goals (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    target_minor INTEGER NOT NULL CHECK (target_minor > 0),
    target_date TEXT NOT NULL,
    scope_kind TEXT NOT NULL CHECK (scope_kind IN ('A','B','specific')),
    asset_bindings TEXT NOT NULL DEFAULT '[]',
    is_primary INTEGER NOT NULL DEFAULT 0 CHECK (is_primary IN (0,1)),
    archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0,1)),
    monthly_plan_minor INTEGER,
    version INTEGER NOT NULL DEFAULT 1,
    note TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_finance_goals_primary
    ON finance_goals(is_primary) WHERE is_primary = 1 AND archived = 0;

CREATE TABLE IF NOT EXISTS goal_changes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    goal_id INTEGER NOT NULL REFERENCES finance_goals(id),
    field TEXT NOT NULL,
    old_value TEXT NOT NULL DEFAULT '',
    new_value TEXT NOT NULL DEFAULT '',
    actor TEXT NOT NULL DEFAULT 'user',
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_goal_changes_goal ON goal_changes(goal_id, created_at);

CREATE TABLE IF NOT EXISTS watch_rules (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    rule_type TEXT NOT NULL CHECK (rule_type IN
        ('cash_short','class_unknown','stale_valuation','allocation_drift',
         'concentration','price_threshold','daily_change','ma_cross',
         'drawdown','strategy_exit','investment_lag')),
    object_key TEXT NOT NULL DEFAULT '',
    params_json TEXT NOT NULL DEFAULT '{}',
    period TEXT NOT NULL DEFAULT 'quote',
    enabled INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0,1)),
    notify_channel TEXT NOT NULL DEFAULT 'in_app',
    version INTEGER NOT NULL DEFAULT 1,
    archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0,1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS watch_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    rule_id INTEGER NOT NULL REFERENCES watch_rules(id),
    rule_version INTEGER NOT NULL,
    rule_type TEXT NOT NULL,
    object_key TEXT NOT NULL DEFAULT '',
    first_triggered_at TEXT NOT NULL,
    last_checked_at TEXT NOT NULL,
    trigger_value TEXT NOT NULL DEFAULT '',
    threshold_value TEXT NOT NULL DEFAULT '',
    snapshot_id TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'open'
        CHECK (status IN ('open','acknowledged','snoozed','resolved')),
    ready_state TEXT NOT NULL DEFAULT 'ready'
        CHECK (ready_state IN ('ready','unknown','stale','error')),
    snoozed_until TEXT,
    resolved_at TEXT,
    note TEXT NOT NULL DEFAULT '',
    notify_result TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_watch_events_open
    ON watch_events(rule_id, object_key) WHERE status != 'resolved';
CREATE INDEX IF NOT EXISTS idx_watch_events_status ON watch_events(status, updated_at);

CREATE TABLE IF NOT EXISTS decision_records (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id INTEGER NOT NULL REFERENCES watch_events(id),
    action TEXT NOT NULL CHECK (action IN ('observe','defer','done')),
    note TEXT NOT NULL DEFAULT '',
    evidence_json TEXT NOT NULL DEFAULT '[]',
    snapshot_id TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_decision_records_event
    ON decision_records(event_id, created_at);

CREATE TABLE IF NOT EXISTS info_sources (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    url TEXT NOT NULL DEFAULT '',
    kind TEXT NOT NULL CHECK (kind IN ('link','excerpt')),
    related_symbol TEXT NOT NULL DEFAULT '',
    thesis_id INTEGER,
    publisher TEXT NOT NULL DEFAULT '',
    published_at TEXT,
    excerpt_text TEXT NOT NULL DEFAULT '',
    excerpt_fetched_at TEXT,
    read_status INTEGER NOT NULL DEFAULT 0 CHECK (read_status IN (0,1)),
    favorite INTEGER NOT NULL DEFAULT 0 CHECK (favorite IN (0,1)),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0,1)),
    archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0,1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS investment_theses (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    holding_reason TEXT NOT NULL DEFAULT '',
    horizon TEXT NOT NULL DEFAULT '',
    verify_metrics TEXT NOT NULL DEFAULT '',
    invalid_condition TEXT NOT NULL DEFAULT '',
    related_symbols TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);


CREATE TABLE IF NOT EXISTS value_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    taken_at TEXT NOT NULL,
    scope TEXT NOT NULL DEFAULT 'A',
    total_minor INTEGER NOT NULL,
    cash_minor INTEGER NOT NULL DEFAULT 0,
    noncash_minor INTEGER NOT NULL DEFAULT 0,
    breakdown_json TEXT NOT NULL DEFAULT '{}',
    quality TEXT NOT NULL DEFAULT 'ready',
    note TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_value_snapshots_time ON value_snapshots(taken_at, id);

-- 旧单目标无损迁移（02 §6.2）：全新库时 investment_goals 为空，本句无操作。
INSERT INTO finance_goals(name, target_minor, target_date, scope_kind, is_primary, note, created_at, updated_at)
SELECT '财务目标', g.target_minor, g.target_date, 'A', 1, '由旧单一目标迁移（保留 A 口径）', g.updated_at, g.updated_at
FROM investment_goals g
WHERE NOT EXISTS (SELECT 1 FROM finance_goals);
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
        let mut conn = Connection::open(&path)?;
        crate::sql_diag::instrument(&mut conn);
        conn.busy_timeout(std::time::Duration::from_millis(2000))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let store = Self {
            conn: Mutex::new(conn),
            information_cache: crate::information::ListCache::new(),
            data_dir: data_dir.to_path_buf(),
            writer_version: Mutex::new(String::new()),
        };
        if store.schema_version()? == 0 {
            store.with_write(|conn| -> rusqlite::Result<()> {
                conn.execute_batch(SCHEMA_V1)?;
                // v6 增量对象（trade_cash 等）不在 SCHEMA_V1 全量 DDL 内，
                // 由版本化迁移步骤补齐（与阶梯迁移同源）。
                conn.execute_batch(crate::migration::SCHEMA_V6_SQL)?;
                conn.execute_batch(crate::migration::SCHEMA_V7_SQL)?;
                conn.execute_batch(crate::migration::SCHEMA_V8_SQL)?;
                conn.execute_batch(crate::migration::SCHEMA_V9_SQL)?;
                conn.execute_batch(crate::migration::SCHEMA_V10_SQL)?;
                conn.execute_batch(crate::migration::SCHEMA_V11_SQL)?;
                conn.execute_batch(crate::migration::SCHEMA_V12_SQL)?;
                conn.execute_batch(crate::migration::SCHEMA_V13_SQL)?;
                conn.execute_batch(crate::migration::SCHEMA_V14_SQL)?;
                conn.execute_batch(crate::migration::SCHEMA_V15_SQL)?;
                conn.execute_batch(crate::migration::SCHEMA_V16_SQL)?;
                conn.execute_batch(crate::migration_late::SCHEMA_V17_SQL)?;
                conn.execute_batch(crate::migration_late::SCHEMA_V18_SQL)?;
                conn.execute_batch(crate::migration_late::SCHEMA_V19_SQL)?;
                conn.execute_batch(crate::migration_late::SCHEMA_V20_SQL)?;
                conn.execute_batch(crate::migration_late::SCHEMA_V21_SQL)?;
                conn.execute_batch(crate::migration_late::SCHEMA_V22_SQL)?;
                conn.execute_batch(crate::migration_late::SCHEMA_V23_SQL)?;
                conn.execute_batch(crate::migration_late::SCHEMA_V24_SQL)?;
                conn.execute_batch(crate::migration_late::SCHEMA_V25_SQL)?;
                conn.execute_batch(crate::migration_late::SCHEMA_V26_SQL)?;
                conn.execute_batch(crate::migration_late::SCHEMA_V27_SQL)?;
                conn.execute_batch(crate::migration_late::SCHEMA_V28_SQL)?;
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
            information_cache: crate::information::ListCache::new(),
            data_dir: std::path::PathBuf::new(),
            writer_version: Mutex::new(String::new()),
        };
        store.with_write(|conn| -> rusqlite::Result<()> {
            conn.execute_batch(SCHEMA_V1)?;
            conn.execute_batch(crate::migration::SCHEMA_V6_SQL)?;
            conn.execute_batch(crate::migration::SCHEMA_V7_SQL)?;
            conn.execute_batch(crate::migration::SCHEMA_V8_SQL)?;
            conn.execute_batch(crate::migration::SCHEMA_V9_SQL)?;
            conn.execute_batch(crate::migration::SCHEMA_V10_SQL)?;
            conn.execute_batch(crate::migration::SCHEMA_V11_SQL)?;
            conn.execute_batch(crate::migration::SCHEMA_V12_SQL)?;
            conn.execute_batch(crate::migration::SCHEMA_V13_SQL)?;
            conn.execute_batch(crate::migration::SCHEMA_V14_SQL)?;
            conn.execute_batch(crate::migration::SCHEMA_V15_SQL)?;
            conn.execute_batch(crate::migration::SCHEMA_V16_SQL)?;
            conn.execute_batch(crate::migration_late::SCHEMA_V17_SQL)?;
            conn.execute_batch(crate::migration_late::SCHEMA_V18_SQL)?;
            conn.execute_batch(crate::migration_late::SCHEMA_V19_SQL)?;
            conn.execute_batch(crate::migration_late::SCHEMA_V20_SQL)?;
            conn.execute_batch(crate::migration_late::SCHEMA_V21_SQL)?;
            conn.execute_batch(crate::migration_late::SCHEMA_V22_SQL)?;
            conn.execute_batch(crate::migration_late::SCHEMA_V23_SQL)?;
            conn.execute_batch(crate::migration_late::SCHEMA_V24_SQL)?;
            conn.execute_batch(crate::migration_late::SCHEMA_V25_SQL)?;
            conn.execute_batch(crate::migration_late::SCHEMA_V26_SQL)?;
            conn.execute_batch(crate::migration_late::SCHEMA_V27_SQL)?;
            conn.execute_batch(crate::migration_late::SCHEMA_V28_SQL)?;
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

    pub fn with_snapshot<T, E>(&self, f: impl FnOnce(&Connection) -> Result<T, E>) -> Result<T, E>
    where
        E: From<rusqlite::Error>,
    {
        let guard = self.conn.lock().expect("storage mutex poisoned");
        let tx = guard.unchecked_transaction()?;
        let result = f(&tx)?;
        tx.commit()?;
        Ok(result)
    }

    pub fn data_revision(conn: &Connection) -> rusqlite::Result<String> {
        conn.query_row(
            "SELECT COALESCE((SELECT value FROM app_meta WHERE key='data_revision'),'0')",
            [],
            |r| r.get(0),
        )
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
        let before = tx.total_changes();
        match f(&tx) {
            Ok(value) => {
                if tx.total_changes() != before {
                    let revision: i64 = Self::data_revision(&tx)?
                        .parse()
                        .map_err(|_| rusqlite::Error::InvalidQuery)?;
                    let next = revision
                        .checked_add(1)
                        .ok_or(rusqlite::Error::InvalidQuery)?;
                    tx.execute("INSERT INTO app_meta(key,value) VALUES('data_revision',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [next.to_string()])?;
                }
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
