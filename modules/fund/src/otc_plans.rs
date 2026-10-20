//! 场外基金定投计划（N09）：计划管理与执行登记。不自动扣款、不自动下单、
//! 不新增常驻进程；下次计划日期按星期历现算（节假日未判定时明确标注），
//! 期次执行经 UNIQUE(plan_id, period_key) 防重复登记，买入复用场外订单流程。

use crate::fixed::Fixed;
use crate::ledger::LedgerError;
use crate::storage::Store;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

use crate::otc_holding::fund_id_by_code;
use crate::otc_ops::{now_string, order_create_tx, weekday_mon1};

pub(crate) struct PlanInput {
    pub account_name: String,
    pub fund_code: String,
    pub fund_name: String,
    pub amount_minor: i64,
    pub freq: String,
    pub weekday: Option<i64>,
    pub monthday: Option<i64>,
    pub start_date: String,
    pub end_date: Option<String>,
    pub non_trading: String,
    pub note: String,
}

fn validate_plan(input: &PlanInput, today: &str) -> Result<(), LedgerError> {
    if input.amount_minor <= 0 {
        return Err(LedgerError::Invalid("定投金额必须大于零".into()));
    }
    if !["daily", "weekly", "monthly"].contains(&input.freq.as_str()) {
        return Err(LedgerError::Invalid("定投周期无效".into()));
    }
    match input.freq.as_str() {
        "daily" => {
            if input.weekday.is_some() || input.monthday.is_some() {
                return Err(LedgerError::Invalid("每日定投不需要星期或日期字段".into()));
            }
        }
        "weekly" => {
            let weekday = input
                .weekday
                .ok_or_else(|| LedgerError::Invalid("每周定投必须选择星期".into()))?;
            if !(1..=7).contains(&weekday) {
                return Err(LedgerError::Invalid("星期取值 1-7（周一到周日）".into()));
            }
        }
        "monthly" => {
            let monthday = input
                .monthday
                .ok_or_else(|| LedgerError::Invalid("每月定投必须选择日期".into()))?;
            if !(1..=31).contains(&monthday) {
                return Err(LedgerError::Invalid(
                    "每月日期取值 1-31；月末不存在的日期按月末顺延规则执行".into(),
                ));
            }
        }
        _ => {}
    }
    if !crate::assets::is_valid_calendar_date(&input.start_date)
        || input.start_date.as_str() > today
    {
        return Err(LedgerError::Invalid(
            "开始日期必须是今天或之前的有效日历日".into(),
        ));
    }
    if let Some(end) = &input.end_date {
        if !crate::assets::is_valid_calendar_date(end) || end < &input.start_date {
            return Err(LedgerError::Invalid(
                "结束日期必须是有效日历日且不早于开始日期".into(),
            ));
        }
    }
    if !["next", "prev", "skip"].contains(&input.non_trading.as_str()) {
        return Err(LedgerError::Invalid("非交易日处理方式无效".into()));
    }
    Ok(())
}

fn month_last_day(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
            if leap {
                29
            } else {
                28
            }
        }
    }
}

/// 周期在给定月份的原始应执行日：每月定投取 min(monthday, 月末)；
/// 每周／每日返回 None（由调用方逐日判定）。
fn monthly_day_in_month(plan: &PlanRow, year: i64, month: i64) -> Option<i64> {
    if plan.freq != "monthly" {
        return None;
    }
    let monthday = plan.monthday.unwrap_or(1);
    Some(monthday.min(month_last_day(year, month)))
}

fn matches_schedule(plan: &PlanRow, date: &str) -> Result<bool, LedgerError> {
    let days = crate::assets::days_from_civil(date)
        .ok_or_else(|| LedgerError::Invalid(format!("日期无效 {date}")))?;
    let (y, m, d) = {
        let civil = crate::assets::civil_from_days(days);
        let parts: Vec<i64> = civil
            .split('-')
            .map(|p| p.parse::<i64>().unwrap_or(0))
            .collect();
        (parts[0], parts[1], parts[2])
    };
    let hit = match plan.freq.as_str() {
        "daily" => true,
        "weekly" => weekday_mon1(days) == plan.weekday.unwrap_or(1),
        "monthly" => monthly_day_in_month(plan, y, m) == Some(d),
        other => return Err(LedgerError::Invalid(format!("未知周期 {other}"))),
    };
    Ok(hit)
}

/// 非交易日调整：仅按星期历判定（周六／周日），节假日当前不可判定，
/// 调用方以 tradingDayVerified=false 标注。'skip' 对每周／每月语义为
/// 顺延到下一个原始应执行日，每日为跳过当天。
fn apply_non_trading(plan: &PlanRow, date: &str) -> Result<Option<String>, LedgerError> {
    let days = crate::assets::days_from_civil(date)
        .ok_or_else(|| LedgerError::Invalid(format!("日期无效 {date}")))?;
    let is_weekend = weekday_mon1(days) > 5;
    if !is_weekend {
        return Ok(Some(date.to_string()));
    }
    match plan.non_trading.as_str() {
        "next" => {
            let mut n = days;
            while weekday_mon1(n) > 5 {
                n += 1;
            }
            Ok(Some(crate::assets::civil_from_days(n)))
        }
        "prev" => {
            let mut n = days;
            while weekday_mon1(n) > 5 {
                n -= 1;
            }
            Ok(Some(crate::assets::civil_from_days(n)))
        }
        "skip" => Ok(None),
        other => Err(LedgerError::Invalid(format!("未知非交易日规则 {other}"))),
    }
}

pub(crate) struct PlanRow {
    pub id: i64,
    pub account_id: i64,
    pub fund_id: i64,
    pub amount_minor: i64,
    pub freq: String,
    pub weekday: Option<i64>,
    pub monthday: Option<i64>,
    pub start_date: String,
    pub end_date: Option<String>,
    pub non_trading: String,
    pub status: String,
    pub version: i64,
    pub note: String,
}

const PLAN_COLS: &str = "id, account_id, fund_id, amount_minor, freq, weekday, monthday, \
    start_date, end_date, non_trading, status, version, note";

fn map_plan_row(r: &rusqlite::Row) -> rusqlite::Result<PlanRow> {
    Ok(PlanRow {
        id: r.get(0)?,
        account_id: r.get(1)?,
        fund_id: r.get(2)?,
        amount_minor: r.get(3)?,
        freq: r.get(4)?,
        weekday: r.get(5)?,
        monthday: r.get(6)?,
        start_date: r.get(7)?,
        end_date: r.get(8)?,
        non_trading: r.get(9)?,
        status: r.get(10)?,
        version: r.get(11)?,
        note: r.get(12)?,
    })
}

fn load_plan(conn: &Connection, id: i64) -> Result<PlanRow, LedgerError> {
    conn.query_row(
        &format!("SELECT {PLAN_COLS} FROM fund_plans WHERE id=?1"),
        [id],
        map_plan_row,
    )
    .optional()?
    .ok_or_else(|| LedgerError::NotFound("定投计划".into()))
}

/// 下一次应执行日期：从 max(start, today) 起按周期找原始应执行日（每月
/// 不存在的日期取当月月末），应用非交易日规则；最多推进 400 天防御。
/// 返回 (执行日期, 交易日是否已判定=false)。节假日无可信日历，不伪造。
pub(crate) fn next_date(
    _conn: &Connection,
    plan: &PlanRow,
    today: &str,
) -> Result<Option<(String, bool)>, LedgerError> {
    if plan.status != "active" {
        return Ok(None);
    }
    let start = std::cmp::max(plan.start_date.as_str(), today);
    let mut days = crate::assets::days_from_civil(start)
        .ok_or_else(|| LedgerError::Invalid("开始日期无效".into()))?;
    let end_days = plan
        .end_date
        .as_deref()
        .and_then(crate::assets::days_from_civil);
    for _ in 0..400 {
        if let Some(end) = end_days {
            if days > end {
                return Ok(None);
            }
        }
        let date = crate::assets::civil_from_days(days);
        if matches_schedule(plan, &date)? {
            if let Some(adjusted) = apply_non_trading(plan, &date)? {
                return Ok(Some((adjusted, false)));
            }
        }
        days += 1;
    }
    Ok(None)
}

fn plan_json(conn: &Connection, plan: &PlanRow, today: &str) -> Result<Value, LedgerError> {
    let (code, name): (String, String) = conn.query_row(
        "SELECT code, name FROM funds WHERE id=?1",
        [plan.fund_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let account: String = conn.query_row(
        "SELECT name FROM accounts WHERE id=?1",
        [plan.account_id],
        |r| r.get(0),
    )?;
    let (next, verified) = match next_date(conn, plan, today)? {
        Some((d, v)) => (Some(d), Some(v)),
        None => (None, None),
    };
    let runs: i64 = conn.query_row(
        "SELECT COUNT(*) FROM fund_plan_runs WHERE plan_id=?1 AND status='registered'",
        [plan.id],
        |r| r.get(0),
    )?;
    Ok(json!({
        "id": plan.id,
        "accountId": plan.account_id, "account": account,
        "fundId": plan.fund_id, "fundCode": code, "fundName": name,
        "amount": Fixed::amount(plan.amount_minor).to_string_value(),
        "freq": plan.freq,
        "weekday": plan.weekday, "monthday": plan.monthday,
        "startDate": plan.start_date, "endDate": plan.end_date,
        "nonTrading": plan.non_trading,
        "status": plan.status,
        "nextDate": next, "tradingDayVerified": verified.unwrap_or(true),
        "registeredRuns": runs,
        "version": plan.version, "note": plan.note,
    }))
}

pub(crate) fn plan_create(
    store: &Store,
    input: &PlanInput,
    today: &str,
) -> Result<Value, LedgerError> {
    validate_plan(input, today)?;
    store.with_write(|conn| {
        let account_id = crate::ledger::ensure_account(conn, &input.account_name)?;
        let fund_id = crate::ledger::ensure_fund(conn, &input.fund_code, &input.fund_name)?;
        conn.execute(
            "INSERT INTO fund_plans(account_id, fund_id, amount_minor, freq, weekday, monthday,
                 start_date, end_date, non_trading, status, note, created_at, updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'active',?10,?11,?11)",
            params![
                account_id,
                fund_id,
                input.amount_minor,
                input.freq,
                input.weekday,
                input.monthday,
                input.start_date,
                input.end_date,
                input.non_trading,
                input.note,
                now_string(),
            ],
        )?;
        Ok(json!({"ok":true,"id":conn.last_insert_rowid()}))
    })
}

/// 计划更新：pause/resume/end/edit，全部走 version 乐观锁；编辑不改写
/// 已登记期次与历史订单。
pub(crate) fn plan_update(
    store: &Store,
    id: i64,
    expected_version: i64,
    action: &str,
    input: Option<&PlanInput>,
    today: &str,
) -> Result<Value, LedgerError> {
    store.with_write(|conn| {
        let plan = load_plan(conn, id)?;
        if plan.version != expected_version {
            return Err(LedgerError::Conflict("计划已变化，请刷新后重新核对".into()));
        }
        let now = now_string();
        match action {
            "pause" if plan.status == "active" => {
                conn.execute(
                    "UPDATE fund_plans SET status='paused', version=version+1, updated_at=?2 WHERE id=?1",
                    params![id, now],
                )?;
            }
            "resume" if plan.status == "paused" => {
                conn.execute(
                    "UPDATE fund_plans SET status='active', version=version+1, updated_at=?2 WHERE id=?1",
                    params![id, now],
                )?;
            }
            "end" if plan.status != "ended" => {
                conn.execute(
                    "UPDATE fund_plans SET status='ended', version=version+1, updated_at=?2 WHERE id=?1",
                    params![id, now],
                )?;
            }
            "edit" => {
                let input = input.ok_or_else(|| LedgerError::Invalid("plan edit fields required".into()))?;
                validate_plan(input, today)?;
                conn.execute(
                    "UPDATE fund_plans SET amount_minor=?3, freq=?4, weekday=?5, monthday=?6,
                         start_date=?7, end_date=?8, non_trading=?9, note=?10,
                         version=version+1, updated_at=?11
                     WHERE id=?1 AND version=?2",
                    params![
                        id,
                        expected_version,
                        input.amount_minor,
                        input.freq,
                        input.weekday,
                        input.monthday,
                        input.start_date,
                        input.end_date,
                        input.non_trading,
                        input.note,
                        now,
                    ],
                )?;
            }
            _ => {
                return Err(LedgerError::Invalid(format!("计划操作 {action} 与当前状态不符",)));
            }
        }
        Ok(json!({"ok":true}))
    })
}

pub(crate) fn plans_list(
    store: &Store,
    account_name: &str,
    fund_code: &str,
    today: &str,
) -> Result<Value, LedgerError> {
    store.with_read(|conn| {
        let account_id = crate::ledger::ensure_account(conn, account_name)?;
        let fund_id = fund_id_by_code(conn, fund_code)?
            .ok_or_else(|| LedgerError::NotFound("标的".into()))?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {PLAN_COLS} FROM fund_plans WHERE account_id=?1 AND fund_id=?2
             ORDER BY id DESC LIMIT 200"
        ))?;
        let rows = stmt
            .query_map(params![account_id, fund_id], map_plan_row)?
            .collect::<Result<Vec<_>, _>>()?;
        let plans = rows
            .iter()
            .map(|p| plan_json(conn, p, today))
            .collect::<Result<Vec<_>, LedgerError>>()?;
        Ok(json!({"plans": plans}))
    })
}

/// 期次执行记录（最近 100 条）。
pub(crate) fn plan_runs(store: &Store, plan_id: i64) -> Result<Value, LedgerError> {
    store.with_read(|conn| {
        let _plan = load_plan(conn, plan_id)?;
        let mut stmt = conn.prepare(
            "SELECT r.period_key, r.status, r.order_id, r.created_at, o.status
             FROM fund_plan_runs r LEFT JOIN fund_orders o ON o.id = r.order_id
             WHERE r.plan_id=?1 ORDER BY r.period_key DESC LIMIT 100",
        )?;
        let runs = stmt
            .query_map(params![plan_id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let runs = runs
            .into_iter()
            .map(|(period_key, status, order_id, created_at, order_status)| {
                json!({
                    "periodKey": period_key, "status": status,
                    "orderId": order_id, "orderStatus": order_status,
                    "createdAt": created_at,
                })
            })
            .collect::<Vec<_>>();
        Ok(json!({"planId": plan_id, "runs": runs}))
    })
}

/// 期次登记：mode='buy' 复用场外加仓订单（pending，不自动扣款不自动下单），
/// mode='skip' 留痕跳过；UNIQUE(plan_id, period_key) 防重复登记。
/// period_key 必须是该计划原始周期中的应执行日（≤今天且 ≥ start_date）。
pub(crate) fn plan_run_register(
    store: &Store,
    plan_id: i64,
    period_key: &str,
    mode: &str,
    cutoff: &str,
    order_request_id: &str,
    today: &str,
) -> Result<Value, LedgerError> {
    if !["buy", "skip"].contains(&mode) {
        return Err(LedgerError::Invalid("期次登记只能是 buy 或 skip".into()));
    }
    if !crate::assets::is_valid_calendar_date(period_key) || period_key > today {
        return Err(LedgerError::Invalid(
            "期次日期必须是今天或之前的有效日历日".into(),
        ));
    }
    store.with_write(|conn| {
        let plan = load_plan(conn, plan_id)?;
        if plan.status != "active" {
            return Err(LedgerError::Invalid(
                "计划已暂停或结束；先恢复计划再登记执行".into(),
            ));
        }
        if period_key < plan.start_date.as_str() {
            return Err(LedgerError::Invalid("期次日期早于计划开始日期".into()));
        }
        if let Some(end) = &plan.end_date {
            if period_key > end.as_str() {
                return Err(LedgerError::Invalid("期次日期晚于计划结束日期".into()));
            }
        }
        if !matches_schedule(&plan, period_key)? {
            return Err(LedgerError::Invalid(
                "期次日期不符合计划周期；请从执行记录列表登记指定期次".into(),
            ));
        }
        let order_id = if mode == "buy" {
            let (code, name): (String, String) = conn.query_row(
                "SELECT code, name FROM funds WHERE id=?1",
                [plan.fund_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let account: String = conn.query_row(
                "SELECT name FROM accounts WHERE id=?1",
                [plan.account_id],
                |r| r.get(0),
            )?;
            let draft = crate::otc_ops::OrderDraft {
                op: crate::otc_ops::OP_BUY,
                account_name: account,
                fund_code: code,
                fund_name: name,
                applied_date: today.to_string(),
                cutoff: if cutoff == "after_1500" {
                    "after_1500".into()
                } else {
                    "before_1500".into()
                },
                amount_minor: Some(plan.amount_minor),
                fee_minor: None,
                fee_known: false,
                shares_raw: None,
                target_code: None,
                target_name: None,
                plan_id: Some(plan.id),
                period_key: period_key.to_string(),
                request_id: order_request_id.to_string(),
                note: format!("定投期次 {period_key}"),
            };
            let created = order_create_tx(conn, &draft, today)?;
            created["id"].as_i64()
        } else {
            None
        };
        // 先查后插把重复期次判为明确 409；并发窗口撞 UNIQUE 也映射 409。
        let duplicate: Option<i64> = conn
            .query_row(
                "SELECT id FROM fund_plan_runs WHERE plan_id=?1 AND period_key=?2",
                params![plan_id, period_key],
                |r| r.get(0),
            )
            .optional()?;
        if duplicate.is_some() {
            return Err(LedgerError::Conflict(
                "该期次已登记执行，不能重复登记".into(),
            ));
        }
        let inserted = conn.execute(
            "INSERT INTO fund_plan_runs(plan_id, period_key, status, order_id, created_at)
             VALUES(?1,?2,?3,?4,?5)",
            params![
                plan_id,
                period_key,
                if mode == "buy" {
                    "registered"
                } else {
                    "skipped"
                },
                order_id,
                now_string()
            ],
        );
        if let Err(rusqlite::Error::SqliteFailure(e, _)) = &inserted {
            if e.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE {
                return Err(LedgerError::Conflict(
                    "该期次已登记执行，不能重复登记".into(),
                ));
            }
        }
        inserted?;
        Ok(json!({"ok":true,"orderId":order_id}))
    })
}
