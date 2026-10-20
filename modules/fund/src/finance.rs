//! finance 模块：投资首页的现金、资金事件、收益与多财务目标（方案 02 契约）。
//!
//! 口径（02 §1.1）：A=管理资产（资产聚合完整值，含现金）；R=预留现金；
//! B=A−R；C=到账现金（manual_assets cash 合计）；L=未成交占用；
//! F=max(0, C−R−L)，缺口显式报告不被 0 隐藏。
//! 金额全部 i64 分位 + add_minor 溢出检查；未知不用 0 表示。

use crate::assets::{add_minor, is_valid_calendar_date, AssetError};
use rusqlite::{params, Connection, OptionalExtension};

pub const EVENT_TYPES: &[&str] = &[
    "opening",
    "external_in",
    "external_out",
    "internal_transfer",
    "dividend_cash",
    "dividend_reinvest",
    "fee",
    "reconciliation",
    "correction",
];

/// 外部投入事件（净投入公式 02 §3.3）。
const EXTERNAL_IN: &str = "external_in";
const EXTERNAL_OUT: &str = "external_out";

#[derive(Debug, PartialEq, Eq)]
pub enum FinanceError {
    NotFound(String),
    Invalid(String),
    Conflict(String),
    Db(String),
    Overflow(String),
}

impl From<rusqlite::Error> for FinanceError {
    fn from(e: rusqlite::Error) -> Self {
        FinanceError::Db(e.to_string())
    }
}

impl From<AssetError> for FinanceError {
    fn from(e: AssetError) -> Self {
        match e {
            AssetError::Db(m) => FinanceError::Db(m),
            AssetError::Overflow(m) => FinanceError::Overflow(m),
            other => FinanceError::Invalid(other.message()),
        }
    }
}

impl FinanceError {
    pub fn code(&self) -> &'static str {
        match self {
            FinanceError::NotFound(_) => "FIN_NOT_FOUND",
            FinanceError::Invalid(_) => "FIN_INVALID",
            FinanceError::Conflict(_) => "FIN_CONFLICT",
            FinanceError::Db(_) => "FIN_DB",
            FinanceError::Overflow(_) => "ASSET_AMOUNT_OVERFLOW",
        }
    }

    pub fn message(&self) -> String {
        match self {
            FinanceError::NotFound(m) => format!("记录不存在：{m}"),
            FinanceError::Invalid(m) => format!("输入无效：{m}"),
            FinanceError::Conflict(m) => format!("冲突：{m}"),
            FinanceError::Db(e) => format!("数据库错误：{e}"),
            FinanceError::Overflow(m) => format!("金额超出可表示范围：{m}"),
        }
    }
}

pub(crate) fn today_cn() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        + 8 * 3600;
    crate::assets::civil_from_days((secs / 86400) as i64)
}

pub(crate) fn now_secs() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}

/// 占用是否缺少关联标的（convert 前置校验）。
fn input_symbol_is_empty(conn: &Connection, id: i64) -> Result<bool, FinanceError> {
    let symbol: String = conn.query_row(
        "SELECT symbol FROM cash_reservations WHERE id=?1",
        params![id],
        |r| r.get(0),
    )?;
    Ok(symbol.trim().is_empty())
}

pub(crate) fn parse_amount_minor(text: &str, what: &str) -> Result<i64, FinanceError> {
    let cleaned: String = text.chars().filter(|c| *c != ',').collect();
    let fixed = crate::fixed::Fixed::parse(cleaned.trim(), 2)
        .map_err(|_| FinanceError::Invalid(format!("{what}最多两位小数")))?;
    Ok(fixed.raw())
}

// ---------------------------------------------------------------------------
// 现金聚合（02 §1.1 / §4）
// ---------------------------------------------------------------------------

/// 现金四量。全部来自同一读快照（with_read）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CashPosition {
    /// 到账现金 C（manual_assets cash 类型合计；无记录 = 未建账，None）。
    pub cash_minor: Option<i64>,
    /// 预留 R；unconfigured 状态时为 None（待确认用途，不默认 0）。
    pub reserved_minor: Option<i64>,
    pub reserve_state: &'static str,
    /// 占用 L（held 状态合计）。
    pub reserved_by_orders_minor: i64,
    /// 可用 F = max(0, C−R−L)；仅 C 与 R 都已知时计算。
    pub available_minor: Option<i64>,
    /// C−R−L < 0 的资金缺口（显式报告，不被 max(0,·) 隐藏）。
    pub shortfall_minor: Option<i64>,
    /// 各账户可用现金（多账户逐户展示，02 §4）。
    pub accounts: Vec<AccountCash>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountCash {
    pub account_id: i64,
    pub account_name: String,
    pub cash_minor: Option<i64>,
    pub reserved_minor: Option<i64>,
    pub reserve_state: &'static str,
    pub reserved_by_orders_minor: i64,
    pub available_minor: Option<i64>,
}

pub fn cash_position(conn: &Connection) -> Result<CashPosition, FinanceError> {
    let mut pos = CashPosition {
        reserve_state: "unconfigured",
        ..Default::default()
    };
    // 各账户现金（manual_assets cash 类型）。
    let mut stmt = conn.prepare(
        "SELECT a.id, a.name, COALESCE(SUM(m.market_value_minor), 0)
         FROM accounts a
         LEFT JOIN manual_assets m
           ON m.account_id = a.id AND m.product_type = 'cash'
         GROUP BY a.id, a.name
         HAVING COUNT(m.id) > 0 OR 1
         ORDER BY a.id",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    // 预留（按账户）。
    let mut reserve_map: Vec<(i64, &'static str, i64)> = Vec::new();
    let mut stmt = conn.prepare("SELECT account_id, state, amount_minor FROM cash_reserves")?;
    let reserve_rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (account_id, state, amount) in reserve_rows {
        let state: &'static str = if state == "configured" {
            "configured"
        } else {
            "unconfigured"
        };
        reserve_map.push((account_id, state, amount));
    }
    // 占用（按账户，held）。
    let mut order_map: Vec<(i64, i64)> = Vec::new();
    let mut stmt = conn.prepare(
        "SELECT account_id, COALESCE(SUM(amount_minor),0) FROM cash_reservations
         WHERE status = 'held' GROUP BY account_id",
    )?;
    let order_rows = stmt
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    for (account_id, total) in order_rows {
        order_map.push((account_id, total));
    }

    let mut any_cash_known = false;
    let mut cash_total: i64 = 0;
    let mut reserve_total: i64 = 0;
    for (account_id, account_name, cash) in rows {
        let has_cash_assets: bool = conn
            .query_row(
                "SELECT 1 FROM manual_assets WHERE account_id=?1 AND product_type='cash' LIMIT 1",
                params![account_id],
                |r| r.get::<_, i64>(0).map(|_| true),
            )
            .optional()?
            .unwrap_or(false);
        let (reserve_state, reserved): (&'static str, Option<i64>) = reserve_map
            .iter()
            .find(|(id, _, _)| *id == account_id)
            .map(|(_, s, a)| (*s, if *s == "configured" { Some(*a) } else { None }))
            .unwrap_or(("unconfigured", None));
        let orders = order_map
            .iter()
            .find(|(id, _)| *id == account_id)
            .map(|(_, v)| *v)
            .unwrap_or(0);
        let available = if has_cash_assets && reserved.is_some() {
            let base = cash;
            let minus_reserve = reserved.unwrap_or(0);
            let net = add_minor(
                add_minor(base, -minus_reserve, "可用现金")?,
                -orders,
                "可用现金",
            )?;
            if net < 0 {
                Some(0)
            } else {
                Some(net)
            }
        } else {
            None
        };
        if has_cash_assets {
            any_cash_known = true;
            cash_total = add_minor(cash_total, cash, "现金合计")?;
            if let Some(r) = reserved {
                reserve_total = add_minor(reserve_total, r, "预留合计")?;
            }
        }
        pos.accounts.push(AccountCash {
            account_id,
            account_name,
            cash_minor: if has_cash_assets { Some(cash) } else { None },
            reserved_minor: reserved,
            reserve_state,
            reserved_by_orders_minor: orders,
            available_minor: available,
        });
    }
    let orders_total = order_map
        .iter()
        .try_fold(0, |n, (_, v)| add_minor(n, *v, "占用合计"))?;
    pos.cash_minor = if any_cash_known {
        Some(cash_total)
    } else {
        None
    };
    // 聚合预留状态（R01）：覆盖「有现金资产」或「有预留配置行」的账户；
    // 只有这些账户全部明确配置预留，汇总才算 configured。
    // partial 时逐户展示、汇总不合并；reserved/available 仅 configured 时给出。
    let covered: Vec<&AccountCash> = pos
        .accounts
        .iter()
        .filter(|a| {
            a.cash_minor.is_some() || reserve_map.iter().any(|(id, _, _)| *id == a.account_id)
        })
        .collect();
    let unconfigured = covered
        .iter()
        .filter(|a| a.reserve_state != "configured")
        .count();
    pos.reserve_state = if covered.is_empty() {
        "unconfigured"
    } else if unconfigured == 0 {
        "configured"
    } else if unconfigured == covered.len() {
        "unconfigured"
    } else {
        "partial"
    };
    if pos.reserve_state == "configured" {
        pos.reserved_minor = Some(reserve_total);
    }
    pos.reserved_by_orders_minor = orders_total;
    if let (Some(c), Some(r)) = (pos.cash_minor, pos.reserved_minor) {
        let net = add_minor(add_minor(c, -r, "可用现金")?, -orders_total, "可用现金")?;
        if net < 0 {
            pos.shortfall_minor = Some(-net);
            pos.available_minor = Some(0);
        } else {
            pos.available_minor = Some(net);
        }
    }
    Ok(pos)
}

// ---------------------------------------------------------------------------
// 资金事件（02 §3.2）
// ---------------------------------------------------------------------------

pub struct CashEventInput {
    pub account_id: i64,
    pub event_type: String,
    pub amount_text: String,
    pub occurred_at: String,
    pub idempotency_key: Option<String>,
    pub note: String,
    /// internal_transfer 必填：目标账户（源账户为 account_id）。
    pub counterparty_account_id: Option<i64>,
}

pub struct CashEventRow {
    pub id: i64,
    pub account_id: i64,
    pub event_type: String,
    pub amount_minor: i64,
    pub occurred_at: String,
    pub note: String,
}

/// 记录资金事件。幂等：同 key 同经济语义（账户/类型/金额/日期）返回既有 ID，
/// 任何维度不同均显式冲突。external_in / external_out 同事务更新到账现金余额
/// （manual_assets cash，02 §3.2 资产影响列）；internal_transfer 原子完成
/// 源账户减少与目标账户增加（组合口径净外部投入为 0，不入 external 统计）。
pub fn record_cash_event(conn: &Connection, input: &CashEventInput) -> Result<i64, FinanceError> {
    if !EVENT_TYPES.contains(&input.event_type.as_str()) {
        return Err(FinanceError::Invalid(format!(
            "未知事件类型 {}",
            input.event_type
        )));
    }
    let amount = parse_amount_minor(&input.amount_text, "金额")?;
    if input
        .idempotency_key
        .as_ref()
        .is_some_and(|k| k.is_empty() || k.len() > 128)
        || input.note.len() > 4096
    {
        return Err(FinanceError::Invalid("请求标识或备注长度无效".into()));
    }
    if amount == 0 {
        return Err(FinanceError::Invalid("金额不能为零".into()));
    }
    let negative_allowed = matches!(input.event_type.as_str(), "reconciliation" | "correction");
    if amount < 0 && !negative_allowed {
        return Err(FinanceError::Invalid("金额不能为负".into()));
    }
    if !is_valid_calendar_date(&input.occurred_at) || input.occurred_at > today_cn() {
        return Err(FinanceError::Invalid("发生日期不是有效日历日期".into()));
    }
    if input.event_type == "internal_transfer" {
        let Some(target) = input.counterparty_account_id else {
            return Err(FinanceError::Invalid("内部转账必须指定目标账户".into()));
        };
        if target == input.account_id {
            return Err(FinanceError::Invalid(
                "内部转账源账户与目标账户不能相同".into(),
            ));
        }
    } else if input.counterparty_account_id.is_some() {
        return Err(FinanceError::Invalid("仅内部转账允许指定对手方账户".into()));
    }
    if let Some(key) = &input.idempotency_key {
        let existing: Option<(i64, i64, i64, String, String, String)> = conn
            .query_row(
                "SELECT id, amount_minor, account_id, event_type, occurred_at, related_id
                 FROM cash_events WHERE idempotency_key = ?1",
                params![key],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                },
            )
            .optional()?;
        if let Some((
            id,
            existing_amount,
            existing_account,
            existing_type,
            existing_date,
            existing_target,
        )) = existing
        {
            let same = existing_amount == amount
                && existing_account == input.account_id
                && existing_type == input.event_type
                && existing_date == input.occurred_at
                && existing_target
                    == input
                        .counterparty_account_id
                        .map(|v| v.to_string())
                        .unwrap_or_default();
            if !same {
                return Err(FinanceError::Conflict(format!(
                    "幂等键 {key} 已存在且经济语义不同"
                )));
            }
            return Ok(id);
        }
    }
    if input.event_type == "dividend_reinvest" {
        return Err(FinanceError::Invalid(
            "再投资必须关联已有人工资产，或按真实基金份额登记分红及交易".into(),
        ));
    }
    if input.event_type == "opening" {
        let exists:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM manual_assets WHERE account_id=?1 AND product_type='cash' AND market_value_minor!=0)",[input.account_id],|r|r.get(0))?;
        if exists {
            return Err(FinanceError::Invalid(
                "已有现金余额，首次建账不能再次增加".into(),
            ));
        }
    }
    let account_exists: bool = conn
        .query_row(
            "SELECT 1 FROM accounts WHERE id = ?1",
            params![input.account_id],
            |r| r.get::<_, i64>(0).map(|_| true),
        )
        .optional()?
        .unwrap_or(false);
    if !account_exists {
        return Err(FinanceError::NotFound(format!("账户 {}", input.account_id)));
    }
    if input.event_type == "internal_transfer" {
        let target = input.counterparty_account_id.unwrap();
        let target_exists: bool = conn
            .query_row(
                "SELECT 1 FROM accounts WHERE id = ?1",
                params![target],
                |r| r.get::<_, i64>(0).map(|_| true),
            )
            .optional()?
            .unwrap_or(false);
        if !target_exists {
            return Err(FinanceError::NotFound(format!("账户 {target}")));
        }
    }
    // 余额校验：external_out / fee / 内部转账源侧不得使现金为负。
    let needs_balance_check = matches!(
        input.event_type.as_str(),
        EXTERNAL_OUT | "fee" | "internal_transfer"
    );
    if needs_balance_check {
        let cash: Option<i64> = conn
            .query_row(
                "SELECT COALESCE(SUM(market_value_minor),0) FROM manual_assets
                 WHERE account_id=?1 AND product_type='cash'",
                params![input.account_id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(cash) = cash {
            if amount > cash {
                return Err(FinanceError::Invalid(format!(
                    "转出 {amount} 分超过账户到账现金 {cash} 分"
                )));
            }
        }
    }
    let now = now_secs();
    conn.execute(
        "INSERT INTO cash_events(account_id, event_type, amount_minor, occurred_at,
                                 idempotency_key, related_type, related_id, note, created_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![
            input.account_id,
            input.event_type,
            amount,
            input.occurred_at,
            input.idempotency_key,
            if input.event_type == "internal_transfer" {
                "account"
            } else {
                ""
            },
            input
                .counterparty_account_id
                .map(|v| v.to_string())
                .unwrap_or_default(),
            input.note,
            now
        ],
    )?;
    let event_id = conn.last_insert_rowid();
    // 资产影响（02 §3.2）：外部转入/转出同事务改变到账现金；内部转账双侧。
    match input.event_type.as_str() {
        EXTERNAL_IN | "dividend_cash" | "opening" | "reconciliation" | "correction" => {
            adjust_account_cash(conn, input.account_id, amount, &input.occurred_at)?
        }
        EXTERNAL_OUT | "fee" => {
            adjust_account_cash(conn, input.account_id, -amount, &input.occurred_at)?
        }
        "internal_transfer" => {
            let target = input.counterparty_account_id.unwrap();
            adjust_account_cash(conn, input.account_id, -amount, &input.occurred_at)?;
            adjust_account_cash(conn, target, amount, &input.occurred_at)?;
        }
        _ => {}
    }
    Ok(event_id)
}

/// 同事务调整账户到账现金：无现金资产记录时新建一条（external_in 建账路径），
/// 余额不足以扣减时报错回滚整个写事务。
pub(crate) fn adjust_account_cash(
    conn: &Connection,
    account_id: i64,
    delta: i64,
    valued_at: &str,
) -> Result<(), FinanceError> {
    let now = now_secs();
    let existing: Option<(i64, i64)> = conn
        .query_row(
            "SELECT id, market_value_minor FROM manual_assets
             WHERE account_id=?1 AND product_type='cash' ORDER BY id LIMIT 1",
            params![account_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((asset_id, cash)) = existing {
        let next = add_minor(cash, delta, "账户现金")?;
        if next < 0 {
            return Err(FinanceError::Invalid("扣减后账户现金为负".into()));
        }
        conn.execute(
            "UPDATE manual_assets SET market_value_minor = ?2,
             updated_at = ?3, valued_at=MAX(valued_at, ?4), version=version+1 WHERE id = ?1",
            params![asset_id, next, now, valued_at],
        )?;
    } else if delta > 0 {
        conn.execute(
            "INSERT INTO manual_assets(account_id, product_type, instrument_key, name,
                                       risk_class, market_value_minor, valued_at,
                                       legacy_overlap_distinct, created_at, updated_at)
             VALUES(?1,'cash','cash','现金','cash',?2,?3,0,?4,?4)",
            params![account_id, delta, valued_at, now],
        )?;
    } else if delta < 0 {
        return Err(FinanceError::Invalid("账户未登记到账现金".into()));
    }
    // delta < 0 且无现金记录：无账可扣，报错（余额校验应已拦截）。
    Ok(())
}

pub fn list_cash_events(
    conn: &Connection,
    limit: usize,
) -> Result<Vec<CashEventRow>, FinanceError> {
    let mut stmt = conn.prepare(
        "SELECT id, account_id, event_type, amount_minor, occurred_at, note
         FROM cash_events ORDER BY occurred_at DESC, id DESC LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(params![limit as i64], |r| {
            Ok(CashEventRow {
                id: r.get(0)?,
                account_id: r.get(1)?,
                event_type: r.get(2)?,
                amount_minor: r.get(3)?,
                occurred_at: r.get(4)?,
                note: r.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 区间外部净投入（external_in − external_out；分位）。
/// 核对差额/更正等事件按真实原因另行登记，不自动计入外部投入（02 §3.2）。
pub fn net_external_flow(
    conn: &Connection,
    from_date: &str,
    to_date: &str,
) -> Result<i64, FinanceError> {
    let flow: i64 = conn.query_row(
        "SELECT COALESCE(SUM(CASE event_type
            WHEN 'external_in' THEN amount_minor
            WHEN 'external_out' THEN -amount_minor ELSE 0 END), 0)
         FROM cash_events
         WHERE occurred_at >= ?1 AND occurred_at <= ?2",
        params![from_date, to_date],
        |r| r.get(0),
    )?;
    Ok(flow)
}

// ---------------------------------------------------------------------------
// 预留与占用（02 §4）
// ---------------------------------------------------------------------------

/// 保存账户预留。amount_text 为空 → unconfigured（待确认用途）；
/// 显式数字（含 0）→ configured。预留不得超过该账户到账现金。
pub fn save_reserve(
    conn: &Connection,
    account_id: i64,
    amount_text: Option<&str>,
    purpose: &str,
) -> Result<(), FinanceError> {
    let (state, amount) = match amount_text.map(str::trim) {
        None | Some("") => ("unconfigured", 0i64),
        Some(text) => {
            let amount = parse_amount_minor(text, "预留额")?;
            if amount < 0 {
                return Err(FinanceError::Invalid("预留额不能为负".into()));
            }
            let cash: i64 = conn.query_row(
                "SELECT COALESCE(SUM(market_value_minor),0) FROM manual_assets
                 WHERE account_id=?1 AND product_type='cash'",
                params![account_id],
                |r| r.get(0),
            )?;
            if amount > cash {
                return Err(FinanceError::Invalid(format!(
                    "预留 {amount} 分超过账户到账现金 {cash} 分"
                )));
            }
            ("configured", amount)
        }
    };
    let now = now_secs();
    conn.execute(
        "INSERT INTO cash_reserves(account_id, state, amount_minor, purpose, updated_at)
         VALUES(?1,?2,?3,?4,?5)
         ON CONFLICT(account_id) DO UPDATE SET state=excluded.state,
             amount_minor=excluded.amount_minor, purpose=excluded.purpose,
             updated_at=excluded.updated_at",
        params![account_id, state, amount, purpose, now],
    )?;
    Ok(())
}

pub struct ReservationInput {
    pub account_id: i64,
    pub symbol: String,
    pub risk_class: String,
    pub amount_text: String,
    pub expected_date: String,
    pub note: String,
}

/// 新建占用。校验账户存在、金额 > 0、不超过账户可用现金。
pub fn create_reservation(
    conn: &Connection,
    input: &ReservationInput,
) -> Result<i64, FinanceError> {
    let amount = parse_amount_minor(&input.amount_text, "占用金额")?;
    if input.symbol.trim().is_empty()
        || input.symbol.len() > 64
        || (!input.expected_date.is_empty() && !is_valid_calendar_date(&input.expected_date))
    {
        return Err(FinanceError::Invalid("占用标的或预计日期无效".into()));
    }
    if !["equity", "gold", "fixed_income", "other"].contains(&input.risk_class.as_str()) {
        return Err(FinanceError::Invalid("占用风险类别无效".into()));
    }
    if amount <= 0 {
        return Err(FinanceError::Invalid("占用金额必须大于零".into()));
    }
    let position = cash_position(conn)?;
    let available = position
        .accounts
        .iter()
        .find(|a| a.account_id == input.account_id)
        .and_then(|a| a.available_minor)
        .ok_or_else(|| FinanceError::Invalid("账户现金或预留未确认".into()))?;
    if amount > available {
        return Err(FinanceError::Invalid("占用超过扣除预留后的可用现金".into()));
    }
    let cash: i64 = conn.query_row(
        "SELECT COALESCE(SUM(market_value_minor),0) FROM manual_assets
         WHERE account_id=?1 AND product_type='cash'",
        params![input.account_id],
        |r| r.get(0),
    )?;
    let reserved: i64 = conn.query_row(
        "SELECT COALESCE(SUM(amount_minor),0) FROM cash_reservations
         WHERE account_id=?1 AND status='held'",
        params![input.account_id],
        |r| r.get(0),
    )?;
    if add_minor(reserved, amount, "占用合计")? > cash {
        return Err(FinanceError::Invalid("占用合计不能超过账户到账现金".into()));
    }
    let now = now_secs();
    conn.execute(
        "INSERT INTO cash_reservations(account_id, symbol, risk_class, amount_minor, status,
                                       expected_date, note, created_at, updated_at)
         VALUES(?1,?2,?3,?4,'held',?5,?6,?7,?7)",
        params![
            input.account_id,
            crate::rules::normalize_symbol_key(&input.symbol),
            input.risk_class,
            amount,
            input.expected_date,
            input.note,
            now
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 占用状态迁移：release（取消释放）/ convert（转正式成交，同事务扣现金并解除）。
/// convert 只允许一次（status 从 held → converted 幂等防护）。
pub fn settle_reservation(conn: &Connection, id: i64, action: &str) -> Result<(), FinanceError> {
    let row: Option<(i64, i64, String)> = conn
        .query_row(
            "SELECT account_id, amount_minor, status FROM cash_reservations WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let (account_id, amount, status) =
        row.ok_or_else(|| FinanceError::NotFound(format!("占用 {id}")))?;
    if status != "held" {
        return Err(FinanceError::Conflict(format!(
            "占用 {id} 已是 {status}，只能对 held 状态操作"
        )));
    }
    let now = now_secs();
    match action {
        "release" => {
            conn.execute(
                "UPDATE cash_reservations SET status='released', updated_at=?2 WHERE id=?1",
                params![id, now],
            )?;
            Ok(())
        }
        "convert" => {
            // 成交扣款：现金 manual 资产同事务减少（不足则报错回滚）。
            let cash: i64 = conn.query_row(
                "SELECT COALESCE(SUM(market_value_minor),0) FROM manual_assets
                 WHERE account_id=?1 AND product_type='cash'",
                params![account_id],
                |r| r.get(0),
            )?;
            if amount > cash {
                return Err(FinanceError::Invalid(format!(
                    "成交扣款 {amount} 分超过账户现金 {cash} 分"
                )));
            }
            if input_symbol_is_empty(conn, id)? {
                return Err(FinanceError::Invalid(
                    "占用缺少关联标的，转成交前必须补录标的（不伪造资产记录）".into(),
                ));
            }
            let symbol: String = conn.query_row(
                "SELECT symbol FROM cash_reservations WHERE id=?1",
                [id],
                |r| r.get(0),
            )?;
            let asset:Option<(i64,i64,String)>=conn.query_row("SELECT id,market_value_minor,risk_class FROM manual_assets WHERE account_id=?1 AND instrument_key=?2 AND product_type!='cash'",params![account_id,symbol],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
            let Some((asset, value, class)) = asset else {
                return Err(FinanceError::Invalid(
                    "先按真实标的登记人工资产，基金成交请使用真实份额流水；不自动推断资产类别"
                        .into(),
                ));
            };
            if class == "unknown" {
                return Err(FinanceError::Invalid("成交目标资产分类待确认".into()));
            }
            let next = add_minor(value, amount, "成交资产")?;
            adjust_account_cash(conn, account_id, -amount, &today_cn())?;
            conn.execute("UPDATE manual_assets SET market_value_minor=?2,valued_at=?3,updated_at=?4,version=version+1 WHERE id=?1",params![asset,next,today_cn(),now])?;
            conn.execute(
                "UPDATE cash_reservations SET status='converted', updated_at=?2 WHERE id=?1",
                params![id, now],
            )?;
            Ok(())
        }
        other => Err(FinanceError::Invalid(format!("未知占用操作 {other}"))),
    }
}

/// 人工资产的真实分红再投资；只改变总估值，不编造基金份额。
pub fn record_reinvestment(
    c: &Connection,
    input: &CashEventInput,
    asset_id: i64,
) -> Result<i64, FinanceError> {
    let amount = parse_amount_minor(&input.amount_text, "再投资金额")?;
    if amount <= 0
        || !is_valid_calendar_date(&input.occurred_at)
        || input.occurred_at > today_cn()
        || input.note.len() > 4096
    {
        return Err(FinanceError::Invalid("再投资金额、日期或说明无效".into()));
    }
    let target = asset_id.to_string();
    if let Some(key) = &input.idempotency_key {
        if key.is_empty() || key.len() > 128 {
            return Err(FinanceError::Invalid("请求编号无效".into()));
        }
        let existing:Option<(i64,i64,i64,String,String,String)>=c.query_row("SELECT id,account_id,amount_minor,event_type,occurred_at,related_id FROM cash_events WHERE idempotency_key=?1",[key],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional()?;
        if let Some((id, account, value, kind, date, related)) = existing {
            if account == input.account_id
                && value == amount
                && kind == "dividend_reinvest"
                && date == input.occurred_at
                && related == target
            {
                return Ok(id);
            }
            return Err(FinanceError::Conflict("请求编号经济语义不同".into()));
        }
    }
    let value:Option<i64>=c.query_row("SELECT market_value_minor FROM manual_assets WHERE id=?1 AND account_id=?2 AND product_type!='cash'",params![asset_id,input.account_id],|r|r.get(0)).optional()?;
    let value =
        value.ok_or_else(|| FinanceError::Invalid("请选择同一账户的非现金人工资产".into()))?;
    let next = add_minor(value, amount, "再投资资产")?;
    c.execute("INSERT INTO cash_events(account_id,event_type,amount_minor,occurred_at,idempotency_key,related_type,related_id,note,created_at) VALUES(?1,'dividend_reinvest',?2,?3,?4,'manual_asset',?5,?6,?7)",params![input.account_id,amount,input.occurred_at,input.idempotency_key,target,input.note,now_secs()])?;
    let id = c.last_insert_rowid();
    c.execute(
        "UPDATE manual_assets SET market_value_minor=?2,valued_at=?3,updated_at=?4,version=version+1 WHERE id=?1",
        params![asset_id, next, input.occurred_at, now_secs()],
    )?;
    Ok(id)
}

pub use crate::returns::{returns_for_range, save_value_snapshot, ReturnResult, ValueSnapshotRow};

#[cfg(test)]
#[path = "finance_tests.rs"]
mod tests;
