use super::*;
use crate::storage::Store;

fn prices() -> BTreeMap<String, i64> {
    [
        ("2025-12-31", 100000),
        ("2026-01-05", 110000),
        ("2026-01-06", 120000),
        ("2026-01-07", 130000),
    ]
    .into_iter()
    .map(|(d, p)| (d.into(), p))
    .collect()
}
fn book() -> IncomeBook {
    IncomeBook {
        opening: Some((100000, Some(10000), "2025-12-31".into())),
        operations: vec![],
        truncated: false,
        calibrations: vec![],
    }
}
#[test]
fn daily_dates_and_money_percentage_use_real_quantities_and_fees() {
    let mut b = book();
    b.operations.push((
        "BUY".into(),
        50000,
        5500,
        10,
        "2026-01-05".into(),
        "manual".into(),
    ));
    b.operations.push((
        "SELL".into(),
        30000,
        3600,
        10,
        "2026-01-06".into(),
        "manual".into(),
    ));
    let p = prices();
    let out = b.project("2026-01-07", &p, Some(15600), Some(12408), Some(488));
    assert_eq!(out["day"]["date"], "2026-01-07");
    assert_eq!(out["day"]["amount"], "12");
    assert_eq!(out["day"]["rateBps"], "833");
    assert_eq!(out["yesterday"]["date"], "2026-01-06");
    assert_eq!(out["yesterday"]["amount"], "14.9"); // 15 shares before sale, fee 0.10.
    assert_eq!(out["week"]["amount"], "36.8"); // 156 − 100 − 55.1 + 35.9.
    assert_eq!(out["week"]["rateBps"], "2372");
    assert_eq!(out["holding"]["amount"], "31.92");
    assert_eq!(out["cumulative"]["amount"], "36.8");
    assert_eq!(out["cumulative"]["rateBps"], "2372");
}
#[test]
fn transfers_are_valued_flows_and_not_artificial_profit() {
    let mut b = book();
    b.operations.push((
        "SELL".into(),
        50000,
        5000,
        0,
        "2026-01-06".into(),
        "transfer".into(),
    ));
    let out = b.range("2026-01-05", "2026-01-07", &prices());
    assert_eq!(out["amount"], "25"); // 65 − 100 + 60; transfer book cost 50 is not flow value.
    let mut incoming = book();
    incoming.opening = None;
    incoming.operations.push((
        "BUY".into(),
        50000,
        5000,
        0,
        "2026-01-06".into(),
        "transfer".into(),
    ));
    assert_eq!(
        incoming.range("2026-01-05", "2026-01-07", &prices())["amount"],
        "5"
    );
}
#[test]
fn gaps_zero_basis_unknown_cost_and_pending_free_holding_age_are_explicit() {
    let b = book();
    let p = BTreeMap::from([("2026-01-07".into(), 130000)]);
    assert_eq!(
        b.range("2026-01-07", "2026-01-07", &p)["reason"],
        "missing_baseline"
    );
    assert_eq!(
        b.project("2026-01-08", &p, Some(13000), None, None)["holding"]["reason"],
        "unknown_cost"
    );
    assert!(
        b.project("2026-01-08", &prices(), None, Some(10000), Some(0))["day"]["amount"].is_null()
    );
    assert!(result(10, Some(0), "2026-01-07", None)["rateBps"].is_null());
    assert_eq!(
        result(i64::MAX as i128 + 1, Some(1), "2026-01-07", None)["reason"],
        "overflow"
    );
    let mut closed = book();
    closed.operations = vec![
        (
            "SELL".into(),
            100000,
            10000,
            0,
            "2026-01-05".into(),
            "manual".into(),
        ),
        (
            "BUY".into(),
            10000,
            1000,
            0,
            "2026-01-06".into(),
            "manual".into(),
        ),
    ];
    assert_eq!(closed.holding_days("2026-01-07"), Some(1));
    let holiday = b.project("2026-01-11", &prices(), Some(13000), Some(10000), Some(0));
    assert_eq!(holiday["day"]["date"], "2026-01-11");
    assert!(holiday["day"]["amount"].is_null());
    assert_eq!(holiday["yesterday"]["date"], "2026-01-10");
    assert!(holiday["yesterday"]["amount"].is_null());
    let next = b.project("2026-01-08", &prices(), Some(13000), Some(10000), Some(0));
    assert!(next["day"]["amount"].is_null());
    assert_eq!(next["yesterday"]["date"], "2026-01-07");
    assert_eq!(next["yesterday"]["amount"], "10");
}
#[test]
fn only_unadjusted_valid_history_and_visible_nav_are_accepted() {
    assert!(market_prices(
        &json!({"adjustment":"qfq","candles":[{"time":"2026-01-07","close":13}]}),
        "2026-01-07"
    )
    .is_empty());
    let store = Store::open_in_memory().unwrap();
    store.with_write(|c| {
        c.execute("INSERT INTO funds(code,name,updated_at) VALUES('110022','F','0')",[])?;
        for (date,raw,source) in [("2026-01-05",0,"untrusted"),("2026-01-06",0,"user_confirmed"),("2026-01-07",130000,"test")] {
            c.execute("INSERT INTO fund_nav(fund_id,nav_date,unit_nav_raw,source,fetched_at) VALUES(1,?1,?2,?3,'0')",params![date,raw,source])?;
        }
        Ok::<_,rusqlite::Error>(())
    }).unwrap();
    assert_eq!(
        store
            .with_read(|c| nav_prices(c, 1, "2026-01-07"))
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn source_records_are_scoped_paginated_and_opening_edits_are_versioned() {
    let store = Store::open_in_memory().unwrap();
    let today = crate::finance::today_cn();
    let fund = crate::ledger::record_opening(
        &store,
        &crate::ledger::OpeningInput {
            account_name: "A".into(),
            fund_code: "110022".into(),
            fund_name: "F".into(),
            quantity: Fixed::share(100000),
            total_cost: Some(Fixed::amount(2000)),
            as_of: today.clone(),
        },
        &today,
    )
    .unwrap();
    let before = super::super::positions::api_holding_records(
        &store,
        &format!("/api/holding/records?accountId=1&fundId={fund}&page=1"),
    )
    .unwrap();
    let before: Value = serde_json::from_str(&before).unwrap();
    assert_eq!(before["opening"]["quantity"], "10");
    assert_eq!(before["opening"]["totalCost"], "20");
    let invalid = json!({"quantity":"0","totalCost":"-1","asOf":today});
    assert!(crate::opening_ops::change(&store, 1, fund, 1, "update", &invalid).is_err());
    assert!(
        crate::opening_ops::list(&store, 1, fund).unwrap()["history"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let update = json!({"quantity":"12.1234","totalCost":"0","asOf":today});
    crate::opening_ops::change(&store, 1, fund, 1, "update", &update).unwrap();
    assert!(crate::opening_ops::change(&store, 1, fund, 1, "update", &update).is_err());
    let position = store.with_read(crate::portfolio::list_positions).unwrap();
    assert_eq!(position[0].quantity_raw, 121234);
    assert_eq!(position[0].cost_raw, 0);
    for n in 0..22 {
        crate::ledger::record_transaction(
            &store,
            &crate::ledger::TransactionInput::manual(
                "A".into(),
                "110022".into(),
                "F".into(),
                "BUY".into(),
                "confirmed".into(),
                Fixed::share(10000),
                Fixed::nav(10000),
                Fixed::amount(0),
                today.clone(),
                format!("edit-page-{n}"),
            ),
        )
        .unwrap();
    }
    for (page, count, more) in [(1, 20, true), (2, 2, false)] {
        let value: Value = serde_json::from_str(
            &super::super::positions::api_holding_records(
                &store,
                &format!("/api/holding/records?accountId=1&fundId={fund}&page={page}"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(value["transactions"].as_array().unwrap().len(), count);
        assert_eq!(value["hasMore"], more);
    }
    assert!(super::super::positions::api_holding_records(
        &store,
        "/api/holding/records?accountId=-1&fundId=1"
    )
    .is_err());
}

#[test]
fn historical_income_uses_actual_quantities_fees_and_calibration_boundaries() {
    let mut b = book();
    b.operations.push((
        "BUY".into(),
        50000,
        5500,
        10,
        "2026-01-05".into(),
        "manual".into(),
    ));
    b.operations.push((
        "SELL".into(),
        30000,
        3600,
        10,
        "2026-01-06".into(),
        "manual".into(),
    ));
    let rows = b.history("2026-01-01", "2026-01-07", &prices());
    assert_eq!(
        rows.iter()
            .map(|r| r["amount"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["9.9", "14.9", "12"]
    );
    assert_eq!(
        b.history("2026-01-07", "2026-01-07", &prices())[0]["amount"],
        "12"
    );
    b.calibrations.push(("2026-01-06".into(), 200000));
    let calibrated = b.history("2026-01-05", "2026-01-07", &prices());
    assert_eq!(calibrated[1]["reason"], "calibration_boundary");
    assert_eq!(calibrated[2]["amount"], "20");
    let mut late = book();
    late.opening = Some((100000, None, "2026-01-06".into()));
    assert!(late
        .history("2026-01-01", "2026-01-07", &prices())
        .iter()
        .all(|r| r["date"].as_str().unwrap() >= "2026-01-06"));
    assert!(b.history("2026-01-08", "2026-01-08", &prices())[0]["amount"].is_null());
}
