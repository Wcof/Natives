use super::*;
use crate::{
    assets,
    ledger::{self, OpeningInput},
    storage::Store,
};
use std::collections::BTreeMap;

#[test]
fn daily_zero_missing_basis_old_dates_and_overflow_are_explicit() {
    let store = Store::open_in_memory().unwrap();
    let today = crate::finance::today_cn();
    ledger::record_opening(
        &store,
        &OpeningInput {
            account_name: "A".into(),
            fund_code: "stock:SSE:600519".into(),
            fund_name: "S".into(),
            quantity: Fixed::share(10000),
            total_cost: Some(Fixed::amount(100)),
            as_of: today.clone(),
        },
        &today,
    )
    .unwrap();
    let mut row = store
        .with_read(|c| assets::collect(c, &today))
        .unwrap()
        .funds
        .remove(0);
    row.price_as_of = Some(today.clone());
    row.price_raw = Some(0);
    row.value_minor = Some(0);
    row.day_change_raw = Some(0);
    let zero = store.with_read(|c| daily(c, &row, &today)).unwrap();
    assert_eq!(zero["amount"], "0");
    assert!(zero["rateBps"].is_null());
    row.day_change_raw = Some(10000);
    assert_eq!(
        store.with_read(|c| daily(c, &row, &today)).unwrap()["reason"],
        "missing_baseline"
    );
    row.price_as_of = Some(assets::minus_days(&today, 1));
    assert_eq!(
        store.with_read(|c| daily(c, &row, &today)).unwrap()["reason"],
        "missing_daily_price"
    );
    row.price_as_of = Some(today.clone());
    row.value_minor = None;
    assert_eq!(
        store.with_read(|c| daily(c, &row, &today)).unwrap()["reason"],
        "unverified_valuation"
    );
    row.value_minor = Some(0);
    row.price_raw = Some(i64::MAX);
    row.day_change_raw = Some(i64::MAX);
    row.quantity_raw = i64::MAX;
    assert!(matches!(
        store.with_read(|c| daily(c, &row, &today)),
        Err(AssetError::Overflow(_))
    ));
}

#[test]
fn home_and_positions_share_precise_daily_scope_and_missing_coverage() {
    let store = Store::open_in_memory().unwrap();
    let today = crate::finance::today_cn();
    let yesterday = assets::minus_days(&today, 1);
    for (code, quantity, cost) in [
        ("110022", "10", "10"),
        ("stock:SSE:600519", "100", "100"),
        ("etf:SSE:510300", "200", "200"),
    ] {
        ledger::record_opening(
            &store,
            &OpeningInput {
                account_name: "A".into(),
                fund_code: code.into(),
                fund_name: code.into(),
                quantity: Fixed::parse(quantity, 4).unwrap(),
                total_cost: Some(Fixed::parse(cost, 2).unwrap()),
                as_of: yesterday.clone(),
            },
            &today,
        )
        .unwrap();
    }
    store.with_write(|c| {
        c.execute("INSERT INTO fund_asset_metadata(fund_id,nav_currency,risk_class,updated_at) SELECT id,'CNY','equity','0' FROM funds WHERE code='110022' ON CONFLICT(fund_id) DO UPDATE SET nav_currency='CNY',risk_class='equity'",[])?;
        let id:i64=c.query_row("SELECT id FROM funds WHERE code='110022'",[],|r|r.get(0))?;
        for (date,price) in [(&yesterday,12000),(&today,13000)] {
            c.execute("INSERT INTO fund_nav(fund_id,nav_date,unit_nav_raw,source,fetched_at) VALUES(?1,?2,?3,'user_confirmed','0')",params![id,date,price])?;
        }
        Ok::<_,rusqlite::Error>(())
    }).unwrap();
    let prices = BTreeMap::from([
        (
            "sh600519".into(),
            assets::QuoteInput {
                price_raw: 20000,
                change_raw: Some(500),
                as_of: Some(today.clone()),
                source: "test".into(),
            },
        ),
        (
            "sh510300".into(),
            assets::QuoteInput {
                price_raw: 30000,
                change_raw: Some(-1000),
                as_of: Some(today.clone()),
                source: "test".into(),
            },
        ),
    ]);
    let shared = store
        .with_read(|c| {
            let mut collected = assets::collect(c, &today)?;
            assets::apply_quotes(&mut collected, &prices, &today);
            let summary = assets::finalize(collected, &today)?;
            ledger(c, &summary, &today)
        })
        .unwrap();
    assert_eq!(shared["marketValueMinor"], "81300");
    assert_eq!(shared["holdingProfitMinor"], "50300");
    assert_eq!(shared["day"]["amountMinor"], "-1400"); // 1 + 5 - 20 yuan.
    assert_eq!(shared["day"]["missingCount"], 0);
    assert_eq!(shared["byType"]["stock"]["marketValueMinor"], "20000");
    assert_eq!(shared["byType"]["stock"]["holdingProfitMinor"], "10000");
    assert_eq!(shared["byType"]["stock"]["day"]["amountMinor"], "500");
    assert_eq!(shared["byType"]["fund"]["marketValueMinor"], "61300");
    assert_eq!(shared["byType"]["fund"]["holdingProfitMinor"], "40300");
    assert_eq!(shared["byType"]["fund"]["day"]["amountMinor"], "-1900");
    store
        .with_write(|c| c.execute("DELETE FROM fund_nav WHERE nav_date=?1", [&today]))
        .unwrap();
    let partial = store
        .with_read(|c| {
            let mut collected = assets::collect(c, &today)?;
            assets::apply_quotes(&mut collected, &prices, &today);
            ledger(c, &assets::finalize(collected, &today)?, &today)
        })
        .unwrap();
    assert_eq!(partial["day"]["missingCount"], 1);
    assert_eq!(partial["day"]["amountMinor"], "-1500");
    assert_eq!(partial["day"]["complete"], false);
    assert_eq!(partial["byType"]["stock"]["day"]["complete"], true);
    assert_eq!(partial["byType"]["fund"]["day"]["missingCount"], 1);
    store
        .with_write(|c| c.execute("UPDATE accounts SET archived_at='0'", []))
        .unwrap();
    let empty = store
        .with_read(|c| ledger(c, &assets::assets_summary(c, &today)?, &today))
        .unwrap();
    assert_eq!(empty["holdingCount"], 0);
    assert!(empty["day"]["amountMinor"].is_null());
}

#[test]
fn classified_realized_returns_keep_closed_positions_and_isolate_unknown_cost() {
    let store = Store::open_in_memory().unwrap();
    let today = crate::finance::today_cn();
    let yesterday = assets::minus_days(&today, 1);
    for code in ["stock:SSE:600519", "etf:SSE:510300", "110022"] {
        ledger::record_opening(
            &store,
            &OpeningInput {
                account_name: "A".into(),
                fund_code: code.into(),
                fund_name: code.into(),
                quantity: Fixed::parse("10", 4).unwrap(),
                total_cost: Some(Fixed::parse("10", 2).unwrap()),
                as_of: yesterday.clone(),
            },
            &today,
        )
        .unwrap();
        ledger::record_transaction(
            &store,
            &ledger::TransactionInput::manual(
                "A".into(),
                code.into(),
                code.into(),
                "SELL".into(),
                "confirmed".into(),
                Fixed::parse("10", 4).unwrap(),
                Fixed::parse("2", 4).unwrap(),
                Fixed::amount(0),
                today.clone(),
                format!("close-{code}"),
            ),
        )
        .unwrap();
    }
    let project = || {
        store
            .with_read(|c| ledger(c, &assets::assets_summary(c, &today)?, &today))
            .unwrap()
    };
    let result = project();
    assert_eq!(result["byType"]["stock"]["holdingCount"], 0);
    assert_eq!(result["byType"]["stock"]["realizedMinor"], "1000");
    assert_eq!(result["byType"]["fund"]["realizedMinor"], "2000");
    store.with_write(|c|c.execute("UPDATE opening_snapshots SET cost_raw=NULL WHERE fund_id=(SELECT id FROM funds WHERE code='110022')",[])).unwrap();
    let gap = project();
    assert!(gap["byType"]["fund"]["realizedMinor"].is_null());
    assert_eq!(gap["byType"]["stock"]["realizedMinor"], "1000");
}
