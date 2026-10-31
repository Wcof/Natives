//! home 模块：投资首页快照聚合（方案 02 §7 契约）。
//!
//! 在同一读快照内聚合资产、现金、收益、主目标与配置；本地快照不等待远程
//! 行情/AI。每个字段带 value/status/asOf/issues，未知不显示 0。
//! 市场行情由前端直接调用现有 /api/market/*，不进入本地聚合。

use crate::finance::{self, FinanceError};
use crate::storage::Store;
use rusqlite::OptionalExtension;

use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeSnapshot {
    pub snapshot_id: String,
    pub generated_at: String,
    pub data_revision: String,
}

/// 快照/证据共用「今天」（北京时间口径）。供 API 层按需取用。
pub fn resolve_today(_conn: &rusqlite::Connection) -> String {
    today_cn()
}

fn today_cn() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        + 8 * 3600;
    crate::assets::civil_from_days((secs / 86400) as i64)
}

/// 收益区间解析：today（当日）/ week（本周）/ month（本月）/ half（近 6 个月）/
/// year1（近 12 个月）/ year（今年，兼容旧客户端）/ all（跟踪以来）/ custom。
/// 返回 (from, to, label)；缺基线时 from 仍为名义起点（收益结果标记 missing_baseline）。
pub fn resolve_range(
    range: &str,
    from: Option<&str>,
    to: Option<&str>,
    today: &str,
) -> (String, String, String) {
    match range {
        "today" => (today.to_string(), today.to_string(), "today".into()),
        "week" => {
            // 周一起算：1970-01-01 是周四，回退 (d+3) mod 7 天即本周一。
            let from = crate::assets::days_from_civil(today)
                .map(|day| crate::assets::civil_from_days(day - (day + 3).rem_euclid(7)))
                .unwrap_or_else(|| today.to_string());
            (from, today.to_string(), "week".into())
        }
        "month" => {
            let from = format!("{}-01", &today[..7]);
            (from, today.to_string(), "month".into())
        }
        "half" => (minus_months(today, 6), today.to_string(), "half".into()),
        "year1" => (minus_months(today, 12), today.to_string(), "year1".into()),
        "year" => {
            let from = format!("{}-01-01", &today[..4]);
            (from, today.to_string(), "year".into())
        }
        "custom" => (
            from.unwrap_or(today).to_string(),
            to.unwrap_or(today).to_string(),
            "custom".into(),
        ),
        _ => (String::new(), today.to_string(), "all".into()),
    }
}

fn is_leap_year(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 30,
    }
}

/// 自然月回退：日号超出目标月天数时取目标月末（如 8-31 回退半年到 2-28/29）。
/// 输入不是合法 `YYYY-MM-DD` 时原样返回，由调用方保持既有失败语义。
fn minus_months(date: &str, months: i64) -> String {
    let (Some(year), Some(month), Some(day)) = (
        date.get(0..4).and_then(|value| value.parse::<i64>().ok()),
        date.get(5..7).and_then(|value| value.parse::<i64>().ok()),
        date.get(8..10).and_then(|value| value.parse::<i64>().ok()),
    ) else {
        return date.to_string();
    };
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return date.to_string();
    }
    let total = year * 12 + (month - 1) - months;
    let target_year = total.div_euclid(12);
    let target_month = total.rem_euclid(12) + 1;
    let target_day = day.min(days_in_month(target_year, target_month));
    format!("{target_year:04}-{target_month:02}-{target_day:02}")
}

fn quality_of(summary: &crate::assets::AssetsSummary) -> (&'static str, Vec<Value>) {
    let mut issues = Vec::new();
    if !summary.has_entries {
        return ("empty", issues);
    }
    if summary.missing_price_count > 0 {
        issues.push(json!({"code":"missing_price","count":summary.missing_price_count}));
    }
    if summary.missing_quote_count > 0 {
        issues.push(json!({"code":"missing_quote","count":summary.missing_quote_count}));
    }
    if summary.currency_unconfirmed_count > 0 {
        issues.push(
            json!({"code":"currency_unconfirmed","count":summary.currency_unconfirmed_count}),
        );
    }
    if summary.currency_unsupported_count > 0 {
        issues.push(
            json!({"code":"currency_unsupported","count":summary.currency_unsupported_count}),
        );
    }
    if summary.overlap_review_count > 0 {
        issues.push(json!({"code":"overlap_review","count":summary.overlap_review_count}));
    }
    if summary.old_valuation_count > 0 {
        issues.push(json!({"code":"stale_valuation","count":summary.old_valuation_count}));
    }
    match summary.quality {
        "ready" => ("ready", issues),
        // A stale valuation already has a complete value; keep the state distinct
        // from an incomplete one so the summary card and detail never disagree.
        "stale" => ("stale", issues),
        _ => {
            if issues.is_empty() {
                issues.push(json!({"code":"incomplete","count":1}));
            }
            ("incomplete", issues)
        }
    }
}

/// 首页快照。只读聚合（单次 with_read），不隐式写入。
pub fn build_snapshot(store: &Store, range: &str) -> Result<Value, FinanceError> {
    build_snapshot_range(store, range, None, None)
}

pub(crate) fn checked_range(
    range: &str,
    from: Option<&str>,
    to: Option<&str>,
    today: &str,
) -> Result<(String, String, String), FinanceError> {
    if ![
        "today", "week", "month", "half", "year1", "year", "all", "custom",
    ]
    .contains(&range)
    {
        return Err(FinanceError::Invalid("收益区间无效".into()));
    }
    if range == "custom" && (from.is_none() || to.is_none()) {
        return Err(FinanceError::Invalid("自定义区间需要起止日期".into()));
    }
    let (from, to, range_label) = resolve_range(range, from, to, today);
    if (!from.is_empty() && !crate::assets::is_valid_calendar_date(&from))
        || !crate::assets::is_valid_calendar_date(&to)
        || (!from.is_empty() && from > to)
        || to.as_str() > today
    {
        return Err(FinanceError::Invalid("收益区间日期无效".into()));
    }

    Ok((from, to, range_label))
}

pub(crate) fn cached_snapshot_range(
    store: &Store,
    range: &str,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<Value, FinanceError> {
    let today = today_cn();
    let (from, to, label) = checked_range(range, from, to, &today)?;
    store.with_snapshot(|conn| {
        let (summary, _) = crate::assets::summary_with_cached_quotes(conn, &today)?;
        snapshot_inner(conn, &today, &from, &to, label, &summary, false)
    })
}

pub fn build_snapshot_range(
    store: &Store,
    range: &str,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<Value, FinanceError> {
    let today = today_cn();
    let (from, to, range_label) = checked_range(range, from, to, &today)?;

    // ---- N04 统一估值：有界身份首读 → 锁外取价 → 统一估值重读 ----
    // 首次打开首页即可取价，不依赖先访问持仓页；网络在锁外，重读核对
    // revision（最多 3 次，标的集合变化时重取行情），不混入旧数量。
    let (initial_symbol_list, _first_revision) = store.with_snapshot(|conn| {
        let collected = crate::assets::collect(conn, &today)?;
        let revision = crate::storage::Store::data_revision(conn)?;
        Ok::<_, FinanceError>((crate::assets::quote_symbols(&collected), revision))
    })?;
    let mut batch = crate::market::fetch_quote_batch(&initial_symbol_list);
    let mut symbols_used: std::collections::BTreeSet<String> =
        initial_symbol_list.into_iter().collect();

    let mut built: Option<(Value, String, String)> = None;
    for _ in 0..3 {
        let (snapshot, revision, current_symbols, batch_id) = store.with_snapshot(|conn| {
            let mut collected = crate::assets::collect(conn, &today)?;
            let revision = crate::storage::Store::data_revision(conn)?;
            let current_symbols: std::collections::BTreeSet<String> =
                crate::assets::quote_symbols(&collected)
                    .into_iter()
                    .collect();
            crate::assets::apply_quotes(&mut collected, &batch.quotes, &today);
            let mut summary = crate::assets::finalize(collected, &today)?;
            summary.valuation_batch =
                crate::assets::valuation_batch(&revision, batch.fetched_at, &summary);
            summary.valuation_fetched_at = Some(batch.fetched_at);
            summary.valuation_stale = false;
            let batch_id = summary.valuation_batch.clone();
            let value = snapshot_inner(
                conn,
                &today,
                &from,
                &to,
                range_label.clone(),
                &summary,
                batch.failed,
            )?;
            Ok::<_, FinanceError>((value, revision, current_symbols, batch_id))
        })?;
        if current_symbols == symbols_used {
            // 行情获取身份与本次快照一致：批次可用。
            built = Some((snapshot, revision, batch_id));
            break;
        }
        // 标的集合变化（新增/清仓持仓）：为当前身份重取行情后重建。
        symbols_used = current_symbols;
        batch = crate::market::fetch_quote_batch(&symbols_used.iter().cloned().collect::<Vec<_>>());
        built = None;
    }
    let Some((snapshot, revision, batch_id)) = built else {
        return Err(FinanceError::Conflict("数据持续变化，请刷新后重试".into()));
    };
    // 共享估值缓存：空间卡/目标/配置等只读消费方同源；进程内写入，
    // 不推进账本 revision（GET 不隐式写账本）。
    let _ = store.with_read(|conn| -> Result<(), rusqlite::Error> {
        crate::assets::store_valuation_cache(
            conn,
            &batch.quotes,
            batch.fetched_at,
            &revision,
            &batch_id,
            batch.failed,
        );
        Ok(())
    });
    Ok(snapshot)
}

pub(crate) fn snapshot_inner(
    conn: &rusqlite::Connection,
    today: &str,
    from: &str,
    to: &str,
    range_label: String,
    summary: &crate::assets::AssetsSummary,
    quote_failed: bool,
) -> Result<Value, FinanceError> {
    let cash = finance::cash_position(conn)?;
    let (wealth_quality, wealth_issues) = quality_of(summary);
    let goals = crate::goals::list_goals(conn, false)?;
    let primary = goals.iter().find(|g| g.is_primary).cloned();
    let open_alerts: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM watch_events WHERE status IN ('open','acknowledged','snoozed')",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let last_check: Option<String> = conn
        .query_row(
            "SELECT value FROM app_meta WHERE key='last_rule_check'",
            [],
            |r| r.get(0),
        )
        .optional()?
        .flatten();

    // ---- 财富四量 ----
    let total_a = summary.complete_value_minor;
    // 现金 C：来自现金资产合计；无现金资产记录时为 None（未建账，不显示 0）。
    let cash_c = cash.cash_minor;
    // A 只含已登记资产；未登记现金不阻断已登记非现金估值，C 仍保持未知。
    let invested_i = total_a
        .map(|a| crate::assets::add_minor(a, -cash_c.unwrap_or(0), "非现金市值"))
        .transpose()
        .map_err(FinanceError::from)?;
    let available_f = cash.available_minor;
    let mut opportunities_json =
        crate::opportunities::projection(conn, summary, today, available_f)?;
    crate::opportunity_records::attach_recorded(conn, &mut opportunities_json)?;
    // 要紧事与机会同快照合并（供体 Dashboard urgent 适配）：事件源＋持仓复核＋
    // 有向候选，权重 event-1000 / holding-500 / |涨跌幅|，取前 8。
    let urgent_json = crate::home_urgent::merge(conn, &opportunities_json)?;
    let shortfall = cash.shortfall_minor;

    let mut returns_json = crate::home_period::project(conn, summary, from, to, today)?;
    returns_json["range"] = range_label.clone().into();
    let mut portfolio = crate::overview::overview_range(conn, summary, today, Some((from, to)))?;
    if range_label != "today" {
        crate::holding_history::project(conn, &mut portfolio["history"], from, to)?;
    }
    if let (Some(base), Some(end)) = (returns_json["from"].as_str(), returns_json["to"].as_str()) {
        portfolio["attribution"] = if returns_json["endSource"] == "current-valuation" {
            crate::portfolio_attribution::range_live(conn, base, end, summary)?
        } else {
            crate::portfolio_attribution::range(conn, base, end)?
        };
    }
    portfolio["dailyReturn"] = crate::home_period::project(conn, summary, today, today, today)?;
    if to == today && summary.old_valuation_count == 0 {
        if let Some(value) = summary.complete_value_minor {
            let points = portfolio["history"]["points"].as_array_mut().unwrap();
            points.retain(|p| p["date"] != today);
            if points.len() >= 400 {
                points.remove(0);
            }
            points.push(json!({"date":today,"valueMinor":value.to_string(),"quality":"reference","periodReturnMinor":null,"twrPpm":null}));
            portfolio["history"]["source"] = "value_snapshots-and-current-valuation".into();
            portfolio["history"]["status"] = "reference".into();
            portfolio["history"]["twrPpm"] = Value::Null;
        }
    }

    // ---- 配置检查 ----
    let allocation = crate::planning::check_allocation(conn, &summary)?;
    let allocation_json =
        serde_json::to_value(&allocation).map_err(|e| FinanceError::Db(e.to_string()))?;

    // ---- 自定义资金分组（同一快照；方案 11 §4）----
    let group_allocation = crate::group_plan::group_allocation(conn, &summary)
        .map_err(|e| FinanceError::Invalid(e.message()))?;

    // ---- 主目标 ----
    let goals_json = match &primary {
        Some(goal) => {
            let progress = crate::goals::goal_scope_value(conn, goal, &summary, cash.cash_minor);
            let _ = cash.cash_minor;
            json!({
                "status": "ready",
                "goal": {
                    "id": goal.id, "name": goal.name,
                    "targetMinor": goal.target_minor, "targetDate": goal.target_date,
                    "scopeKind": goal.scope_kind, "version": goal.version,
                },
                "progress": {
                    "computable": progress.computable,
                    "reasonCodes": progress.reason_codes,
                    "scopeValueMinor": progress.scope_value_minor,
                    "ratioTenths": progress.ratio_tenths,
                    "ratioPpm": progress.ratio_ppm,
                    "gapMinor": progress.gap_minor,
                    "surplusMinor": progress.surplus_minor,
                    "remainingInvestments": progress.remaining_investments,
                    "requiredMonthlyMinor": progress.required_monthly_minor,
                    "expired": progress.expired,
                },
            })
        }
        None => json!({"status": "empty"}),
    };

    let data_revision = Store::data_revision(conn)?;
    let snapshot_id = format!("home-{today}-{data_revision}-{from}-{to}");
    let generated_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default();

    let mut snapshot = json!({
        "snapshotId": snapshot_id,
        "generatedAt": generated_at,
        "dataRevision": data_revision,
        "scope": {"currency": "CNY", "basis": "fund-managed-assets"},
        "wealth": {
            "quality": wealth_quality,
            "issues": wealth_issues,
            "latestValuation": summary.latest_valuation,
            "totalA": total_a,
            "knownA": summary.known_value_minor,
            "investedI": invested_i,
            "valuation": {
                "batch": summary.valuation_batch,
                "fetchedAt": summary.valuation_fetched_at,
                "stale": summary.valuation_stale,
                "quoteFailed": quote_failed,
                "quoteCount": summary.quote_count,
                "missingQuotes": summary.missing_quote_count,
                "missingNavs": summary
                    .funds
                    .iter()
                    .filter(|row| row.value_gap.as_deref() == Some("missing_nav"))
                    .count(),
                "unknownCosts": summary.unknown_cost_count,
                "unconfirmedCurrencies": summary.currency_unconfirmed_count,
                "unsupportedCurrencies": summary.currency_unsupported_count,
            },
            "floating": {
                "minor": summary.floating_minor,
                "unknownCostCount": summary.floating_unknown_cost_count,
            },
            "cumulative": crate::returns::cumulative_projection(conn, summary, today)?,
            "cash": {
                "cashMinor": cash_c,
                "reservedMinor": cash.reserved_minor,
                "reserveState": cash.reserve_state,
                "reservedByOrdersMinor": cash.reserved_by_orders_minor,
                "availableMinor": available_f,
                "shortfallMinor": shortfall,
                "accounts": cash.accounts.iter().map(|a| json!({
                    "accountId": a.account_id,
                    "accountName": a.account_name,
                    "cashMinor": a.cash_minor,
                    "reservedMinor": a.reserved_minor,
                    "reserveState": a.reserve_state,
                    "reservedByOrdersMinor": a.reserved_by_orders_minor,
                    "availableMinor": a.available_minor,
                })).collect::<Vec<_>>(),
            },
            "returns": returns_json,
        },
        "allocation": allocation_json,
        "groupAllocation": group_allocation,
        "goals": goals_json,
        "portfolio":portfolio,
        "requestedRange":{"key":range_label,"from":from,"to":to,"today":today},
        "financialDisplay":crate::settings::read(conn)?,
        "opportunities": opportunities_json,
        "alerts": {
            "openCount": open_alerts,
            "lastCheck": last_check,
            "coverage": "投资应用打开期间检查；关闭后不监控",
        },
        "urgent": urgent_json,
    });
    crate::valuation_history::project(conn, &mut snapshot, from, to)?;
    Ok(snapshot)
}

pub use crate::evidence::{evidence, evidence_task};

/// Preserve fixed-point integers across JavaScript's 53-bit JSON number boundary.
pub(crate) fn wire_numbers(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            for (key, v) in fields {
                if key.ends_with("Minor")
                    || key.ends_with("Raw")
                    || key.ends_with("_minor")
                    || key.ends_with("_raw")
                    || matches!(
                        key.as_str(),
                        "totalA" | "knownA" | "investedI" | "allocatableB"
                    )
                {
                    if let Some(n) = v.as_i64() {
                        *v = Value::String(n.to_string());
                        continue;
                    }
                }
                wire_numbers(v);
            }
        }
        Value::Array(items) => {
            for v in items {
                wire_numbers(v);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod wire_tests {
    #[test]
    fn amounts_survive_javascript_boundary() {
        let mut v = serde_json::json!({"wealth":{"totalA":i64::MAX},"targetMinor":i64::MAX,"ratioTenths":500,"id":1});
        super::wire_numbers(&mut v);
        assert_eq!(v["targetMinor"], i64::MAX.to_string());
        assert_eq!(v["wealth"]["totalA"], i64::MAX.to_string());
        assert_eq!(v["ratioTenths"], 500);
        assert_eq!(v["id"], 1);
    }
}

#[cfg(test)]
#[path = "home_range_tests.rs"]
mod range_tests;
