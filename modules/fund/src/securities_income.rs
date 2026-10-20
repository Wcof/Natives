//! Shared current-day income and active ledger scope for Home and Positions.
use crate::assets::{add_minor, AssetError, AssetsSummary, FundValuationRow};
use crate::finance::FinanceError;
use crate::fixed::Fixed;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

pub(crate) fn daily(
    conn: &Connection,
    row: &FundValuationRow,
    today: &str,
) -> Result<Value, AssetError> {
    let unavailable = |reason: &str| json!({"amount":null,"rateBps":null,"date":today,"baselineDate":null,"reason":reason});
    if row.price_as_of.as_deref().and_then(|d| d.get(..10)) != Some(today) {
        return Ok(unavailable("missing_daily_price"));
    }
    if row.value_minor.is_none() {
        return Ok(unavailable("unverified_valuation"));
    }
    let Some(now) = row.price_raw else {
        return Ok(unavailable("missing_daily_price"));
    };
    let (baseline_date, base) = if row.instrument_kind == "fund" {
        let previous: Option<(String, i64)> = conn.query_row(
            "SELECT nav_date,unit_nav_raw FROM fund_nav WHERE fund_id=?1 AND nav_date<?2
             AND (unit_nav_raw>0 OR (unit_nav_raw=0 AND source='user_confirmed')) ORDER BY nav_date DESC LIMIT 1",
            params![row.fund_id,today], |r| Ok((r.get(0)?,r.get(1)?)),
        ).optional()?;
        let Some((date, price)) = previous else {
            return Ok(unavailable("missing_baseline"));
        };
        (Some(date), price as i128)
    } else {
        let Some(change) = row.day_change_raw else {
            return Ok(unavailable("missing_baseline"));
        };
        (None, now as i128 - change as i128)
    };
    if base < 0 {
        return Ok(unavailable("missing_baseline"));
    }
    let change = now as i128 - base;
    let amount = i64::try_from(row.quantity_raw as i128 * change / 1_000_000)
        .map_err(|_| AssetError::Overflow("daily income".into()))?;
    let rate = if base > 0 {
        i64::try_from(change * 10_000 / base).ok()
    } else {
        None
    };
    Ok(
        json!({"amount":Fixed::amount(amount).to_string_value(),"rateBps":rate.map(|v|v.to_string()),
        "date":today,"baselineDate":baseline_date,"reason":if rate.is_none(){Some("missing_rate_basis")}else{None::<&str>}}),
    )
}

pub(crate) fn ledger(
    conn: &Connection,
    summary: &AssetsSummary,
    today: &str,
) -> Result<Value, AssetError> {
    let mut stmt = conn.prepare("SELECT id FROM accounts WHERE archived_at IS NULL")?;
    let active = stmt
        .query_map([], |r| r.get::<_, i64>(0))?
        .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
    let realized = realized_rows(conn).map_err(|e| AssetError::Invalid(e.message()))?;
    let mut stmt = conn.prepare("SELECT id,code FROM funds")?;
    let kinds = stmt
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
        .map(|r| {
            let (id, code) = r?;
            let (kind, _, _) = crate::ledger::instrument_parts(&code)
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            Ok((id, kind.to_owned()))
        })
        .collect::<Result<std::collections::BTreeMap<_, _>, rusqlite::Error>>()?;
    let project = |kind| ledger_for_kind(conn, summary, today, &active, &realized, &kinds, kind);
    let mut total = project(None)?;
    total["byType"] = json!({"stock":project(Some("stock"))?,"fund":project(Some("fund"))?});
    Ok(total)
}

fn matches_kind(actual: &str, kind: Option<&str>) -> bool {
    match kind {
        None => true,
        Some("fund") => actual == "fund" || actual == "etf",
        Some(kind) => actual == kind,
    }
}

fn ledger_for_kind(
    conn: &Connection,
    summary: &AssetsSummary,
    today: &str,
    active: &std::collections::BTreeSet<i64>,
    realized: &[(i64, i64, Option<i64>)],
    kinds: &std::collections::BTreeMap<i64, String>,
    kind: Option<&str>,
) -> Result<Value, AssetError> {
    let mut value = 0;
    let mut cost = 0;
    let mut profit = 0;
    let mut day = 0;
    let mut count = 0;
    let mut missing_value = 0;
    let mut unknown_cost = 0;
    let mut missing_day = 0;
    let mut estimated_day = 0;
    let mut estimated_count = 0;
    let mut estimated_missing = 0;
    let mut navs = Vec::new();
    for row in summary
        .funds
        .iter()
        .filter(|row| active.contains(&row.account_id) && matches_kind(&row.instrument_kind, kind))
    {
        count += 1;
        if let Some(v) = row.value_minor {
            value = add_minor(value, v, "ledger value")?;
        } else {
            missing_value += 1;
        }
        if row.cost_unknown {
            unknown_cost += 1;
        } else {
            cost = add_minor(cost, row.cost_raw, "ledger cost")?;
            if let Some(v) = row.value_minor {
                let p = v
                    .checked_sub(row.cost_raw)
                    .ok_or_else(|| AssetError::Overflow("holding income".into()))?;
                profit = add_minor(profit, p, "holding income")?;
            }
        }
        let fact = daily(conn, row, today)?;
        if let Some(amount) = fact["amount"].as_str() {
            day = add_minor(
                day,
                Fixed::parse(amount, 2)
                    .map_err(|_| AssetError::Overflow("daily income".into()))?
                    .raw(),
                "daily income",
            )?;
            estimated_day = add_minor(
                estimated_day,
                Fixed::parse(amount, 2)
                    .map_err(|_| AssetError::Overflow("daily income".into()))?
                    .raw(),
                "estimated daily income",
            )?;
        } else {
            missing_day += 1;
            let estimate = crate::nav::cached_estimate(row, today)
                .map(|value| crate::nav::estimated_income(row, &value, today));
            if let Some(amount) = estimate.as_ref().and_then(|value| value["amount"].as_str()) {
                estimated_day = add_minor(
                    estimated_day,
                    Fixed::parse(amount, 2)
                        .map_err(|_| AssetError::Overflow("estimated income".into()))?
                        .raw(),
                    "estimated daily income",
                )?;
                estimated_count += 1;
            } else {
                estimated_missing += 1;
            }
        }
        if row.instrument_kind == "fund" {
            navs.push(json!({"fundCode":row.fund_code,"navDate":row.nav_date}));
        }
    }
    let mut realized_total = 0;
    let mut realized_gaps = 0;
    for (account, fund, amount) in realized {
        if !active.contains(account)
            || !matches_kind(
                kinds
                    .get(fund)
                    .map(String::as_str)
                    .ok_or_else(|| AssetError::Invalid("missing instrument identity".into()))?,
                kind,
            )
        {
            continue;
        }
        if let Some(amount) = amount {
            realized_total = add_minor(realized_total, *amount, "realized income")?;
        } else {
            realized_gaps += 1;
        }
    }
    Ok(
        json!({"status":if count==0{"empty"}else if missing_value>0||unknown_cost>0{"partial"}else{"ready"},"scope":"active-ledger-securities","holdingCount":count,"knownValueMinor":value.to_string(),
        "realizedMinor":if realized_gaps==0 {Some(realized_total.to_string())}else{None},"realizedGapCount":realized_gaps,
        "marketValueMinor":if missing_value==0 {Some(value.to_string())}else{None},"knownCostMinor":cost.to_string(),
        "costMinor":if unknown_cost==0 {Some(cost.to_string())}else{None},
        "holdingProfitMinor":if unknown_cost==0 && missing_value==0 {Some(profit.to_string())}else{None},
        "knownHoldingProfitMinor":profit.to_string(),"missingValueCount":missing_value,"unknownCostCount":unknown_cost,
        "day":{"date":today,"amountMinor":if count>missing_day {Some(day.to_string())}else{None},"missingCount":missing_day,
            "complete":missing_day==0,"holdingCount":count},"estimatedDay":{"date":today,"amountMinor":if estimated_count>0{Some(estimated_day.to_string())}else{None},"estimatedCount":estimated_count,"missingCount":estimated_missing,"complete":false},"navs":navs}),
    )
}

pub(crate) fn realized_rows(
    conn: &Connection,
) -> Result<Vec<(i64, i64, Option<i64>)>, FinanceError> {
    let mut stmt = conn.prepare(
        "SELECT account_id,fund_id FROM opening_snapshots WHERE deleted_at IS NULL
         UNION SELECT account_id,fund_id FROM transactions WHERE state='confirmed' AND deleted_at IS NULL",
    )?;
    let pairs = stmt
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut rows = Vec::new();
    for (account, fund) in pairs {
        let gap: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM opening_snapshots o
             JOIN transactions t ON t.account_id=o.account_id AND t.fund_id=o.fund_id
             WHERE o.account_id=?1 AND o.fund_id=?2 AND o.deleted_at IS NULL
             AND o.cost_raw IS NULL AND o.quantity_raw>0
             AND t.type='SELL' AND t.source!='transfer' AND t.state='confirmed' AND t.deleted_at IS NULL)",
            params![account,fund], |r|r.get(0),
        )?;
        if gap {
            rows.push((account, fund, None));
            continue;
        }
        if let Some(p) = crate::portfolio::replay_position(conn, account, fund)
            .map_err(|e| FinanceError::Db(e.to_string()))?
        {
            rows.push((account, fund, Some(p.realized_raw)));
        }
    }
    Ok(rows)
}

#[cfg(test)]
#[path = "securities_income_tests.rs"]
mod tests;
