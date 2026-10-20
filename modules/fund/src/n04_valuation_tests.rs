//! N04 统一估值与收益聚合验收测试（A64 独立精确对账 / A67 事件对账）。
//! 预期值来自独立手算（文档 07 A64），不用被测实现生成；全部隔离内存库。

use crate::assets::{self, Collected, QuoteInput};
use crate::finance::CashEventInput;
use crate::fixed::Fixed;
use crate::ledger::{self, OpeningInput, TransactionInput};
use crate::storage::Store;
use std::collections::BTreeMap;

fn opening(
    store: &Store,
    account: &str,
    code: &str,
    name: &str,
    qty: &str,
    cost: Option<&str>,
    as_of: &str,
) {
    ledger::record_opening(
        store,
        &OpeningInput {
            account_name: account.into(),
            fund_code: code.into(),
            fund_name: name.into(),
            quantity: Fixed::parse(qty, 4).unwrap(),
            as_of: as_of.into(),
            total_cost: cost.map(|c| Fixed::parse(c, 2).unwrap()),
        },
        "2026-10-03",
    )
    .unwrap();
}

fn buy(
    store: &Store,
    account: &str,
    code: &str,
    name: &str,
    qty: &str,
    price: &str,
    fee: &str,
    date: &str,
) {
    ledger::record_transaction(
        store,
        &TransactionInput::manual(
            account.into(),
            code.into(),
            name.into(),
            "BUY".into(),
            "confirmed".into(),
            Fixed::parse(qty, 4).unwrap(),
            Fixed::parse(price, 4).unwrap(),
            Fixed::parse(fee, 2).unwrap(),
            date.into(),
            format!("buy-{account}-{code}-{date}-{qty}-{price}-{fee}"),
        ),
    )
    .unwrap();
}

fn sell(
    store: &Store,
    account: &str,
    code: &str,
    name: &str,
    qty: &str,
    price: &str,
    fee: &str,
    date: &str,
) {
    ledger::record_transaction(
        store,
        &TransactionInput::manual(
            account.into(),
            code.into(),
            name.into(),
            "SELL".into(),
            "confirmed".into(),
            Fixed::parse(qty, 4).unwrap(),
            Fixed::parse(price, 4).unwrap(),
            Fixed::parse(fee, 2).unwrap(),
            date.into(),
            format!("sell-{account}-{code}-{date}-{qty}-{price}-{fee}"),
        ),
    )
    .unwrap();
}

fn quote_map(entries: &[(&str, &str, &str)]) -> BTreeMap<String, QuoteInput> {
    let mut map = BTreeMap::new();
    for (symbol, price, as_of) in entries {
        map.insert(
            symbol.to_string(),
            QuoteInput {
                price_raw: Fixed::parse(price, 4).unwrap().raw(),
                change_raw: None,
                as_of: Some((*as_of).to_string()),
                source: "test".into(),
            },
        );
    }
    map
}

fn unified(store: &Store, quotes: &BTreeMap<String, QuoteInput>) -> assets::AssetsSummary {
    let mut collected = store
        .with_read(|conn| assets::collect(conn, "2026-10-03"))
        .unwrap();
    assets::apply_quotes(&mut collected, quotes, "2026-10-03");
    assets::finalize(collected, "2026-10-03").unwrap()
}

#[test]
fn cached_quotes_survive_metadata_and_revalue_current_quantities() {
    let store = Store::open_in_memory().unwrap();
    let today = crate::finance::today_cn();
    let input = OpeningInput {
        account_name: "Cache".into(),
        fund_code: "stock:SSE:600519".into(),
        fund_name: "Stock".into(),
        quantity: Fixed::share(20_000),
        total_cost: Some(Fixed::amount(1_000)),
        as_of: today.clone(),
    };
    let fund = ledger::record_opening(&store, &input, &today).unwrap();
    let quotes = quote_map(&[("sh600519", "10.00", &today)]);
    let first = store
        .with_read(|conn| {
            let mut collected = assets::collect(conn, &today)?;
            assets::apply_quotes(&mut collected, &quotes, &today);
            let summary = assets::finalize(collected, &today)?;
            let revision = Store::data_revision(conn)?;
            let batch = assets::valuation_batch(&revision, 1234, &summary);
            assets::store_valuation_cache(conn, &quotes, 1234, &revision, &batch, false);
            Ok::<_, assets::AssetError>(batch)
        })
        .unwrap();
    store
        .with_write(|conn| {
            conn.execute(
                "INSERT INTO app_meta(key,value) VALUES('home_display_mode','amount')",
                [],
            )
        })
        .unwrap();
    store
        .with_read(|conn| {
            let (summary, _) = assets::summary_with_cached_quotes(conn, &today)?;
            assert_eq!(summary.fund_value_minor, Some(2_000));
            assert_ne!(summary.valuation_batch, first);
            assert_eq!(summary.valuation_fetched_at, Some(1234));
            assert_eq!(
                summary.funds[0].price_as_of.as_deref(),
                Some(today.as_str())
            );
            Ok::<_, assets::AssetError>(())
        })
        .unwrap();
    crate::opening_ops::change(
        &store,
        1,
        fund,
        1,
        "update",
        &serde_json::json!({
            "quantity":"3","totalCost":"10.00","asOf":today,
        }),
    )
    .unwrap();
    store
        .with_read(|conn| {
            let (summary, _) = assets::summary_with_cached_quotes(conn, &today)?;
            assert_eq!(
                summary.fund_value_minor,
                Some(3_000),
                "current quantity, not cached quantity"
            );
            assert_eq!(summary.valuation_fetched_at, Some(1234));
            Ok::<_, assets::AssetError>(())
        })
        .unwrap();
    let other = Store::open_in_memory().unwrap();
    ledger::record_opening(&other, &input, &today).unwrap();
    other
        .with_read(|conn| {
            let (summary, cache) = assets::summary_with_cached_quotes(conn, &today)?;
            assert!(cache.is_none());
            assert!(
                summary.funds[0].value_minor.is_none(),
                "another store cannot reuse prices"
            );
            Ok::<_, assets::AssetError>(())
        })
        .unwrap();
}

/// A64：隔离库同一批次含股票、场内 ETF、场外基金、现金与线下资产；
/// 股票 100 股×12.34＝1234.00，ETF 200 份×2.3456＝469.12，
/// OTC 100.1234 份×1.2345＝123.60（既有截断），现金 1000.00、线下 500.00，
/// 完整 A＝3326.72；ETF 另有不同 NAV 也不得替代行情；跨账户同批对账。
#[test]
fn unified_valuation_reconciles_exact_amounts() {
    let store = Store::open_in_memory().unwrap();
    opening(
        &store,
        "A",
        "stock:SSE:600519",
        "茅台",
        "100",
        Some("1000.00"),
        "2026-01-10",
    );
    opening(
        &store,
        "A",
        "stock:SSE:510300",
        "沪深300ETF",
        "200",
        Some("400.00"),
        "2026-01-10",
    );
    opening(
        &store,
        "B",
        "110022",
        "消费基金",
        "100.1234",
        Some("100.00"),
        "2026-01-10",
    );
    // 场外净值 1.2345；ETF 另有不同 NAV：行情必须优先，不得被净值替代。
    store
        .with_write(|conn| -> Result<(), crate::assets::AssetError> {
            conn.execute(
                "INSERT INTO fund_nav(fund_id,nav_date,unit_nav_raw,source,fetched_at)
             SELECT id,'2026-10-02',99999,'eastmoney','0' FROM funds WHERE code='stock:SSE:510300'",
                [],
            )?;
            conn.execute(
                "INSERT INTO fund_nav(fund_id,nav_date,unit_nav_raw,source,fetched_at)
             SELECT id,'2026-10-02',12345,'eastmoney','0' FROM funds WHERE code='110022'",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    // 现金（账户 A）与线下资产 500.00。
    store
        .with_write(|conn| -> Result<(), crate::assets::AssetError> {
            assets::create_manual_asset(
                conn,
                &assets::ManualAssetInput {
                    account_name: "A".into(),
                    product_type: "cash".into(),
                    market: None,
                    code: None,
                    name: "现金".into(),
                    risk_class: "cash".into(),
                    market_value_text: "1000.00".into(),
                    valued_at: "2026-10-03".into(),
                    confirm_distinct: false,
                },
                "2026-10-03",
            )?;
            Ok(())
        })
        .unwrap();
    crate::offline::create_asset(
        &store,
        &crate::offline::OfflineInput {
            product_type: "other".into(),
            name: "线下资产".into(),
            location: "隔离".into(),
            value_minor: 50_000,
            valued_at: "2026-10-03".into(),
            cost_minor: None,
            risk_class: "other".into(),
            note: String::new(),
            attrs_json: "{}".into(),
        },
    )
    .unwrap();

    let quotes = quote_map(&[
        ("sh600519", "12.34", "2026-10-03 15:00:00"),
        ("sh510300", "2.3456", "2026-10-03 15:00:00"),
    ]);
    let summary = unified(&store, &quotes);
    let value_of = |code: &str| {
        summary
            .funds
            .iter()
            .find(|row| row.fund_code == code)
            .unwrap()
            .value_minor
            .unwrap()
    };
    assert_eq!(value_of("stock:SSE:600519"), 123_400); // 100 × 12.34
    assert_eq!(value_of("stock:SSE:510300"), 46_912); // 200 × 2.3456，行情优先
    assert_eq!(value_of("110022"), 12_360); // 100.1234 × 1.2345 向零截断
    assert_eq!(summary.fund_value_minor, Some(123_400 + 46_912 + 12_360));
    assert_eq!(summary.manual_value_minor, Some(100_000));
    assert_eq!(summary.offline_value_minor, Some(50_000));
    // 完整 A = 1234.00 + 469.12 + 123.60 + 1000.00 + 500.00 = 3326.72。
    assert_eq!(summary.complete_value_minor, Some(332_672));
    assert_eq!(summary.known_value_minor, summary.complete_value_minor);
    assert_eq!(summary.quote_count, 3);
    assert_eq!(summary.missing_quote_count, 0);
    assert_eq!(summary.missing_price_count, 0);
    // 浮动收益 = Σ(市值−剩余成本) = 234.00 + 69.12 + 23.60 = 326.72。
    assert_eq!(summary.floating_minor, Some(32_672));
    assert_eq!(summary.floating_unknown_cost_count, 0);
    // 批次身份：同输入稳定；价格变化（revision 不变）批次必须变化。
    let batch = assets::valuation_batch("7", 1_000, &summary);
    assert_eq!(
        batch,
        assets::valuation_batch("7", 1_000, &unified(&store, &quotes))
    );
    let shifted = quote_map(&[
        ("sh600519", "12.35", "2026-10-03 15:00:00"),
        ("sh510300", "2.3456", "2026-10-03 15:00:00"),
    ]);
    assert_ne!(
        batch,
        assets::valuation_batch("7", 1_000, &unified(&store, &shifted))
    );

    // 缺一只报价：完整 A 显式缺位，已知部分单独成立并计数。
    let partial = quote_map(&[("sh600519", "12.34", "2026-10-03 15:00:00")]);
    let summary = unified(&store, &partial);
    assert_eq!(summary.complete_value_minor, None);
    assert_eq!(summary.known_value_minor, Some(332_672 - 46_912));
    assert_eq!(summary.missing_quote_count, 1);
    assert_eq!(summary.quote_count, 2);
}

/// A67：独立事件对账——期初、买入（费用入成本）、部分卖出（费用入已实现）、
/// 现金分红、独立费用、外部投入与内部转移不计收益；浮动/已实现/累计分立。
#[test]
fn cumulative_returns_reconcile_ledger_events() {
    let store = Store::open_in_memory().unwrap();
    let account = store
        .with_read(|conn| ledger::ensure_account(conn, "A"))
        .unwrap();
    opening(
        &store,
        "A",
        "stock:SSE:600519",
        "茅台",
        "100",
        Some("1000.00"),
        "2026-01-10",
    );
    buy(
        &store,
        "A",
        "stock:SSE:600519",
        "茅台",
        "10",
        "11.00",
        "1.00",
        "2026-02-01",
    );
    // 现金分红 50.00 与独立费用 5.00（均不经交易，未含于成本/已实现）。
    let event = |conn: &rusqlite::Connection,
                 event_type: &str,
                 amount: &str,
                 date: &str,
                 key: &str|
     -> Result<i64, crate::assets::AssetError> {
        crate::finance::record_cash_event(
            conn,
            &CashEventInput {
                account_id: account,
                event_type: event_type.into(),
                amount_text: amount.into(),
                occurred_at: date.into(),
                idempotency_key: Some(key.into()),
                note: String::new(),
                counterparty_account_id: None,
            },
        )
        .map_err(|e| crate::assets::AssetError::Invalid(e.message()))
    };
    store
        .with_write(|conn| -> Result<(), crate::assets::AssetError> {
            event(conn, "dividend_cash", "50.00", "2026-03-01", "div-1")?;
            event(conn, "fee", "5.00", "2026-03-02", "fee-1")?;
            // 外部投入 100.00：本金投入，不算收益。
            event(conn, "external_in", "100.00", "2026-03-03", "ext-1")?;
            Ok(())
        })
        .unwrap();
    // 部分卖出 20 股 @12.00 费 2.00：卖出成本 = 1111.00 × 20/110 = 202.00
    // （整数除法向零），已实现 = 240 − 2 − 202 = 36.00。
    sell(
        &store,
        "A",
        "stock:SSE:600519",
        "茅台",
        "20",
        "12.00",
        "2.00",
        "2026-04-01",
    );

    let quotes = quote_map(&[("sh600519", "12.34", "2026-10-03 15:00:00")]);
    let summary = unified(&store, &quotes);
    // 浮动：剩余 90 股 × 12.34 = 1110.60；剩余成本 909.00 → 201.60。
    assert_eq!(summary.floating_minor, Some(20_160));
    let cumulative = store
        .with_read(|conn| crate::returns::cumulative_projection(conn, &summary, "2026-10-03"))
        .unwrap();
    assert_eq!(cumulative["floatingMinor"], serde_json::json!("20160"));
    assert_eq!(cumulative["realizedMinor"], serde_json::json!("3600"));
    assert_eq!(cumulative["dividendsMinor"], serde_json::json!("5000"));
    assert_eq!(cumulative["unbookedFeesMinor"], serde_json::json!("500"));
    // 累计 = 201.60 + 36.00 + 50.00 − 5.00 = 282.60。
    assert_eq!(cumulative["totalMinor"], serde_json::json!("28260"));
    assert_eq!(cumulative["complete"], serde_json::json!(true));
    assert_eq!(cumulative["coverageFrom"], serde_json::json!("2026-01-10"));
}

/// A67 缺口表达：未知期初成本 → 卖出不产生已实现（不为零），浮动成本未知
/// 单列；累计收益 complete=false 且保留已知部分。
#[test]
fn cumulative_returns_explicit_gaps_for_unknown_cost() {
    let store = Store::open_in_memory().unwrap();
    opening(&store, "A", "110022", "消费基金", "100", None, "2026-01-10");
    sell(
        &store,
        "A",
        "110022",
        "消费基金",
        "40",
        "1.50",
        "0.00",
        "2026-02-01",
    );
    let quotes = quote_map(&[("110022-placeholder-unused", "1", "2026-10-03")]);
    let _ = quotes; // 场外走净值：写入一条 NAV。
    store
        .with_write(|conn| -> Result<(), crate::assets::AssetError> {
            conn.execute(
                "INSERT INTO fund_nav(fund_id,nav_date,unit_nav_raw,source,fetched_at)
             SELECT id,'2026-10-02',15000,'eastmoney','0' FROM funds WHERE code='110022'",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    let summary = unified(&store, &BTreeMap::new());
    let row = summary
        .funds
        .iter()
        .find(|row| row.fund_code == "110022")
        .unwrap();
    assert_eq!(row.value_minor, Some(9_000)); // 60 × 1.5，市值保留
    assert!(row.cost_unknown);
    assert_eq!(summary.floating_minor, None); // 成本未知 → 浮动整体缺位
    assert_eq!(summary.floating_unknown_cost_count, 1);
    let cumulative = store
        .with_read(|conn| crate::returns::cumulative_projection(conn, &summary, "2026-10-03"))
        .unwrap();
    assert_eq!(cumulative["realizedGapCount"], serde_json::json!(1));
    assert_eq!(cumulative["complete"], serde_json::json!(false));
    assert_eq!(
        cumulative["coverageNote"],
        serde_json::json!("unknown-cost-gaps")
    );
}

#[test]
fn legacy_home_preserves_recorded_basics_before_currency_confirmation() {
    let store = Store::open_in_memory().unwrap();
    let today = crate::finance::today_cn();
    ledger::record_opening(
        &store,
        &OpeningInput {
            account_name: "isolated".into(),
            fund_code: "110022".into(),
            fund_name: "Legacy fund".into(),
            quantity: Fixed::parse("100", 4).unwrap(),
            total_cost: Some(Fixed::parse("120", 2).unwrap()),
            as_of: today.clone(),
        },
        &today,
    )
    .unwrap();
    store.with_write(|conn| -> Result<(), crate::assets::AssetError> {
        conn.execute("DELETE FROM fund_asset_metadata", [])?;
        conn.execute("INSERT INTO fund_nav(fund_id,nav_date,unit_nav_raw,source,fetched_at) VALUES(1,?1,15000,'eastmoney-mobile','0')", [&today])?;
        Ok(())
    }).unwrap();
    let before = crate::home::build_snapshot(&store, "all").unwrap();
    assert_eq!(before["portfolio"]["securities"]["holdingCount"], 1);
    assert!(before["wealth"]["totalA"].is_null());
    assert!(before["wealth"]["knownA"].is_null());
    assert_eq!(before["portfolio"]["securities"]["knownCostMinor"], 12_000);
    assert_eq!(before["wealth"]["valuation"]["unconfirmedCurrencies"], 1);
    let nav_before = store
        .with_read(|c| {
            c.query_row(
                "SELECT unit_nav_raw,source FROM fund_nav WHERE fund_id=1",
                [],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
            )
        })
        .unwrap();
    let body = serde_json::json!({"fundId":1,"navCurrency":"CNY","riskClass":"unknown","dataRevision":before["dataRevision"]}).to_string();
    let confirm = || {
        let request = app_runtime_core::http::HttpRequest::for_internal(
            "POST",
            "/api/fund-asset-metadata",
            body.as_bytes().to_vec(),
        );
        crate::api::handle_api_request(&store, None, &request, "/api/fund-asset-metadata")
    };
    confirm().unwrap();
    let after = crate::home::build_snapshot(&store, "all").unwrap();
    assert_eq!(after["wealth"]["totalA"], 15_000);
    assert_eq!(after["wealth"]["cumulative"]["totalMinor"], "3000");
    assert_eq!(after["wealth"]["valuation"]["unconfirmedCurrencies"], 0);
    assert_eq!(
        nav_before,
        store
            .with_read(|c| c.query_row(
                "SELECT unit_nav_raw,source FROM fund_nav WHERE fund_id=1",
                [],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            ))
            .unwrap()
    );
    let conflict = confirm().unwrap_err();
    assert_eq!(conflict.0, 409);
    let revision = after["dataRevision"].clone();
    for invalid in [
        serde_json::Value::Null,
        serde_json::json!(7),
        serde_json::json!("invalid"),
    ] {
        let body =
            serde_json::json!({"fundId":1,"navCurrency":"unsupported","dataRevision":invalid})
                .to_string();
        let request = app_runtime_core::http::HttpRequest::for_internal(
            "POST",
            "/api/fund-asset-metadata",
            body.into_bytes(),
        );
        assert_eq!(
            crate::api::handle_api_request(&store, None, &request, "/api/fund-asset-metadata")
                .unwrap_err()
                .0,
            400
        );
    }
    let current = crate::home::build_snapshot(&store, "all").unwrap();
    assert_eq!(current["dataRevision"], revision);
    assert_eq!(current["wealth"]["totalA"], 15_000);
}

#[test]
fn recorded_cost_distinguishes_empty_unknown_and_confirmed_zero() {
    let store = Store::open_in_memory().unwrap();
    let today = crate::finance::today_cn();
    assert!(
        crate::home::build_snapshot(&store, "all").unwrap()["portfolio"]["securities"]
            ["knownCostMinor"]
            .is_null()
    );
    for (account, cost, expected) in [
        ("unknown", None, None),
        ("zero", Some("0"), Some(0)),
        ("known", Some("120"), Some(12_000)),
    ] {
        ledger::record_opening(
            &store,
            &OpeningInput {
                account_name: account.into(),
                fund_code: "110022".into(),
                fund_name: "Cost fixture".into(),
                quantity: Fixed::parse("100", 4).unwrap(),
                total_cost: cost.map(|v| Fixed::parse(v, 2).unwrap()),
                as_of: today.clone(),
            },
            &today,
        )
        .unwrap();
        let snapshot = crate::home::build_snapshot(&store, "all").unwrap();
        assert_eq!(
            snapshot["portfolio"]["securities"]["knownCostMinor"],
            serde_json::json!(expected)
        );
    }
}
