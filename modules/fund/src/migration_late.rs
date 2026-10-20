//! Fund 迁移阶梯 v8～v19（自动资讯、共享账本、线下资产、提醒、溯源、退出历史、
//! 报告软删除、策略版本、账户元数据）。从 migration.rs 拆出（架构规模检查
//! >= 1000 硬限制）；SQL 为已定案历史，不因拆分变化。

use crate::migration::Migration;

/// v8：自动资讯（PanWatch M01～M05 迁移，方案 03 §4/§6、04 §3）。
/// - 自动条目独立表：info_sources.kind CHECK 无法扩展且禁止重建用户主表；
///   个人状态（read/favorite/archived）在本表上自成权威，API 层与手工资料共用 flags/archive 语义。
/// - 正文最多一份 authority（content_text），列表摘要单独列；不与 info_sources 双写。
/// - 关联按稳定证券身份，candidate 不自动晋升 direct；来源状态为固定白名单，不存凭据。
pub const SCHEMA_V8_SQL: &str = r#"
CREATE TABLE information_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_id TEXT NOT NULL,
    external_id TEXT NOT NULL,
    title TEXT NOT NULL,
    url TEXT NOT NULL DEFAULT '',
    summary_text TEXT NOT NULL DEFAULT '',
    content_text TEXT,
    published_at TEXT,
    time_precision TEXT NOT NULL DEFAULT 'unknown'
        CHECK (time_precision IN ('second','minute','day','unknown')),
    retrieved_at TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    content_version INTEGER NOT NULL DEFAULT 1,
    completeness TEXT NOT NULL DEFAULT 'link_only'
        CHECK (completeness IN ('link_only','excerpt','full','truncated','unavailable')),
    event_type TEXT NOT NULL DEFAULT '',
    event_date TEXT,
    importance INTEGER NOT NULL DEFAULT 0,
    read_status INTEGER NOT NULL DEFAULT 0 CHECK (read_status IN (0,1)),
    favorite INTEGER NOT NULL DEFAULT 0 CHECK (favorite IN (0,1)),
    archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0,1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(source_id, external_id)
);
CREATE INDEX idx_information_items_published ON information_items(published_at DESC, id DESC);
CREATE INDEX idx_information_items_event ON information_items(event_type) WHERE event_type != '';

CREATE TABLE information_relations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    item_id INTEGER NOT NULL REFERENCES information_items(id),
    instrument_ref TEXT NOT NULL,
    relation TEXT NOT NULL CHECK (relation IN ('direct','indirect','candidate')),
    reason TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    UNIQUE(item_id, instrument_ref)
);
CREATE INDEX idx_information_relations_ref ON information_relations(instrument_ref);

CREATE TABLE information_source_state (
    source_id TEXT PRIMARY KEY,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0,1)),
    last_attempt_at TEXT,
    last_success_at TEXT,
    last_error_code TEXT,
    last_error_at TEXT,
    last_count INTEGER NOT NULL DEFAULT 0,
    last_latency_ms INTEGER NOT NULL DEFAULT 0
);
"#;

/// v9：股票/基金共享账本扩展（方案 03 §3/§4、01 P04）。
/// - funds 增量列承载证券身份：instrument_kind（存量默认 fund 语义不变）、market、
///   display_code（原始展示代码）。code 列保持 UNIQUE：股票/ETF 写入内部键
///   `stock:CN:<code>`（避免与基金同码冲突），行情与 UI 使用 display_code。
/// - transactions 软删除：删除误录不物理清空（审计与回放排除），可恢复。
/// - 期初快照继承同一身份（按 fund_id 关联，无需新列）。
pub const SCHEMA_V9_SQL: &str = r#"
ALTER TABLE funds ADD COLUMN instrument_kind TEXT NOT NULL DEFAULT 'fund'
    CHECK (instrument_kind IN ('fund','stock','etf'));
ALTER TABLE funds ADD COLUMN market TEXT NOT NULL DEFAULT 'CN';
ALTER TABLE funds ADD COLUMN display_code TEXT NOT NULL DEFAULT '';
CREATE UNIQUE INDEX IF NOT EXISTS idx_funds_instrument
    ON funds(instrument_kind, market, display_code) WHERE display_code != '';
ALTER TABLE transactions ADD COLUMN deleted_at TEXT;
ALTER TABLE transactions ADD COLUMN deleted_reason TEXT NOT NULL DEFAULT '';
"#;

/// v10：线下投资资产（方案 01 P05、03 §4；O01～O05）。
/// - 四类 productType（实物黄金/定期存款/银行理财/其他手工估值）；排除房产等固定资产。
/// - 估值历史独立窄表；更新估值不记收益、不动现金。
/// - 退出事实在行内（exit_*）：日期/回收/费用/到账；已到账经 cash correction 事件入账，
///   未到账仅形成应收显示；删除误录=软删除。
/// - 旧 manual gold 继续以原 ID 在 assets 路径呈现（不迁移数据，无双计价）。
pub const SCHEMA_V10_SQL: &str = r#"
CREATE TABLE offline_assets (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    product_type TEXT NOT NULL CHECK (product_type IN ('gold','deposit','wealth','other')),
    name TEXT NOT NULL,
    location TEXT NOT NULL DEFAULT '',
    value_minor INTEGER NOT NULL CHECK (value_minor >= 0),
    valued_at TEXT NOT NULL,
    cost_minor INTEGER,
    risk_class TEXT NOT NULL DEFAULT 'unknown'
        CHECK (risk_class IN ('equity','gold','fixed_income','cash','other','unknown')),
    note TEXT NOT NULL DEFAULT '',
    attrs_json TEXT NOT NULL DEFAULT '{}',
    exit_date TEXT,
    exit_amount_minor INTEGER CHECK (exit_amount_minor IS NULL OR exit_amount_minor >= 0),
    exit_fee_minor INTEGER CHECK (exit_fee_minor IS NULL OR exit_fee_minor >= 0),
    exit_settled INTEGER NOT NULL DEFAULT 0 CHECK (exit_settled IN (0,1)),
    cash_event_ref INTEGER,
    deleted_at TEXT,
    deleted_reason TEXT NOT NULL DEFAULT '',
    version INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX idx_offline_assets_type ON offline_assets(product_type) WHERE deleted_at IS NULL;

CREATE TABLE offline_value_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    asset_id INTEGER NOT NULL REFERENCES offline_assets(id),
    value_minor INTEGER NOT NULL,
    valued_at TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_offline_history ON offline_value_history(asset_id, valued_at);
"#;

/// v11：资讯复核提醒（方案 05 §8；R02）。watch_rules.rule_type CHECK 无法扩展且
/// 禁止重建用户主表——按方案 03 §4 以独立窄表承载，事件状态机与规则事件同语义
/// （open/acknowledged/snoozed/resolved），API 层以稳定引用关联，不冒充 /api/events。
/// 幂等：assetRef+sourceRef 唯一（未解除期间重复采集/重开不重复提醒）；
/// 内容更正（contentVersion 升级）在实质变化时产生新提醒。
pub const SCHEMA_V11_SQL: &str = r#"
CREATE TABLE information_alerts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    asset_ref TEXT NOT NULL,
    source_ref TEXT NOT NULL,
    content_version INTEGER NOT NULL,
    title TEXT NOT NULL,
    event_type TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'open'
        CHECK (status IN ('open','acknowledged','snoozed','resolved')),
    snoozed_until TEXT,
    resolved_at TEXT,
    note TEXT NOT NULL DEFAULT '',
    first_triggered_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(asset_ref, source_ref)
);
CREATE INDEX idx_information_alerts_status ON information_alerts(status, updated_at);
"#;

/// v12：更正交易溯源（审计 P0-3）。correction_of 指向被更正的原交易；
/// source='correction' 行必须携带，幂等重试按 (request_id, correction_of, 数量/价格/费用/日期) 判等。
pub const SCHEMA_V12_SQL: &str = r#"
ALTER TABLE transactions ADD COLUMN correction_of INTEGER REFERENCES transactions(id);
"#;

/// v13：线下退出历史（审计 P0-2/四-4）。支持多次部分退出（分批赎回）与
/// 幂等重试：idempotency_key 唯一；行内 exit_* 保留最近一次供投影。
pub const SCHEMA_V13_SQL: &str = r#"
CREATE TABLE offline_exit_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    asset_id INTEGER NOT NULL REFERENCES offline_assets(id),
    exit_date TEXT NOT NULL,
    amount_minor INTEGER NOT NULL CHECK (amount_minor >= 0),
    fee_minor INTEGER NOT NULL DEFAULT 0 CHECK (fee_minor >= 0),
    settled INTEGER NOT NULL DEFAULT 0 CHECK (settled IN (0,1)),
    cash_event_ref INTEGER,
    idempotency_key TEXT UNIQUE,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_offline_exits_asset ON offline_exit_history(asset_id);
"#;

/// v14：分析报告软删除（方案 05 §7；审计五-4）。删除=带时点标记，已删除可查、可恢复；
/// 恢复不重新推理、不改证据。
pub const SCHEMA_V14_SQL: &str = r#"
ALTER TABLE analysis_reports ADD COLUMN deleted_at TEXT;
CREATE INDEX idx_analysis_reports_deleted ON analysis_reports(deleted_at);
"#;

/// v15：用户策略版本号（方案 05 §2）。编辑递增 version；归档/恢复不推进版本
/// 但变更 updated_at；已有报告快照保留其引用时的策略内容。
pub const SCHEMA_V15_SQL: &str = r#"
ALTER TABLE investment_theses ADD COLUMN version INTEGER NOT NULL DEFAULT 1;
"#;

/// v16：保存退出原始载荷，独立承载新增资产引用，不重建旧 CHECK 表。
pub const SCHEMA_V16_SQL: &str = r#"
ALTER TABLE transactions ADD COLUMN version INTEGER NOT NULL DEFAULT 1;
ALTER TABLE transactions ADD COLUMN correction_confirmation_json TEXT NOT NULL DEFAULT '';
ALTER TABLE opening_snapshots ADD COLUMN version INTEGER NOT NULL DEFAULT 1;
ALTER TABLE opening_snapshots ADD COLUMN deleted_at TEXT;
CREATE TABLE opening_history (id INTEGER PRIMARY KEY AUTOINCREMENT,account_id INTEGER NOT NULL,fund_id INTEGER NOT NULL,version INTEGER NOT NULL,payload_json TEXT NOT NULL,created_at TEXT NOT NULL);
ALTER TABLE offline_exit_history ADD COLUMN settled_at TEXT;
UPDATE offline_exit_history SET settled_at=(SELECT occurred_at FROM cash_events WHERE id=cash_event_ref) WHERE settled=1;
ALTER TABLE offline_exit_history ADD COLUMN request_json TEXT;
ALTER TABLE offline_exit_history ADD COLUMN remaining_minor INTEGER;
ALTER TABLE offline_exit_history ADD COLUMN settlement_account_id INTEGER REFERENCES accounts(id);
ALTER TABLE offline_exit_history ADD COLUMN version INTEGER NOT NULL DEFAULT 1;
CREATE TABLE group_plan_extra_bindings (
    group_id TEXT NOT NULL REFERENCES group_plan_groups(group_id),
    source_kind TEXT NOT NULL CHECK (source_kind IN ('offline','cash')),
    source_id INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY(source_kind,source_id)
);
ALTER TABLE manual_assets ADD COLUMN deleted_at TEXT;
ALTER TABLE manual_assets ADD COLUMN version INTEGER NOT NULL DEFAULT 1;
ALTER TABLE manual_assets ADD COLUMN converted_ref TEXT;
CREATE TABLE legacy_asset_history(id INTEGER PRIMARY KEY AUTOINCREMENT,asset_id INTEGER NOT NULL REFERENCES manual_assets(id),payload_json TEXT NOT NULL,created_at TEXT NOT NULL);
ALTER TABLE analysis_reports ADD COLUMN version INTEGER NOT NULL DEFAULT 1;
ALTER TABLE analysis_reports ADD COLUMN validation_json TEXT NOT NULL DEFAULT '{}';
"#;

pub const SCHEMA_V17_SQL: &str = "ALTER TABLE information_alerts ADD COLUMN notify_result TEXT NOT NULL DEFAULT 'pending' CHECK (notify_result IN ('pending','claimed','delivered','failed'));";

pub const SCHEMA_V18_SQL: &str =
    "ALTER TABLE information_items ADD COLUMN content_fetched_at TEXT;";

/// v19：结构化账户元数据（方案 03「账户结构化查询」；R2 T03）。
/// - version：账户命令乐观锁（改名／排序／归档／恢复必须携带 expectedVersion）；
///   历史自动建档（ensure_account 按名 upsert）不推进版本，避免旧导入链 409。
/// - sort_order：用户排序；全 0 时 (sort_order,id) 与旧 ORDER BY id 等价。
/// - archived_at：归档标记；只隐藏于交易选择与旧名称数组，历史关联按 ID 保留，不物理删。
/// - account_ops：账户命令 requestId 幂等表（同 ID 同载荷返回原结果，异载荷 409）。
pub const SCHEMA_V19_SQL: &str = r#"
ALTER TABLE accounts ADD COLUMN version INTEGER NOT NULL DEFAULT 1;
ALTER TABLE accounts ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0;
ALTER TABLE accounts ADD COLUMN archived_at TEXT;
CREATE INDEX IF NOT EXISTS idx_accounts_order ON accounts(sort_order, id);
CREATE TABLE account_ops (
    request_id TEXT PRIMARY KEY,
    account_id INTEGER NOT NULL,
    op TEXT NOT NULL CHECK (op IN ('update','archive','restore')),
    payload_json TEXT NOT NULL,
    result_json TEXT NOT NULL,
    created_at TEXT NOT NULL
);
"#;

/// v20：不可变机会批次与版本化反馈；不改变财务账本。
pub const SCHEMA_V20_SQL: &str = r#"
CREATE TABLE opportunity_snapshots (
    snapshot_id TEXT PRIMARY KEY,
    facts_key TEXT NOT NULL UNIQUE,
    data_revision TEXT NOT NULL,
    generated_at TEXT NOT NULL,
    projection_json TEXT NOT NULL
);
CREATE TABLE opportunity_feedback (
    snapshot_id TEXT NOT NULL REFERENCES opportunity_snapshots(snapshot_id),
    candidate_id TEXT NOT NULL,
    version INTEGER NOT NULL CHECK(version > 0),
    useful INTEGER NOT NULL CHECK(useful IN (0,1)),
    reason TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY(snapshot_id,candidate_id,version)
);
CREATE TABLE opportunity_ops (
    request_id TEXT PRIMARY KEY,
    payload_json TEXT NOT NULL,
    result_json TEXT NOT NULL
);
"#;

pub(crate) const SCHEMA_V21_SQL: &str = r#"
CREATE TABLE opportunity_observations (
 snapshot_id TEXT NOT NULL REFERENCES opportunity_snapshots(snapshot_id),
 candidate_id TEXT NOT NULL,
 horizon INTEGER NOT NULL CHECK(horizon IN (3,5,10)),
 result_json TEXT NOT NULL,
 PRIMARY KEY(snapshot_id,candidate_id,horizon)
);
"#;

pub(crate) const SCHEMA_V22_SQL: &str = r#"
CREATE TABLE strategy_catalog (
 code TEXT PRIMARY KEY,
 name TEXT NOT NULL,
 description TEXT NOT NULL DEFAULT '',
 version TEXT NOT NULL DEFAULT 'v1',
 enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0,1)),
 market_scope TEXT NOT NULL DEFAULT 'ALL',
 risk_level TEXT NOT NULL DEFAULT 'medium',
 params_json TEXT NOT NULL DEFAULT '{}',
 default_weight_ppm INTEGER NOT NULL,
 updated_at TEXT NOT NULL DEFAULT ''
);
CREATE TABLE strategy_weight_overrides (
 code TEXT PRIMARY KEY REFERENCES strategy_catalog(code),
 weight_ppm INTEGER NOT NULL,
 updated_at TEXT NOT NULL
);
CREATE TABLE strategy_config (
 id INTEGER PRIMARY KEY CHECK(id=1),
 revision INTEGER NOT NULL DEFAULT 1,
 updated_at TEXT NOT NULL DEFAULT ''
);
INSERT INTO strategy_config(id,revision,updated_at) VALUES(1,1,'');
"#;

pub(crate) const SCHEMA_V23_SQL: &str = r#"
CREATE TABLE opportunity_discovery (
 symbol TEXT PRIMARY KEY,
 market TEXT NOT NULL,
 kind TEXT NOT NULL,
 name TEXT NOT NULL,
 source TEXT NOT NULL,
 change_pct_raw INTEGER NOT NULL DEFAULT 0,
 discovered_at TEXT NOT NULL
);
"#;

pub(crate) const SCHEMA_V24_SQL: &str = r#"
CREATE TABLE notify_channels (
 id INTEGER PRIMARY KEY AUTOINCREMENT,
 kind TEXT NOT NULL CHECK (kind IN ('telegram','bark','dingtalk','wecom','feishu','serverchan','pushplus','discord','pushover')),
 name TEXT NOT NULL,
 config_json TEXT NOT NULL DEFAULT '{}',
 secret_refs TEXT NOT NULL DEFAULT '[]',
 enabled INTEGER NOT NULL DEFAULT 1,
 is_default INTEGER NOT NULL DEFAULT 0,
 archived INTEGER NOT NULL DEFAULT 0,
 last_test_at TEXT,
 last_test_ok INTEGER,
 last_error TEXT NOT NULL DEFAULT '',
 version INTEGER NOT NULL DEFAULT 1,
 created_at TEXT NOT NULL,
 updated_at TEXT NOT NULL
);
CREATE UNIQUE INDEX idx_notify_channels_one_default ON notify_channels(is_default) WHERE is_default=1 AND archived=0;
CREATE TABLE notify_deliveries (
 id INTEGER PRIMARY KEY AUTOINCREMENT,
 channel_id INTEGER NOT NULL REFERENCES notify_channels(id),
 kind TEXT NOT NULL,
 subject_ref TEXT NOT NULL DEFAULT '',
 status TEXT NOT NULL CHECK (status IN ('sent','failed','skipped_silent','skipped_dedup','cancelled')),
 error_code TEXT NOT NULL DEFAULT '',
 detail TEXT NOT NULL DEFAULT '',
 created_at TEXT NOT NULL
);
CREATE INDEX idx_notify_deliveries_channel ON notify_deliveries(channel_id, created_at DESC);
"#;

pub const SCHEMA_V25_SQL: &str = r#"
CREATE TABLE analysis_report_feedback (
 report_id INTEGER NOT NULL REFERENCES analysis_reports(id),
 version INTEGER NOT NULL CHECK(version > 0),
 useful INTEGER NOT NULL CHECK(useful IN (0,1)),
 reason TEXT NOT NULL DEFAULT '',
 created_at TEXT NOT NULL,
 PRIMARY KEY(report_id,version)
);
"#;

pub const LATE_MIGRATIONS: &[Migration] = &[
    Migration {
        version: 8,
        name: "information_items_v8",
        sql: SCHEMA_V8_SQL,
    },
    Migration {
        version: 9,
        name: "shared_instrument_ledger_v9",
        sql: SCHEMA_V9_SQL,
    },
    Migration {
        version: 10,
        name: "offline_assets_v10",
        sql: SCHEMA_V10_SQL,
    },
    Migration {
        version: 11,
        name: "information_alerts_v11",
        sql: SCHEMA_V11_SQL,
    },
    Migration {
        version: 12,
        name: "correction_provenance_v12",
        sql: SCHEMA_V12_SQL,
    },
    Migration {
        version: 13,
        name: "offline_exit_history_v13",
        sql: SCHEMA_V13_SQL,
    },
    Migration {
        version: 14,
        name: "analysis_reports_soft_delete_v14",
        sql: SCHEMA_V14_SQL,
    },
    Migration {
        version: 15,
        name: "thesis_version_v15",
        sql: SCHEMA_V15_SQL,
    },
    Migration {
        version: 16,
        name: "investment_integrity_v16",
        sql: SCHEMA_V16_SQL,
    },
    Migration {
        version: 17,
        name: "information_notification_v17",
        sql: SCHEMA_V17_SQL,
    },
    Migration {
        version: 18,
        name: "information_content_cache_v18",
        sql: SCHEMA_V18_SQL,
    },
    Migration {
        version: 19,
        name: "account_metadata_v19",
        sql: SCHEMA_V19_SQL,
    },
    Migration {
        version: 20,
        name: "opportunity_feedback_v20",
        sql: SCHEMA_V20_SQL,
    },
    Migration {
        version: 21,
        name: "opportunity_observations_v21",
        sql: SCHEMA_V21_SQL,
    },
    Migration {
        version: 22,
        name: "strategy_catalog_v22",
        sql: SCHEMA_V22_SQL,
    },
    Migration {
        version: 23,
        name: "opportunity_discovery_v23",
        sql: SCHEMA_V23_SQL,
    },
    Migration {
        version: 24,
        name: "notify_channels_v24",
        sql: SCHEMA_V24_SQL,
    },
    Migration {
        version: 25,
        name: "analysis_report_feedback_v25",
        sql: SCHEMA_V25_SQL,
    },
    Migration {
        version: 26,
        name: "otc_fund_ops_v26",
        sql: SCHEMA_V26_SQL,
    },
    Migration {
        version: 27,
        name: "group_plan_description_v27",
        sql: SCHEMA_V27_SQL,
    },
    Migration {
        version: 28,
        name: "information_decisions_v28",
        sql: SCHEMA_V28_SQL,
    },
];

/// v26：场外基金操作（N09）：订单（加仓/减仓/转换两腿）、定投计划与期次、
/// 持仓校准锚点。持仓权威仍是 portfolio 回放；本批表只承载订单状态机、
/// 计划与审计锚点，不承载第二持仓投影。
pub const SCHEMA_V26_SQL: &str = r#"
CREATE TABLE fund_orders (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    op TEXT NOT NULL CHECK (op IN ('buy','sell','convert_out','convert_in')),
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending','confirmed','cancelled','failed')),
    account_id INTEGER NOT NULL REFERENCES accounts(id),
    fund_id INTEGER NOT NULL REFERENCES funds(id),
    conversion_id INTEGER,
    plan_id INTEGER,
    period_key TEXT NOT NULL DEFAULT '',
    applied_date TEXT NOT NULL,
    cutoff TEXT NOT NULL DEFAULT 'before_1500'
        CHECK (cutoff IN ('before_1500','after_1500')),
    amount_minor INTEGER,
    fee_minor INTEGER,
    fee_known INTEGER NOT NULL DEFAULT 0 CHECK (fee_known IN (0,1)),
    shares_raw INTEGER,
    nav_date TEXT,
    nav_raw INTEGER,
    expected_nav_date TEXT NOT NULL DEFAULT '',
    confirmed_shares_raw INTEGER,
    confirmed_amount_minor INTEGER,
    confirmed_fee_minor INTEGER,
    confirmed_nav_date TEXT,
    confirmed_nav_raw INTEGER,
    confirmed_at TEXT,
    tx_id INTEGER,
    cash_arrival TEXT,
    reservation_id INTEGER,
    request_id TEXT NOT NULL UNIQUE,
    version INTEGER NOT NULL DEFAULT 1,
    note TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX idx_fund_orders_scope ON fund_orders(account_id, fund_id, status);
CREATE INDEX idx_fund_orders_conversion ON fund_orders(conversion_id) WHERE conversion_id IS NOT NULL;
CREATE INDEX idx_fund_orders_plan ON fund_orders(plan_id) WHERE plan_id IS NOT NULL;

CREATE TABLE fund_plans (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id INTEGER NOT NULL REFERENCES accounts(id),
    fund_id INTEGER NOT NULL REFERENCES funds(id),
    amount_minor INTEGER NOT NULL CHECK (amount_minor > 0),
    freq TEXT NOT NULL CHECK (freq IN ('daily','weekly','monthly')),
    weekday INTEGER CHECK (weekday IS NULL OR (weekday BETWEEN 1 AND 7)),
    monthday INTEGER CHECK (monthday IS NULL OR (monthday BETWEEN 1 AND 31)),
    start_date TEXT NOT NULL,
    end_date TEXT,
    non_trading TEXT NOT NULL DEFAULT 'next'
        CHECK (non_trading IN ('next','prev','skip')),
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','paused','ended')),
    version INTEGER NOT NULL DEFAULT 1,
    note TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX idx_fund_plans_scope ON fund_plans(account_id, fund_id, status);

CREATE TABLE fund_plan_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    plan_id INTEGER NOT NULL REFERENCES fund_plans(id),
    period_key TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('registered','skipped')),
    order_id INTEGER,
    created_at TEXT NOT NULL,
    UNIQUE(plan_id, period_key)
);

CREATE TABLE position_calibrations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id INTEGER NOT NULL REFERENCES accounts(id),
    fund_id INTEGER NOT NULL REFERENCES funds(id),
    as_of_date TEXT NOT NULL,
    quantity_raw INTEGER NOT NULL CHECK (quantity_raw >= 0),
    cost_minor INTEGER NOT NULL CHECK (cost_minor >= 0),
    nav_raw INTEGER,
    nav_date TEXT,
    value_minor INTEGER,
    gain_minor INTEGER,
    reason TEXT NOT NULL,
    delta_quantity_raw INTEGER,
    delta_cost_minor INTEGER,
    request_id TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_position_calibrations_scope ON position_calibrations(account_id, fund_id, as_of_date);
"#;

/// v27：组合计划自定义分类说明（P03）。
pub const SCHEMA_V27_SQL: &str = r#"
ALTER TABLE group_plan_groups ADD COLUMN description TEXT NOT NULL DEFAULT '';
"#;

/// v28: User decisions reference real information and immutable saved reports.
pub const SCHEMA_V28_SQL: &str = r#"
CREATE TABLE information_decisions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    information_id INTEGER NOT NULL REFERENCES information_items(id),
    report_id INTEGER NOT NULL REFERENCES analysis_reports(id),
    action TEXT NOT NULL CHECK (action IN ('observe','defer','done')),
    note TEXT NOT NULL,
    request_id TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_information_decisions_item ON information_decisions(information_id,created_at);
"#;
