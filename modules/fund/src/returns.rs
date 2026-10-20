//! 真实端点与资金核对状态决定收益可计算性；请求日期不能替代实际估值。
use crate::assets::{add_minor, is_valid_calendar_date, AssetsSummary};
use crate::finance::{now_secs, FinanceError};
use crate::storage::Store;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueSnapshotRow {
    pub id: i64,
    pub taken_at: String,
    pub total_minor: i64,
    pub cash_minor: i64,
    pub noncash_minor: i64,
    pub quality: String,
}

fn save_snapshot(
    conn: &Connection,
    summary: &AssetsSummary,
    cash: i64,
    note: &str,
    date: &str,
    checked: bool,
) -> Result<i64, FinanceError> {
    if !is_valid_calendar_date(date) {
        return Err(FinanceError::Invalid("估值日期无效".into()));
    }
    let total = summary
        .complete_value_minor
        .or(summary.known_value_minor)
        .ok_or_else(|| FinanceError::Invalid("没有可保存的估值".into()))?;
    let through: i64 = conn.query_row("SELECT COALESCE(MAX(id),0) FROM cash_events", [], |r| {
        r.get(0)
    })?;
    let trades: i64 = conn.query_row("SELECT COALESCE(MAX(id),0) FROM transactions", [], |r| {
        r.get(0)
    })?;
    let detail = json!({"flowsChecked":checked,"transactionThrough":trades,"cashEventThrough":through,"revision":Store::data_revision(conn)?,
        "valuationBatch":summary.valuation_batch,"valuationFetchedAt":summary.valuation_fetched_at,"valuationStale":summary.valuation_stale,
        "attribution":crate::portfolio_attribution::capture(conn,summary)?});
    conn.execute("INSERT INTO value_snapshots(taken_at,total_minor,cash_minor,noncash_minor,quality,note,created_at,breakdown_json) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)", params![date,total,cash,add_minor(total,-cash,"非现金资产")?,summary.quality,note,now_secs(),detail.to_string()])?;
    Ok(conn.last_insert_rowid())
}

/// 供可信历史导入／测试使用，传入的是已核对的历史估值，页面不能调用此入口回填当前余额。
pub fn save_value_snapshot(
    store: &Store,
    summary: &AssetsSummary,
    cash: i64,
    note: &str,
    date: &str,
) -> Result<i64, FinanceError> {
    store.with_write(|c| save_snapshot(c, summary, cash, note, date, true))
}

pub fn save_current_snapshot(
    store: &Store,
    note: &str,
    flows_checked: bool,
) -> Result<i64, FinanceError> {
    let today = crate::finance::today_cn();
    store.with_write(|c| {
        // N04：保存快照消费统一估值（含共享行情批次），批次与质量写入 breakdown。
        let (summary, _) = crate::assets::summary_with_cached_quotes(c, &today)?;
        let cash = summary
            .manual
            .iter()
            .filter(|m| m.product_type == "cash" && !m.overlap_review)
            .try_fold(0, |n, m| add_minor(n, m.market_value_minor, "现金合计"))?;
        save_snapshot(c, &summary, cash, note, &today, flows_checked)
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReturnResult {
    pub computable: bool,
    pub reason: Option<String>,
    pub from_date: String,
    pub to_date: String,
    pub begin_total_minor: Option<i64>,
    pub end_total_minor: Option<i64>,
    pub net_external_minor: Option<i64>,
    pub net_return_minor: Option<i64>,
}

pub fn returns_for_range(
    conn: &Connection,
    from: &str,
    to: &str,
) -> Result<ReturnResult, FinanceError> {
    returns_with_start_order(conn, from, to, "ASC")
}

/// 日末序列取前一日最后一个核对快照，与曲线使用的日点一致。
pub(crate) fn daily_returns(
    conn: &Connection,
    from: &str,
    to: &str,
) -> Result<ReturnResult, FinanceError> {
    returns_with_start_order(conn, from, to, "DESC")
}

fn returns_with_start_order(
    conn: &Connection,
    from: &str,
    to: &str,
    start_order: &str,
) -> Result<ReturnResult, FinanceError> {
    returns_with_endpoint(conn, from, to, start_order, None)
}

pub(crate) fn live_reference(
    conn: &Connection,
    from: &str,
    to: &str,
    summary: &AssetsSummary,
) -> Result<ReturnResult, FinanceError> {
    let total = summary
        .complete_value_minor
        .ok_or_else(|| FinanceError::Invalid("Incomplete live valuation".into()))?;
    let meta = live_metadata(conn, summary)?;
    returns_with_endpoint(conn, from, to, "DESC", Some((-1, total, meta)))
}

pub(crate) fn live_metadata(
    conn: &Connection,
    summary: &AssetsSummary,
) -> Result<Value, FinanceError> {
    let events: i64 = conn.query_row("SELECT COALESCE(MAX(id),0) FROM cash_events", [], |r| {
        r.get(0)
    })?;
    let trades: i64 = conn.query_row("SELECT COALESCE(MAX(id),0) FROM transactions", [], |r| {
        r.get(0)
    })?;
    let meta = json!({"flowsChecked":false,"cashEventThrough":events,"transactionThrough":trades,"revision":Store::data_revision(conn)?,"attribution":crate::portfolio_attribution::capture(conn,summary)?});
    Ok(meta)
}

fn returns_with_endpoint(
    conn: &Connection,
    from: &str,
    to: &str,
    start_order: &str,
    live: Option<(i64, i64, Value)>,
) -> Result<ReturnResult, FinanceError> {
    if !is_valid_calendar_date(from) || !is_valid_calendar_date(to) || from > to {
        return Err(FinanceError::Invalid("收益区间日期无效或倒置".into()));
    }
    let mut out = ReturnResult {
        computable: false,
        reason: None,
        from_date: from.into(),
        to_date: to.into(),
        begin_total_minor: None,
        end_total_minor: None,
        net_external_minor: None,
        net_return_minor: None,
    };
    let load = |date: &str, order: &str| -> Result<Option<(i64, i64, Value)>, FinanceError> {
        // 日末取最新记录，包括未完成记录，不能跳过它沿用当天较早的完整估值。
        let quality_filter = if order == "ASC" {
            " AND quality='ready'"
        } else {
            ""
        };
        let sql = format!("SELECT id,total_minor,breakdown_json,quality FROM value_snapshots WHERE taken_at=?1 AND scope='A'{quality_filter} ORDER BY id {order} LIMIT 1");
        let row: Option<(i64, i64, String, String)> = conn
            .query_row(&sql, [date], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .optional()?;
        Ok(row.filter(|r| r.3 == "ready").map(|(id, total, text, _)| {
            (
                id,
                total,
                serde_json::from_str(&text).unwrap_or(Value::Null),
            )
        }))
    };
    let Some((begin_id, begin, bm)) = load(from, start_order)? else {
        out.reason = Some("missing_baseline".into());
        return Ok(out);
    };
    out.begin_total_minor = Some(begin);
    let reference = live.is_some();
    let Some((end_id, end, em)) = (match live {
        Some(end) => Some(end),
        None => load(to, "DESC")?,
    }) else {
        out.reason = Some("missing_end".into());
        return Ok(out);
    };
    if end_id == begin_id {
        out.reason = Some("missing_end".into());
        return Ok(out);
    }
    out.end_total_minor = Some(end);
    if reference {
        let Some(revision) = bm["revision"].as_str().and_then(|v| v.parse::<i64>().ok()) else {
            out.reason = Some("missing_external_flow".into());
            return Ok(out);
        };
        let changed:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM transactions t LEFT JOIN trade_cash x ON x.tx_id=t.id WHERE t.trade_date<=?1 AND (t.id>?2 OR x.changed_revision>?3))",params![from,bm["transactionThrough"].as_i64().unwrap_or(-1),revision],|r|r.get(0))?;
        if changed {
            out.reason = Some("unexplained_adjustment".into());
            return Ok(out);
        }
    }

    let Some(start_id) = bm["cashEventThrough"].as_i64() else {
        out.reason = Some("missing_external_flow".into());
        return Ok(out);
    };
    let Some(end_event_id) = em["cashEventThrough"].as_i64() else {
        out.reason = Some("missing_external_flow".into());
        return Ok(out);
    };
    if bm["flowsChecked"].as_bool() != Some(true)
        || (!reference && em["flowsChecked"].as_bool() != Some(true))
        || end_event_id < start_id
    {
        out.reason = Some("missing_external_flow".into());
        return Ok(out);
    }
    let mut stmt = conn.prepare("SELECT event_type,amount_minor,occurred_at FROM cash_events WHERE id>?1 AND id<=?2 ORDER BY id")?;
    let rows = stmt
        .query_map(params![start_id, end_event_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut external = 0;
    for (kind, amount, date) in rows {
        if date.as_str() < from
            || date.as_str() > to
            || matches!(kind.as_str(), "correction" | "reconciliation" | "opening")
        {
            out.reason = Some("unexplained_adjustment".into());
            return Ok(out);
        }
        match kind.as_str() {
            "external_in" => external = add_minor(external, amount, "净投入")?,
            "external_out" => {
                external = add_minor(
                    external,
                    amount
                        .checked_neg()
                        .ok_or_else(|| FinanceError::Overflow("外部转出".into()))?,
                    "净投入",
                )?
            }
            _ => {}
        }
    }
    // 后补历史流水会改变已保存收益区间，要求重新核对期末，不静默沿用。
    let backfilled: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM cash_events WHERE id>?1 AND occurred_at<=?2)",
        params![end_event_id, to],
        |r| r.get(0),
    )?;
    if backfilled {
        out.reason = Some("missing_external_flow".into());
        return Ok(out);
    }
    if let (Some(tx), Some(rev)) = (
        em["transactionThrough"].as_i64(),
        em["revision"].as_str().and_then(|s| s.parse::<i64>().ok()),
    ) {
        let changed:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM transactions t LEFT JOIN trade_cash x ON x.tx_id=t.id WHERE t.trade_date<=?1 AND (t.id>?2 OR x.changed_revision>?3))",params![to,tx,rev],|r|r.get(0))?;
        if changed {
            out.reason = Some("missing_external_flow".into());
            return Ok(out);
        }
    } else {
        out.reason = Some("missing_external_flow".into());
        return Ok(out);
    }
    out.net_external_minor = Some(external);
    let difference = add_minor(
        end,
        begin
            .checked_neg()
            .ok_or_else(|| FinanceError::Overflow("期初".into()))?,
        "净收益",
    )?;
    out.net_return_minor = Some(add_minor(
        difference,
        external
            .checked_neg()
            .ok_or_else(|| FinanceError::Overflow("净投入".into()))?,
        "净收益",
    )?);
    out.computable = true;
    if reference {
        out.reason = Some("live_reference".into());
    }
    Ok(out)
}

/// N04 累计投资收益投影（证券账本范围）：
/// 浮动收益（市值−剩余成本，成本未知者单列缺口）＋已实现收益（含清仓历史；
/// 未知期初成本的卖出计缺口不冒充零）＋现金分红/再投资（既有记账未含于
/// 成本或已实现）−独立费用事件（交易费用已在成本/已实现内，不重复扣）。
/// 外部投入/转出、内部转移、对账与更正不是收益。覆盖起点为账本最早事件；
/// 数据不全时保留已知部分并给出缺口，不伪造完整值。
pub(crate) fn cumulative_projection(
    conn: &rusqlite::Connection,
    summary: &crate::assets::AssetsSummary,
    today: &str,
) -> Result<Value, FinanceError> {
    // 已实现收益：期初/交易触及的全部（账户，标的）对，含清仓。
    let realized_rows = crate::securities_income::realized_rows(conn)?;
    let mut realized_known = 0i64;
    let mut realized_gap_count = 0usize;
    for (_, _, value) in &realized_rows {
        match value {
            Some(v) => {
                realized_known = crate::assets::add_minor(realized_known, *v, "已实现收益")
                    .map_err(|e| FinanceError::Invalid(e.message()))?;
            }
            None => realized_gap_count += 1,
        }
    }
    // 现金事件：分红/再投资为收益，独立费用为扣减；其余事件与本投影无关。
    let mut stmt = conn.prepare(
        "SELECT event_type, COALESCE(SUM(amount_minor),0) FROM cash_events
         WHERE event_type IN ('dividend_cash','dividend_reinvest','fee') GROUP BY event_type",
    )?;
    let mut dividends = 0i64;
    let mut unbooked_fees = 0i64;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    for (event_type, total) in rows {
        match event_type.as_str() {
            "dividend_cash" | "dividend_reinvest" => {
                dividends = crate::assets::add_minor(dividends, total, "分红收益")
                    .map_err(|e| FinanceError::Invalid(e.message()))?;
            }
            "fee" => {
                unbooked_fees = crate::assets::add_minor(unbooked_fees, total, "独立费用")
                    .map_err(|e| FinanceError::Invalid(e.message()))?;
            }
            _ => {}
        }
    }
    // 覆盖起点：期初、交易与收益相关现金事件的最早日期。
    let coverage_from: Option<String> = conn
        .query_row(
            "SELECT MIN(d) FROM (
                 SELECT MIN(as_of) AS d FROM opening_snapshots WHERE deleted_at IS NULL
                 UNION ALL
                 SELECT MIN(trade_date) AS d FROM transactions WHERE deleted_at IS NULL AND state='confirmed'
                 UNION ALL
                 SELECT MIN(occurred_at) AS d FROM cash_events
                  WHERE event_type IN ('dividend_cash','dividend_reinvest','fee')
             )",
            [],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten();
    // 累计 = 浮动 + 已实现 + 分红 − 独立费用；任一分量未知则整体显式未知。
    let total = match summary.floating_minor {
        Some(floating) => {
            let partial = crate::assets::add_minor(floating, realized_known, "累计收益")
                .map_err(|e| FinanceError::Invalid(e.message()))?;
            let partial = crate::assets::add_minor(partial, dividends, "累计收益")
                .map_err(|e| FinanceError::Invalid(e.message()))?;
            Some(
                crate::assets::add_minor(partial, -unbooked_fees, "累计收益")
                    .map_err(|e| FinanceError::Invalid(e.message()))?,
            )
        }
        None => None,
    };
    let complete = realized_gap_count == 0
        && summary.floating_unknown_cost_count == 0
        && summary.floating_minor.is_some();
    // wire 约定：*Minor 字段为整数分字符串（过 53 位 JSON 边界，UI 以 BigInt 消费）。
    let money = |v: Option<i64>| -> Value {
        match v {
            Some(raw) => Value::String(raw.to_string()),
            None => Value::Null,
        }
    };
    Ok(json!({
        "scope": "securities-ledger",
        "today": today,
        "floatingMinor": money(summary.floating_minor),
        "floatingUnknownCostCount": summary.floating_unknown_cost_count,
        "realizedMinor": money(Some(realized_known)),
        "realizedGapCount": realized_gap_count,
        "dividendsMinor": money(Some(dividends)),
        "unbookedFeesMinor": money(Some(unbooked_fees)),
        "totalMinor": money(total),
        "complete": complete,
        "coverageFrom": coverage_from,
        "coverageNote": if realized_gap_count > 0 || summary.floating_unknown_cost_count > 0 {
            Value::String("unknown-cost-gaps".into())
        } else {
            Value::Null
        },
    }))
}
