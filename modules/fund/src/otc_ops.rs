//! 场外基金操作（N09）：申请订单（加仓／减仓／转换两腿）、持仓校准锚点。
//! 持仓权威仍是 portfolio::replay_position 回放；本模块只登记申请与实际
//! 确认结果，联动既有现金占用（cash_reservations）与到账（trade_cash）。
//! 定投计划在 otc_plans.rs；HTTP 层在 api/otc_api.rs。

use crate::finance::{self, ReservationInput};
use crate::fixed::{Fixed, SCALE_SHARE};
use crate::ledger::{self, LedgerError, TransactionInput};
use crate::otc_holding::fund_id_by_code;
use crate::storage::Store;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

pub(crate) const OP_BUY: &str = "buy";
pub(crate) const OP_SELL: &str = "sell";
pub(crate) const OP_CONVERT_OUT: &str = "convert_out";
pub(crate) const OP_CONVERT_IN: &str = "convert_in";

/// 星期一=1…星期日=7（proleptic Gregorian，1970-01-01 是周四）。
pub(crate) fn weekday_mon1(days: i64) -> i64 {
    (((days % 7) + 7 + 3) % 7) + 1
}

fn next_weekday(date: &str) -> Option<String> {
    let d = crate::assets::days_from_civil(date)?;
    let mut n = d + 1;
    while weekday_mon1(n) > 5 {
        n += 1;
    }
    Some(crate::assets::civil_from_days(n))
}

/// 15:00 截止口径的预计净值日期：15:00 前为申请日（工作日），否则次一
/// 工作日；周末顺延。交易日历（节假日）当前不可用，调用方必须标注
/// 「按工作日推算、节假日未判定」，不伪造确认／到账日期。
pub(crate) fn expected_nav_date(applied_date: &str, cutoff: &str) -> Option<String> {
    let days = crate::assets::days_from_civil(applied_date)?;
    if weekday_mon1(days) > 5 {
        return next_weekday(applied_date);
    }
    if cutoff == "after_1500" {
        return next_weekday(applied_date);
    }
    Some(applied_date.to_string())
}

pub(crate) fn now_string() -> String {
    crate::finance::now_secs().to_string()
}

/// 金额（分，2 位）＝份额 raw × 净值 raw ÷ 1e6（半进位）。
pub(crate) fn shares_to_amount_minor(shares_raw: i64, nav_raw: i64) -> Option<i64> {
    if shares_raw < 0 || nav_raw <= 0 {
        return None;
    }
    let num = (shares_raw as i128) * (nav_raw as i128);
    let amount = (num + 500_000) / 1_000_000;
    i64::try_from(amount).ok()
}

struct ResolvedFund {
    account_id: i64,
    fund_id: i64,
}

fn resolve(
    conn: &Connection,
    account_name: &str,
    fund_code: &str,
    fund_name: &str,
) -> Result<ResolvedFund, LedgerError> {
    let account_id = ledger::ensure_account(conn, account_name)?;
    let fund_id = ledger::ensure_fund(conn, fund_code, fund_name)?;
    Ok(ResolvedFund {
        account_id,
        fund_id,
    })
}

fn has_cash_account(conn: &Connection, account_id: i64) -> Result<bool, LedgerError> {
    let hit: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM manual_assets WHERE account_id=?1 AND product_type='cash' LIMIT 1",
            [account_id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(hit.is_some())
}

fn release_order_reservation(conn: &Connection, order_id: i64) -> Result<(), LedgerError> {
    let rid: Option<i64> = conn
        .query_row(
            "SELECT reservation_id FROM fund_orders WHERE id=?1",
            [order_id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    if let Some(rid) = rid {
        finance::settle_reservation(conn, rid, "release")
            .map_err(|e| LedgerError::Invalid(e.message()))?;
    }
    Ok(())
}

/// 待确认减仓／转出占用的份额（同账户同基金）。
pub(crate) fn pending_occupied_shares_raw(
    conn: &Connection,
    account_id: i64,
    fund_id: i64,
) -> Result<i64, LedgerError> {
    let occupied: i64 = conn.query_row(
        "SELECT COALESCE(SUM(shares_raw),0) FROM fund_orders
         WHERE account_id=?1 AND fund_id=?2 AND status='pending'
           AND op IN ('sell','convert_out') AND shares_raw IS NOT NULL",
        params![account_id, fund_id],
        |r| r.get(0),
    )?;
    Ok(occupied)
}

// ---------------------------------------------------------------------------
// 订单：创建（加仓／减仓／转换两腿）
// ---------------------------------------------------------------------------

pub(crate) struct OrderDraft {
    pub op: &'static str,
    pub account_name: String,
    pub fund_code: String,
    pub fund_name: String,
    pub applied_date: String,
    pub cutoff: String,
    pub amount_minor: Option<i64>,
    pub fee_minor: Option<i64>,
    pub fee_known: bool,
    pub shares_raw: Option<i64>,
    pub target_code: Option<String>,
    pub target_name: Option<String>,
    pub plan_id: Option<i64>,
    pub period_key: String,
    pub request_id: String,
    pub note: String,
}

fn validate_applied_date(applied_date: &str, today: &str) -> Result<(), LedgerError> {
    if !crate::assets::is_valid_calendar_date(applied_date) || applied_date > today {
        return Err(LedgerError::Invalid(
            "申请日期必须是今天或之前的有效日历日".into(),
        ));
    }
    Ok(())
}

fn insert_order(
    conn: &Connection,
    draft: &OrderDraft,
    account_id: i64,
    fund_id: i64,
    conversion_id: Option<i64>,
    expected_nav: Option<String>,
) -> Result<i64, LedgerError> {
    let now = now_string();
    conn.execute(
        "INSERT INTO fund_orders(op, status, account_id, fund_id, conversion_id, plan_id,
             period_key, applied_date, cutoff, amount_minor, fee_minor, fee_known, shares_raw,
             expected_nav_date, request_id, note, created_at, updated_at)
         VALUES(?1,'pending',?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?16)",
        params![
            draft.op,
            account_id,
            fund_id,
            conversion_id,
            draft.plan_id,
            draft.period_key,
            draft.applied_date,
            draft.cutoff,
            draft.amount_minor,
            draft.fee_minor,
            draft.fee_known as i64,
            draft.shares_raw,
            expected_nav.unwrap_or_default(),
            draft.request_id,
            draft.note,
            now,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 创建订单。buy：金额>0，账户有现金账时建 held 占用（可用资金下降，
/// 实际扣款发生在确认）；sell：份额>0 且不超过可减份额；convert：同事务
/// 建两腿（convert_out + convert_in，conversion_id=转出腿 id）。
pub(crate) fn order_create(
    store: &Store,
    draft: &OrderDraft,
    today: &str,
) -> Result<Value, LedgerError> {
    if draft.request_id.trim().is_empty() {
        return Err(LedgerError::Invalid("requestId 缺失".into()));
    }
    validate_applied_date(&draft.applied_date, today)?;
    store.with_write(|conn| order_create_tx(conn, draft, today))
}

/// 订单创建内层（须在写事务内调用；定投期次登记复用）。
pub(crate) fn order_create_tx(
    conn: &Connection,
    draft: &OrderDraft,
    _today: &str,
) -> Result<Value, LedgerError> {
    if draft.request_id.trim().is_empty() {
        return Err(LedgerError::Invalid("requestId 缺失".into()));
    }
    let expected_nav = expected_nav_date(&draft.applied_date, &draft.cutoff)
        .ok_or_else(|| LedgerError::Invalid("申请日期无效".into()))?;
    if let Some(existing) = retry_order(conn, draft)? {
        return Ok(existing);
    }
    if draft.op == OP_BUY {
        let amount = draft
            .amount_minor
            .ok_or_else(|| LedgerError::Invalid("加仓金额缺失".into()))?;
        if amount <= 0 {
            return Err(LedgerError::Invalid("加仓金额必须大于零".into()));
        }
        if let Some(fee) = draft.fee_minor {
            if fee < 0 || (draft.fee_known && fee >= amount) {
                return Err(LedgerError::Invalid(
                    "手续费非法（不能为负或超过支付总额）".into(),
                ));
            }
        }
        {
            let r = resolve(
                conn,
                &draft.account_name,
                &draft.fund_code,
                &draft.fund_name,
            )?;
            let mut reserved_cash = false;
            let mut reservation_id = None;
            if has_cash_account(conn, r.account_id)? {
                reservation_id = Some(
                    finance::create_reservation(
                        conn,
                        &ReservationInput {
                            account_id: r.account_id,
                            symbol: draft.fund_code.clone(),
                            risk_class: "other".into(),
                            amount_text: Fixed::amount(amount).to_string_value(),
                            expected_date: expected_nav.clone(),
                            note: format!("场外加仓占用 {}", draft.fund_code),
                        },
                    )
                    .map_err(|e| LedgerError::Invalid(e.message()))?,
                );
                reserved_cash = true;
            }
            let id = insert_order(
                conn,
                draft,
                r.account_id,
                r.fund_id,
                None,
                Some(expected_nav.clone()),
            )?;
            if let Some(rid) = reservation_id {
                conn.execute(
                    "UPDATE fund_orders SET reservation_id=?2 WHERE id=?1",
                    params![id, rid],
                )?;
            }
            Ok(json!({"ok":true,"id":id,"expectedNavDate":expected_nav,
                "cashReserved":reserved_cash,"tradingDayVerified":false}))
        }
    } else if draft.op == OP_SELL {
        let shares_raw = draft
            .shares_raw
            .ok_or_else(|| LedgerError::Invalid("减仓份额缺失".into()))?;
        if shares_raw <= 0 {
            return Err(LedgerError::Invalid("减仓份额必须大于零".into()));
        }
        {
            let r = resolve(
                conn,
                &draft.account_name,
                &draft.fund_code,
                &draft.fund_name,
            )?;
            check_sell_occupancy(conn, r.account_id, r.fund_id, shares_raw)?;
            let id = insert_order(
                conn,
                draft,
                r.account_id,
                r.fund_id,
                None,
                Some(expected_nav.clone()),
            )?;
            Ok(json!({"ok":true,"id":id,"expectedNavDate":expected_nav,"tradingDayVerified":false}))
        }
    } else if draft.op == "convert" {
        let shares_raw = draft
            .shares_raw
            .ok_or_else(|| LedgerError::Invalid("转出份额缺失".into()))?;
        if shares_raw <= 0 {
            return Err(LedgerError::Invalid("转出份额必须大于零".into()));
        }
        let target_code = draft
            .target_code
            .clone()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| LedgerError::Invalid("目标基金缺失".into()))?;
        if target_code == draft.fund_code {
            return Err(LedgerError::Invalid(
                "转出与目标基金相同；请选择明确的目标基金身份".into(),
            ));
        }
        {
            let r = resolve(
                conn,
                &draft.account_name,
                &draft.fund_code,
                &draft.fund_name,
            )?;
            check_sell_occupancy(conn, r.account_id, r.fund_id, shares_raw)?;
            let target_id = ledger::ensure_fund(
                conn,
                &target_code,
                draft.target_name.as_deref().unwrap_or(""),
            )?;
            if target_id == r.fund_id {
                return Err(LedgerError::Invalid("目标基金身份与转出相同".into()));
            }
            let mut out_draft = draft_clone_for(draft, OP_CONVERT_OUT);
            out_draft.target_code = None;
            let out_id = insert_order(
                conn,
                &out_draft,
                r.account_id,
                r.fund_id,
                None,
                Some(expected_nav.clone()),
            )?;
            let in_draft = draft_clone_for(draft, OP_CONVERT_IN);
            let in_id = insert_order(
                conn,
                &in_draft,
                r.account_id,
                target_id,
                Some(out_id),
                Some(expected_nav.clone()),
            )?;
            Ok(
                json!({"ok":true,"id":out_id,"inId":in_id,"conversionId":out_id,
                "expectedNavDate":expected_nav,"tradingDayVerified":false}),
            )
        }
    } else {
        Err(LedgerError::Invalid("未知订单类型".into()))
    }
}

// Check the original intent before occupancy: a committed conversion owns its shares.
fn retry_order(conn: &Connection, draft: &OrderDraft) -> Result<Option<Value>, LedgerError> {
    let id: Option<i64> = conn
        .query_row(
            "SELECT id FROM fund_orders WHERE request_id IN (?1,?2)",
            params![
                draft.request_id,
                format!("{}#convert_out", draft.request_id)
            ],
            |r| r.get(0),
        )
        .optional()?;
    let Some(id) = id else {
        return Ok(None);
    };
    let existing = load_order(conn, id)?;
    let identity: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM accounts a, funds f WHERE a.id=?1 AND f.id=?2 AND a.name=?3 AND f.code=?4)",
        params![existing.account_id, existing.fund_id, draft.account_name.trim(), draft.fund_code.trim()], |r| r.get(0),
    )?;
    let expected_op = if draft.op == "convert" {
        OP_CONVERT_OUT
    } else {
        draft.op
    };
    if !identity
        || existing.op != expected_op
        || existing.applied_date != draft.applied_date
        || existing.cutoff != draft.cutoff
        || existing.amount_minor != draft.amount_minor
        || existing.shares_raw != draft.shares_raw
        || existing.fee_minor != draft.fee_minor
        || existing.fee_known != draft.fee_known
        || existing.plan_id != draft.plan_id
        || existing.period_key != draft.period_key
    {
        return Err(LedgerError::Conflict("requestId payload changed".into()));
    }
    if draft.op == "convert" {
        let (in_id, target): (i64, String) = conn.query_row(
            "SELECT o.id,f.code FROM fund_orders o JOIN funds f ON o.fund_id=f.id WHERE o.conversion_id=?1 AND o.op='convert_in'",
            [id], |r| Ok((r.get(0)?,r.get(1)?)),
        )?;
        if draft.target_code.as_deref().map(str::trim) != Some(target.as_str()) {
            return Err(LedgerError::Conflict("requestId target changed".into()));
        }
        return Ok(Some(
            json!({"ok":true,"id":id,"inId":in_id,"conversionId":id,"expectedNavDate":existing.expected_nav_date,"tradingDayVerified":false}),
        ));
    }
    Ok(Some(
        json!({"ok":true,"id":id,"expectedNavDate":existing.expected_nav_date,"cashReserved":existing.reservation_id.is_some(),"tradingDayVerified":false}),
    ))
}

fn draft_clone_for(draft: &OrderDraft, op: &'static str) -> OrderDraft {
    OrderDraft {
        op,
        account_name: draft.account_name.clone(),
        fund_code: draft.fund_code.clone(),
        fund_name: draft.fund_name.clone(),
        applied_date: draft.applied_date.clone(),
        cutoff: draft.cutoff.clone(),
        amount_minor: draft.amount_minor,
        fee_minor: draft.fee_minor,
        fee_known: draft.fee_known,
        shares_raw: draft.shares_raw,
        target_code: draft.target_code.clone(),
        target_name: draft.target_name.clone(),
        plan_id: draft.plan_id,
        period_key: draft.period_key.clone(),
        request_id: format!("{}#{op}", draft.request_id),
        note: draft.note.clone(),
    }
}

fn check_sell_occupancy(
    conn: &Connection,
    account_id: i64,
    fund_id: i64,
    shares_raw: i64,
) -> Result<(), LedgerError> {
    let position = crate::portfolio::replay_position(conn, account_id, fund_id)?
        .ok_or_else(|| LedgerError::Invalid("该账户没有该基金的可减持仓".into()))?;
    let occupied = pending_occupied_shares_raw(conn, account_id, fund_id)?;
    let available = position
        .quantity_raw
        .checked_sub(occupied)
        .ok_or_else(|| LedgerError::Invalid("待确认占用份额溢出".into()))?;
    if shares_raw > available {
        return Err(LedgerError::Conflict(format!(
            "可减份额不足：可用 {} 份，申请 {} 份（待确认占用已扣除）",
            Fixed::share(available).to_string_value(),
            Fixed::share(shares_raw).to_string_value(),
        )));
    }
    Ok(())
}

/// 订单行 JSON（列表与详情共用）。
fn order_json(o: &OrderRow, account: &str, code: &str, name: &str) -> Value {
    json!({
        "id": o.id, "op": o.op, "status": o.status,
        "accountId": o.account_id, "account": account,
        "fundId": o.fund_id, "fundCode": code, "fundName": name,
        "conversionId": o.conversion_id, "planId": o.plan_id, "periodKey": o.period_key,
        "appliedDate": o.applied_date, "cutoff": o.cutoff,
        "amount": o.amount_minor.map(|v| Fixed::amount(v).to_string_value()),
        "fee": o.fee_minor.map(|v| Fixed::amount(v).to_string_value()),
        "feeKnown": o.fee_known,
        "shares": o.shares_raw.map(|v| Fixed::share(v).to_string_value()),
        "navDate": o.nav_date, "nav": o.nav_raw.map(|v| Fixed::nav(v).to_string_value()),
        "expectedNavDate": o.expected_nav_date, "tradingDayVerified": false,
        "confirmedShares": o.confirmed_shares_raw.map(|v| Fixed::share(v).to_string_value()),
        "confirmedAmount": o.confirmed_amount_minor.map(|v| Fixed::amount(v).to_string_value()),
        "confirmedFee": o.confirmed_fee_minor.map(|v| Fixed::amount(v).to_string_value()),
        "confirmedNavDate": o.confirmed_nav_date,
        "confirmedNav": o.confirmed_nav_raw.map(|v| Fixed::nav(v).to_string_value()),
        "confirmedAt": o.confirmed_at,
        "cashArrival": o.cash_arrival,
        "requestId": o.request_id,
        "version": o.version, "note": o.note,
    })
}

pub(crate) struct OrderRow {
    pub id: i64,
    pub op: String,
    pub status: String,
    pub account_id: i64,
    pub fund_id: i64,
    pub conversion_id: Option<i64>,
    pub plan_id: Option<i64>,
    pub period_key: String,
    pub applied_date: String,
    pub cutoff: String,
    pub amount_minor: Option<i64>,
    pub fee_minor: Option<i64>,
    pub fee_known: bool,
    pub shares_raw: Option<i64>,
    pub nav_date: Option<String>,
    pub nav_raw: Option<i64>,
    pub expected_nav_date: String,
    pub confirmed_shares_raw: Option<i64>,
    pub confirmed_amount_minor: Option<i64>,
    pub confirmed_fee_minor: Option<i64>,
    pub confirmed_nav_date: Option<String>,
    pub confirmed_nav_raw: Option<i64>,
    pub confirmed_at: Option<String>,
    pub cash_arrival: Option<String>,
    pub tx_id: Option<i64>,
    pub reservation_id: Option<i64>,
    pub request_id: String,
    pub version: i64,
    pub note: String,
}

const ORDER_COLS: &str = "id, op, status, account_id, fund_id, conversion_id, plan_id, period_key, \
    applied_date, cutoff, amount_minor, fee_minor, fee_known, shares_raw, nav_date, nav_raw, \
    expected_nav_date, confirmed_shares_raw, confirmed_amount_minor, confirmed_fee_minor, \
    confirmed_nav_date, confirmed_nav_raw, confirmed_at, cash_arrival, tx_id, reservation_id, request_id, version, note";

fn load_order(conn: &Connection, id: i64) -> Result<OrderRow, LedgerError> {
    conn.query_row(
        &format!("SELECT {ORDER_COLS} FROM fund_orders WHERE id=?1"),
        [id],
        |r| {
            Ok(OrderRow {
                id: r.get(0)?,
                op: r.get(1)?,
                status: r.get(2)?,
                account_id: r.get(3)?,
                fund_id: r.get(4)?,
                conversion_id: r.get(5)?,
                plan_id: r.get(6)?,
                period_key: r.get(7)?,
                applied_date: r.get(8)?,
                cutoff: r.get(9)?,
                amount_minor: r.get(10)?,
                fee_minor: r.get(11)?,
                fee_known: r.get::<_, i64>(12)? != 0,
                shares_raw: r.get(13)?,
                nav_date: r.get(14)?,
                nav_raw: r.get(15)?,
                expected_nav_date: r.get(16)?,
                confirmed_shares_raw: r.get(17)?,
                confirmed_amount_minor: r.get(18)?,
                confirmed_fee_minor: r.get(19)?,
                confirmed_nav_date: r.get(20)?,
                confirmed_nav_raw: r.get(21)?,
                confirmed_at: r.get(22)?,
                cash_arrival: r.get(23)?,
                tx_id: r.get(24)?,
                reservation_id: r.get(25)?,
                request_id: r.get(26)?,
                version: r.get(27)?,
                note: r.get(28)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| LedgerError::NotFound("订单".into()))
}

// ---------------------------------------------------------------------------
// 订单：确认 / 取消 / 到账 / 列表
// ---------------------------------------------------------------------------

pub(crate) struct OrderConfirmInput {
    pub id: i64,
    pub expected_version: i64,
    pub confirmed_shares_text: Option<String>,
    pub confirmed_amount_text: Option<String>,
    pub fee_text: Option<String>,
    pub fee_known: bool,
    pub confirmed_nav_date: Option<String>,
    pub confirmed_nav_text: Option<String>,
}

/// 确认入账（同一写事务）：按实际确认结果落账（幂等 request_id）、申购
/// 释放现金占用并核销 trade_cash（现金恰扣一次）、赎回挂 receivable 待到账。
/// 缺确认净值时拒绝；份额／金额／费用关系不一致（>2 分）拒绝。
pub(crate) fn order_confirm(
    store: &Store,
    input: &OrderConfirmInput,
    today: &str,
) -> Result<Value, LedgerError> {
    store.with_write(|conn| {
        let order = load_order(conn, input.id)?;
        if order.op == OP_CONVERT_IN {
            let Some(conversion_id) = order.conversion_id else {
                return Err(LedgerError::Invalid("转入腿缺少转换关联".into()));
            };
            let out_status: String = conn.query_row(
                "SELECT status FROM fund_orders WHERE id=?1",
                [conversion_id],
                |r| r.get(0),
            )?;
            if out_status != "confirmed" {
                return Err(LedgerError::Conflict(
                    "转出尚未确认；转换须先完成转出再确认转入".into(),
                ));
            }
        }
        if order.status != "pending" {
            return Err(LedgerError::Conflict(format!(
                "订单已处理（{}），不能重复确认",
                order.status
            )));
        }
        if order.version != input.expected_version {
            return Err(LedgerError::Conflict(
                "订单状态已变化，请刷新后重新核对".into(),
            ));
        }
        let nav_raw = parse_fixed_param(input.confirmed_nav_text.as_deref(), 4, "确认净值")?
            .ok_or_else(|| {
                LedgerError::Invalid("缺少实际确认净值；请按确认单填写或先取消".into())
            })?;
        let nav_date = input
            .confirmed_nav_date
            .clone()
            .filter(|d| !d.is_empty())
            .ok_or_else(|| LedgerError::Invalid("缺少实际确认净值日期".into()))?;
        if !crate::assets::is_valid_calendar_date(&nav_date) || nav_date.as_str() > today {
            return Err(LedgerError::Invalid("确认净值日期无效".into()));
        }
        let fee_minor = parse_fixed_param(input.fee_text.as_deref(), 2, "确认费用")?;
        let confirmed_amount =
            parse_fixed_param(input.confirmed_amount_text.as_deref(), 2, "确认金额")?;
        let (code, name): (String, String) = conn.query_row(
            "SELECT code, name FROM funds WHERE id=?1",
            [order.fund_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let account: String = conn.query_row(
            "SELECT name FROM accounts WHERE id=?1",
            [order.account_id],
            |r| r.get(0),
        )?;
        let is_out = order.op == OP_BUY || order.op == OP_CONVERT_IN;
        let shares_raw = match &input.confirmed_shares_text {
            Some(text) => Some(
                Fixed::parse(text, SCALE_SHARE)
                    .map_err(|_| LedgerError::Invalid("确认份额精度非法".into()))?
                    .raw(),
            ),
            None => None,
        };
        let (shares_raw, net_minor, fee_minor, fee_known) = if is_out {
            // 申购／转入：支付总额 = 净申购 + 费用；份额×净值 ≈ 净申购。
            let total = confirmed_amount
                .or(order.amount_minor)
                .ok_or_else(|| LedgerError::Invalid("缺少确认金额（支付总额）".into()))?;
            let (shares_raw, net_minor, fee_minor, fee_known) = match (shares_raw, fee_minor) {
                (Some(shares), Some(fee)) => {
                    let net = shares_to_amount_minor(shares, nav_raw)
                        .ok_or_else(|| LedgerError::Invalid("份额或净值溢出".into()))?;
                    if (net - (total - fee)).abs() > 2 {
                        return Err(LedgerError::Invalid(
                            "份额×净值与（支付总额−费用）相差超过 2 分；请核对确认单".into(),
                        ));
                    }
                    (shares, total - fee, fee, true)
                }
                (Some(shares), None) => {
                    let net = shares_to_amount_minor(shares, nav_raw)
                        .ok_or_else(|| LedgerError::Invalid("份额或净值溢出".into()))?;
                    let fee = total - net;
                    if fee < 0 {
                        return Err(LedgerError::Invalid(
                            "支付总额小于份额×净值；请核对确认净值与份额".into(),
                        ));
                    }
                    (shares, net, fee, false)
                }
                (None, Some(fee)) => {
                    let net = total - fee;
                    let shares = crate::otc_holding::derive_shares_raw(net, nav_raw)
                        .ok_or_else(|| LedgerError::Invalid("份额推算溢出".into()))?;
                    (shares, net, fee, true)
                }
                (None, None) => {
                    return Err(LedgerError::Invalid(
                        "确认份额与费用都缺失：无法确定净申购与费用关系".into(),
                    ));
                }
            };
            (shares_raw, net_minor, fee_minor, fee_known)
        } else {
            // 赎回／转出：确认金额=实际到账金额（可空→按份额×净值推算）；
            // 费用未知按 0 入账并在订单与列表中保留 feeKnown=false 口径。
            let shares_raw = shares_raw
                .or(order.shares_raw)
                .ok_or_else(|| LedgerError::Invalid("缺少确认份额".into()))?;
            let gross = shares_to_amount_minor(shares_raw, nav_raw)
                .ok_or_else(|| LedgerError::Invalid("份额或净值溢出".into()))?;
            let fee = fee_minor.unwrap_or(0);
            let amount = confirmed_amount.unwrap_or(gross - fee);
            (shares_raw, amount, fee, input.fee_known)
        };
        if shares_raw <= 0 {
            return Err(LedgerError::Invalid("确认份额必须大于零".into()));
        }
        let tx_type = if is_out { "BUY" } else { "SELL" };
        let tx = TransactionInput {
            account_name: account,
            fund_code: code,
            fund_name: name,
            tx_type: tx_type.to_string(),
            state: "confirmed".to_string(),
            quantity: Fixed::share(shares_raw),
            price: Fixed::nav(nav_raw),
            fee: Fixed::amount(fee_minor),
            amount: Some(Fixed::amount(net_minor)),
            trade_date: nav_date.clone(),
            source: if order.op == OP_CONVERT_OUT || order.op == OP_CONVERT_IN {
                "otc_convert".to_string()
            } else {
                "otc".to_string()
            },
            request_id: Some(format!("{}#c", order.id)),
            external_id: None,
            batch_id: None,
            line_no: None,
            payload_hash: None,
        };
        let (tx_id, _created) = ledger::record_transaction_tx(conn, &tx)?;
        // 申购：释放占用并把本笔资金核销为已扣款（现金恰扣一次）。
        if order.op == OP_BUY {
            release_order_reservation(conn, order.id)?;
            let entry: Option<i64> = conn
                .query_row(
                    "SELECT tx_id FROM trade_cash WHERE tx_id=?1 AND status!='settled'",
                    [tx_id],
                    |r| r.get(0),
                )
                .optional()?;
            if entry.is_some() {
                crate::trade_cash::settle(conn, tx_id, "post", &nav_date, "场外加仓确认扣款")
                    .map_err(|e| LedgerError::Invalid(e.message()))?;
            }
        }
        let now = now_string();
        let updated = conn.execute(
            "UPDATE fund_orders SET status='confirmed', confirmed_shares_raw=?3,
                 confirmed_amount_minor=?4, confirmed_fee_minor=?5, confirmed_nav_date=?6,
                 confirmed_nav_raw=?7, confirmed_at=?8, tx_id=?9, fee_known=?10, version=version+1,
                 updated_at=?8 WHERE id=?1 AND version=?2",
            params![
                order.id,
                input.expected_version,
                shares_raw,
                net_minor,
                fee_minor,
                nav_date,
                nav_raw,
                now,
                tx_id,
                fee_known as i64,
            ],
        )?;
        // 版本守卫必须真实生效：0 行说明并发已改，整个事务回滚（含落账）。
        if updated != 1 {
            return Err(LedgerError::Conflict(
                "订单状态已变化，确认未生效；请刷新后重新核对".into(),
            ));
        }
        Ok(json!({"ok":true,"txId":tx_id}))
    })
}

/// 取消／失败：仅 pending；释放资金占用；记录保留。
pub(crate) fn order_cancel(
    store: &Store,
    id: i64,
    expected_version: i64,
    outcome: &str,
    reason: &str,
) -> Result<Value, LedgerError> {
    if !["cancelled", "failed"].contains(&outcome) {
        return Err(LedgerError::Invalid(
            "取消结果只能是 cancelled 或 failed".into(),
        ));
    }
    store.with_write(|conn| {
        let order = load_order(conn, id)?;
        if order.status != "pending" {
            return Err(LedgerError::Conflict("只有待确认订单可以取消或标记失败".into()));
        }
        if order.version != expected_version {
            return Err(LedgerError::Conflict("订单状态已变化，请刷新后重新核对".into()));
        }
        release_order_reservation(conn, order.id)?;
        let mut note = order.note.clone();
        if !reason.is_empty() {
            note = if note.is_empty() {
                reason.to_string()
            } else {
                format!("{note}；{reason}")
            };
        }
        let updated = conn.execute(
            "UPDATE fund_orders SET status=?3, note=?4, version=version+1, updated_at=?5 WHERE id=?1 AND version=?2",
            params![id, expected_version, outcome, note, now_string()],
        )?;
        if updated != 1 {
            return Err(LedgerError::Conflict(
                "订单状态已变化，取消未生效；请刷新后重新核对".into(),
            ));
        }
        Ok(json!({"ok":true}))
    })
}

/// 赎回／转出到账：复用 trade_cash::settle（只入现金一次；重复到账 Conflict）。
pub(crate) fn order_arrival(
    store: &Store,
    id: i64,
    date: &str,
    mode: &str,
    note: &str,
) -> Result<Value, LedgerError> {
    store.with_write(|conn| {
        let order = load_order(conn, id)?;
        // 只有真实现金赎回（sell）有「到账」概念；转换转出腿的资金直接
        // 过渡为转入份额（otc_convert 不生成 trade_cash），登记到账反而
        // 会让资金凭空进现金。
        if order.status != "confirmed" || order.op != OP_SELL {
            return Err(LedgerError::Invalid(
                "只有已确认的减仓赎回可以登记到账；转换资金随转入腿确认过渡".into(),
            ));
        }
        if order.cash_arrival.is_some() {
            return Err(LedgerError::Conflict("该订单已登记到账".into()));
        }
        let tx_id = order
            .tx_id
            .ok_or_else(|| LedgerError::Invalid("订单缺少落账回执".into()))?;
        crate::trade_cash::settle(conn, tx_id, mode, date, note)
            .map_err(|e| LedgerError::Invalid(e.message()))?;
        conn.execute(
            "UPDATE fund_orders SET cash_arrival=?2, version=version+1, updated_at=?3 WHERE id=?1",
            params![id, date, now_string()],
        )?;
        Ok(json!({"ok":true}))
    })
}

/// 订单列表（账户＋标的过滤）与占用派生。
pub(crate) fn orders_list(
    store: &Store,
    account_name: &str,
    fund_code: &str,
) -> Result<Value, LedgerError> {
    store.with_read(|conn| {
        let account_id = ledger::ensure_account(conn, account_name)?;
        let fund_id = fund_id_by_code(conn, fund_code)?
            .ok_or_else(|| LedgerError::NotFound("标的".into()))?;
        let columns = ORDER_COLS.split(',').map(|column| format!("o.{}", column.trim())).collect::<Vec<_>>();
        let metadata = columns.len();
        let mut stmt = conn.prepare(&format!(
            "SELECT {}, f.code, f.name, a.name FROM fund_orders o
             JOIN funds f ON f.id=o.fund_id JOIN accounts a ON a.id=o.account_id
             WHERE o.account_id=?1 AND (o.fund_id=?2 OR COALESCE(o.conversion_id,o.id) IN
                 (SELECT COALESCE(conversion_id,id) FROM fund_orders WHERE account_id=?1 AND fund_id=?2 AND op IN ('convert_out','convert_in')))
             ORDER BY o.id DESC LIMIT 200", columns.join(", ")
        ))?;
        let orders = stmt.query_map(params![account_id, fund_id], |row| {
            Ok(order_json(&map_order_row(row)?, &row.get::<_, String>(metadata + 2)?,
                &row.get::<_, String>(metadata)?, &row.get::<_, String>(metadata + 1)?))
        })?.collect::<Result<Vec<_>, _>>()?;
        let position = crate::portfolio::replay_position(conn, account_id, fund_id)?;
        let occupied = pending_occupied_shares_raw(conn, account_id, fund_id)?;
        let available = position.map(|p| p.quantity_raw - occupied).unwrap_or(0);
        let pending_buy: i64 = conn.query_row(
            "SELECT COALESCE(SUM(amount_minor),0) FROM fund_orders
             WHERE account_id=?1 AND fund_id=?2 AND status='pending' AND op=?3",
            params![account_id, fund_id, OP_BUY],
            |r| r.get(0),
        )?;
        let in_transit: i64 = conn.query_row(
            "SELECT COALESCE(SUM(confirmed_amount_minor),0) FROM fund_orders
             WHERE account_id=?1 AND fund_id=?2 AND status='confirmed' AND cash_arrival IS NULL
               AND op IN ('sell','convert_out')",
            params![account_id, fund_id],
            |r| r.get(0),
        )?;
        Ok(json!({
            "orders": orders,
            "availableShares": Fixed::share(available).to_string_value(),
            "pendingBuyMinor": pending_buy,
            "inTransitMinor": in_transit,
        }))
    })
}

fn map_order_row(r: &rusqlite::Row) -> rusqlite::Result<OrderRow> {
    Ok(OrderRow {
        id: r.get(0)?,
        op: r.get(1)?,
        status: r.get(2)?,
        account_id: r.get(3)?,
        fund_id: r.get(4)?,
        conversion_id: r.get(5)?,
        plan_id: r.get(6)?,
        period_key: r.get(7)?,
        applied_date: r.get(8)?,
        cutoff: r.get(9)?,
        amount_minor: r.get(10)?,
        fee_minor: r.get(11)?,
        fee_known: r.get::<_, i64>(12)? != 0,
        shares_raw: r.get(13)?,
        nav_date: r.get(14)?,
        nav_raw: r.get(15)?,
        expected_nav_date: r.get(16)?,
        confirmed_shares_raw: r.get(17)?,
        confirmed_amount_minor: r.get(18)?,
        confirmed_fee_minor: r.get(19)?,
        confirmed_nav_date: r.get(20)?,
        confirmed_nav_raw: r.get(21)?,
        confirmed_at: r.get(22)?,
        cash_arrival: r.get(23)?,
        tx_id: r.get(24)?,
        reservation_id: r.get(25)?,
        request_id: r.get(26)?,
        version: r.get(27)?,
        note: r.get(28)?,
    })
}

fn parse_fixed_param(
    text: Option<&str>,
    scale: u32,
    label: &str,
) -> Result<Option<i64>, LedgerError> {
    match text {
        None => Ok(None),
        Some(s) if s.trim().is_empty() => Ok(None),
        Some(s) => Fixed::parse(s, scale)
            .map(|v| Some(v.raw()))
            .map_err(|_| LedgerError::Invalid(format!("{label} 精度非法"))),
    }
}

#[cfg(test)]
#[path = "otc_ops_tests.rs"]
mod tests;
