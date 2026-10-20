//! 日末真实证券估值与期间有效交易对账，不用当前数量回推历史。
use crate::assets::{add_minor, AssetsSummary};
use crate::finance::FinanceError;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn capture(conn: &Connection, summary: &AssetsSummary) -> Result<Value, FinanceError> {
    let mut securities = BTreeMap::<String, (i64, i64)>::new();
    let mut complete = true;
    for row in &summary.funds {
        let Some(value) = row.value_minor else {
            complete = false;
            continue;
        };
        if row.stale {
            complete = false;
        }
        let entry = securities.entry(row.fund_code.clone()).or_default();
        entry.0 = add_minor(entry.0, row.quantity_raw, "归因证券数量")?;
        entry.1 = add_minor(entry.1, value, "归因证券市值")?;
    }
    // Snapshot growth is bounded; excessive pending history remains an explicit gap.
    let mut stmt=conn.prepare("SELECT id FROM transactions WHERE state='pending' AND deleted_at IS NULL ORDER BY id LIMIT 2001")?;
    let mut pending = stmt
        .query_map([], |r| r.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    if pending.len() > 2000 {
        complete = false;
        pending.clear();
    }
    Ok(
        json!({"version":1,"complete":complete,"pendingTransactionIds":pending,
        "securities":securities.into_iter().map(|(code,(quantity,value))|json!({"code":code,"quantityRaw":quantity,"valueMinor":value})).collect::<Vec<_>>()}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Store;

    #[test]
    fn actual_endpoints_transactions_fees_income_and_transfer_reconcile_without_current_holdings() {
        let store = Store::open_in_memory().unwrap();
        let today = crate::finance::today_cn();
        let from =
            crate::assets::civil_from_days(crate::assets::days_from_civil(&today).unwrap() - 2);
        store.with_write(|c|->Result<(),FinanceError>{
            c.execute("INSERT INTO accounts(name,created_at,updated_at) VALUES('隔离','0','0')",[])?;
            c.execute("INSERT INTO funds(code,name,updated_at) VALUES('110022','隔离基金','0')",[])?;
            // 期初待确认买入，原交易日期早于区间；期末已确认，必须计入期间。
            c.execute("INSERT INTO transactions(account_id,fund_id,type,state,quantity_raw,price_raw,amount_raw,fee_raw,trade_date,source,created_at) VALUES(1,1,'BUY','confirmed',10000,10000,1000,10,?1,'manual','0')",[crate::assets::civil_from_days(crate::assets::days_from_civil(&from).unwrap()-1)])?;
            c.execute("INSERT INTO transactions(account_id,fund_id,type,state,quantity_raw,price_raw,amount_raw,fee_raw,trade_date,source,created_at) VALUES(1,1,'SELL','confirmed',20000,10000,2400,20,?1,'manual','0')",[&today])?;
            for kind in ["BUY","SELL"] { c.execute("INSERT INTO transactions(account_id,fund_id,type,state,quantity_raw,price_raw,amount_raw,trade_date,source,created_at) VALUES(1,1,?1,'confirmed',5000,10000,500,?2,'transfer','0')",params![kind,today])?; }
            for (kind,amount) in [("external_in",5000),("dividend_cash",100),("fee",30)] { c.execute("INSERT INTO cash_events(account_id,event_type,amount_minor,occurred_at,created_at) VALUES(1,?1,?2,?3,'0')",params![kind,amount,today])?; }
            for (date,total,tx,events,quantity,value,pending) in [(&from,10000,1,0,100000,8000,vec![1]),(&today,15900,4,3,90000,7500,vec![])] {
                let meta=json!({"flowsChecked":true,"transactionThrough":tx,"cashEventThrough":events,"revision":"0","attribution":{"version":1,"complete":true,"pendingTransactionIds":pending,"securities":[{"code":"110022","quantityRaw":quantity,"valueMinor":value}]}});
                c.execute("INSERT INTO value_snapshots(taken_at,total_minor,cash_minor,noncash_minor,quality,note,created_at,breakdown_json) VALUES(?1,?2,0,?2,'ready','','0',?3)",params![date,total,meta.to_string()])?;
            }
            Ok(())
        }).unwrap();
        let run = || store.with_read(|c| range(c, &from, &today)).unwrap();
        let out = run();
        assert_eq!(out["status"], "ready");
        assert_eq!(out["securitiesNetContributionMinor"], 870); // -500+2400-1000-30。
        assert_eq!(out["tradeFeesMinor"], 30);
        assert_eq!(out["unallocatedIncomeMinor"], 100);
        assert_eq!(out["cashFeesMinor"], 30);
        assert_eq!(out["otherContributionMinor"], -40);
        assert_eq!(out["totalNetReturnMinor"], 900);
        assert_eq!(out["items"][0]["netContributionMinor"], 870);
        // 期初／数量更正未被期间交易解释，不能算成收益。
        store.with_write(|c|c.execute("UPDATE value_snapshots SET breakdown_json=json_set(breakdown_json,'$.attribution.securities[0].quantityRaw',80000) WHERE taken_at=?1",[&today])).unwrap();
        assert_eq!(run()["reason"], "quantity_mismatch");
        store.with_write(|c|c.execute("UPDATE value_snapshots SET breakdown_json=json_remove(breakdown_json,'$.attribution') WHERE taken_at=?1",[&from])).unwrap();
        assert_eq!(run()["reason"], "missing_components");
    }

    #[test]
    fn new_snapshot_captures_true_zero_and_pending_but_old_snapshot_is_not_backfilled() {
        let store = Store::open_in_memory().unwrap();
        let today = crate::finance::today_cn();
        let summary = AssetsSummary {
            complete_value_minor: Some(100),
            known_value_minor: Some(100),
            quality: "ready",
            ..Default::default()
        };
        let id =
            crate::returns::save_value_snapshot(&store, &summary, 100, "隔离", &today).unwrap();
        let value: Value = store
            .with_read(|c| {
                c.query_row(
                    "SELECT breakdown_json FROM value_snapshots WHERE id=?1",
                    [id],
                    |r| r.get::<_, String>(0),
                )
            })
            .map(|s| serde_json::from_str(&s).unwrap())
            .unwrap();
        assert_eq!(value["attribution"]["complete"], true);
        assert_eq!(value["attribution"]["securities"], json!([]));
        assert_eq!(value["attribution"]["pendingTransactionIds"], json!([]));
    }
}

fn components(meta: &Value) -> Option<BTreeMap<String, (i64, i64)>> {
    let detail = &meta["attribution"];
    if detail["version"] != 1 || detail["complete"] != true {
        return None;
    }
    let mut out = BTreeMap::new();
    for row in detail["securities"].as_array()? {
        let code = row["code"].as_str()?;
        if crate::ledger::instrument_parts(code).is_err() {
            return None;
        }
        let quantity = row["quantityRaw"].as_i64()?;
        let value = row["valueMinor"].as_i64()?;
        if quantity < 0 || value < 0 || out.insert(code.to_string(), (quantity, value)).is_some() {
            return None;
        }
    }
    Some(out)
}

pub(crate) fn range(conn: &Connection, from: &str, to: &str) -> Result<Value, FinanceError> {
    range_impl(conn, from, to, None)
}
pub(crate) fn range_live(
    conn: &Connection,
    from: &str,
    to: &str,
    summary: &AssetsSummary,
) -> Result<Value, FinanceError> {
    range_impl(conn, from, to, Some(summary))
}
fn range_impl(
    conn: &Connection,
    from: &str,
    to: &str,
    live: Option<&AssetsSummary>,
) -> Result<Value, FinanceError> {
    let missing =
        |reason| json!({"status":"incomplete","reason":reason,"from":from,"to":to,"items":[]});
    let result = match live {
        Some(summary) => crate::returns::live_reference(conn, from, to, summary)?,
        None => crate::returns::daily_returns(conn, from, to)?,
    };
    if !result.computable {
        return Ok(missing("unverified_history"));
    }
    let load = |date: &str| -> Result<(i64, Value), FinanceError> {
        let (id,text):(i64,String)=conn.query_row("SELECT id,breakdown_json FROM value_snapshots WHERE scope='A' AND taken_at=?1 ORDER BY id DESC LIMIT 1",[date],|r|Ok((r.get(0)?,r.get(1)?)))?;
        Ok((id, serde_json::from_str(&text).unwrap_or(Value::Null)))
    };
    let (begin_id, bm) = load(from)?;
    let (end_id, em) = match live {
        Some(summary) => (-1, crate::returns::live_metadata(conn, summary)?),
        None => load(to)?,
    };
    let (Some(begin), Some(end)) = (components(&bm), components(&em)) else {
        return Ok(missing("missing_components"));
    };
    let (Some(start_tx), Some(end_tx)) = (
        bm["transactionThrough"].as_i64(),
        em["transactionThrough"].as_i64(),
    ) else {
        return Ok(missing("missing_components"));
    };
    if end_tx < start_tx {
        return Ok(missing("unverified_history"));
    }
    let Some(pending) = bm["attribution"]["pendingTransactionIds"].as_array() else {
        return Ok(missing("missing_components"));
    };
    let Some(pending) = pending
        .iter()
        .map(Value::as_i64)
        .collect::<Option<BTreeSet<_>>>()
    else {
        return Ok(missing("missing_components"));
    };
    // Iterate the bounded period plus the captured pending IDs without SQL interpolation.
    let mut stmt=conn.prepare("SELECT t.id,f.code,t.type,t.quantity_raw,t.amount_raw,t.fee_raw,t.trade_date,t.source FROM transactions t JOIN funds f ON f.id=t.fund_id WHERE t.id<=?1 AND t.state='confirmed' AND t.deleted_at IS NULL AND (t.id>?2 OR t.id IN (SELECT value FROM json_each(?3))) ORDER BY t.id")?;
    let pending_json = serde_json::to_string(&pending).unwrap();
    let rows = stmt
        .query_map(params![end_tx, start_tx, pending_json], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut flows = BTreeMap::<String, (i64, i64, i64)>::new();
    for (id, code, kind, quantity, amount, fee, date, source) in rows {
        if (date.as_str() < from && !pending.contains(&id))
            || date.as_str() > to
            || quantity < 0
            || amount < 0
            || fee < 0
        {
            return Ok(missing("unverified_history"));
        }
        let sign = if kind == "BUY" { 1 } else { -1 };
        let entry = flows.entry(code).or_default();
        entry.0 = add_minor(entry.0, quantity * sign, "归因数量")?;
        if source != "transfer" {
            entry.1 = add_minor(entry.1, amount * (-sign), "归因交易金额")?;
            entry.2 = add_minor(entry.2, fee, "归因交易费用")?;
        }
    }
    let codes: BTreeSet<_> = begin
        .keys()
        .chain(end.keys())
        .chain(flows.keys())
        .cloned()
        .collect();
    let mut items = Vec::new();
    let mut securities_total = 0;
    let mut fees_total = 0;
    for code in codes {
        let (bq, bv) = begin.get(&code).copied().unwrap_or_default();
        let (eq, ev) = end.get(&code).copied().unwrap_or_default();
        let (quantity, flow, fees) = flows.get(&code).copied().unwrap_or_default();
        if eq as i128 - bq as i128 != quantity as i128 {
            return Ok(missing("quantity_mismatch"));
        }
        let gross = add_minor(add_minor(ev, -bv, "归因估值变化")?, flow, "归因价格贡献")?;
        let net = add_minor(gross, -fees, "归因净贡献")?;
        securities_total = add_minor(securities_total, net, "证券贡献合计")?;
        fees_total = add_minor(fees_total, fees, "证券费用合计")?;
        items.push(json!({"code":code,"priceContributionMinor":gross,"feesMinor":fees,"netContributionMinor":net}));
    }
    let mut stmt=conn.prepare("SELECT event_type,amount_minor FROM cash_events WHERE id>?1 AND id<=?2 AND event_type IN ('dividend_cash','fee')")?;
    let mut income = 0;
    let mut cash_fees = 0;
    for row in stmt.query_map(
        params![
            bm["cashEventThrough"].as_i64().unwrap(),
            em["cashEventThrough"].as_i64().unwrap()
        ],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
    )? {
        let (kind, amount) = row?;
        if kind == "fee" {
            cash_fees = add_minor(cash_fees, amount, "现金费用")?;
        } else {
            income = add_minor(income, amount, "未分配收入")?;
        }
    }
    let total = result.net_return_minor.unwrap();
    let residual = i64::try_from(
        total as i128 - securities_total as i128 - income as i128 + cash_fees as i128,
    )
    .map_err(|_| FinanceError::Overflow("其他贡献".into()))?;
    items.sort_by_key(|item| {
        std::cmp::Reverse((item["netContributionMinor"].as_i64().unwrap() as i128).abs())
    });
    let count = items.len();
    items.truncate(10);
    Ok(
        json!({"status":if live.is_some(){"reference"}else{"ready"},"from":from,"to":to,"source":"value_snapshots+transactions+cash_events",
        "beginSnapshotId":begin_id,"endSnapshotId":end_id,"scope":"ledger-securities-and-explicit-residual",
        "totalNetReturnMinor":total,"securitiesNetContributionMinor":securities_total,"tradeFeesMinor":fees_total,
        "unallocatedIncomeMinor":income,"cashFeesMinor":cash_fees,"otherContributionMinor":residual,
        "itemCount":count,"items":items}),
    )
}
