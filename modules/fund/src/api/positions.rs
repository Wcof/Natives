//! HTTP API 业务请求处理（拆分自 api.rs，仅移动，无逻辑变更）。

use super::{api_error, json_str, query_param};
use crate::fixed::{Fixed, SCALE_SHARE};
use crate::ledger;
use crate::portfolio;
use crate::storage::Store;
use rusqlite::OptionalExtension;

pub(crate) fn api_positions(store: &Store) -> Result<String, (u16, String)> {
    let rows = store
        .with_read(portfolio::list_positions)
        .map_err(|e| api_error(500, "PORTFOLIO_DB", &e.to_string()))?;
    let items: Vec<String> = rows
        .iter()
        .map(|p| {
            let quantity = Fixed::nav(p.quantity_raw);
            let cost = Fixed::amount(p.cost_raw);
            format!(
                "{{\"account\":{},\"fundCode\":{},\"fundName\":{},\"quantity\":{},\"cost\":{},\"realized\":{},\"asOf\":{}}}",
                json_str(&p.account_name),
                json_str(&p.fund_code),
                json_str(&p.fund_name),
                json_str(&quantity.to_string_value()),
                json_str(&cost.to_string_value()),
                json_str(&Fixed::amount(p.realized_raw).to_string_value()),
                json_str(&p.as_of),
            )
        })
        .collect();
    Ok(format!("{{\"positions\":[{}]}}", items.join(",")))
}

pub(crate) fn api_transactions(store: &Store) -> Result<String, (u16, String)> {
    let rows = ledger::list_transactions(store, 200)
        .map_err(|e| api_error(500, e.code(), &e.message()))?;
    let items: Vec<String> = rows
        .iter()
        .map(|t| {
            format!(
                "{{\"id\":{},\"account\":{},\"fundCode\":{},\"type\":{},\"state\":{},\"quantity\":{},\"price\":{},\"amount\":{},\"fee\":{},\"tradeDate\":{},\"source\":{}}}",
                t.id,
                json_str(&t.account_name),
                json_str(&t.fund_code),
                json_str(&t.tx_type),
                json_str(&t.state),
                json_str(&Fixed::nav(t.quantity_raw).to_string_value()),
                json_str(&Fixed::nav(t.price_raw).to_string_value()),
                json_str(&Fixed::amount(t.amount_raw).to_string_value()),
                json_str(&Fixed::amount(t.fee_raw).to_string_value()),
                json_str(&t.trade_date),
                json_str(&t.source),
            )
        })
        .collect();
    Ok(format!("{{\"transactions\":[{}]}}", items.join(",")))
}

pub(crate) fn api_record_transaction(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| api_error(400, "LEDGER_INVALID", &e.to_string()))?;
    let get_str = |key: &str| -> Result<String, (u16, String)> {
        value
            .get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| api_error(400, "LEDGER_INVALID", &format!("{key} 缺失")))
    };
    let parse_fixed = |key: &str, scale: u32| -> Result<Fixed, (u16, String)> {
        Fixed::parse(&get_str(key)?, scale)
            .map_err(|_| api_error(400, "LEDGER_INVALID", &format!("{key} 精度非法")))
    };
    let fee_fixed = value
        .get("fee")
        .and_then(|v| v.as_str())
        .map(|s| Fixed::parse(s, 2))
        .transpose()
        .map_err(|_| api_error(400, "LEDGER_INVALID", "fee 精度非法"))?
        .unwrap_or_else(|| Fixed::zero(2));

    let input = ledger::TransactionInput::manual(
        get_str("account")?,
        get_str("fundCode")?,
        value
            .get("fundName")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        get_str("type")?,
        value
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or(ledger::STATE_CONFIRMED)
            .to_string(),
        parse_fixed("quantity", SCALE_SHARE)?,
        parse_fixed("price", SCALE_SHARE)?,
        fee_fixed,
        get_str("tradeDate")?,
        get_str("requestId")?,
    );
    let (id, created) = ledger::record_transaction(store, &input)
        .map_err(|e| api_error(409, e.code(), &e.message()))?;
    Ok(format!("{{\"id\":{id},\"created\":{created}}}"))
}

pub(crate) fn api_history(store: &Store, path: &str) -> Result<String, (u16, String)> {
    let account = query_param(path, "account")
        .ok_or_else(|| api_error(400, "PORTFOLIO_DB", "account 缺失"))?;
    let days: i64 = query_param(path, "days")
        .and_then(|d| d.parse().ok())
        .unwrap_or(30);
    // 找账户 id。
    let account_id: i64 = store
        .with_read(|conn| {
            conn.query_row(
                "SELECT id FROM accounts WHERE name = ?1",
                rusqlite::params![account],
                |r| r.get(0),
            )
            .optional()
        })
        .map_err(|e| api_error(500, "PORTFOLIO_DB", &e.to_string()))?
        .ok_or_else(|| api_error(404, "LEDGER_NOT_FOUND", "账户不存在"))?;
    // 日期窗口（自然日）。
    let today = today_iso();
    let from = minus_days(&today, days);
    let points = store
        .with_read(|conn| portfolio::replay_history(conn, account_id, &from, &today))
        .map_err(|e| api_error(500, "PORTFOLIO_DB", &e.to_string()))?;
    let items: Vec<String> = points
        .iter()
        .map(|p| {
            let value = p
                .value_raw
                .map(|raw| json_str(&Fixed::amount(raw).to_string_value()))
                .unwrap_or_else(|| "null".into());
            format!(
                "{{\"date\":{},\"value\":{},\"cost\":{}}}",
                json_str(&p.date),
                value,
                json_str(&Fixed::amount(p.cost_raw).to_string_value()),
            )
        })
        .collect();
    Ok(format!("{{\"history\":[{}]}}", items.join(",")))
}

pub(crate) fn api_accounts(store: &Store) -> Result<String, (u16, String)> {
    let names: Vec<String> = store
        .with_read(|conn| -> rusqlite::Result<Vec<String>> {
            let mut stmt = conn.prepare("SELECT name FROM accounts ORDER BY id")?;
            let rows = stmt
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .map_err(|e| api_error(500, "LEDGER_DB", &e.to_string()))?;
    let items: Vec<String> = names.iter().map(|n| json_str(n)).collect();
    Ok(format!("{{\"accounts\":[{}]}}", items.join(",")))
}

pub(crate) fn api_create_account(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| api_error(400, "LEDGER_INVALID", &e.to_string()))?;
    let name = value
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| api_error(400, "LEDGER_INVALID", "name 缺失"))?;
    let id = store
        .with_write(|conn| -> Result<i64, ledger::LedgerError> {
            ledger::ensure_account(conn, name)
        })
        .map_err(|e| api_error(409, e.code(), &e.message()))?;
    Ok(format!("{{\"id\":{id}}}"))
}

/// 本地日期 YYYY-MM-DD（UTC+8 基准，人民币市场；本地记账以日为粒度无外部依赖）。
pub(crate) fn today_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        + 8 * 3600;
    let days = secs / 86400;
    civil_from_days(days as i64)
}

pub(crate) fn minus_days(today: &str, days: i64) -> String {
    let y: i64 = today[0..4].parse().unwrap_or(2026);
    let m: i64 = today[5..7].parse().unwrap_or(1);
    let d: i64 = today[8..10].parse().unwrap_or(1);
    let to_days = |y: i64, m: i64, d: i64| -> i64 {
        // days-from-civil（Howard Hinnant 算法）。
        let y = if m <= 2 { y - 1 } else { y };
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let mp = (m + 9) % 12;
        let doy = (153 * mp + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146097 + doe - 719468
    };
    civil_from_days(to_days(y, m, d) - days)
}

pub(crate) fn civil_from_days(days: i64) -> String {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}
