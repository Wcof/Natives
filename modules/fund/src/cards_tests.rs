//! cards 模块测试：P0 持仓摘要投影（方案 05 §6 验收例题）。

use super::*;
use crate::ledger::{self, TransactionInput};
use crate::storage::Store;
use rusqlite::params;

fn seed_account(conn: &Connection, name: &str) -> i64 {
    ledger::ensure_account(conn, name).unwrap()
}

fn seed_fund(conn: &Connection, code: &str, name: &str) -> i64 {
    ledger::ensure_fund(conn, code, name).unwrap()
}

fn buy(store: &Store, account: &str, code: &str, name: &str, qty: &str, price: &str, date: &str) {
    let input = TransactionInput::manual(
        account.to_string(),
        code.to_string(),
        name.to_string(),
        "BUY".to_string(),
        "confirmed".to_string(),
        Fixed::parse(qty, 4).unwrap(),
        Fixed::parse(price, 4).unwrap(),
        Fixed::zero(2),
        date.to_string(),
        format!("req-{account}-{code}-{date}-{qty}-{price}"),
    );
    ledger::record_transaction(store, &input).unwrap();
}

fn snapshot(store: &Store) -> serde_json::Value {
    let result = store.with_snapshot(build_snapshot).unwrap();
    serde_json::to_value(&result).unwrap()
}

#[test]
fn card_snapshot_requires_a_read_transaction() {
    let store = Store::open_in_memory().unwrap();
    assert!(store.with_read(build_snapshot).is_err());
}

#[test]
fn empty_store_reports_empty_without_zero_cost() {
    let store = Store::open_in_memory().unwrap();
    let snap = snapshot(&store);
    let holdings = &snap["payload"]["registeredFundHoldings"];
    assert_eq!(holdings["quality"], "empty");
    assert_eq!(holdings["positionCount"], 0);
    assert!(holdings["cost"].is_null(), "空态不得输出 ¥0.00 总成本");
}

#[test]
fn counts_positions_and_accounts_across_accounts() {
    let store = Store::open_in_memory().unwrap();
    // A 账户 2 笔在持，B 账户 1 笔在持。
    buy(
        &store,
        "A",
        "000001",
        "基金一",
        "100.0000",
        "1.0000",
        "2026-09-01",
    );
    buy(
        &store,
        "A",
        "000002",
        "基金二",
        "200.0000",
        "2.0000",
        "2026-09-05",
    );
    buy(
        &store,
        "B",
        "000003",
        "基金三",
        "50.0000",
        "3.0000",
        "2026-09-10",
    );
    let snap = snapshot(&store);
    let holdings = &snap["payload"]["registeredFundHoldings"];
    assert_eq!(holdings["positionCount"], 3);
    assert_eq!(holdings["accountCount"], 2);
    assert_eq!(holdings["quality"], "ready");
    assert_eq!(holdings["cost"]["complete"], true);
    // 成本 = 100*1 + 200*2 + 50*3 = 650.00
    assert_eq!(holdings["cost"]["total"]["amount"], "650");
    assert_eq!(holdings["holdingDates"]["earliest"], "2026-09-01");
    assert_eq!(holdings["holdingDates"]["latest"], "2026-09-10");
}

#[test]
fn cleared_position_excluded_from_cost() {
    let store = Store::open_in_memory().unwrap();
    buy(
        &store,
        "A",
        "000001",
        "基金一",
        "100.0000",
        "1.0000",
        "2026-09-01",
    );
    // 清仓：卖出后不进持有成本。
    let input = TransactionInput::manual(
        "A".to_string(),
        "000001".to_string(),
        "基金一".to_string(),
        "SELL".to_string(),
        "confirmed".to_string(),
        Fixed::parse("100.0000", 4).unwrap(),
        Fixed::parse("1.1000", 4).unwrap(),
        Fixed::zero(2),
        "2026-09-08".to_string(),
        "req-sell-1".into(),
    );
    ledger::record_transaction(&store, &input).unwrap();
    buy(
        &store,
        "A",
        "000002",
        "基金二",
        "10.0000",
        "2.0000",
        "2026-09-09",
    );
    let snap = snapshot(&store);
    let holdings = &snap["payload"]["registeredFundHoldings"];
    assert_eq!(holdings["positionCount"], 1);
    assert_eq!(holdings["accountCount"], 1);
    assert_eq!(holdings["cost"]["total"]["amount"], "20");
}

#[test]
fn unknown_opening_cost_reports_incomplete_with_known_part() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    // 已知成本持仓。
    buy(
        &store,
        "A",
        "000001",
        "基金一",
        "10.0000",
        "100.0000",
        "2026-09-01",
    );
    // 期初快照成本未知。
    store
        .with_write(|conn| -> rusqlite::Result<()> {
            let account_id = seed_account(conn, "B");
            let fund_id = seed_fund(conn, "000009", "基金九");
            conn.execute(
                "INSERT INTO opening_snapshots(account_id, fund_id, as_of, quantity_raw, cost_raw, derived_cost, nav_raw, nav_date, created_at)
                 VALUES(?1,?2,'2026-09-02',500000, NULL, 0, NULL, NULL, '2026-09-02')",
                rusqlite::params![account_id, fund_id],
            )?;
            crate::portfolio::recompute_position(conn, account_id, fund_id).map_err(|e| {
                rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
                    Some(e.to_string()),
                )
            })?;
            Ok(())
        })
        .unwrap();
    let snap = snapshot(&store);
    let holdings = &snap["payload"]["registeredFundHoldings"];
    assert_eq!(holdings["positionCount"], 2);
    assert_eq!(holdings["quality"], "incomplete");
    assert_eq!(holdings["cost"]["complete"], false);
    // 已知部分 = 10*100 = 1000.00；不得输出看似完整的总成本字段。
    assert_eq!(holdings["cost"]["known"]["amount"], "1000");
    assert_eq!(holdings["cost"]["unknownCount"], 1);
    assert!(holdings["cost"]["total"].is_null());
}

#[test]
fn schema_and_generated_at_shape() {
    let store = Store::open_in_memory().unwrap();
    let result = store.with_snapshot(build_snapshot).unwrap();
    assert_eq!(result.schema, "fund.cards.v1");
    assert!(result.generated_at.ends_with("+08:00"));
    assert!(result.generated_at.len() >= 20);
}

// ---------------------------------------------------------------------------
// P1/P2 投影字段（方案 02 §3：字段随阶段增量出现）
// ---------------------------------------------------------------------------

use crate::assets;
use crate::planning;

fn configure_reserve_zero(store: &Store, names: &[&str]) {
    store
        .with_write(|conn| -> Result<(), crate::finance::FinanceError> {
            for name in names {
                let id: i64 =
                    conn.query_row("SELECT id FROM accounts WHERE name=?1", [name], |r| {
                        r.get(0)
                    })?;
                crate::finance::save_reserve(conn, id, Some("0.00"), "")?;
            }
            Ok(())
        })
        .unwrap();
}

fn ensure_accounts(store: &Store, names: &[&str]) {
    store
        .with_write(|conn| -> rusqlite::Result<()> {
            for name in names {
                ledger::ensure_account(conn, name).map_err(|e| {
                    rusqlite::Error::SqliteFailure(
                        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
                        Some(format!("{e:?}")),
                    )
                })?;
            }
            Ok(())
        })
        .unwrap();
}

#[test]
fn snapshot_contains_assets_goal_allocation_sections() {
    let store = Store::open_in_memory().unwrap();
    let snap = snapshot(&store);
    assert!(snap["payload"]["recordedInvestmentAssets"].is_object());
    assert!(snap["payload"]["goal"].is_object());
    assert!(snap["payload"]["allocation"].is_object());
    assert!(snap["payload"]["allocationCheck"].is_object());
    // 空库语义：empty 且无伪造金额。
    assert_eq!(
        snap["payload"]["recordedInvestmentAssets"]["quality"],
        "empty"
    );
    assert_eq!(snap["payload"]["goal"]["configured"], false);
    assert_eq!(snap["payload"]["allocation"]["configured"], false);
    assert_eq!(snap["payload"]["groupAllocation"]["configured"], false);
    assert!(snap["payload"]["cash"]["cash"].is_null());
    assert!(snap["payload"]["cash"]["available"].is_null());
    assert!(snap["payload"]["dataRevision"].is_string());
}

#[test]
fn snapshot_matches_home_domain_aggregates_without_identifiers() {
    let store = Store::open_in_memory().unwrap();
    let today = crate::finance::today_cn();
    buy(
        &store,
        "private-account",
        "stock:SSE:600519",
        "股票",
        "1",
        "100",
        &today,
    );
    buy(
        &store,
        "private-account",
        "110022",
        "基金",
        "1",
        "1",
        &today,
    );
    let cash = store
        .with_write(|conn| {
            assets::create_manual_asset(
                conn,
                &assets::ManualAssetInput {
                    account_name: "private-account".into(),
                    product_type: "cash".into(),
                    market: None,
                    code: None,
                    name: "现金".into(),
                    risk_class: "cash".into(),
                    market_value_text: "50".into(),
                    valued_at: today.clone(),
                    confirm_distinct: false,
                },
                &today,
            )
        })
        .unwrap();
    configure_reserve_zero(&store, &["private-account"]);
    crate::group_plan::save_plan_tx(
        &store,
        &crate::group_plan::GroupPlanInput {
            expected_revision: 0,
            groups: vec![crate::group_plan::GroupInput {
                group_id: String::new(),
                name: "应急资金".into(),
                description: "日常流动资金预留".into(),
                ratio_tenths: 1000,
                sort_order: 0,
                fund_bindings: vec![],
                manual_bindings: vec![cash],
                offline_bindings: vec![],
                cash_bindings: vec![],
            }],
        },
    )
    .unwrap();
    let snap = snapshot(&store);
    let payload = &snap["payload"];
    assert_eq!(
        payload["registeredFundHoldings"]["positionCount"], 2,
        "stock and fund share the same ledger projection"
    );
    let home = crate::home::build_snapshot(&store, "all").unwrap();
    assert_eq!(payload["dataRevision"], home["dataRevision"]);
    assert_eq!(payload["cash"]["cash"]["amount"], "50");
    assert_eq!(payload["cash"]["available"]["amount"], "50");
    assert_eq!(home["wealth"]["cash"]["availableMinor"], 5000);
    assert_eq!(
        payload["groupAllocation"]["groups"][0]["amount"]["amount"],
        "50"
    );
    assert_eq!(
        payload["groupAllocation"]["groups"][0]["actualTenths"],
        home["groupAllocation"]["groups"][0]["currentTenths"]
    );
    let mut securities = home["portfolio"]["securities"].clone();
    crate::home::wire_numbers(&mut securities);
    assert_eq!(payload["portfolio"]["securities"], securities);
    let encoded = payload.to_string();
    for forbidden in [
        "accountId",
        "accountName",
        "fundId",
        "assetId",
        "groupId",
        "bindings",
        "private-account",
        "history",
        "diagnostics",
    ] {
        assert!(
            !encoded.contains(forbidden),
            "card payload exposes {forbidden}"
        );
    }
    assert!(encoded.len() < 64 * 1024);
}

#[test]
fn snapshot_assets_complete_case_matches_detail() {
    let store = Store::open_in_memory().unwrap();
    // 日期相对今天构造：写死日期会随真实时钟跨过 7 天新鲜度边界（2026-10-06 实际过期）。
    let today = crate::finance::today_cn();
    let recent = assets::minus_days(&today, 1);
    ensure_accounts(&store, &["A"]);
    buy(
        &store,
        "A",
        "110022",
        "消费行业",
        "100.0000",
        "1.2345",
        &today,
    );
    // 昨日净值 1.0000 → 现值 ¥100.00。
    store
        .with_write(|conn| -> rusqlite::Result<()> {
            conn.execute(
                "INSERT INTO fund_nav(fund_id, nav_date, unit_nav_raw, source, fetched_at)
                 SELECT id, ?1, 10000, 'eastmoney-mobile', '1' FROM funds WHERE code='110022'",
                params![recent],
            )?;
            Ok(())
        })
        .unwrap();
    let fund_id: i64 = store
        .with_read(|conn| {
            conn.query_row("SELECT id FROM funds WHERE code='110022'", [], |r| r.get(0))
        })
        .unwrap();
    store
        .with_write(|conn| assets::set_fund_metadata(conn, fund_id, "CNY", "equity"))
        .unwrap();
    store
        .with_write(|conn| {
            assets::create_manual_asset(
                conn,
                &crate::assets::ManualAssetInput {
                    account_name: "A".into(),
                    product_type: "cash".into(),
                    market: None,
                    code: None,
                    name: "现金".into(),
                    risk_class: "cash".into(),
                    market_value_text: "50.00".into(),
                    valued_at: recent.clone(),
                    confirm_distinct: false,
                },
                &today,
            )
        })
        .unwrap();
    // 目标 ¥200 → 进度 75.0%。
    planning::save_goal_tx(&store, "200.00", "2026-12-31", false, &today).unwrap();
    let snap = snapshot(&store);
    let assets_section = &snap["payload"]["recordedInvestmentAssets"];
    assert_eq!(assets_section["quality"], "ready");
    assert_eq!(assets_section["completeValue"]["amount"], "150");
    assert_eq!(assets_section["fundValue"]["amount"], "100");
    assert_eq!(assets_section["manualValue"]["amount"], "50");
    assert_eq!(assets_section["pricedCount"], 2);
    // 目标进度：75.0%。
    assert_eq!(snap["payload"]["goal"]["configured"], true);
    assert_eq!(snap["payload"]["goal"]["progress"]["computable"], true);
    assert_eq!(snap["payload"]["goal"]["progress"]["ratioPercent"], "75.0");
}

#[test]

fn snapshot_allocation_check_reports_out_of_range() {
    // 日期相对今天构造：写死日期会随真实时钟跨过 7 天新鲜度边界。
    let today = crate::finance::today_cn();
    let recent = assets::minus_days(&today, 1);
    let store = Store::open_in_memory().unwrap();
    ensure_accounts(&store, &["A"]);
    configure_reserve_zero(&store, &["A"]);
    store
        .with_write(|conn| {
            assets::create_manual_asset(
                conn,
                &crate::assets::ManualAssetInput {
                    account_name: "A".into(),
                    product_type: "cash".into(),
                    market: None,
                    code: None,
                    name: "现金".into(),
                    risk_class: "cash".into(),
                    market_value_text: "100.00".into(),
                    valued_at: recent.clone(),
                    confirm_distinct: false,
                },
                &today,
            )
        })
        .unwrap();
    let targets: Vec<planning::AllocationTargetRow> = planning::TARGET_CLASSES
        .iter()
        .map(|c| planning::AllocationTargetRow {
            risk_class: c.to_string(),
            target_tenths: 200,
            lower_tenths: 100,
            upper_tenths: 300,
        })
        .collect();
    planning::save_allocation_tx(&store, &targets).unwrap();
    let snap = snapshot(&store);
    let check = &snap["payload"]["allocationCheck"];
    assert_eq!(check["computable"], true);
    // 现金 100% 超过上限 30%；其余 4 类为 0 低于下限 10%。
    assert_eq!(check["outOfRange"].as_array().unwrap().len(), 5);
    let cash = check["classes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["riskClass"] == "cash")
        .unwrap();
    assert_eq!(cash["actualTenths"], 1000);
    assert_eq!(cash["status"], "above");
}
