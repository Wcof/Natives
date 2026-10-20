//! migration 模块：业务 schema 版本化迁移与备份（契约 §4.1 / 实施方案 B3）。
//!
//! - 业务 DB 的 schema version 权威是 `PRAGMA user_version`；journal 只负责
//!   跨步骤崩溃恢复，不含业务内容。
//! - journal `data/.migration.json`：prepared→migrating→verified→committed；
//!   失败为 restored/failed；temp+fsync+rename 原子写入。
//! - 备份在 `backups/<migrationId>/fund.db.bak`，用 SQLite 在线 backup API
//!   产生一致性快照（含 WAL 已提交内容），禁止只复制正在使用的 db 文件。
//! - 迁移失败且未接受新写入：恢复备份后报告，不进入 ready。

use rusqlite::Connection;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// 单个版本化迁移。
#[derive(Debug, Clone)]
pub struct Migration {
    /// 迁移到这一步后的 schema 版本（PRAGMA user_version 目标值）。
    pub version: i64,
    pub name: &'static str,
    /// 已应用版本（version 之前）的库上执行的 SQL。
    pub sql: &'static str,
}

pub const SCHEMA_V2_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS watchlists (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS watchlist_items (
    watchlist_id TEXT NOT NULL,
    symbol TEXT NOT NULL,
    name TEXT NOT NULL,
    asset_type TEXT NOT NULL DEFAULT 'stock',
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    PRIMARY KEY (watchlist_id, symbol)
);

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

-- 与 storage::SCHEMA_V1 的新库初始化保持一致：v1 时代创建但漏进本步的
-- 索引，旧库升级后必须同样存在（升级结果 == 新库结果）。
CREATE INDEX IF NOT EXISTS idx_drawings_symbol ON chart_drawings(symbol, period);
"#;

/// v3：人工资产当前快照与基金币种／分类补充信息（P1，方案 05 §2/§3）。
pub const SCHEMA_V3_SQL: &str = r#"
-- 人工登记的非基金资产当前快照：金额固定人民币分位（CNY 总估值），
-- 数量/单价不存（避免与总估值形成两个计算权威）。
-- instrument_key 由 Fund 生成：cash 固定 'cash'；股票/ETF 共用
-- security:<market>:<code>；黄金 gold:<归一化名称>。
-- (account_id, instrument_key) 唯一：产品类型不参与唯一约束，
-- 防止同一证券分别按股票、ETF 录入两次。
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

-- 基金补充信息：净值币种与主风险类别，来源固定为用户确认
-- （旧库迁移后默认 unknown，不按基金代码推断）。
CREATE TABLE IF NOT EXISTS fund_asset_metadata (
    fund_id INTEGER PRIMARY KEY REFERENCES funds(id),
    nav_currency TEXT NOT NULL DEFAULT 'unknown' CHECK (nav_currency IN ('unknown','CNY','unsupported')),
    risk_class TEXT NOT NULL DEFAULT 'unknown'
        CHECK (risk_class IN ('equity','gold','fixed_income','cash','other','unknown')),
    updated_at TEXT NOT NULL
);
"#;

/// v4：唯一投资资产目标与 5 类配置目标（P2，方案 05 §4）。
pub const SCHEMA_V4_SQL: &str = r#"
-- 唯一目标（固定 id=1）：金额必须大于零（人民币分位）。
CREATE TABLE IF NOT EXISTS investment_goals (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    target_minor INTEGER NOT NULL CHECK (target_minor > 0),
    target_date TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- 配置策略：目标比例与上下限用百分比 1 位小数的定点数（tenths）存储。
CREATE TABLE IF NOT EXISTS allocation_targets (
    risk_class TEXT PRIMARY KEY
        CHECK (risk_class IN ('equity','gold','fixed_income','cash','other')),
    target_tenths INTEGER NOT NULL CHECK (target_tenths BETWEEN 0 AND 1000),
    lower_tenths INTEGER NOT NULL CHECK (lower_tenths BETWEEN 0 AND 1000),
    upper_tenths INTEGER NOT NULL CHECK (upper_tenths BETWEEN 0 AND 1000),
    updated_at TEXT NOT NULL
);
"#;

/// v5：投资首页（方案 02/03 契约）——资金事件账本、现金预留/占用、
/// 多财务目标（旧单目标无损迁移）、规则与事件、决策记录、信息源与主线。
pub const SCHEMA_V5_SQL: &str = r#"
-- 资金事件账本（02 §3.2 最小完整事件集合）。金额人民币分位；
-- 外部净投入 = external_in − external_out；内部转账组合口径为 0（两条事件
-- 由同一 idempotency 关联，各记一减一增）。
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

-- 现金预留（02 §1.1 R）：account 级单条；未配置（unconfigured）区别于明确 0。
CREATE TABLE IF NOT EXISTS cash_reserves (
    account_id INTEGER PRIMARY KEY REFERENCES accounts(id),
    state TEXT NOT NULL CHECK (state IN ('unconfigured','configured')),
    amount_minor INTEGER NOT NULL DEFAULT 0 CHECK (amount_minor >= 0),
    purpose TEXT NOT NULL DEFAULT '',
    updated_at TEXT NOT NULL
);

-- 现金占用（02 §1.1 L）：待成交买入等已确认占用；成交后 converted 并关联交易。
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

-- 多财务目标（02 §6）：旧 investment_goals(id=1) 无损迁移为 scope_kind='A'
-- 的整体主目标；旧表保留只读。主目标唯一（部分唯一索引）。
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

-- 目标关键修改记录（02 §6.4）：前后值、操作者类别与时间。
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

-- 规则（03 §5.1）：阈值等参数必须由用户保存确认后启用；编辑产生新版本。
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

-- 事件（03 §5）：同规则版本+同对象+同触发类型维护一个未解除事件（部分唯一索引）。
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

-- 决策记录（F09）：观察/暂缓/已处理 + 理由与证据引用；不改变持仓。
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

-- 信息源（F06）：链接收藏与用户摘录两类；摘录保存发布者/时间/URL/取得时间。
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

-- 投资主线（F06）：持有理由/期限/验证指标/失效条件；标签可重叠，不参与五类总和。
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


-- 估值快照（02 §3.3）：收益区间的期初/期末基线；quality 非 ready 的快照不参与。
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

-- 旧单目标（investment_goals id=1）无损迁移：整体目标（A 口径）+ 主目标。
-- 旧表保留只读；金额、日期原样保留（02 §6.2：不自动改 B、不自动减预留）。
INSERT INTO finance_goals(name, target_minor, target_date, scope_kind, is_primary, note, created_at, updated_at)
SELECT '财务目标', g.target_minor, g.target_date, 'A', 1, '由旧单一目标迁移（保留 A 口径）', g.updated_at, g.updated_at
FROM investment_goals g
WHERE NOT EXISTS (SELECT 1 FROM finance_goals);
"#;

/// 迁移阶梯：v0（空库）→ v1 基线 → v2 自选/划线 → v3 人工资产/币种 → v4 目标/配置
/// → v5 投资首页（资金账本/多目标/规则事件/信息源）。
/// 新版本只追加，不改历史条目。
/// v6：归档保留持有理由，现金占用显式分类（旧记录未知，须核对）。
pub const SCHEMA_V6_SQL: &str = r#"
CREATE TABLE trade_cash (
    tx_id INTEGER PRIMARY KEY REFERENCES transactions(id),
    account_id INTEGER NOT NULL REFERENCES accounts(id),
    delta_minor INTEGER NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('unreconciled','receivable','settled')),
    settled_at TEXT NOT NULL DEFAULT '',
    changed_revision INTEGER NOT NULL DEFAULT 0,
    note TEXT NOT NULL DEFAULT ''
);
ALTER TABLE investment_theses ADD COLUMN archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0,1));
ALTER TABLE cash_reservations ADD COLUMN risk_class TEXT NOT NULL DEFAULT 'unknown'
    CHECK (risk_class IN ('equity','gold','fixed_income','other','unknown'));
"#;

/// v7：首页自定义资金分组（方案 11 §4）。
/// group_plan 单行方案（id=1）；组名大小写归一唯一；绑定引用不存金额。
/// 旧 allocation_targets 与旧事件原样保留，不在迁移中把 B 比例当 A 生效。
pub const SCHEMA_V7_SQL: &str = r#"
CREATE TABLE group_plan (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    revision INTEGER NOT NULL DEFAULT 0,
    paused_legacy_rules INTEGER NOT NULL DEFAULT 0 CHECK (paused_legacy_rules IN (0,1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE group_plan_groups (
    group_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    name_norm TEXT NOT NULL UNIQUE,
    ratio_tenths INTEGER NOT NULL CHECK (ratio_tenths BETWEEN 0 AND 1000),
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE group_plan_bindings (
    group_id TEXT NOT NULL REFERENCES group_plan_groups(group_id),
    source_kind TEXT NOT NULL CHECK (source_kind IN ('fund','manual')),
    account_id INTEGER,
    fund_id INTEGER,
    asset_id INTEGER,
    created_at TEXT NOT NULL,
    CHECK (
        (source_kind = 'fund' AND account_id IS NOT NULL AND fund_id IS NOT NULL AND asset_id IS NULL)
        OR
        (source_kind = 'manual' AND asset_id IS NOT NULL AND account_id IS NULL AND fund_id IS NULL)
    )
);
CREATE UNIQUE INDEX idx_group_binding_fund
    ON group_plan_bindings(account_id, fund_id) WHERE source_kind='fund';
CREATE UNIQUE INDEX idx_group_binding_manual
    ON group_plan_bindings(asset_id) WHERE source_kind='manual';

-- AI 分析报告（方案 11 §7）：明确的非交易分析记录；不伪造 event_id；
-- 用户主动保存，支持明确删除；列表分页每页最多 50 条。
CREATE TABLE analysis_reports (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task TEXT NOT NULL,
    scope_json TEXT NOT NULL,
    evidence_json TEXT NOT NULL,
    snapshot_id TEXT NOT NULL,
    data_revision TEXT NOT NULL,
    provider_id TEXT NOT NULL DEFAULT '',
    model_id TEXT NOT NULL DEFAULT '',
    model_name TEXT NOT NULL DEFAULT '',
    result_text TEXT NOT NULL,
    generated_at TEXT NOT NULL,
    saved_at TEXT NOT NULL
);
CREATE INDEX idx_analysis_reports_saved ON analysis_reports(saved_at DESC, id DESC);
"#;

pub use crate::migration_late::{
    SCHEMA_V10_SQL, SCHEMA_V11_SQL, SCHEMA_V12_SQL, SCHEMA_V13_SQL, SCHEMA_V14_SQL, SCHEMA_V15_SQL,
    SCHEMA_V16_SQL, SCHEMA_V8_SQL, SCHEMA_V9_SQL,
};

pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "baseline_schema_v1",
        sql: crate::storage::SCHEMA_V1,
    },
    Migration {
        version: 2,
        name: "watchlists_and_drawings_v2",
        sql: SCHEMA_V2_SQL,
    },
    Migration {
        version: 3,
        name: "manual_assets_and_fund_metadata_v3",
        sql: SCHEMA_V3_SQL,
    },
    Migration {
        version: 4,
        name: "investment_goal_and_allocation_v4",
        sql: SCHEMA_V4_SQL,
    },
    Migration {
        version: 5,
        name: "home_finance_goals_rules_v5",
        sql: SCHEMA_V5_SQL,
    },
    Migration {
        version: 6,
        name: "thesis_archive_and_reservation_class_v6",
        sql: SCHEMA_V6_SQL,
    },
    Migration {
        version: 7,
        name: "home_group_plan_v7",
        sql: SCHEMA_V7_SQL,
    },
];

/// 全量迁移阶梯：v1～v7（本文件）+ v8～v15（migration_late.rs）。
pub fn all_migrations() -> &'static [Migration] {
    static ALL: std::sync::OnceLock<Vec<Migration>> = std::sync::OnceLock::new();
    ALL.get_or_init(|| {
        let mut v = MIGRATIONS.to_vec();
        v.extend_from_slice(crate::migration_late::LATE_MIGRATIONS);
        v
    })
}

/// 迁移 journal（契约 §4.1 字段，不含业务内容与 Secret）。
#[derive(Serialize, serde::Deserialize, PartialEq, Eq, Debug, Clone)]
pub struct MigrationJournal {
    #[serde(rename = "migrationId")]
    pub migration_id: String,
    #[serde(rename = "fromSchema")]
    pub from_schema: i64,
    #[serde(rename = "toSchema")]
    pub to_schema: i64,
    #[serde(rename = "fromAppVersion")]
    pub from_app_version: String,
    #[serde(rename = "toAppVersion")]
    pub to_app_version: String,
    pub state: String,
    #[serde(rename = "backupId")]
    pub backup_id: String,
    #[serde(rename = "hasCommittedNewWrites")]
    pub has_committed_new_writes: bool,
    #[serde(rename = "previousVersionCompatible", default = "default_true")]
    pub previous_version_compatible: bool,
    /// 迁移提交时刻（P3-4：真实记录，不推测；旧 journal 无此字段）。
    #[serde(rename = "migratedAt", default)]
    pub migrated_at: Option<String>,
}

fn default_true() -> bool {
    true
}

/// 标记 journal 已产生新业务写入（防止后续误回滚覆盖新数据）。
pub fn mark_journal_new_writes(data_dir: &Path) -> Result<(), MigrationError> {
    let journal_path = data_dir.join(".migration.json");
    if let Some(mut journal) = read_journal(&journal_path) {
        if !journal.has_committed_new_writes {
            journal.has_committed_new_writes = true;
            write_journal_atomic(&journal_path, &journal)?;
        }
    }
    Ok(())
}

/// 迁移错误。
#[derive(Debug, PartialEq, Eq)]
pub enum MigrationError {
    /// 迁移失败，已恢复备份；须先修复，禁止 ready。
    Restored(String),
    /// 迁移失败且恢复也失败（failed）；数据保留在备份中。
    Failed(String),
    /// journal 存在未完成迁移（previous crash）；先恢复再重试。
    PendingJournal(String),
}

impl MigrationError {
    pub fn message(&self) -> &str {
        match self {
            MigrationError::Restored(m)
            | MigrationError::Failed(m)
            | MigrationError::PendingJournal(m) => m,
        }
    }
}

fn now_iso() -> String {
    // 有界、无外部依赖的时间戳（日志/journal 排序用）。
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

fn migration_id(from: i64, to: i64) -> String {
    format!("m{from}-to-{to}-{}", now_iso())
}

/// 原子写入 journal：temp + fsync + rename。
fn write_journal_atomic(path: &Path, journal: &MigrationJournal) -> Result<(), MigrationError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| MigrationError::Failed(e.to_string()))?;
    }
    let body = serde_json::to_vec(journal).map_err(|e| MigrationError::Failed(e.to_string()))?;
    let temp = path.with_extension("json.tmp");
    {
        use std::io::Write;
        let mut file =
            std::fs::File::create(&temp).map_err(|e| MigrationError::Failed(e.to_string()))?;
        file.write_all(&body)
            .and_then(|_| file.sync_all())
            .map_err(|e| MigrationError::Failed(e.to_string()))?;
    }
    std::fs::rename(&temp, path).map_err(|e| MigrationError::Failed(e.to_string()))?;
    Ok(())
}

/// 读取 journal；文件缺失或解析失败都返回 None（调用方按需区分）。
pub(crate) fn read_journal(path: &Path) -> Option<MigrationJournal> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// 把当前库一致性快照备份到 backups/<id>/fund.db.bak（在线 backup API）。
fn backup_database(data_dir: &Path, id: &str, live: &Connection) -> Result<PathBuf, String> {
    let dir = data_dir.join("backups").join(id);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let target_path = dir.join("fund.db.bak");
    // 备份到临时文件再 rename，避免半写备份被当作可用快照。
    let temp_path = dir.join("fund.db.bak.tmp");
    std::fs::remove_file(&temp_path).ok();
    let mut target =
        Connection::open(&temp_path).map_err(|e| format!("open backup target: {e}"))?;
    let backup = rusqlite::backup::Backup::new(live, &mut target)
        .map_err(|e| format!("init backup: {e}"))?;
    backup
        .run_to_completion(64, std::time::Duration::from_millis(5), None)
        .map_err(|e| format!("run backup: {e}"))?;
    drop(backup);
    target
        .close()
        .map_err(|(_, e)| format!("close backup: {e}"))?;
    std::fs::rename(&temp_path, &target_path).map_err(|e| e.to_string())?;
    Ok(target_path)
}

/// 用备份恢复业务库（复制回 fund.db；调用方负责保证此时无写入）。
fn restore_database(data_dir: &Path, backup_id: &str) -> Result<(), String> {
    let source = data_dir.join("backups").join(backup_id).join("fund.db.bak");
    if !source.exists() {
        return Err(format!("backup missing: {backup_id}"));
    }
    let meta = std::fs::metadata(&source).map_err(|e| e.to_string())?;
    if meta.len() == 0 {
        return Err(format!("backup is empty: {backup_id}"));
    }
    let target = data_dir.join("fund.db");
    let temp_target = data_dir.join("fund.db.restore.tmp");
    std::fs::copy(&source, &temp_target).map_err(|e| format!("copy backup: {e}"))?;
    // 先清 WAL 与 SHM，恢复后的库自身就是一致快照。
    let _ = std::fs::remove_file(data_dir.join("fund.db-wal"));
    let _ = std::fs::remove_file(data_dir.join("fund.db-shm"));
    std::fs::rename(&temp_target, &target).map_err(|e| format!("rename restored db: {e}"))?;
    Ok(())
}

/// 已开始但未完成的 journal 恢复：有备份且未提交新写入则回滚数据。
/// 若已有新写入，拒绝覆盖并报错（保护已提交业务数据）。
/// restored 是终态，不再重复恢复。
pub fn recover_pending_journal(data_dir: &Path) -> Result<bool, String> {
    let journal_path = data_dir.join(".migration.json");
    let Some(journal) = read_journal(&journal_path) else {
        return Ok(false);
    };
    match journal.state.as_str() {
        "committed" | "failed" | "restored" => Ok(false),
        _ => {
            if journal.has_committed_new_writes {
                return Err("cannot restore backup: database has committed new writes".into());
            }
            restore_database(data_dir, &journal.backup_id)
                .map_err(|e| format!("restore backup: {e}"))?;
            let restored = MigrationJournal {
                state: "restored".into(),
                ..journal
            };
            write_journal_atomic(&journal_path, &restored).map_err(|e| e.message().to_string())?;
            Ok(true)
        }
    }
}

/// 把数据库从当前版本迁移到 MIGRATIONS 目标版本。
/// conn 打开时已应用 WAL/FK PRAGMA；调用方持独占运行环境（无并发写）。
pub fn run_migrations(
    data_dir: &Path,
    conn: &Connection,
    app_version: &str,
) -> Result<i64, MigrationError> {
    let journal_path = data_dir.join(".migration.json");

    // 崩溃恢复：上一迁移未 committed 且有备份 → 先恢复，禁止在半迁移库上继续。
    if let Some(journal) = read_journal(&journal_path) {
        if journal.state != "committed" && journal.state != "failed" && journal.state != "restored"
        {
            return Err(MigrationError::PendingJournal(format!(
                "migration {} in state {}",
                journal.migration_id, journal.state
            )));
        }
    }

    let current: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|e| MigrationError::Failed(e.to_string()))?;
    let target = all_migrations().last().map(|m| m.version).unwrap_or(0);
    if current == target {
        return Ok(current);
    }
    if current > target {
        return Err(MigrationError::Failed(format!(
            "data schema {current} is newer than app schema {target}"
        )));
    }

    // 找到第一个待应用的迁移起点。
    let pending: Vec<&Migration> = all_migrations()
        .iter()
        .filter(|m| m.version > current)
        .collect();

    let id = migration_id(current, target);
    let mut journal = MigrationJournal {
        migration_id: id.clone(),
        from_schema: current,
        to_schema: target,
        from_app_version: app_version.to_string(),
        to_app_version: app_version.to_string(),
        state: "prepared".into(),
        backup_id: id.clone(),
        has_committed_new_writes: false,
        previous_version_compatible: true,
        migrated_at: None,
    };
    write_journal_atomic(&journal_path, &journal)?;

    // 一致性备份（prepared→migrating 切点）。
    backup_database(data_dir, &id, conn).map_err(MigrationError::Failed)?;
    journal.state = "migrating".into();
    write_journal_atomic(&journal_path, &journal)?;

    // 逐版本应用（幂等失败回滚）。
    for migration in &pending {
        let apply = || -> Result<(), rusqlite::Error> {
            conn.execute_batch(migration.sql)?;
            conn.pragma_update(None, "user_version", migration.version)?;
            Ok(())
        };
        if let Err(error) = apply() {
            let message = format!("migration {}: {error}", migration.name);
            if !journal.has_committed_new_writes && restore_database(data_dir, &id).is_ok() {
                journal.state = "restored".into();
                write_journal_atomic(&journal_path, &journal)?;
                return Err(MigrationError::Restored(message));
            }
            journal.state = "failed".into();
            write_journal_atomic(&journal_path, &journal)?;
            return Err(MigrationError::Failed(message));
        }
    }

    // 验证：user_version 已达目标且核心表可读。
    let verified: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|e| MigrationError::Failed(e.to_string()))?;
    if verified != target {
        journal.state = "failed".into();
        let _ = write_journal_atomic(&journal_path, &journal);
        return Err(MigrationError::Failed(format!(
            "verified {verified} != target {target}"
        )));
    }

    journal.state = "verified".into();
    write_journal_atomic(&journal_path, &journal)?;
    journal.state = "committed".into();
    journal.migrated_at = Some(now_iso());
    write_journal_atomic(&journal_path, &journal)?;
    Ok(target)
}

/// 数据状态（契约 §4.1 / --inspect-data 与 app:data_status 共用）。
///
/// `readiness`（ADR-0036）是可判别的数据就绪状态：`ready | uninitialized |
/// migration_required | recovery_required | incompatible | unreadable`。
/// 缺库、打不开库或 journal 解析失败不得回退为「当前 schema 即 ready」。
/// `uninitialized` 同时覆盖「库文件不存在」与「库存在但 user_version=0（空库／
/// 建库中断）」；二者的 `currentSchema` 不同（缺库回落目标版本，空库报 0），
/// 因此 `currentSchema` 与 `readiness` 始终自洽。
#[derive(Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct DataStatus {
    #[serde(rename = "currentSchema")]
    pub current_schema: i64,
    #[serde(rename = "migrationState")]
    pub migration_state: String,
    #[serde(rename = "lastDataWriterVersion")]
    pub last_data_writer_version: String,
    #[serde(rename = "hasCommittedNewWrites")]
    pub has_committed_new_writes: bool,
    #[serde(rename = "previousVersionCompatible")]
    pub previous_version_compatible: bool,
    pub readiness: String,
}

/// readiness 取值常量（ADR-0036；卡片只放行 `ready`）。
pub mod readiness {
    pub const READY: &str = "ready";
    pub const UNINITIALIZED: &str = "uninitialized";
    pub const MIGRATION_REQUIRED: &str = "migration_required";
    pub const RECOVERY_REQUIRED: &str = "recovery_required";
    pub const INCOMPATIBLE: &str = "incompatible";
    pub const UNREADABLE: &str = "unreadable";
}

pub fn read_data_status(data_dir: &Path, default_app_version: &str) -> DataStatus {
    let db_path = data_dir.join("fund.db");
    let mut db_schema: Option<i64> = None;
    let mut db_has_writes = false;
    let mut db_writer_version: Option<String> = None;
    // 只读连接探测：打不开/读不出 schema 都按 unreadable 处理，禁止把
    // 「读不到」当作「已是当前版本」。
    let mut db_unreadable = false;

    if db_path.exists() {
        match Connection::open_with_flags(
            &db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ) {
            Ok(conn) => match conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)) {
                Ok(version) => {
                    db_schema = Some(version);
                    db_has_writes = crate::storage::Store::has_committed_new_writes(&conn);
                    db_writer_version = crate::storage::Store::last_data_writer_version(&conn);
                }
                Err(_) => db_unreadable = true,
            },
            Err(_) => db_unreadable = true,
        }
    }

    let journal_path = data_dir.join(".migration.json");
    // journal 文件存在但解析失败：无法判定上次迁移状态，按待恢复处理。
    let journal_corrupt = journal_path.exists() && read_journal(&journal_path).is_none();
    let journal = read_journal(&journal_path);
    let journal_pending = journal
        .as_ref()
        .is_some_and(|j| j.state != "committed" && j.state != "failed" && j.state != "restored");

    let target_schema = crate::storage::SCHEMA_VERSION;
    // currentSchema 必须与 readiness 自洽：空库（user_version=0）如实报 0，
    // 不抹成目标版本；缺库且无 journal 时才回落到目标版本（无库可报）。
    let current_schema = match db_schema {
        Some(version) => version,
        None if db_unreadable => 0,
        None => journal
            .as_ref()
            .map(|j| j.to_schema)
            .unwrap_or(target_schema),
    };

    let readiness = if journal_corrupt || journal_pending {
        // 未完成迁移：先由用户进入投资详情走恢复，卡片不 start。
        readiness::RECOVERY_REQUIRED
    } else if !db_path.exists() {
        readiness::UNINITIALIZED
    } else if db_unreadable || db_schema.is_none() {
        readiness::UNREADABLE
    } else if db_schema == Some(0) {
        // 空库：文件存在但没有任何 schema（含建库中断），需要初始化。
        // 与「缺库」区分，也不冒充 schema 低于目标。
        readiness::UNINITIALIZED
    } else if current_schema > target_schema {
        readiness::INCOMPATIBLE
    } else if current_schema < target_schema {
        readiness::MIGRATION_REQUIRED
    } else if journal
        .as_ref()
        .is_some_and(|j| j.state == "failed" || j.state == "restored")
    {
        // 迁移失败/已恢复的终态：schema 恰为目标值时上面已放行，这里兜底
        // 提示用户打开投资详情核对，不让卡片在异常终态上读数。
        readiness::RECOVERY_REQUIRED
    } else {
        // committed journal 是迁移成功后的正常残留，不阻止卡片。
        readiness::READY
    };

    let migration_state = journal
        .as_ref()
        .map(|j| j.state.clone())
        .unwrap_or_else(|| "committed".to_string());

    let has_committed_new_writes = db_has_writes
        || journal
            .as_ref()
            .map(|j| j.has_committed_new_writes)
            .unwrap_or(false);

    let previous_version_compatible = journal
        .as_ref()
        .map(|j| j.previous_version_compatible)
        .unwrap_or(true);

    let last_data_writer_version = db_writer_version
        .or_else(|| journal.as_ref().map(|j| j.to_app_version.clone()))
        .unwrap_or_else(|| default_app_version.to_string());

    DataStatus {
        current_schema,
        migration_state,
        last_data_writer_version,
        has_committed_new_writes,
        previous_version_compatible,
        readiness: readiness.to_string(),
    }
}

#[cfg(test)]
#[path = "migration_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "migration_v6_tests.rs"]
mod v6_tests;
