//! 场外持仓更正：期初或校准与稳定回执共用一个事务。
use super::{fund_id_by_code, holding_amounts, nav_as_of, NavPick};
use crate::{fixed::Fixed, ledger::LedgerError, otc_ops::now_string, storage::Store};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

pub(crate) fn holding_edit(
    store: &Store,
    mode: &str,
    account_name: &str,
    fund_code: &str,
    value_minor: i64,
    gain_minor: i64,
    shares_raw: Option<i64>,
    as_of: &str,
    reason: &str,
    request_id: &str,
    accept_earlier_nav: bool,
    expected_opening_version: Option<i64>,
    today: &str,
) -> Result<Value, LedgerError> {
    if reason.trim().is_empty() || request_id.trim().is_empty() || request_id.len() > 128 {
        return Err(LedgerError::Invalid(
            "更正原因必填；requestId 长度须为 1..=128".into(),
        ));
    }
    if !matches!(mode, "opening" | "calibration") {
        return Err(LedgerError::Invalid(format!("未知编辑模式 {mode}")));
    }
    let input = json!({"mode":mode,"account":account_name.trim(),"code":fund_code,
        "value":value_minor,"gain":gain_minor,"shares":shares_raw,"asOf":as_of,
        "reason":reason.trim(),"acceptEarlierNav":accept_earlier_nav,"version":expected_opening_version});
    let key = format!("otc_holding_edit:{request_id}");
    store.with_write(|conn| {
        let receipt: Option<String> = conn
            .query_row("SELECT value FROM app_meta WHERE key=?1", [&key], |row| {
                row.get(0)
            })
            .optional()?;
        if let Some(receipt) = receipt {
            let receipt: Value = serde_json::from_str(&receipt)
                .map_err(|_| LedgerError::Db("持仓更正回执损坏".into()))?;
            if receipt["input"] != input {
                return Err(LedgerError::Conflict(request_id.into()));
            }
            return Ok(receipt["result"].clone());
        }
        if !crate::assets::is_valid_calendar_date(as_of) || as_of > today {
            return Err(LedgerError::Invalid("更正截至日期无效或晚于今天".into()));
        }
        let account: i64 = conn
            .query_row(
                "SELECT id FROM accounts WHERE name=?1 AND archived_at IS NULL",
                [account_name.trim()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| LedgerError::NotFound("活动账户".into()))?;
        let fund = fund_id_by_code(conn, fund_code)?
            .ok_or_else(|| LedgerError::NotFound("标的".into()))?;
        let nav = nav_as_of(conn, fund, as_of)?.map(|(nav_raw, nav_date)| NavPick {
            exact: nav_date == as_of,
            nav_raw,
            nav_date,
        });
        if let Some(pick) = &nav {
            if !pick.exact && !accept_earlier_nav {
                return Err(LedgerError::Invalid(format!(
                    "{} 无当日净值，最近净值为 {}；请同步后重试、确认改用该净值或填写实际份额",
                    as_of, pick.nav_date
                )));
            }
        }
        let (cost, shares, derived, nav_raw, nav_date) =
            holding_amounts(value_minor, gain_minor, shares_raw, nav)?;
        let shares = shares
            .filter(|shares| *shares > 0)
            .ok_or_else(|| LedgerError::Invalid("更正份额须为正".into()))?;
        let result = if mode == "opening" {
            let version = expected_opening_version
                .ok_or_else(|| LedgerError::Invalid("缺少期初版本号".into()))?;
            let change = json!({"quantity":Fixed::share(shares).to_string_value(),
                "totalCost":Fixed::amount(cost).to_string_value(),"asOf":as_of});
            crate::opening_ops::change_tx(conn, account, fund, version, "update", &change)?;
            conn.execute(
                "UPDATE opening_snapshots SET derived_cost=?3,nav_raw=?4,nav_date=?5
                WHERE account_id=?1 AND fund_id=?2",
                params![account, fund, derived as i64, nav_raw, nav_date],
            )?;
            json!({"ok":true,"mode":"opening"})
        } else {
            calibrate(
                conn,
                account,
                fund,
                shares,
                cost,
                nav_raw,
                nav_date.as_deref(),
                value_minor,
                gain_minor,
                as_of,
                reason.trim(),
                request_id,
                derived,
            )?
        };
        conn.execute(
            "INSERT INTO app_meta(key,value) VALUES(?1,?2)",
            params![key, json!({"input":input,"result":result}).to_string()],
        )?;
        Ok(result)
    })
}

fn calibrate(
    conn: &Connection,
    account: i64,
    fund: i64,
    shares: i64,
    cost: i64,
    nav_raw: Option<i64>,
    nav_date: Option<&str>,
    value: i64,
    gain: i64,
    as_of: &str,
    reason: &str,
    request_id: &str,
    derived: bool,
) -> Result<Value, LedgerError> {
    let duplicate: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM position_calibrations WHERE request_id=?1)",
        [request_id],
        |row| row.get(0),
    )?;
    // 旧版本只有校准记录，没有输入回执，不能猜测是否同一次请求。
    if duplicate {
        return Err(LedgerError::Conflict(
            "该编辑已提交（requestId 重复）".into(),
        ));
    }
    let before = crate::portfolio::replay_position(conn, account, fund)?
        .ok_or_else(|| LedgerError::Invalid("该账户与标的没有可校准的持仓".into()))?;
    if as_of < before.as_of.as_str() {
        return Err(LedgerError::Invalid(
            "校准截至日期早于最近持仓变动；请核对日期".into(),
        ));
    }
    let delta_shares = shares
        .checked_sub(before.quantity_raw)
        .ok_or_else(|| LedgerError::Invalid("份额差异溢出".into()))?;
    let delta_cost = cost
        .checked_sub(before.cost_raw)
        .ok_or_else(|| LedgerError::Invalid("成本差异溢出".into()))?;
    conn.execute(
        "INSERT INTO position_calibrations(account_id,fund_id,as_of_date,
        quantity_raw,cost_minor,nav_raw,nav_date,value_minor,gain_minor,reason,
        delta_quantity_raw,delta_cost_minor,request_id,created_at)
        VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
        params![
            account,
            fund,
            as_of,
            shares,
            cost,
            nav_raw,
            nav_date,
            value,
            gain,
            reason,
            delta_shares,
            delta_cost,
            request_id,
            now_string()
        ],
    )?;
    crate::portfolio::recompute_position(conn, account, fund)?;
    Ok(json!({"ok":true,"mode":"calibration",
        "deltaShares":Fixed::share(delta_shares.abs()).to_string_value(),
        "deltaCost":Fixed::amount(delta_cost.abs()).to_string_value(),"sharesDerived":derived}))
}
