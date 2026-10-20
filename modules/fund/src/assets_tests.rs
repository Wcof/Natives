//! assets 模块测试（P1，方案 05 §3/§6 验收例题）。

use super::*;
use crate::ledger::{self, TransactionInput};
use crate::storage::Store;

#[test]
fn new_cash_account_and_balance_are_atomic() {
    let store = Store::open_in_memory().unwrap();
    let today = crate::finance::today_cn();
    let mut input = ManualAssetInput {
        account_name: "New cash".into(),
        product_type: "cash".into(),
        market: None,
        code: None,
        name: "Cash".into(),
        risk_class: "cash".into(),
        market_value_text: "-1".into(),
        valued_at: today.clone(),
        confirm_distinct: false,
    };
    assert!(create_asset(&store, &input, &today).is_err());
    assert_eq!(
        store
            .with_read(|c| c.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get::<_, i64>(0)))
            .unwrap(),
        0
    );
    input.market_value_text = "1000.01".into();
    create_asset(&store, &input, &today).unwrap();
    assert!(create_asset(&store, &input, &today).is_err());
    assert_eq!(
        summary(&store, &today).unwrap().known_value_minor,
        Some(100001)
    );
}

/// 建测试账户（人工资产要求账户已存在，不悬空）。
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

fn input(
    account: &str,
    pt: &str,
    code: Option<&str>,
    name: &str,
    value: &str,
    date: &str,
) -> ManualAssetInput {
    ManualAssetInput {
        account_name: account.to_string(),
        product_type: pt.to_string(),
        market: code.map(|_| "SSE".to_string()),
        code: code.map(str::to_string),
        name: name.to_string(),
        risk_class: "unknown".to_string(),
        market_value_text: value.to_string(),
        valued_at: date.to_string(),
        confirm_distinct: false,
    }
}

#[test]
fn cash_and_gold_keys_and_normalization() {
    let store = Store::open_in_memory().unwrap();
    ensure_accounts(&store, &["A"]);
    store
        .with_write(|conn| {
            create_manual_asset(
                conn,
                &input("A", "cash", None, "现金", "5000.00", "2026-09-29"),
                "2026-09-29",
            )
        })
        .unwrap();
    // 黄金名称归一化：内部空白统一。
    store
        .with_write(|conn| {
            create_manual_asset(
                conn,
                &input(
                    "A",
                    "gold",
                    None,
                    "金条  克重1000",
                    "100000.00",
                    "2026-09-29",
                ),
                "2026-09-29",
            )
        })
        .unwrap();
    let key: String = store
        .with_read(|conn| {
            conn.query_row(
                "SELECT instrument_key FROM manual_assets WHERE product_type='gold'",
                [],
                |r| r.get(0),
            )
        })
        .unwrap();
    assert_eq!(key, "gold:金条 克重1000");
    // 现金账户内唯一：第二条 cash 拒绝。
    let err = store
        .with_write(|conn| {
            create_manual_asset(
                conn,
                &input("A", "cash", None, "现金2", "1.00", "2026-09-29"),
                "2026-09-29",
            )
        })
        .unwrap_err();
    assert_eq!(err.code(), "ASSET_DUPLICATE");
}

#[test]
fn stock_and_etf_share_security_key() {
    let store = Store::open_in_memory().unwrap();
    ensure_accounts(&store, &["A"]);
    store
        .with_write(|conn| {
            create_manual_asset(
                conn,
                &input(
                    "A",
                    "stock",
                    Some("600519"),
                    "贵州茅台",
                    "1500000.00",
                    "2026-09-29",
                ),
                "2026-09-29",
            )
        })
        .unwrap();
    // 同账户同市场同代码换产品类型仍被唯一键拒绝（方案 05 §6）。
    let err = store
        .with_write(|conn| {
            create_manual_asset(
                conn,
                &input(
                    "A",
                    "etf",
                    Some("600519"),
                    "贵州茅台ETF",
                    "1500000.00",
                    "2026-09-29",
                ),
                "2026-09-29",
            )
        })
        .unwrap_err();
    assert_eq!(err.code(), "ASSET_DUPLICATE");
}

#[test]
fn zero_value_valid_but_empty_invalid() {
    let store = Store::open_in_memory().unwrap();
    ensure_accounts(&store, &["A"]);
    // 零是有效金额。
    let id = store
        .with_write(|conn| {
            create_manual_asset(
                conn,
                &input("A", "cash", None, "现金", "0.00", "2026-09-29"),
                "2026-09-29",
            )
        })
        .unwrap();
    assert!(id > 0);
    // 空输入无效。
    let err = store
        .with_write(|conn| {
            create_manual_asset(
                conn,
                &input("A", "gold", None, "金币", "", "2026-09-29"),
                "2026-09-29",
            )
        })
        .unwrap_err();
    assert_eq!(err.code(), "ASSET_INVALID");
    // 超过两位小数拒绝。
    let err = store
        .with_write(|conn| {
            create_manual_asset(
                conn,
                &input("A", "gold", None, "金币", "10.123", "2026-09-29"),
                "2026-09-29",
            )
        })
        .unwrap_err();
    assert_eq!(err.code(), "ASSET_INVALID");
}

#[test]
fn future_date_rejected_and_calendar_validated() {
    let store = Store::open_in_memory().unwrap();
    ensure_accounts(&store, &["A"]);
    let err = store
        .with_write(|conn| {
            create_manual_asset(
                conn,
                &input("A", "cash", None, "现金", "1.00", "2026-10-01"),
                "2026-09-29",
            )
        })
        .unwrap_err();
    assert_eq!(err.code(), "ASSET_INVALID");
    let err = store
        .with_write(|conn| {
            create_manual_asset(
                conn,
                &input("A", "cash", None, "现金", "1.00", "2026-02-30"),
                "2026-09-29",
            )
        })
        .unwrap_err();
    assert_eq!(err.code(), "ASSET_INVALID");
}

#[test]
fn overlap_review_flow_with_current_fund_holding() {
    let store = Store::open_in_memory().unwrap();
    buy(
        &store,
        "A",
        "600519",
        "贵州茅台",
        "100.0000",
        "1500.0000",
        "2026-09-01",
    );
    // 同账户同代码在持基金：未确认时拒绝（ASSET_OVERLAP_REVIEW）。
    let err = store
        .with_write(|conn| {
            create_manual_asset(
                conn,
                &input(
                    "A",
                    "stock",
                    Some("600519"),
                    "茅台（港股通？）",
                    "1.00",
                    "2026-09-29",
                ),
                "2026-09-29",
            )
        })
        .unwrap_err();
    assert!(matches!(err, AssetError::OverlapReview { .. }), "{err:?}");
    // 用户确认是不同资产后允许保存，且详情可见 distinct 标记。
    let mut confirmed = input(
        "A",
        "stock",
        Some("600519"),
        "茅台ADR",
        "1.00",
        "2026-09-29",
    );
    confirmed.confirm_distinct = true;
    let id = store
        .with_write(|conn| create_manual_asset(conn, &confirmed, "2026-09-29"))
        .unwrap();
    let distinct = store
        .with_read(|conn| {
            conn.query_row(
                "SELECT legacy_overlap_distinct FROM manual_assets WHERE id=?1",
                params![id],
                |r| r.get::<_, i64>(0),
            )
        })
        .unwrap();
    assert_eq!(distinct, 1);
    // 清仓记录不触发当前资产重复冲突。
    let input2 = TransactionInput::manual(
        "B".to_string(),
        "000001".to_string(),
        "华夏成长".to_string(),
        "BUY".to_string(),
        "confirmed".to_string(),
        Fixed::parse("100.0000", 4).unwrap(),
        Fixed::parse("1.0000", 4).unwrap(),
        Fixed::zero(2),
        "2026-09-01".to_string(),
        "req-b-000001".into(),
    );
    ledger::record_transaction(&store, &input2).unwrap();
    let input3 = TransactionInput::manual(
        "B".to_string(),
        "000001".to_string(),
        "华夏成长".to_string(),
        "SELL".to_string(),
        "confirmed".to_string(),
        Fixed::parse("100.0000", 4).unwrap(),
        Fixed::parse("1.0500", 4).unwrap(),
        Fixed::zero(2),
        "2026-09-10".to_string(),
        "req-b-000001-sell".into(),
    );
    ledger::record_transaction(&store, &input3).unwrap();
    store
        .with_write(|conn| {
            create_manual_asset(
                conn,
                &input(
                    "B",
                    "stock",
                    Some("000001"),
                    "平安银行",
                    "1.00",
                    "2026-09-29",
                ),
                "2026-09-29",
            )
        })
        .unwrap();
}

#[test]
fn update_identity_change_resets_distinct_flag() {
    let store = Store::open_in_memory().unwrap();
    ensure_accounts(&store, &["A"]);
    buy(
        &store,
        "A",
        "600519",
        "贵州茅台",
        "100.0000",
        "1500.0000",
        "2026-09-01",
    );
    let mut confirmed = input(
        "A",
        "stock",
        Some("600519"),
        "茅台ADR",
        "1.00",
        "2026-09-29",
    );
    confirmed.confirm_distinct = true;
    let id = store
        .with_write(|conn| create_manual_asset(conn, &confirmed, "2026-09-29"))
        .unwrap();
    // 改代码：旧确认失效，新代码无冲突则 distinct 归零。
    let renamed = input(
        "A",
        "stock",
        Some("600036"),
        "招商银行",
        "2.00",
        "2026-09-29",
    );
    store
        .with_write(|conn| update_manual_asset(conn, id, &renamed, "2026-09-29"))
        .unwrap();
    let distinct = store
        .with_read(|conn| {
            conn.query_row(
                "SELECT legacy_overlap_distinct FROM manual_assets WHERE id=?1",
                params![id],
                |r| r.get::<_, i64>(0),
            )
        })
        .unwrap();
    assert_eq!(distinct, 0);
}

#[test]
fn delete_removes_row() {
    let store = Store::open_in_memory().unwrap();
    ensure_accounts(&store, &["A"]);
    let id = store
        .with_write(|conn| {
            create_manual_asset(
                conn,
                &input("A", "cash", None, "现金", "1.00", "2026-09-29"),
                "2026-09-29",
            )
        })
        .unwrap();
    store
        .with_write(|conn| delete_manual_asset(conn, id))
        .unwrap();
    let err = store
        .with_write(|conn| delete_manual_asset(conn, id))
        .unwrap_err();
    assert_eq!(err.code(), "ASSET_NOT_FOUND");
}

#[test]
fn fund_currency_confirmation_gates_cny_valuation() {
    let store = Store::open_in_memory().unwrap();
    ensure_accounts(&store, &["A"]);
    buy(
        &store,
        "A",
        "110022",
        "消费行业",
        "100.0000",
        "1.2345",
        "2026-09-01",
    );
    // 手动记账现在随建档自动确认境内场外基金为 CNY（api::positions 快照测试
    // 覆盖该路径）；本测试守护的是「未确认币种不产生现值」的确认门，故还原
    // 为导入旧基金场景：无元数据行 = 币种未确认（assets 模块规则）。
    store
        .with_write(|conn| -> rusqlite::Result<()> {
            conn.execute(
                "DELETE FROM fund_asset_metadata WHERE fund_id IN (SELECT id FROM funds WHERE code='110022')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    // 保存净值（fund_nav 权威表）。
    store
        .with_write(|conn| -> rusqlite::Result<()> {
            conn.execute(
                "INSERT INTO fund_nav(fund_id, nav_date, unit_nav_raw, source, fetched_at)
                 SELECT id, '2026-09-28', 12345, 'eastmoney-mobile', '1' FROM funds WHERE code='110022'",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    // 未确认币种：现值 None，currencyUnconfirmedCount=1（方案 05 §6）。
    let summary = store
        .with_read(|conn| assets_summary(conn, "2026-09-29"))
        .unwrap();
    assert_eq!(summary.currency_unconfirmed_count, 1);
    assert!(summary.funds[0].value_minor.is_none());
    assert!(summary.complete_value_minor.is_none());
    // 确认为 CNY：现值 = 100.0000 × 1.2345 = ¥123.45。
    let fund_id: i64 = store
        .with_read(|conn| {
            conn.query_row("SELECT id FROM funds WHERE code='110022'", [], |r| r.get(0))
        })
        .unwrap();
    store
        .with_write(|conn| set_fund_metadata(conn, fund_id, "CNY", "equity"))
        .unwrap();
    let summary = store
        .with_read(|conn| assets_summary(conn, "2026-09-29"))
        .unwrap();
    assert_eq!(summary.funds[0].value_minor, Some(12345));
    assert_eq!(summary.fund_value_minor, Some(12345));
    assert_eq!(summary.complete_value_minor, Some(12345));
    assert_eq!(summary.quality, "ready");
    // 标记为非人民币：暂不支持换算，不计入人民币总额。
    store
        .with_write(|conn| set_fund_metadata(conn, fund_id, "unsupported", "unknown"))
        .unwrap();
    let summary = store
        .with_read(|conn| assets_summary(conn, "2026-09-29"))
        .unwrap();
    assert_eq!(summary.currency_unsupported_count, 1);
    assert!(summary.complete_value_minor.is_none());
}

#[test]
fn missing_nav_is_not_replaced_by_cost() {
    let store = Store::open_in_memory().unwrap();
    ensure_accounts(&store, &["A"]);
    buy(
        &store,
        "A",
        "110022",
        "消费行业",
        "100.0000",
        "1.2345",
        "2026-09-01",
    );
    let fund_id: i64 = store
        .with_read(|conn| {
            conn.query_row("SELECT id FROM funds WHERE code='110022'", [], |r| r.get(0))
        })
        .unwrap();
    store
        .with_write(|conn| set_fund_metadata(conn, fund_id, "CNY", "unknown"))
        .unwrap();
    let summary = store
        .with_read(|conn| assets_summary(conn, "2026-09-29"))
        .unwrap();
    assert_eq!(summary.missing_price_count, 1);
    assert!(
        summary.funds[0].value_minor.is_none(),
        "缺净值不得拿成本冒充市值"
    );
    assert_eq!(summary.quality, "incomplete");
}

#[test]
fn manual_zero_is_priced_and_overlap_excluded_from_totals() {
    let store = Store::open_in_memory().unwrap();
    ensure_accounts(&store, &["A"]);
    store
        .with_write(|conn| {
            create_manual_asset(
                conn,
                &input("A", "cash", None, "现金", "0.00", "2026-09-29"),
                "2026-09-29",
            )
        })
        .unwrap();
    // 无冲突的人工证券记录。
    let suspect = input(
        "A",
        "stock",
        Some("000002"),
        "万家红筹",
        "500.00",
        "2026-09-29",
    );
    store
        .with_write(|conn| create_manual_asset(conn, &suspect, "2026-09-29"))
        .unwrap();
    // 零值现金仍算已计价。
    let summary = store
        .with_read(|conn| assets_summary(conn, "2026-09-29"))
        .unwrap();
    assert!(summary.has_entries);
    assert_eq!(summary.manual_value_minor, Some(50000));
    assert_eq!(summary.quality, "ready");
}

#[test]
fn stale_valuation_marked_after_seven_days() {
    let store = Store::open_in_memory().unwrap();
    ensure_accounts(&store, &["A"]);
    store
        .with_write(|conn| {
            create_manual_asset(
                conn,
                &input("A", "cash", None, "现金", "100.00", "2026-09-20"),
                "2026-09-29",
            )
        })
        .unwrap();
    let summary = store
        .with_read(|conn| assets_summary(conn, "2026-09-29"))
        .unwrap();
    // 9 天前：较旧；完整性不受影响 → stale 且完整合计存在。
    assert_eq!(summary.old_valuation_count, 1);
    assert_eq!(summary.quality, "stale");
    assert_eq!(summary.complete_value_minor, Some(10000));
}

#[test]
fn valuation_dates_cover_items_not_fake_single_date() {
    let store = Store::open_in_memory().unwrap();
    ensure_accounts(&store, &["A"]);
    store
        .with_write(|conn| {
            create_manual_asset(
                conn,
                &input("A", "cash", None, "现金", "100.00", "2026-09-01"),
                "2026-09-29",
            )
        })
        .unwrap();
    store
        .with_write(|conn| {
            create_manual_asset(
                conn,
                &input("A", "gold", None, "金币", "200.00", "2026-09-25"),
                "2026-09-29",
            )
        })
        .unwrap();
    let summary = store
        .with_read(|conn| assets_summary(conn, "2026-09-29"))
        .unwrap();
    assert_eq!(summary.earliest_valuation.as_deref(), Some("2026-09-01"));
    assert_eq!(summary.latest_valuation.as_deref(), Some("2026-09-25"));
}

/// 分位合计溢出必须显式报错：饱和值会变成一个看似合法的错误金额
/// （方案 03 验收「金额溢出不得给出错误合计」）。
#[test]
fn amount_overflow_is_reported_not_saturated() {
    let store = Store::open_in_memory().unwrap();
    ensure_accounts(&store, &["A", "B"]);
    store
        .with_write(|conn| -> rusqlite::Result<()> {
            for (account, name) in [("A", "现金A"), ("B", "现金B")] {
                let account_id: i64 =
                    conn.query_row("SELECT id FROM accounts WHERE name = ?1", [account], |r| {
                        r.get(0)
                    })?;
                conn.execute(
                    "INSERT INTO manual_assets(account_id, product_type, instrument_key, name, \
                     risk_class, market_value_minor, valued_at, created_at, updated_at) \
                     VALUES(?1,'cash','cash',?2,'cash',?3,'2026-09-29','x','x')",
                    rusqlite::params![account_id, name, i64::MAX],
                )?;
            }
            Ok(())
        })
        .unwrap();
    let err = store
        .with_read(|conn| assets_summary(conn, "2026-09-29"))
        .unwrap_err();
    assert_eq!(err.code(), "ASSET_AMOUNT_OVERFLOW");
    assert!(
        err.message().contains("超出可表示范围"),
        "{}",
        err.message()
    );
}
