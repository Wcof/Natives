use super::*;
use crate::assets::FundValuationRow;
use crate::storage::Store;

fn holding(account: i64, code: &str, value: Option<i64>, cost: i64) -> FundValuationRow {
    FundValuationRow {
        account_id: account,
        account_name: "隔离账户".into(),
        fund_id: account,
        value_source: if value.is_some() {
            "nav".into()
        } else {
            "none".into()
        },
        price_raw: value.map(|_| 10_000),
        price_as_of: None,
        day_change_raw: None,
        instrument_kind: "stock".into(),
        market: "SSE".into(),
        fund_code: code.into(),
        fund_name: "隔离证券".into(),
        quantity_raw: 10_000,
        cost_raw: cost,
        cost_unknown: false,
        holding_as_of: "2026-01-01".into(),
        nav_raw: Some(10_000),
        nav_date: None,
        nav_source: None,
        nav_currency: "CNY".into(),
        risk_class: "equity".into(),
        value_minor: value,
        value_gap: None,
        stale: false,
    }
}

#[test]
fn concentration_merges_security_across_accounts_and_marks_gaps() {
    let store = Store::open_in_memory().unwrap();
    let mut summary = AssetsSummary {
        funds: vec![
            holding(1, "stock:SSE:600519", Some(300), 0),
            holding(2, "stock:SSE:600519", Some(300), 100),
            holding(3, "510300", Some(400), 200),
        ],
        ..Default::default()
    };
    let today = crate::finance::today_cn();
    let run = |s: &AssetsSummary| store.with_read(|c| overview(c, s, &today)).unwrap();
    let out = run(&summary);
    assert_eq!(out["diagnostics"]["hhiBps"], 5200);
    assert_eq!(out["diagnostics"]["securityCount"], 2);
    assert_eq!(out["securities"]["unrealizedMinor"], 700); // True zero cost is known.
    summary.funds[2].value_minor = None;
    summary.funds[1].cost_unknown = true;
    let out = run(&summary);
    assert_eq!(out["diagnostics"]["coverage"], "partial");
    assert_eq!(out["diagnostics"]["hhiBps"], 10_000);
    assert!(out["securities"]["unrealizedMinor"].is_null());
    summary.funds[0].value_minor = Some(0);
    summary.funds[1].value_minor = Some(0);
    assert!(run(&summary)["diagnostics"]["hhiBps"].is_null());
    let largest = i64::MAX as i128;
    assert_eq!(fraction_bps(largest * largest, largest * largest), 10_000);
    assert_eq!(fraction_bps(largest * largest / 2, largest * largest), 4999);
}

#[test]
fn legacy_manual_securities_share_identity_without_inventing_cost_or_double_counting() {
    let store = Store::open_in_memory().unwrap();
    let today = crate::finance::today_cn();
    let mut summary = AssetsSummary {
        funds: vec![holding(1, "stock:SSE:600519", Some(600), 100)],
        manual: vec![crate::assets::ManualAssetRow {
            id: 1,
            account_id: 2,
            account_name: "人工账户".into(),
            product_type: "stock".into(),
            instrument_key: "security:SSE:600519".into(),
            name: "人工证券".into(),
            risk_class: "equity".into(),
            market_value_minor: 400,
            valued_at: today.clone(),
            stale: false,
            overlap_review: false,
            legacy_overlap_distinct: false,
        }],
        ..Default::default()
    };
    let run = |s: &AssetsSummary| store.with_read(|c| overview(c, s, &today)).unwrap();
    let out = run(&summary);
    assert_eq!(out["diagnostics"]["securityCount"], 1);
    assert_eq!(out["diagnostics"]["hhiBps"], 10000);
    assert_eq!(out["securities"]["knownValueMinor"], 1000);
    assert!(out["securities"]["unrealizedMinor"].is_null());
    assert_eq!(out["securities"]["knownUnrealizedMinor"], 500);
    summary.manual[0].overlap_review = true;
    let out = run(&summary);
    assert_eq!(out["securities"]["knownValueMinor"], 600);
    assert_eq!(out["securities"]["excludedOverlapCount"], 1);
    assert_eq!(out["diagnostics"]["coverage"], "partial");
}

#[test]
fn health_projection_aggregates_types_accounts_and_liquidity_from_one_batch() {
    let store = Store::open_in_memory().unwrap();
    let today = crate::finance::today_cn();
    let mut summary = AssetsSummary {
        funds: vec![
            holding(1, "stock:SSE:600519", Some(300), 0),
            {
                let mut r = holding(2, "etf:SSE:510300", None, 100);
                r.account_name = "二账户".into();
                r.value_gap = Some("missing_nav".into());
                r.instrument_kind = "etf".into();
                r
            },
            {
                let mut r = holding(3, "fund:CN:110022", Some(500), 200);
                r.instrument_kind = "fund".into();
                r.market = "CN".into();
                r
            },
        ],
        manual: vec![crate::assets::ManualAssetRow {
            id: 1,
            account_id: 1,
            account_name: "隔离账户".into(),
            product_type: "etf".into(),
            instrument_key: "security:SSE:510300".into(),
            name: "人工ETF".into(),
            risk_class: "equity".into(),
            market_value_minor: 200,
            valued_at: today.clone(),
            stale: false,
            overlap_review: false,
            legacy_overlap_distinct: false,
        }],
        ..Default::default()
    };
    let run = |s: &AssetsSummary| store.with_read(|c| overview(c, s, &today)).unwrap();
    let h = run(&summary)["health"].clone();
    // 三类：stock 已知 300；etf 已知 200（缺价行不入值）；场外 fund 500。
    let types = h["types"].as_array().unwrap();
    let stock = types
        .iter()
        .find(|t| t["instrumentKind"] == "stock")
        .unwrap();
    assert_eq!(stock["knownValueMinor"], 300);
    assert_eq!(stock["holdingCount"], 1);
    let etf = types.iter().find(|t| t["instrumentKind"] == "etf").unwrap();
    assert_eq!(etf["knownValueMinor"], 200);
    assert_eq!(etf["missingValueCount"], 1);
    assert_eq!(etf["holdingCount"], 2, "manual etf shares the type bucket");
    // 分母为同一批次已知证券现值 1000；stock 权重 300/1000=30%。
    assert_eq!(stock["weightBps"], 3000);
    // 账户维度聚合跨两行（账户1 stock+manual etf）。
    let accounts = h["accounts"].as_array().unwrap();
    let a1 = accounts.iter().find(|a| a["accountId"] == 1).unwrap();
    assert_eq!(a1["holdingCount"], 2);
    assert_eq!(a1["knownValueMinor"], 500);
    assert_eq!(h["accountsTruncated"], false);
    // 流动性事实来自真实表计数（此处空库）。
    assert_eq!(h["liquidity"]["otcPendingCount"], 0);
    assert_eq!(h["liquidity"]["offlineCount"], 0);
    // 未知成本行进入账户/类型缺口计数。
    summary.funds[2].cost_unknown = true;
    let h = run(&summary)["health"].clone();
    let fund = h["types"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["instrumentKind"] == "fund")
        .unwrap();
    assert_eq!(fund["unknownCostCount"], 1);
}

#[test]
fn closed_realized_return_is_retained_but_unknown_opening_sale_is_not_zero() {
    let store = Store::open_in_memory().unwrap();
    let today = crate::finance::today_cn();
    for (account, cost) in [
        ("已知", Some(crate::fixed::Fixed::amount(10000))),
        ("未知", None),
    ] {
        let opening = crate::ledger::OpeningInput {
            account_name: account.into(),
            fund_code: "stock:SSE:600519".into(),
            fund_name: "隔离证券".into(),
            quantity: crate::fixed::Fixed::nav(10000),
            as_of: today.clone(),
            total_cost: cost,
        };
        crate::ledger::record_opening(&store, &opening, &today).unwrap();
        let tx = crate::ledger::TransactionInput::manual(
            account.into(),
            opening.fund_code.clone(),
            opening.fund_name.clone(),
            "SELL".into(),
            "confirmed".into(),
            crate::fixed::Fixed::nav(10000),
            crate::fixed::Fixed::nav(1_200_000),
            crate::fixed::Fixed::amount(0),
            today.clone(),
            format!("closed-{account}"),
        );
        crate::ledger::record_transaction(&store, &tx).unwrap();
        let out = store
            .with_read(|c| {
                let (summary, _) = crate::assets::summary_with_cached_quotes(c, &today)?;
                overview(c, &summary, &today)
            })
            .unwrap();
        assert_eq!(out["securities"]["holdingCount"], 0);
        assert_eq!(out["securities"]["knownRealizedMinor"], 2000);
        if cost.is_none() {
            assert!(out["securities"]["realizedMinor"].is_null());
            assert_eq!(out["securities"]["realizedGapCount"], 1);
        } else {
            assert_eq!(out["securities"]["realizedMinor"], 2000);
        }
    }
}

#[test]
fn daily_history_excludes_deposits_uses_latest_daily_snapshot_and_rejects_gaps() {
    let store = Store::open_in_memory().unwrap();
    let today = crate::finance::today_cn();
    let start = days_from_civil(&today).unwrap() - 60;
    store.with_write(|c| -> Result<(),FinanceError> {
        c.execute("INSERT INTO accounts(name,created_at,updated_at) VALUES('隔离','0','0')",[])?;
        let deposit_date = crate::assets::civil_from_days(start+20);
        c.execute("INSERT INTO cash_events(account_id,event_type,amount_minor,occurred_at,created_at) VALUES(1,'external_in',10000,?1,'0')",[deposit_date])?;
        for day in 0..=60 {
            let date = crate::assets::civil_from_days(start+day);
            let total = if day<20 {10000} else if day==20 {20000} else {22000};
            let meta=json!({"flowsChecked":true,"cashEventThrough":if day<20 {0} else {1},"transactionThrough":0,"revision":"0"}).to_string();
            if day==20 {
                c.execute("INSERT INTO value_snapshots(taken_at,total_minor,cash_minor,noncash_minor,quality,note,created_at,breakdown_json) VALUES(?1,19000,19000,0,'ready','','0',?2)",params![date,meta])?;
            }
            c.execute("INSERT INTO value_snapshots(taken_at,total_minor,cash_minor,noncash_minor,quality,note,created_at,breakdown_json) VALUES(?1,?2,?2,0,'ready','','0',?3)",params![date,total,meta])?;
        }
        Ok(())
    }).unwrap();
    let h = store.with_read(|c| history(c, &today)).unwrap();
    assert_eq!(h["status"], "ready");
    assert_eq!(h["points"].as_array().unwrap().len(), 61);
    assert_eq!(h["points"][20]["valueMinor"], 20000);
    assert_eq!(h["twrPpm"], "100000");
    assert!(h["benchmark"]["excessPpm"].is_null());
    // 最新日末记录未完成时，不得回退到同日较早的 ready 记录。
    store.with_write(|c| c.execute("INSERT INTO value_snapshots(taken_at,total_minor,cash_minor,noncash_minor,quality,note,created_at,breakdown_json) SELECT taken_at,total_minor,cash_minor,noncash_minor,'incomplete','','0',breakdown_json FROM value_snapshots WHERE taken_at=?1 ORDER BY id DESC LIMIT 1", [&today])).unwrap();
    let daily = store
        .with_read(|c| {
            crate::returns::daily_returns(c, &crate::assets::civil_from_days(start + 59), &today)
        })
        .unwrap();
    assert_eq!(daily.reason.as_deref(), Some("missing_end"));
    let baseline = store
        .with_read(|c| crate::returns::daily_returns(c, &today, &today))
        .unwrap();
    assert_eq!(baseline.reason.as_deref(), Some("missing_baseline"));
    store
        .with_write(|c| {
            c.execute(
                "DELETE FROM value_snapshots WHERE taken_at=?1 AND quality='incomplete'",
                [&today],
            )
        })
        .unwrap();
    store.with_write(|c|c.execute("UPDATE value_snapshots SET breakdown_json=json_set(breakdown_json,'$.flowsChecked',json('false')) WHERE taken_at=?1",[crate::assets::civil_from_days(start)])).unwrap();
    let h = store.with_read(|c| history(c, &today)).unwrap();
    assert_eq!(h["reason"], "unverified_history");
    assert!(h["twrPpm"].is_null());
    store.with_write(|c|c.execute("UPDATE value_snapshots SET breakdown_json=json_set(breakdown_json,'$.flowsChecked',json('true')) WHERE taken_at=?1",[crate::assets::civil_from_days(start)])).unwrap();
    store
        .with_write(|c| {
            c.execute(
                "DELETE FROM value_snapshots WHERE taken_at=?1",
                [crate::assets::civil_from_days(start + 25)],
            )
        })
        .unwrap();
    let h = store.with_read(|c| history(c, &today)).unwrap();
    assert_eq!(h["reason"], "missing_daily_history");
    assert!(h["twrPpm"].is_null());
}
