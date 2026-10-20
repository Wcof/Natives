//! rules 模块：统一规则、事件与行动候选（方案 03 契约，G4）。
//!
//! - 规则 CRUD：阈值必须由用户保存确认后启用；编辑产生新版本。
//! - 事件去重状态机：同规则+同对象维护一个未解除事件；解除后再进入才新建。
//! - 行动候选：数据质量 → 资金 → 集中度 → 配置 → 策略依次检查；
//!   六种结果：建仓/补仓/减仓/持有/等待/补齐数据。金额是可复算上界，
//!   不自动交易、不冒充概率。

use crate::finance::{self, FinanceError};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

pub const RULE_TYPES: &[&str] = &[
    "cash_short",
    "class_unknown",
    "stale_valuation",
    "allocation_drift",
    "concentration",
    "price_threshold",
    "daily_change",
    "ma_cross",
    "drawdown",
    "strategy_exit",
    "investment_lag",
];

#[derive(Debug, PartialEq, Eq)]
pub enum RuleError {
    NotFound(String),
    Invalid(String),
    Conflict(String),
    Db(String),
}

impl From<rusqlite::Error> for RuleError {
    fn from(e: rusqlite::Error) -> Self {
        RuleError::Db(e.to_string())
    }
}

impl From<FinanceError> for RuleError {
    fn from(e: FinanceError) -> Self {
        match e {
            FinanceError::Db(m) => RuleError::Db(m),
            FinanceError::Conflict(m) => RuleError::Conflict(m),
            other => RuleError::Invalid(other.message()),
        }
    }
}

impl RuleError {
    pub fn code(&self) -> &'static str {
        match self {
            RuleError::NotFound(_) => "RULE_NOT_FOUND",
            RuleError::Invalid(_) => "RULE_INVALID",
            RuleError::Conflict(_) => "RULE_CONFLICT",
            RuleError::Db(_) => "RULE_DB",
        }
    }
    pub fn message(&self) -> String {
        match self {
            RuleError::NotFound(m) => format!("记录不存在：{m}"),
            RuleError::Invalid(m) => format!("输入无效：{m}"),
            RuleError::Conflict(m) => format!("冲突：{m}"),
            RuleError::Db(e) => format!("数据库错误：{e}"),
        }
    }
}

pub(crate) fn now_secs() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// 规则 CRUD
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleRow {
    pub id: i64,
    pub name: String,
    pub rule_type: String,
    pub object_key: String,
    pub params_json: String,
    pub period: String,
    pub enabled: bool,
    pub version: i64,
    pub archived: bool,
}

fn row_to_rule(r: &rusqlite::Row) -> rusqlite::Result<RuleRow> {
    Ok(RuleRow {
        id: r.get(0)?,
        name: r.get(1)?,
        rule_type: r.get(2)?,
        object_key: r.get(3)?,
        params_json: r.get(4)?,
        period: r.get(5)?,
        enabled: r.get::<_, i64>(6)? == 1,
        version: r.get(7)?,
        archived: r.get::<_, i64>(8)? == 1,
    })
}

const RULE_COLS: &str =
    "id, name, rule_type, object_key, params_json, period, enabled, version, archived";

pub fn list_rules(conn: &Connection, include_archived: bool) -> Result<Vec<RuleRow>, RuleError> {
    let sql = if include_archived {
        format!("SELECT {RULE_COLS} FROM watch_rules ORDER BY archived, id")
    } else {
        format!("SELECT {RULE_COLS} FROM watch_rules WHERE archived=0 ORDER BY id")
    };
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([], row_to_rule)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub struct RuleInput {
    pub name: String,
    pub rule_type: String,
    pub object_key: String,
    pub params: Value,
    pub period: String,
    /// enabled=true 必须携带已确认阈值（03 §3：投资阈值由用户保存确认后启用）。
    pub enabled: bool,
}

/// 规则参数校验（有限规则类型 + 有效阈值）。
fn validate_params(rule_type: &str, p: &Value) -> Result<(), RuleError> {
    if !RULE_TYPES.contains(&rule_type) || !p.is_object() {
        return Err(RuleError::Invalid("规则类型或参数无效".into()));
    }
    if matches!(
        rule_type,
        "concentration" | "price_threshold" | "daily_change" | "drawdown"
    ) {
        let v = p["threshold"]
            .as_f64()
            .filter(|v| v.is_finite())
            .ok_or_else(|| RuleError::Invalid("必须设置有限阈值".into()))?;
        if matches!(rule_type, "concentration" | "drawdown") {
            crate::rules_eval::percent_tenths(v).map_err(RuleError::Invalid)?;
            if v <= 0.0 {
                return Err(RuleError::Invalid("上限必须大于零".into()));
            }
        }
        if rule_type == "price_threshold" && v <= 0.0 {
            return Err(RuleError::Invalid("价格必须大于零".into()));
        }
    }
    if p.get("direction").is_some()
        && !matches!(
            p["direction"].as_str(),
            Some("above" | "below" | "absolute" | "up" | "down")
        )
    {
        return Err(RuleError::Invalid("方向无效".into()));
    }
    if rule_type == "drawdown"
        && p.get("window").is_some()
        && !p["window"].as_u64().is_some_and(|n| (2..=800).contains(&n))
    {
        return Err(RuleError::Invalid("回撤窗口须为 2～800".into()));
    }
    if rule_type == "investment_lag"
        && (!p["monthlyPlan"]
            .as_f64()
            .is_some_and(|v| v.is_finite() && v > 0.0)
            || !p["dueDate"]
                .as_str()
                .is_some_and(crate::assets::is_valid_calendar_date))
    {
        return Err(RuleError::Invalid("投入规则需要正金额及核对日期".into()));
    }
    if rule_type == "strategy_exit" {
        let valid = match p["condition"].as_str() {
            Some("price") => p["threshold"]
                .as_f64()
                .is_some_and(|v| v.is_finite() && v > 0.0),
            Some("expiry") => p["expiryDate"]
                .as_str()
                .is_some_and(crate::assets::is_valid_calendar_date),
            Some("thesis") => p["invalidated"].is_boolean(),
            _ => false,
        };
        if !valid {
            return Err(RuleError::Invalid(
                "退出规则需要用户确认的价格、期限或持有理由失效条件".into(),
            ));
        }
    }
    Ok(())
}

pub(crate) fn validate_rule(input: &RuleInput) -> Result<(), RuleError> {
    if input.name.trim().is_empty()
        || input.name.chars().count() > 64
        || input.object_key.len() > 128
        || !matches!(input.period.as_str(), "quote" | "day" | "month")
        || input.params.to_string().len() > 4096
    {
        return Err(RuleError::Invalid(
            "规则名称、对象、周期或参数长度无效".into(),
        ));
    }
    validate_params(&input.rule_type, &input.params)
}

pub fn create_rule(conn: &Connection, input: &RuleInput) -> Result<i64, RuleError> {
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > 64 {
        return Err(RuleError::Invalid("规则名称需 1~64 字符".into()));
    }
    validate_rule(input)?;
    let now = now_secs();
    conn.execute(
        "INSERT INTO watch_rules(name, rule_type, object_key, params_json, period, enabled,
                                 version, archived, created_at, updated_at)
         VALUES(?1,?2,?3,?4,?5,?6,1,0,?7,?7)",
        params![
            name,
            input.rule_type,
            input.object_key,
            input.params.to_string(),
            input.period,
            input.enabled as i64,
            now
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 编辑：产生新版本（version+1）；仅因编辑仍成立的规则不重复通知（03 §5.2）。
pub fn update_rule(conn: &Connection, id: i64, input: &RuleInput) -> Result<i64, RuleError> {
    let existing: Option<RuleRow> = conn
        .query_row(
            &format!("SELECT {RULE_COLS} FROM watch_rules WHERE id=?1"),
            params![id],
            row_to_rule,
        )
        .optional()?;
    let Some(existing) = existing else {
        return Err(RuleError::NotFound(format!("规则 {id}")));
    };
    validate_rule(input)?;
    let now = now_secs();
    conn.execute(
        "UPDATE watch_rules SET name=?2, rule_type=?3, object_key=?4, params_json=?5,
         period=?6, enabled=?7, version=version+1, updated_at=?8 WHERE id=?1",
        params![
            id,
            input.name.trim(),
            input.rule_type,
            input.object_key,
            input.params.to_string(),
            input.period,
            input.enabled as i64,
            now
        ],
    )?;
    let new_version = existing.version + 1;
    // 编辑后条件仍成立：更新未解除事件的 rule_version，标注规则更新（不重复通知）。
    conn.execute(
        "UPDATE watch_events SET rule_version=?2, updated_at=?3
         WHERE rule_id=?1 AND status != 'resolved'",
        params![id, new_version, now],
    )?;
    Ok(new_version)
}

pub fn set_rule_enabled(conn: &Connection, id: i64, enabled: bool) -> Result<(), RuleError> {
    let affected = conn.execute(
        "UPDATE watch_rules SET enabled=?2, updated_at=?3 WHERE id=?1 AND archived=0",
        params![id, enabled as i64, now_secs()],
    )?;
    if affected == 0 {
        return Err(RuleError::NotFound(format!("活跃规则 {id}")));
    }
    Ok(())
}

pub fn archive_rule(conn: &Connection, id: i64) -> Result<(), RuleError> {
    let affected = conn.execute(
        "UPDATE watch_rules SET archived=1, enabled=0, updated_at=?2 WHERE id=?1 AND archived=0",
        params![id, now_secs()],
    )?;
    if affected == 0 {
        return Err(RuleError::NotFound(format!("活跃规则 {id}")));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 事件状态机（03 §5）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventRow {
    pub id: i64,
    pub rule_id: i64,
    pub rule_version: i64,
    pub rule_type: String,
    pub object_key: String,
    pub first_triggered_at: String,
    pub last_checked_at: String,
    pub trigger_value: String,
    pub threshold_value: String,
    pub status: String,
    pub ready_state: String,
    pub snoozed_until: Option<String>,
}

fn row_to_event(r: &rusqlite::Row) -> rusqlite::Result<EventRow> {
    Ok(EventRow {
        id: r.get(0)?,
        rule_id: r.get(1)?,
        rule_version: r.get(2)?,
        rule_type: r.get(3)?,
        object_key: r.get(4)?,
        first_triggered_at: r.get(5)?,
        last_checked_at: r.get(6)?,
        trigger_value: r.get(7)?,
        threshold_value: r.get(8)?,
        status: r.get(9)?,
        ready_state: r.get(10)?,
        snoozed_until: r.get(11)?,
    })
}

const EVENT_COLS: &str = "id, rule_id, rule_version, rule_type, object_key, first_triggered_at,
                          last_checked_at, trigger_value, threshold_value, status, ready_state,
                          snoozed_until";

pub fn list_events(
    conn: &Connection,
    status_filter: Option<&str>,
) -> Result<Vec<EventRow>, RuleError> {
    let (sql, bind): (String, Vec<String>) = match status_filter {
        Some(s) => (
            format!("SELECT {EVENT_COLS} FROM watch_events WHERE status=?1 ORDER BY last_checked_at DESC, id DESC LIMIT 200"),
            vec![s.to_string()],
        ),
        None => (
            format!("SELECT {EVENT_COLS} FROM watch_events WHERE status != 'resolved' ORDER BY last_checked_at DESC, id DESC LIMIT 200"),
            vec![],
        ),
    };
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(bind.iter()), row_to_event)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 事件登记/去重核心：同 (rule_id, object_key) 的未解除事件只更新不新建；
/// 已解除（resolved）后再满足条件才创建新事件（03 §5.2）。
/// 返回 (event_id, created)。
pub fn record_trigger(
    conn: &Connection,
    rule: &RuleRow,
    object_key: &str,
    trigger_value: &str,
    threshold_value: &str,
    ready_state: &str,
) -> Result<(i64, bool), RuleError> {
    let now = now_secs();
    let existing: Option<(i64, String)> = conn
        .query_row(
            "SELECT id, status FROM watch_events
             WHERE rule_id=?1 AND object_key=?2 AND status != 'resolved'",
            params![rule.id, object_key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((id, status)) = existing {
        // 同一未解除事件：更新检查时间与峰值；snoozed 到期后恢复 open（不新建）。
        let new_status = if status == "snoozed" {
            let snoozed_until: Option<String> = conn
                .query_row(
                    "SELECT snoozed_until FROM watch_events WHERE id=?1",
                    params![id],
                    |r| r.get(0),
                )
                .optional()?
                .flatten();
            match snoozed_until {
                Some(until) if until.as_str() > today_cn().as_str() => "snoozed".to_string(),
                _ => "open".to_string(),
            }
        } else {
            status
        };
        conn.execute(
            "UPDATE watch_events SET last_checked_at=?2, trigger_value=?3, status=?4,
             ready_state=?5, updated_at=?2 WHERE id=?1",
            params![id, now, trigger_value, new_status, ready_state],
        )?;
        Ok((id, false))
    } else {
        conn.execute(
            "INSERT INTO watch_events(rule_id, rule_version, rule_type, object_key,
                                      first_triggered_at, last_checked_at, trigger_value,
                                      threshold_value, status, ready_state, created_at, updated_at)
             VALUES(?1,?2,?3,?4,?5,?5,?6,?7,'open',?8,?5,?5)",
            params![
                rule.id,
                rule.version,
                rule.rule_type,
                object_key,
                now,
                trigger_value,
                threshold_value,
                ready_state
            ],
        )?;
        Ok((conn.last_insert_rowid(), true))
    }
}

/// 条件解除：resolved + resolved_at；不自动把失败数据标成解除（ready_state 独立）。
pub fn resolve_event(conn: &Connection, event_id: i64, note: &str) -> Result<(), RuleError> {
    let now = now_secs();
    let affected = conn.execute(
        "UPDATE watch_events SET status='resolved', resolved_at=?2, note=?3, updated_at=?2
         WHERE id=?1 AND status != 'resolved'",
        params![event_id, now, note],
    )?;
    if affected == 0 {
        return Err(RuleError::NotFound(format!("未解除事件 {event_id}")));
    }
    Ok(())
}

pub fn acknowledge_event(conn: &Connection, event_id: i64) -> Result<(), RuleError> {
    let affected = conn.execute(
        "UPDATE watch_events SET status='acknowledged', updated_at=?2 WHERE id=?1 AND status='open'",
        params![event_id, now_secs()],
    )?;
    if affected == 0 {
        return Err(RuleError::Conflict("事件不存在或非 open 状态".into()));
    }
    Ok(())
}

pub fn snooze_event(conn: &Connection, event_id: i64, until_date: &str) -> Result<(), RuleError> {
    if !crate::assets::is_valid_calendar_date(until_date) {
        return Err(RuleError::Invalid("暂缓日期无效".into()));
    }
    let affected = conn.execute(
        "UPDATE watch_events SET status='snoozed', snoozed_until=?2, updated_at=?3
         WHERE id=?1 AND status IN ('open','acknowledged')",
        params![event_id, until_date, now_secs()],
    )?;
    if affected == 0 {
        return Err(RuleError::Conflict("事件不存在或状态不允许暂缓".into()));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 检查引擎：按方案 03 §2 统一流程生成事件与行动候选
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionResult {
    pub result_type: &'static str,
    pub object_key: String,
    pub object_name: String,
    pub reason: String,
    pub rule_type: String,
    pub event_id: Option<i64>,
    /// 金额上界（分位）；None = 不测算金额。
    pub amount_cap_minor: Option<i64>,
    pub missing: Vec<String>,
}

/// 单次检查入口（开页/手动刷新/规则变更后）。只查持仓与正式自选的必要序列。
/// 快照一致性：summary 与现金在写事务外单读取得（02 §7），规则评估基于该快照。
/// 返回 (事件数, 行动候选列表)。
pub use crate::rules_eval::{run_check, run_check_at};

pub(crate) fn computable_base(
    summary: &crate::assets::AssetsSummary,
    cash: &finance::CashPosition,
) -> Option<i64> {
    // B = A − R；A 用 complete（完整）值；R 未配置时 B 不可算（03 §3）。
    let a = summary.complete_value_minor?;
    let r = cash.reserved_minor?;
    Some(a - r)
}

pub(crate) fn normalize_symbol_key(key: &str) -> String {
    let key = key.trim_start_matches("symbol:");
    for (prefix, market) in [("sh", "SSE"), ("sz", "SZSE"), ("bj", "BSE")] {
        if key.len() == 8 && key.starts_with(prefix) && key[2..].bytes().all(|b| b.is_ascii_digit())
        {
            return format!("security:{market}:{}", &key[2..]);
        }
    }
    key.to_string()
}

fn today_cn() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        + 8 * 3600;
    crate::assets::civil_from_days((secs / 86400) as i64)
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod tests;
