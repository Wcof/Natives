//! Read-only valuation replay from registered quantities and published local NAV.
use crate::{assets, finance::FinanceError, holding_income};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub(crate) fn project(
    conn: &Connection,
    history: &mut Value,
    from: &str,
    to: &str,
) -> Result<(), FinanceError> {
    let mut stmt = conn.prepare("SELECT x.account_id,x.fund_id,f.code,COALESCE(m.nav_currency,'unknown') FROM (
        SELECT account_id,fund_id FROM opening_snapshots WHERE deleted_at IS NULL
        UNION SELECT account_id,fund_id FROM transactions WHERE deleted_at IS NULL AND state='confirmed'
        UNION SELECT account_id,fund_id FROM position_calibrations) x
        JOIN funds f ON f.id=x.fund_id LEFT JOIN fund_asset_metadata m ON m.fund_id=x.fund_id
        ORDER BY x.account_id,x.fund_id LIMIT 201")?;
    let identities = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if identities.is_empty() {
        return Ok(());
    }
    if identities.len() > 200 {
        history["replayReason"] = "record_limit".into();
        return Ok(());
    }
    let mut books = Vec::new();
    let mut prices = BTreeMap::new();
    let mut record_count = 0;
    for (account, fund, code, currency) in identities {
        let book = holding_income::load(conn, account, fund)?;
        record_count += book.record_count();
        if record_count > 10000 {
            history["replayReason"] = "record_limit".into();
            return Ok(());
        }
        let supported = crate::ledger::instrument_parts(&code)
            .is_ok_and(|(kind, _, _)| kind == "fund")
            && currency == "CNY";
        if supported && !prices.contains_key(&fund) {
            prices.insert(fund, holding_income::nav_prices(conn, fund, to)?);
        }
        books.push((account, fund, book, supported));
    }
    let Some(first) = books.iter().filter_map(|(_, _, b, _)| b.first_date()).min() else {
        return Ok(());
    };
    let end = assets::days_from_civil(to)
        .ok_or_else(|| FinanceError::Invalid("历史估值日期无效".into()))?;
    let start = assets::days_from_civil(first.max(from))
        .ok_or_else(|| FinanceError::Invalid("历史登记日期无效".into()))?;
    if start > end {
        return Ok(());
    }
    // Unknown manual/offline history must not silently disappear from total assets.
    let other_assets: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM manual_assets) OR EXISTS(SELECT 1 FROM offline_assets)",
        [],
        |r| r.get(0),
    )?;
    let plan = crate::group_plan::get_plan(conn).map_err(|e| FinanceError::Invalid(e.message()))?;
    let mut points: BTreeMap<String, Value> = history["points"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| p["date"].as_str().map(|d| (d.to_string(), p.clone())))
        .collect();
    let mut reconstructed = false;
    for day in start.max(end - 399)..=end {
        let date = assets::civil_from_days(day);
        if points.contains_key(&date) {
            continue;
        }
        let mut values = BTreeMap::new();
        let mut price_from: Option<&str> = None;
        let mut price_to: Option<&str> = None;
        for (account, fund, book, supported) in &books {
            if book.first_date().is_none_or(|first| first > date.as_str()) {
                continue;
            }
            let nav = prices.get(fund);
            let value = if *supported {
                nav.and_then(|p| book.valuation(&date, p))
            } else {
                None
            };
            if let Some((priced_at, _)) = nav.and_then(|p| p.range(..=date.clone()).next_back()) {
                price_from = Some(price_from.map_or(priced_at.as_str(), |d| d.min(priced_at)));
                price_to = Some(price_to.map_or(priced_at.as_str(), |d| d.max(priced_at)));
            }
            values.insert((*account, *fund), value);
        }
        let total = if other_assets {
            None
        } else {
            sum(values.values().copied())
        };
        let groups: Vec<Value> = plan.iter().flat_map(|p| &p.groups).map(|g| {
            let supported = g.manual_bindings.is_empty() && g.offline_bindings.is_empty() && g.cash_bindings.is_empty();
            let members: Vec<Option<i64>> = g.fund_bindings.iter().filter_map(|id|values.get(id).copied()).collect();
            let value = if supported && !members.is_empty() {sum(members.into_iter())} else {None};
                json!({"groupId":g.group_id,"valueMinor":value.map(|v|v.to_string()),"basis":"current-bindings"})
        }).collect();
        points.insert(
            date.clone(),
            json!({"date":date,"valueMinor":total.map(|v|v.to_string()),"quality":"reference",
            "groups":groups,"source":"holding-nav-replay","priceFrom":price_from,"priceTo":price_to,
            "periodReturnMinor":null,"twrPpm":null}),
        );
        reconstructed = true;
    }
    if reconstructed {
        let mut rows: Vec<Value> = points.into_values().collect();
        if rows.len() > 400 {
            rows.drain(..rows.len() - 400);
        }
        let mut bytes: usize = rows.iter().map(|p| p.to_string().len() + 1).sum();
        let mut skip = 0;
        while bytes > 95 * 1024 && skip + 2 < rows.len() {
            bytes -= rows[skip].to_string().len() + 1;
            skip += 1;
        }
        if skip > 0 {
            rows.drain(..skip);
            history["truncated"] = true.into();
        }
        history["from"] = if from.is_empty() {
            rows.first()
                .map(|p| p["date"].clone())
                .unwrap_or(Value::Null)
        } else {
            from.into()
        };
        history["points"] = rows.into();
        history["reconstructed"] = true.into();
        history["source"] = "snapshots-and-holding-nav-replay".into();
        history["status"] = "reference".into();
        history["twrPpm"] = Value::Null;
        if start < end - 399 {
            history["truncated"] = true.into();
        }
    }
    Ok(())
}

fn sum(mut values: impl Iterator<Item = Option<i64>>) -> Option<i64> {
    values.try_fold(0_i64, |total, value| total.checked_add(value?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        fixed::Fixed,
        group_plan::{GroupInput, GroupPlanInput},
        ledger,
        storage::Store,
    };
    fn fixture() -> (Store, i64, String, String) {
        let store = Store::open_in_memory().unwrap();
        let end = crate::finance::today_cn();
        let start = assets::minus_days(&end, 3);
        let fund = ledger::record_opening(
            &store,
            &ledger::OpeningInput {
                account_name: "A".into(),
                fund_code: "110022".into(),
                fund_name: "Fixture".into(),
                quantity: Fixed::share(100000),
                total_cost: None,
                as_of: start.clone(),
            },
            &end,
        )
        .unwrap();
        store.with_write(|c| -> rusqlite::Result<()> {
            c.execute("INSERT INTO fund_asset_metadata(fund_id,nav_currency,risk_class,updated_at) VALUES(?1,'CNY','equity','0') ON CONFLICT(fund_id) DO UPDATE SET nav_currency='CNY'",[fund])?;
            for n in 0..4 {
                c.execute("INSERT INTO fund_nav(fund_id,nav_date,unit_nav_raw,source,fetched_at) VALUES(?1,?2,?3,'user_confirmed','0')",rusqlite::params![fund,assets::minus_days(&end,3-n),10000+n*1000])?;
            }
            Ok(())
        }).unwrap();
        crate::group_plan::save_plan_tx(
            &store,
            &GroupPlanInput {
                expected_revision: 0,
                groups: vec![GroupInput {
                    group_id: "".into(),
                    name: "Fund category".into(),
                    description: "Historical NAV fixture".into(),
                    ratio_tenths: 1000,
                    sort_order: 0,
                    fund_bindings: vec![(1, fund)],
                    manual_bindings: vec![],
                    offline_bindings: vec![],
                    cash_bindings: vec![],
                }],
            },
        )
        .unwrap();
        (store, fund, start, end)
    }
    fn read(store: &Store, from: &str, to: &str) -> Value {
        store
            .with_read(|c| {
                let mut h = json!({"points":[],"twrPpm":null});
                project(c, &mut h, from, to)?;
                Ok::<_, FinanceError>(h)
            })
            .unwrap()
    }
    #[test]
    fn groups_have_real_history_and_mixed_assets_cannot_become_fund_only_totals() {
        let (store, _, start, end) = fixture();
        let h = read(&store, "", &end);
        assert_eq!(h["from"], start);
        assert_eq!(h["points"][0]["groups"][0]["valueMinor"], "1000");
        assert_eq!(h["points"][3]["groups"][0]["valueMinor"], "1300");
        assert_eq!(h["points"][0]["groups"][0]["basis"], "current-bindings");
        assert_eq!(h["points"][0]["priceFrom"], start);
        assert_eq!(
            read(&store, &start, &start)["points"][0]["valueMinor"],
            "1000"
        );
        assert_eq!(
            read(&store, &assets::minus_days(&end, 1), &end)["points"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        store.with_write(|c|c.execute("INSERT INTO cash_events(account_id,event_type,amount_minor,occurred_at,created_at) VALUES(1,'opening',1000,?1,'0')",[&start])).unwrap();
        assert_eq!(
            read(&store, "", &end)["points"][0]["valueMinor"],
            "1000",
            "cash events are flows, not additional assets"
        );
        store.with_write(|c|c.execute("INSERT INTO manual_assets(account_id,product_type,instrument_key,name,market_value_minor,valued_at,created_at,updated_at) VALUES(1,'cash','cash','Fixture cash',1000,?1,'0','0')",[&start])).unwrap();
        let mixed = read(&store, "", &end);
        assert!(mixed["points"][0]["valueMinor"].is_null());
        assert_eq!(mixed["points"][0]["groups"][0]["valueMinor"], "1000");
    }
    #[test]
    fn saved_history_wins_and_currency_and_price_gaps_are_explicit() {
        let (store, fund, start, end) = fixture();
        let mut h =
            json!({"points":[{"date":start,"valueMinor":"9999","quality":"ready","groups":[]}]});
        store.with_read(|c| project(c, &mut h, "", &end)).unwrap();
        assert_eq!(h["points"][0]["valueMinor"], "9999");
        assert!(
            h["points"][0]["groups"].as_array().unwrap().is_empty(),
            "never alter an older saved grouping"
        );
        store
            .with_write(|c| c.execute("DELETE FROM fund_nav WHERE nav_date=?1", [&start]))
            .unwrap();
        let missing = read(&store, "", &end);
        assert!(missing["points"][0]["valueMinor"].is_null());
        assert_eq!(missing["points"][1]["valueMinor"], "1100");
        store
            .with_write(|c| {
                c.execute(
                    "UPDATE fund_asset_metadata SET nav_currency='unknown' WHERE fund_id=?1",
                    [fund],
                )
            })
            .unwrap();
        assert!(read(&store, "", &end)["points"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["valueMinor"].is_null()));
        assert_eq!(sum([Some(i64::MAX), Some(1)].into_iter()), None);
    }
    #[test]
    fn lookback_is_bounded_and_never_generates_pre_registration_values() {
        let (store, _, start, end) = fixture();
        let h = read(&store, &assets::minus_days(&end, 1000), &end);
        assert_eq!(h["points"].as_array().unwrap().len(), 4);
        assert_eq!(h["points"][0]["date"], start);
        store
            .with_write(|c| {
                c.execute(
                    "UPDATE opening_snapshots SET as_of=?1",
                    [assets::minus_days(&end, 500)],
                )
            })
            .unwrap();
        let h = read(&store, "", &end);
        assert!((2..=400).contains(&h["points"].as_array().unwrap().len()));
        assert!(h["points"].to_string().len() <= 96 * 1024);
        assert_eq!(h["points"].as_array().unwrap().last().unwrap()["date"], end);
        assert_eq!(h["truncated"], true);
        assert!(h["points"][0]["valueMinor"].is_null());
    }
}
