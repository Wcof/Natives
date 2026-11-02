//! 投资账本 V2 的端到端测试：走真实 Store，覆盖建账户 → 建标的 → 录流水 →
//! 查持仓 → 总览 的完整链路。单元测试验证算法，这里验证装配。

use super::model::{CreateAccount, CreateFlow, CreateInstrument, UpdateAccount};
use crate::storage::Store;

fn setup() -> Store {
    Store::open_in_memory().unwrap()
}

fn seed_account(store: &Store) -> i64 {
    let account = super::crud::create_account(
        store,
        &CreateAccount {
            name: "华泰证券".into(),
            channel: "domestic_broker".into(),
            currency: "CNY".into(),
            note: String::new(),
        },
    )
    .unwrap();
    account.id
}

fn seed_instrument(store: &Store) -> i64 {
    let inst = super::crud::create_instrument(
        store,
        &CreateInstrument {
            code: "600519".into(),
            name: "贵州茅台".into(),
            asset_class: "stock".into(),
            market: "CN".into(),
            currency: "CNY".into(),
            group_code: None,
            share_class: None,
        },
    )
    .unwrap();
    inst.id
}

fn buy(
    store: &Store,
    account: i64,
    instrument: i64,
    qty: &str,
    price: &str,
    date: &str,
) -> super::model::FlowView {
    let amount = format!(
        "{:.2}",
        qty.parse::<f64>().unwrap() * price.parse::<f64>().unwrap()
    );
    super::flows::create_flow(
        store,
        &CreateFlow {
            account_id: account,
            instrument_id: instrument,
            kind: "BUY".into(),
            quantity: qty.into(),
            price: price.into(),
            amount,
            fee: String::new(),
            split_from: None,
            split_to: None,
            trade_date: date.into(),
            note: String::new(),
            request_id: None,
        },
    )
    .unwrap()
}

#[test]
fn full_chain_from_account_to_holding() {
    let store = setup();
    let account = seed_account(&store);
    let instrument = seed_instrument(&store);

    // 第一笔：买 100 股 @10
    buy(&store, account, instrument, "100", "10", "2026-01-05");
    let holdings = super::holdings::list_holdings(&store).unwrap();
    assert_eq!(holdings.len(), 1);
    assert_eq!(holdings[0].quantity, "100");
    assert_eq!(holdings[0].avg_cost, "10");
    assert_eq!(holdings[0].cost, "1000");

    // 第二笔：再买 100 股 @20 → 均价 15
    buy(&store, account, instrument, "100", "20", "2026-02-05");
    let holdings = super::holdings::list_holdings(&store).unwrap();
    assert_eq!(holdings[0].quantity, "200");
    assert_eq!(holdings[0].avg_cost, "15");
    assert_eq!(holdings[0].cost, "3000");

    // 设一个最新价，市值与浮盈应随之出现
    store
        .with_write(|conn| {
            conn.execute(
                "UPDATE instruments SET latest_price_raw=?1, latest_price_at='2026-03-01', price_stale=0 WHERE id=?2",
                rusqlite::params![1_800_000_i64, instrument],
            )?;
            Ok::<(), rusqlite::Error>(())
        })
        .unwrap();
    let holdings = super::holdings::list_holdings(&store).unwrap();
    assert_eq!(holdings[0].price.as_deref(), Some("180"));
    assert_eq!(holdings[0].market_value.as_deref(), Some("36000"));
    assert_eq!(holdings[0].unrealized.as_deref(), Some("33000"));
    assert_eq!(holdings[0].price_at.as_deref(), Some("2026-03-01"));
}

#[test]
fn oversell_is_blocked_at_entry_time() {
    let store = setup();
    let account = seed_account(&store);
    let instrument = seed_instrument(&store);
    buy(&store, account, instrument, "100", "10", "2026-01-05");

    // 没有先买就卖 → 录入即拒绝，而不是等到展示时出现负持仓
    let result = super::flows::create_flow(
        &store,
        &CreateFlow {
            account_id: account,
            instrument_id: instrument,
            kind: "SELL".into(),
            quantity: "150".into(),
            price: "12".into(),
            amount: "600".into(),
            fee: String::new(),
            split_from: None,
            split_to: None,
            trade_date: "2026-01-06".into(),
            note: String::new(),
            request_id: None,
        },
    );
    assert!(result.is_err(), "超卖必须在录入时被拦住");
}

#[test]
fn deleting_a_flow_that_breaks_later_records_is_refused() {
    let store = setup();
    let account = seed_account(&store);
    let instrument = seed_instrument(&store);
    let first = buy(&store, account, instrument, "100", "10", "2026-01-05");
    super::flows::create_flow(
        &store,
        &CreateFlow {
            account_id: account,
            instrument_id: instrument,
            kind: "SELL".into(),
            quantity: "100".into(),
            price: "12".into(),
            amount: "1200".into(),
            fee: String::new(),
            split_from: None,
            split_to: None,
            trade_date: "2026-01-06".into(),
            note: String::new(),
            request_id: None,
        },
    )
    .unwrap();

    // 删掉买入会让后面的卖出失去依据 → 必须拒绝
    assert!(
        super::flows::delete_flow(&store, first.id).is_err(),
        "删除会导致后续流水不成立时必须拒绝"
    );

    // 先删卖出，再删买入则应当允许
    let flows = super::flows::list_flows(&store, Some(account), Some(instrument)).unwrap();
    let sell = flows.iter().find(|f| f.kind == "SELL").unwrap();
    super::flows::delete_flow(&store, sell.id).unwrap();
    super::flows::delete_flow(&store, first.id).unwrap();
    assert!(super::holdings::list_holdings(&store).unwrap().is_empty());
}

#[test]
fn request_id_makes_flow_entry_idempotent() {
    let store = setup();
    let account = seed_account(&store);
    let instrument = seed_instrument(&store);
    let payload = CreateFlow {
        account_id: account,
        instrument_id: instrument,
        kind: "BUY".into(),
        quantity: "10".into(),
        price: "100".into(),
        amount: "1000".into(),
        fee: String::new(),
        split_from: None,
        split_to: None,
        trade_date: "2026-01-05".into(),
        note: String::new(),
        request_id: Some("dup-key-1".into()),
    };
    let first = super::flows::create_flow(&store, &payload).unwrap();
    let second = super::flows::create_flow(&store, &payload).unwrap();
    assert_eq!(first.id, second.id, "同一 request_id 不应重复入库");
    let holdings = super::holdings::list_holdings(&store).unwrap();
    assert_eq!(holdings[0].quantity, "10", "重复提交不应把数量累加两次");
}

#[test]
fn overview_marks_incomplete_when_price_missing() {
    let store = setup();
    let account = seed_account(&store);
    let instrument = seed_instrument(&store);
    buy(&store, account, instrument, "100", "10", "2026-01-05");

    // 无行情：净值不应凭空估算，且必须标记不完整
    let overview = super::overview::build(&store).unwrap();
    assert!(overview.incomplete, "缺行情时必须标记不完整");
    assert_eq!(overview.net_worth, "0");

    // 补上行情后恢复正常
    store
        .with_write(|conn| {
            conn.execute(
                "UPDATE instruments SET latest_price_raw=1_200_000 WHERE id=?1",
                [instrument],
            )?;
            Ok::<(), rusqlite::Error>(())
        })
        .unwrap();
    let overview = super::overview::build(&store).unwrap();
    assert!(!overview.incomplete);
    assert_eq!(overview.net_worth, "12000");
    // 成本 100 × 10 = 1000，市值 100 × 120 = 12000，浮盈 11000
    assert_eq!(overview.unrealized, "11000");
    assert_eq!(overview.by_class.len(), 1);
    assert_eq!(overview.by_class[0].asset_class, "stock");
}

#[test]
fn foreign_currency_without_rate_is_flagged_not_faked() {
    let store = setup();
    let account = super::crud::create_account(
        &store,
        &CreateAccount {
            name: "富途".into(),
            channel: "overseas_broker".into(),
            currency: "USD".into(),
            note: String::new(),
        },
    )
    .unwrap();
    let inst = super::crud::create_instrument(
        &store,
        &CreateInstrument {
            code: "AAPL".into(),
            name: "Apple".into(),
            asset_class: "stock".into(),
            market: "US".into(),
            currency: "USD".into(),
            group_code: None,
            share_class: None,
        },
    )
    .unwrap();
    buy(&store, account.id, inst.id, "10", "100", "2026-01-05");
    store
        .with_write(|conn| {
            conn.execute(
                "UPDATE instruments SET latest_price_raw=1_100_000 WHERE id=?1",
                [inst.id],
            )?;
            Ok::<(), rusqlite::Error>(())
        })
        .unwrap();

    // 没有 USD/CNY 汇率时必须标记 incomplete，而不是按 1:1 假装精确
    let overview = super::overview::build(&store).unwrap();
    assert!(overview.incomplete, "缺汇率时必须标记不完整");

    // 补上汇率后按 7.1 折算
    store
        .with_write(|conn| {
            conn.execute(
                "INSERT INTO fx_rates(base,quote,rate_raw,rate_date,source,fetched_at)
                 VALUES('USD','CNY',7100000,'2026-01-05','test','0')",
                [],
            )?;
            Ok::<(), rusqlite::Error>(())
        })
        .unwrap();
    let overview = super::overview::build(&store).unwrap();
    assert!(!overview.incomplete);
    // 10 股 × 110 USD = 1100 USD × 7.1 = 7810 CNY
    assert_eq!(overview.net_worth, "7810");
}

#[test]
fn archiving_account_hides_it_from_but_keeps_flows() {
    let store = setup();
    let account = seed_account(&store);
    let instrument = seed_instrument(&store);
    buy(&store, account, instrument, "100", "10", "2026-01-05");

    super::crud::update_account(
        &store,
        account,
        &UpdateAccount {
            name: None,
            note: None,
            archived: Some(true),
        },
    )
    .unwrap();

    let accounts = super::crud::list_accounts(&store).unwrap();
    assert!(accounts[0].archived);
    // 归档不删流水：持仓仍然成立，避免历史被抹掉
    let flows = super::flows::list_flows(&store, Some(account), None).unwrap();
    assert_eq!(flows.len(), 1);
}

// ---------- HTTP 路由装配 ----------

fn call(store: &Store, method: &str, path: &str, body: &str) -> Result<String, (u16, String)> {
    let request =
        app_runtime_core::http::HttpRequest::for_internal(method, path, body.as_bytes().to_vec());
    let route = path.split('?').next().unwrap_or(path);
    crate::api::handle_api_request(store, None, &request, route)
}

#[test]
fn v2_routes_are_wired_through_the_api_entry() {
    let store = setup();

    // 建账户
    let created = call(
        &store,
        "POST",
        "/api/v2/accounts",
        r#"{"name":"华泰证券","channel":"domestic_broker","currency":"CNY"}"#,
    )
    .expect("创建账户应成功");
    assert!(created.contains("华泰证券"));
    let account_id: i64 = super::crud::list_accounts(&store).unwrap()[0].id;

    // 建标的
    call(
        &store,
        "POST",
        "/api/v2/instruments",
        r#"{"code":"600519","name":"贵州茅台","asset_class":"stock","market":"CN","currency":"CNY"}"#,
    )
    .expect("创建标的应成功");
    let instrument_id = super::crud::list_instruments(&store).unwrap()[0].id;

    // 录流水
    let flow_body = format!(
        r#"{{"account_id":{account_id},"instrument_id":{instrument_id},"type":"BUY",
             "quantity":"100","price":"10","amount":"1000","trade_date":"2026-01-05"}}"#
    );
    call(&store, "POST", "/api/v2/flows", &flow_body).expect("录入流水应成功");

    // 查持仓
    let holdings = call(&store, "GET", "/api/v2/holdings", "").expect("查持仓应成功");
    assert!(holdings.contains("贵州茅台"));
    assert!(holdings.contains("\"quantity\":\"100\""));

    // 查总览（同时会写当天快照）
    let overview = call(&store, "GET", "/api/v2/overview", "").expect("查总览应成功");
    assert!(overview.contains("by_class"));
    let snapshot_count: i64 = store
        .with_read(|c| c.query_row("SELECT COUNT(*) FROM snapshots", [], |r| r.get(0)))
        .unwrap();
    assert_eq!(snapshot_count, 1, "打开总览应沉淀当天快照");
}

#[test]
fn v2_routes_map_business_errors_to_http_codes() {
    let store = setup();
    let account = seed_account(&store);
    let instrument = seed_instrument(&store);

    // 超卖 → 400
    let body = format!(
        r#"{{"account_id":{account},"instrument_id":{instrument},"type":"SELL",
             "quantity":"10","price":"10","amount":"100","trade_date":"2026-01-05"}}"#
    );
    let err = call(&store, "POST", "/api/v2/flows", &body).unwrap_err();
    assert_eq!(err.0, 400, "超卖应映射为 400");

    // 未知路由 → 404
    let err = call(&store, "GET", "/api/v2/nope", "").unwrap_err();
    assert_eq!(err.0, 404);

    // 非法 JSON → 400
    let err = call(&store, "POST", "/api/v2/accounts", "{").unwrap_err();
    assert_eq!(err.0, 400);

    // 非法渠道 → 400（CHECK 与业务校验双保险）
    let err = call(
        &store,
        "POST",
        "/api/v2/accounts",
        r#"{"name":"x","channel":"nope","currency":"CNY"}"#,
    )
    .unwrap_err();
    assert_eq!(err.0, 400);
}

/// 端到端：美股持仓 → 刷行情 → 刷汇率 → 总览不再标「未折算」。
/// 依赖真实上游，故标 ignore；运行 `cargo test -p fund-module live_e2e -- --ignored`。
#[test]
#[ignore = "需要网络：端到端验证行情+汇率+总览"]
fn live_e2e_refresh_fills_market_value_and_fx() {
    let store = setup();
    let account = super::crud::create_account(
        &store,
        &CreateAccount {
            name: "富途".into(),
            channel: "overseas_broker".into(),
            currency: "USD".into(),
            note: String::new(),
        },
    )
    .unwrap();
    let inst = super::crud::create_instrument(
        &store,
        &CreateInstrument {
            code: "AAPL".into(),
            name: "Apple".into(),
            asset_class: "stock".into(),
            market: "US".into(),
            currency: "USD".into(),
            group_code: None,
            share_class: None,
        },
    )
    .unwrap();
    buy(&store, account.id, inst.id, "10", "180", "2026-01-05");

    // 刷新前：没有行情也没有汇率，必须标记不完整
    let before = super::overview::build(&store).unwrap();
    assert!(before.incomplete, "刷新前应标记不完整");
    assert_eq!(before.net_worth, "0");

    let quote_result = super::quotes::refresh_all(&store).unwrap();
    assert!(quote_result.updated >= 1, "应至少刷新到一只标的的行情");

    let fx_result = super::fx::refresh_all(&store).unwrap();
    assert!(fx_result.updated >= 1, "应至少写入一组汇率");
    assert!(fx_result.pairs.iter().any(|p| p.starts_with("USD/CNY")));

    let after = super::overview::build(&store).unwrap();
    assert!(!after.incomplete, "行情与汇率齐备后不应再标不完整");
    assert_ne!(after.net_worth, "0", "净值应为非零");
    assert_eq!(after.by_class.len(), 1);
}

/// 验证交易日历能真正落库并用于判定最近交易日。
/// `cargo test -p fund-module live_calendar -- --ignored`
#[test]
#[ignore = "需要网络：验证日历落库"]
fn live_calendar_populates_and_drives_last_trading_day() {
    let store = setup();
    let result = super::calendar::refresh(&store, 60).unwrap();
    assert!(result.days_written > 0, "应写入交易日");
    assert!(
        result.markets.len() >= 2,
        "至少应覆盖 A 股与一个海外市场，实际 {:?}",
        result.markets
    );

    let today = crate::assets::today_iso();
    let last_cn = super::calendar::last_trading_day(&store, "CN", &today);
    assert!(last_cn.is_some(), "A 股应有最近交易日");

    let count: i64 = store
        .with_read(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM trading_days WHERE market='CN'",
                [],
                |r| r.get(0),
            )
        })
        .unwrap();
    assert!(count >= 20, "60 天回溯应至少写入 20 个交易日，实际 {count}");

    // 开闭市状态缓存应已被填充（供 trading 使用）
    assert!(
        super::calendar::cached_status().is_some(),
        "刷新后应缓存市场开闭市状态"
    );
}
