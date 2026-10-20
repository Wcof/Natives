//! 场外基金持仓登记与更正（N09）：净值推导、期初登记留痕、截至日期
//! 校准锚点。份额／成本／收益权威仍是 portfolio::replay_position；
//! 本文件只做输入推导与审计留痕，不新增持仓计算权威。

use crate::fixed::Fixed;
use crate::ledger::{self, LedgerError};
use crate::storage::Store;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

/// asOf 当日或之前最近一期净值；绝不使用晚于 asOf 的净值反推。
pub(crate) fn nav_as_of(
    conn: &Connection,
    fund_id: i64,
    as_of: &str,
) -> Result<Option<(i64, String)>, LedgerError> {
    let row: Option<(i64, String)> = conn
        .query_row(
            "SELECT unit_nav_raw, nav_date FROM fund_nav
             WHERE fund_id=?1 AND nav_date<=?2 ORDER BY nav_date DESC LIMIT 1",
            params![fund_id, as_of],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(row)
}

/// 份额推算（4 位定点，半进位）：shares_raw = round_half_up(value_minor×1e6 / nav_raw)。
pub(crate) fn derive_shares_raw(value_minor: i64, nav_raw: i64) -> Option<i64> {
    if value_minor < 0 || nav_raw <= 0 {
        return None;
    }
    let num = (value_minor as i128) * 1_000_000;
    let shares = (num * 2 + nav_raw as i128) / (2 * nav_raw as i128);
    i64::try_from(shares).ok()
}

// ---------------------------------------------------------------------------
// 新增／编辑持仓：净值推导与校准预览
// ---------------------------------------------------------------------------

pub(crate) fn fund_id_by_code(conn: &Connection, code: &str) -> Result<Option<i64>, LedgerError> {
    let row: Option<i64> = conn
        .query_row("SELECT id FROM funds WHERE code=?1", [code], |r| r.get(0))
        .optional()?;
    Ok(row)
}

pub(crate) struct NavPick {
    pub nav_raw: i64,
    pub nav_date: String,
    pub exact: bool,
}

/// 新增持仓推导：成本＝金额−收益；份额＝金额÷同日净值（或用户确认的
/// 最近早期净值／实际份额）。负成本、非法日期拒绝；无可用净值且无实际
/// 份额时不产生份额。
pub(crate) fn holding_amounts(
    value_minor: i64,
    gain_minor: i64,
    shares_raw: Option<i64>,
    nav: Option<NavPick>,
) -> Result<(i64, Option<i64>, bool, Option<i64>, Option<String>), LedgerError> {
    if value_minor <= 0 {
        return Err(LedgerError::Invalid("持有金额必须大于零".into()));
    }
    let cost_minor = value_minor
        .checked_sub(gain_minor)
        .ok_or_else(|| LedgerError::Invalid("持有金额或收益溢出".into()))?;
    if cost_minor < 0 {
        return Err(LedgerError::Invalid(
            "持有收益大于持有金额会导致负成本；请核对收益口径（渠道累计盈亏≠当前浮动收益）".into(),
        ));
    }
    match (nav, shares_raw) {
        (Some(pick), shares) => {
            let derived = derive_shares_raw(value_minor, pick.nav_raw);
            let shares_raw = shares.or(derived);
            Ok((
                cost_minor,
                shares_raw,
                shares.is_none(),
                Some(pick.nav_raw),
                Some(pick.nav_date),
            ))
        }
        (None, Some(shares)) => Ok((cost_minor, Some(shares), false, None, None)),
        (None, None) => Err(LedgerError::Invalid(
            "该日期无可用净值且未填写实际份额；不生成虚假份额，请重试同步净值或填写实际份额".into(),
        )),
    }
}

fn calibration_before(
    conn: &Connection,
    account_id: i64,
    fund_id: i64,
    as_of: &str,
) -> Result<Option<crate::portfolio::Position>, LedgerError> {
    // 校准前的“当前回放值”：完整回放后按锚点顺序重放到 as_of。
    // 这里直接复用 replay_position 的完整结果（校准锚点本身参与回放），
    // 与已有锚点的叠加语义见 portfolio::replay_position。
    let position = crate::portfolio::replay_position(conn, account_id, fund_id)?
        .ok_or_else(|| LedgerError::Invalid("该账户与标的没有可校准的持仓".into()))?;
    let _ = as_of;
    Ok(Some(position))
}

/// 编辑预览：仅期初 → mode='opening'（走期初更正）；有后续交易 →
/// mode='calibration'，回传校准前后的份额／成本与差异。
pub(crate) fn holding_edit_preview(
    store: &Store,
    account_name: &str,
    fund_code: &str,
    value_minor: i64,
    gain_minor: i64,
    shares_raw: Option<i64>,
    as_of: &str,
) -> Result<Value, LedgerError> {
    if account_name.trim().is_empty() || account_name.chars().count() > 64 {
        return Err(LedgerError::Invalid("账户名不能为空或超过 64 字".into()));
    }
    store.with_read(|conn| {
        // 新增持仓的标的可能尚未建档：无 fund 行时仅做推导（mode=new）。
        let fund_id = fund_id_by_code(conn, fund_code)?;
        let Some(fund_id) = fund_id else {
            let nav_absent: Option<NavPick> = None;
            let (cost_minor, shares_raw, shares_derived, nav_raw, nav_date) =
                holding_amounts(value_minor, gain_minor, shares_raw, nav_absent)?;
            return Ok(json!({
                "mode": "new",
                "value": Fixed::amount(value_minor).to_string_value(),
                "gain": Fixed::amount(gain_minor).to_string_value(),
                "cost": Fixed::amount(cost_minor).to_string_value(),
                "shares": shares_raw.map(|v| Fixed::share(v).to_string_value()),
                "sharesDerived": shares_derived,
                "navRaw": nav_raw.map(|v| Fixed::nav(v).to_string_value()),
                "navDate": nav_date,
                "navIsExact": false,
                "tradingDayVerified": false,
            }));
        };
        let account_id: Option<i64> = conn.query_row(
            "SELECT id FROM accounts WHERE name=?1 AND archived_at IS NULL",
            [account_name.trim()], |row| row.get(0),
        ).optional()?;
        let opening: Option<(i64, Option<i64>, String, i64)> = conn
            .query_row(
                "SELECT quantity_raw, cost_raw, as_of, version FROM opening_snapshots
                 WHERE account_id=?1 AND fund_id=?2 AND deleted_at IS NULL",
                params![account_id, fund_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        let tx_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM transactions WHERE account_id=?1 AND fund_id=?2 AND deleted_at IS NULL",
            params![account_id, fund_id],
            |r| r.get(0),
        )?;
        let nav = nav_as_of(conn, fund_id, as_of)?;
        let (cost_minor, shares_raw, shares_derived, nav_raw, nav_date) =
            holding_amounts(value_minor, gain_minor, shares_raw, nav.map(|(raw, date)| NavPick {
                nav_raw: raw,
                nav_date: date.clone(),
                exact: date == as_of,
            }))?;
        let mode = if tx_count > 0 { "calibration" } else if opening.is_some() { "opening" } else { "new" };
        let mut out = json!({
            "mode": mode,
            "value": Fixed::amount(value_minor).to_string_value(),
            "gain": Fixed::amount(gain_minor).to_string_value(),
            "cost": Fixed::amount(cost_minor).to_string_value(),
            "shares": shares_raw.map(|v| Fixed::share(v).to_string_value()),
            "sharesDerived": shares_derived,
            "navRaw": nav_raw.map(|v| Fixed::nav(v).to_string_value()),
            "navDate": nav_date,
            "navIsExact": nav_date.as_ref().map(|d| d == &as_of).unwrap_or(false),
            "tradingDayVerified": false,
        });
        if mode != "calibration" {
            out["opening"] = opening
                .map(|(qty, cost, date, version)| {
                    json!({
                        "quantity": Fixed::share(qty).to_string_value(),
                        "totalCost": cost.map(|v| Fixed::amount(v).to_string_value()),
                        "asOf": date,
                        "version": version,
                    })
                })
                .unwrap_or(Value::Null);
        } else {
            let account_id = account_id.ok_or_else(|| LedgerError::Invalid("缺少持仓账户".into()))?;
            let before = calibration_before(conn, account_id, fund_id, as_of)?;
            let before = before.ok_or_else(|| LedgerError::Invalid("该账户与标的没有可校准的持仓".into()))?;
            out["before"] = json!({
                "quantity": Fixed::share(before.quantity_raw).to_string_value(),
                "cost": Fixed::amount(before.cost_raw).to_string_value(),
                "asOf": before.as_of,
            });
            out["after"] = json!({
                "quantity": shares_raw.map(|v| Fixed::share(v).to_string_value()),
                "cost": Fixed::amount(cost_minor).to_string_value(),
                "asOf": as_of,
            });
            out["opening"] = opening
                .map(|(qty, cost, date, version)| {
                    json!({
                        "quantity": Fixed::share(qty).to_string_value(),
                        "totalCost": cost.map(|v| Fixed::amount(v).to_string_value()),
                        "asOf": date,
                        "version": version,
                    })
                })
                .unwrap_or(Value::Null);
        }
        Ok(out)
    })
}

/// 新增持仓提交：经 record_opening_tx 登记期初（不扣现金、不造交易），
/// 同事务补记净值推导留痕（derived_cost、nav_raw、nav_date）。
pub(crate) fn holding_create(
    store: &Store,
    account_name: &str,
    fund_code: &str,
    fund_name: &str,
    value_minor: i64,
    gain_minor: i64,
    shares_raw: Option<i64>,
    as_of: &str,
    accept_earlier_nav: bool,
    request_id: &str,
    today: &str,
) -> Result<Value, LedgerError> {
    if request_id.trim().is_empty() || request_id.len() > 128 {
        return Err(LedgerError::Invalid("requestId 长度须为 1..=128".into()));
    }
    let receipt_input = json!({"account":account_name.trim(),"code":fund_code,"name":fund_name.trim(),
        "value":value_minor,"gain":gain_minor,"shares":shares_raw,"asOf":as_of,
        "acceptEarlierNav":accept_earlier_nav});
    let receipt_key = format!("otc_holding_create:{request_id}");
    store.with_write(|conn| {
        let receipt: Option<String> = conn
            .query_row(
                "SELECT value FROM app_meta WHERE key=?1",
                [&receipt_key],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(receipt) = receipt {
            let receipt: Value = serde_json::from_str(&receipt)
                .map_err(|_| LedgerError::Db("持仓登记回执损坏".into()))?;
            if receipt["input"] != receipt_input {
                return Err(LedgerError::Conflict(request_id.into()));
            }
            return Ok(receipt["result"].clone());
        }
        let account_id = ledger::ensure_account(conn, account_name)?;
        let fund_id = ledger::ensure_fund(conn, fund_code, fund_name)?;
        let pick = nav_as_of(conn, fund_id, as_of)?.map(|(raw, date)| {
            let exact = date == as_of;
            NavPick {
                nav_raw: raw,
                nav_date: date,
                exact,
            }
        });
        if let Some(pick) = &pick {
            if !pick.exact && !accept_earlier_nav {
                return Err(LedgerError::Invalid(format!(
                    "{} 无当日净值，最近净值为 {}；请同步后重试、确认改用该净值或填写实际份额",
                    as_of, pick.nav_date
                )));
            }
        }
        let (cost_minor, shares_raw, shares_derived, nav_raw, nav_date) =
            holding_amounts(value_minor, gain_minor, shares_raw, pick)?;
        let Some(shares_raw) = shares_raw else {
            return Err(LedgerError::Invalid("份额缺失：无法登记持仓".into()));
        };
        if shares_raw <= 0 {
            return Err(LedgerError::Invalid(
                "推算份额为零或为负；请核对金额与净值日期".into(),
            ));
        }
        let input = ledger::OpeningInput {
            account_name: account_name.to_string(),
            fund_code: fund_code.to_string(),
            fund_name: fund_name.to_string(),
            quantity: Fixed::share(shares_raw),
            as_of: as_of.to_string(),
            total_cost: Some(Fixed::amount(cost_minor)),
        };
        let fund_id = ledger::record_opening_tx(conn, &input, today)?;
        conn.execute(
            "UPDATE opening_snapshots SET derived_cost=?3, nav_raw=?4, nav_date=?5
             WHERE account_id=?1 AND fund_id=?2",
            params![
                account_id,
                fund_id,
                shares_derived as i64,
                nav_raw,
                nav_date
            ],
        )?;
        let result = json!({"ok":true,"fundId":fund_id,"sharesDerived":shares_derived});
        conn.execute(
            "INSERT INTO app_meta(key,value) VALUES(?1,?2)",
            params![
                receipt_key,
                json!({"input":receipt_input,"result":result}).to_string()
            ],
        )?;
        Ok(result)
    })
}

#[path = "otc_holding_edit.rs"]
mod edit;
pub(crate) use edit::holding_edit;
