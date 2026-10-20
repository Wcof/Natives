//! R2 首页组合概览。复用资产／收益权威，不请求行情，不写入历史。
use crate::assets::{add_minor, days_from_civil, AssetsSummary};
use crate::finance::FinanceError;
use crate::securities_income::realized_rows;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub(crate) fn overview(
    conn: &Connection,
    summary: &AssetsSummary,
    today: &str,
) -> Result<Value, FinanceError> {
    overview_range(conn, summary, today, None)
}

pub(crate) fn overview_range(
    conn: &Connection,
    summary: &AssetsSummary,
    today: &str,
    selected: Option<(&str, &str)>,
) -> Result<Value, FinanceError> {
    let mut securities = BTreeMap::<String, i64>::new();
    let mut security_names = BTreeMap::<String, String>::new();
    let mut products = BTreeMap::<String, i64>::new();
    let mut markets = BTreeMap::<String, i64>::new();
    let mut known_value = 0;
    let mut known_cost = None;
    let mut unrealized = 0;
    let mut missing_value = 0;
    let mut unknown_cost = 0;
    let mut stale = 0;
    let mut manual_count = 0;
    let mut overlap_count = 0;
    for row in &summary.funds {
        // Internal fund code is a stable, market-qualified ledger identity.
        let (kind, market, code) = crate::ledger::instrument_parts(&row.fund_code).unwrap_or((
            "fund",
            "CN",
            row.fund_code.as_str(),
        ));
        let identity = if kind == "fund" {
            format!("fund:{market}:{code}")
        } else {
            format!("security:{market}:{code}")
        };
        if row.cost_unknown {
            unknown_cost += 1;
        } else {
            known_cost = Some(add_minor(
                known_cost.unwrap_or(0),
                row.cost_raw,
                "证券已知成本",
            )?);
        }
        if row.stale {
            stale += 1;
        }
        let Some(value) = row.value_minor else {
            missing_value += 1;
            continue;
        };
        known_value = add_minor(known_value, value, "证券现值")?;
        for (map, key) in [
            (&mut securities, identity.as_str()),
            (&mut products, kind),
            (&mut markets, market),
        ] {
            let amount = map.entry(key.into()).or_default();
            *amount = add_minor(*amount, value, "证券集中度")?;
        }
        security_names
            .entry(identity)
            .or_insert_with(|| row.fund_name.clone());
        if !row.cost_unknown {
            let profit = value
                .checked_sub(row.cost_raw)
                .ok_or_else(|| FinanceError::Overflow("证券浮盈".into()))?;
            unrealized = add_minor(unrealized, profit, "证券浮盈")?;
        }
    }
    for row in &summary.manual {
        if !matches!(row.product_type.as_str(), "stock" | "etf") {
            continue;
        }
        manual_count += 1;
        if row.overlap_review {
            overlap_count += 1;
            continue;
        }
        unknown_cost += 1; // 人工估值不提供交易成本，不能推算浮盈。
        if row.stale {
            stale += 1;
        }
        let value = row.market_value_minor;
        known_value = add_minor(known_value, value, "证券现值")?;
        let market = row.instrument_key.split(':').nth(1).unwrap_or("unknown");
        for (map, key) in [
            (&mut securities, row.instrument_key.as_str()),
            (&mut products, row.product_type.as_str()),
            (&mut markets, market),
        ] {
            let amount = map.entry(key.into()).or_default();
            *amount = add_minor(*amount, value, "证券集中度")?;
        }
        security_names
            .entry(row.instrument_key.clone())
            .or_insert_with(|| row.name.clone());
    }
    let coverage_complete = missing_value == 0 && stale == 0 && overlap_count == 0;
    let unrealized_complete = coverage_complete && unknown_cost == 0;
    let ratios = |values: &BTreeMap<String, i64>| -> Vec<Value> {
        values
            .iter()
            .map(|(key, value)| {
                json!({"key":key,"name":security_names.get(key),"valueMinor":value,"weightBps":
                    if known_value > 0 {Some((*value as i128 * 10_000 / known_value as i128) as i64)} else {None}})
            })
            .collect()
    };
    // Square exact values before division; do not square already rounded weights.
    let hhi = if known_value > 0 {
        let numerator = securities.values().try_fold(0_i128, |n, value| {
            n.checked_add(*value as i128 * *value as i128)
                .ok_or_else(|| FinanceError::Overflow("集中度平方合计".into()))
        })?;
        Some(fraction_bps(
            numerator,
            known_value as i128 * known_value as i128,
        ))
    } else {
        None
    };

    let mut realized = 0;
    let mut realized_gaps = 0;
    for (_, _, value) in realized_rows(conn)? {
        if let Some(value) = value {
            realized = add_minor(realized, value, "已实现收益")?;
        } else {
            realized_gaps += 1;
        }
    }
    let yesterday = crate::assets::civil_from_days(
        days_from_civil(today).ok_or_else(|| FinanceError::Invalid("概览日期无效".into()))? - 1,
    );
    let daily = crate::returns::daily_returns(conn, &yesterday, today)?;
    let history = match selected {
        Some((from, to)) => history_range(conn, from, to)?,
        None => history(conn, today)?,
    };
    let dates = history["points"].as_array().unwrap();
    let attribution = match (dates.first(), dates.last()) {
        (Some(first), Some(last)) if first["date"] != last["date"] => {
            crate::portfolio_attribution::range(
                conn,
                first["date"].as_str().unwrap(),
                last["date"].as_str().unwrap(),
            )?
        }
        _ => json!({"status":"incomplete","reason":"missing_baseline","items":[]}),
    };
    Ok(json!({
        "ledger":crate::securities_income::ledger(conn,summary,today)?,
        "offlineIncome":crate::offline::income_summary(conn,today)?,
        "basis":"recorded-ledger-valuations", "asOf":summary.latest_valuation,
        "securities":{
            "scope":"recorded-securities", "knownValueMinor":known_value,
            "knownCostMinor":known_cost,
            "unrealizedMinor":if unrealized_complete {Some(unrealized)} else {None},
            "knownUnrealizedMinor":unrealized,
            "realizedMinor":if realized_gaps==0 {Some(realized)} else {None},
            "knownRealizedMinor":realized,"realizedGapCount":realized_gaps,
            "realizedScope":"ledger-securities",
            "holdingCount":summary.funds.len()+manual_count,"manualCount":manual_count,
            "excludedOverlapCount":overlap_count,"missingValueCount":missing_value+overlap_count,
            "unknownCostCount":unknown_cost,"staleCount":stale,
        },
        "diagnostics":{
            "scope":"recorded-securities","coverage":if coverage_complete {"complete"} else {"partial"},
            "fundLookThrough":"unavailable", "hhiBps":hhi,
            "securityCount":securities.len(), "securities":ratios(&securities),
            "products":ratios(&products),"markets":ratios(&markets),
        },
        "health": health(conn, summary, known_value)?,
        "dailyReturn":{
            "status":if daily.computable {"ready"} else {"incomplete"},
            "reason":daily.reason,"from":daily.from_date,"to":daily.to_date,
            "netReturnMinor":daily.net_return_minor,"netExternalMinor":daily.net_external_minor,
        },
        "history":history,"attribution":attribution,
    }))
}

/// DES-D04 五维体检只读投影：与首页同一快照分母（已知证券现值），
/// 不请求行情、不写库。类型／账户维度逐行聚合自同一估值批次；
/// 账户列表有界（最多 50 个，超出标 truncated）；流动性证券侧事实
/// （场外待确认、线下资产）经有界计数读取。现金侧由快照 wealth.cash 提供。
fn health(
    conn: &Connection,
    summary: &AssetsSummary,
    known_value: i64,
) -> Result<Value, FinanceError> {
    #[derive(Default)]
    struct Bucket {
        holding_count: usize,
        known_value: i64,
        missing_value: usize,
        unknown_cost: usize,
        stale: usize,
    }
    let mut types = BTreeMap::<&'static str, Bucket>::new();
    let mut accounts = BTreeMap::<i64, (String, Bucket)>::new();
    let bucket = |types: &mut BTreeMap<&'static str, Bucket>,
                  accounts: &mut BTreeMap<i64, (String, Bucket)>,
                  kind: &'static str,
                  account_id: i64,
                  account_name: &str,
                  row_value: Option<i64>,
                  cost_unknown: bool,
                  stale: bool| {
        let type_bucket = types.entry(kind).or_default();
        type_bucket.holding_count += 1;
        if let Some(value) = row_value {
            type_bucket.known_value = add_minor(type_bucket.known_value, value, "体检类型现值")?;
        } else {
            type_bucket.missing_value += 1;
        }
        if cost_unknown {
            type_bucket.unknown_cost += 1;
        }
        if stale {
            type_bucket.stale += 1;
        }
        let account_bucket = &mut accounts
            .entry(account_id)
            .or_insert_with(|| (account_name.to_string(), Bucket::default()))
            .1;
        account_bucket.holding_count += 1;
        if let Some(value) = row_value {
            account_bucket.known_value =
                add_minor(account_bucket.known_value, value, "体检账户现值")?;
        } else {
            account_bucket.missing_value += 1;
        }
        if cost_unknown {
            account_bucket.unknown_cost += 1;
        }
        if stale {
            account_bucket.stale += 1;
        }
        Ok::<(), FinanceError>(())
    };
    for row in &summary.funds {
        let kind: &'static str = match row.instrument_kind.as_str() {
            "stock" => "stock",
            "etf" => "etf",
            _ => "fund",
        };
        bucket(
            &mut types,
            &mut accounts,
            kind,
            row.account_id,
            &row.account_name,
            row.value_minor,
            row.cost_unknown,
            row.stale,
        )?;
    }
    for row in &summary.manual {
        if !matches!(row.product_type.as_str(), "stock" | "etf") {
            continue;
        }
        let kind: &'static str = if row.product_type == "stock" {
            "stock"
        } else {
            "etf"
        };
        bucket(
            &mut types,
            &mut accounts,
            kind,
            row.account_id,
            &row.account_name,
            Some(row.market_value_minor),
            true,
            row.stale,
        )?;
    }
    let type_json = |key: &str, bucket: &Bucket| {
        json!({
            "instrumentKind": key, "holdingCount": bucket.holding_count,
            "knownValueMinor": bucket.known_value,
            "missingValueCount": bucket.missing_value,
            "unknownCostCount": bucket.unknown_cost,
            "staleCount": bucket.stale,
            "weightBps": if known_value > 0 { Some((bucket.known_value as i128 * 10_000 / known_value as i128) as i64) } else { None },
        })
    };
    let type_json_all = |types: &BTreeMap<&'static str, Bucket>| -> Vec<Value> {
        ["stock", "etf", "fund"]
            .iter()
            .filter_map(|k| types.get(*k).map(|b| type_json(k, b)))
            .collect()
    };
    // 账户列表按已知现值倒序有界返回；归档标记来自账户表，折叠不排除资产。
    // 账户归档标记读取失败不阻断体检（按未归档处理），与既有 unwrap_or(0) 口径一致。
    let archived: std::collections::BTreeMap<i64, bool> = conn
        .prepare("SELECT id, archived_at IS NOT NULL FROM accounts")
        .and_then(|mut stmt| {
            stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, bool>(1)?)))?
                .collect::<rusqlite::Result<std::collections::BTreeMap<i64, bool>>>()
        })
        .unwrap_or_default();
    let mut account_rows: Vec<(i64, &String, &Bucket)> = accounts
        .iter()
        .map(|(id, (name, bucket))| (*id, name, bucket))
        .collect();
    account_rows.sort_by(|a, b| b.2.known_value.cmp(&a.2.known_value).then(a.0.cmp(&b.0)));
    const ACCOUNT_LIMIT: usize = 50;
    let truncated = account_rows.len() > ACCOUNT_LIMIT;
    let accounts_json: Vec<Value> = account_rows
        .into_iter()
        .take(ACCOUNT_LIMIT)
        .map(|(id, name, bucket)| {
            json!({
                "accountId": id, "name": name, "archived": archived.get(&id).copied().unwrap_or(false),
                "holdingCount": bucket.holding_count, "knownValueMinor": bucket.known_value,
                "missingValueCount": bucket.missing_value, "unknownCostCount": bucket.unknown_cost,
                "staleCount": bucket.stale,
            })
        })
        .collect();
    // 流动性证券侧事实：场外待确认交易数、线下资产条目与未结算退出数。
    let otc_pending: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE state='pending' AND deleted_at IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let offline_unsettled_exits: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM offline_assets WHERE deleted_at IS NULL AND exit_date IS NOT NULL AND exit_settled=0",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    Ok(json!({
        "scope": "recorded-securities",
        "types": type_json_all(&types),
        "accounts": accounts_json, "accountsTruncated": truncated,
        "liquidity": {
            "otcPendingCount": otc_pending,
            "offlineCount": summary.offline_count,
            "offlineReceivableMinor": summary.offline_receivable_minor,
            "offlineUnsettledExits": offline_unsettled_exits,
        },
    }))
}

// Four decimal digits by long division. Intermediate numerator * 10_000 would
// overflow for valid i64 money values even though HHI itself is bounded by one.
fn fraction_bps(numerator: i128, denominator: i128) -> i64 {
    let mut result = numerator / denominator;
    let mut remainder = numerator % denominator;
    for _ in 0..4 {
        let mut next = 0;
        let mut digit = 0;
        for _ in 0..10 {
            if next >= denominator - remainder {
                next -= denominator - remainder;
                digit += 1;
            } else {
                next += remainder;
            }
        }
        result = result * 10 + digit;
        remainder = next;
    }
    result as i64
}

pub(crate) fn history(conn: &Connection, today: &str) -> Result<Value, FinanceError> {
    let from = crate::assets::civil_from_days(days_from_civil(today).unwrap() - 60);
    history_range(conn, &from, today)
}

pub(crate) fn history_range(
    conn: &Connection,
    from: &str,
    to: &str,
) -> Result<Value, FinanceError> {
    let from = if from.is_empty() {
        conn.query_row(
            "SELECT MIN(taken_at) FROM value_snapshots WHERE scope='A'",
            [],
            |r| r.get::<_, Option<String>>(0),
        )?
        .unwrap_or_else(|| to.to_string())
    } else {
        from.to_string()
    };
    let mut stmt = conn.prepare(
        "SELECT taken_at,MAX(id) FROM value_snapshots
         WHERE scope='A' AND taken_at>=?1 AND taken_at<=?2
         GROUP BY taken_at ORDER BY taken_at DESC LIMIT 401",
    )?;
    let mut days = stmt
        .query_map(params![from, to], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let truncated = days.len() > 400;
    days.truncate(400);
    days.reverse();
    let mut points = Vec::new();
    let mut reason = if days.first().map(|d| d.0.as_str()) != Some(from.as_str()) {
        Some("missing_baseline")
    } else {
        None
    };
    let mut factor = 1_000_000_i128;
    for (i, (date, last)) in days.iter().enumerate() {
        if !date.is_ascii() || !crate::assets::is_valid_calendar_date(date) {
            return Err(FinanceError::Invalid("历史估值日期无效".into()));
        }
        let (value, quality): (i64, String) = conn.query_row(
            "SELECT total_minor,quality FROM value_snapshots WHERE id=?1",
            [last],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let mut period = None;
        if i > 0 && reason.is_none() {
            let previous = &days[i - 1].0;
            if days_from_civil(date).unwrap() - days_from_civil(previous).unwrap() != 1 {
                reason = Some("missing_daily_history");
            } else {
                let result = crate::returns::daily_returns(conn, previous, date)?;
                if !result.computable {
                    reason = Some("unverified_history");
                } else if result.begin_total_minor.unwrap_or(0) <= 0
                    || result.end_total_minor.unwrap_or(0) <= 0
                {
                    reason = Some("nonpositive_baseline");
                } else {
                    let begin = result.begin_total_minor.unwrap() as i128;
                    let end = result.end_total_minor.unwrap() as i128;
                    let flow = result.net_external_minor.unwrap() as i128;
                    if end - flow <= 0 {
                        reason = Some("nonpositive_baseline");
                    } else {
                        factor = factor
                            .checked_mul(end - flow)
                            .ok_or_else(|| FinanceError::Overflow("时间加权收益".into()))?
                            / begin;
                        period = Some(result.net_return_minor);
                    }
                }
            }
        }
        if quality != "ready" {
            reason = Some("unverified_history");
        }
        points.push(json!({"date":date,"valueMinor":value,"quality":quality,
            "periodReturnMinor":period.flatten(),"twrPpm":if reason.is_none() {Some((factor-1_000_000).to_string())} else {None}}));
    }
    if days.last().map(|d| d.0.as_str()) != Some(to) && reason.is_none() {
        reason = Some("missing_end");
    }
    Ok(
        json!({"scope":"A","from":from,"to":to,"source":"value_snapshots","truncated":truncated,
        "cashFlowTiming":"day-end","points":points,
        "status":if reason.is_none() {"ready"} else {"incomplete"},"reason":reason,
        "twrPpm":if reason.is_none() {Some((factor-1_000_000).to_string())} else {None},
        "benchmark":{"status":"incomplete","reason":"missing_benchmark","excessPpm":null}}),
    )
}

#[cfg(test)]
#[path = "overview_tests.rs"]
mod tests;
