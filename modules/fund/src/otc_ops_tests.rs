//! 场外基金操作测试（N09）：持仓登记／校准、订单状态机、资金占用与到账、
//! 转换两腿、定投计划与期次。全部在隔离内存库验证，不触真实用户账本。

use crate::fixed::Fixed;
use crate::ledger::{self, TransactionInput};
use crate::otc_ops::weekday_mon1 as weekday_mon1_pub;
use crate::storage::Store;

fn buy(
    store: &Store,
    account: &str,
    code: &str,
    name: &str,
    qty: &str,
    price: &str,
    date: &str,
    rid: &str,
) {
    let input = TransactionInput::manual(
        account.into(),
        code.into(),
        name.into(),
        "BUY".into(),
        "confirmed".into(),
        Fixed::parse(qty, 4).unwrap(),
        Fixed::parse(price, 4).unwrap(),
        Fixed::parse("0", 2).unwrap(),
        date.into(),
        rid.into(),
    );
    ledger::record_transaction(store, &input).unwrap();
}

fn sell(
    store: &Store,
    account: &str,
    code: &str,
    name: &str,
    qty: &str,
    price: &str,
    date: &str,
    rid: &str,
) {
    let input = TransactionInput::manual(
        account.into(),
        code.into(),
        name.into(),
        "SELL".into(),
        "confirmed".into(),
        Fixed::parse(qty, 4).unwrap(),
        Fixed::parse(price, 4).unwrap(),
        Fixed::parse("0", 2).unwrap(),
        date.into(),
        rid.into(),
    );
    ledger::record_transaction(store, &input).unwrap();
}

fn seed_cash(store: &Store, account: &str, amount: &str) {
    store
        .with_write(|conn| ledger::ensure_account(conn, account))
        .unwrap();
    store
        .with_write(|conn| {
            crate::assets::create_manual_asset(
                conn,
                &crate::assets::ManualAssetInput {
                    account_name: account.into(),
                    product_type: "cash".into(),
                    market: None,
                    code: None,
                    name: "现金".into(),
                    risk_class: "cash".into(),
                    market_value_text: amount.into(),
                    valued_at: crate::finance::today_cn(),
                    confirm_distinct: false,
                },
                &crate::finance::today_cn(),
            )?;
            let account_id: i64 =
                conn.query_row("SELECT id FROM accounts WHERE name=?1", [account], |r| {
                    r.get(0)
                })?;
            crate::finance::save_reserve(conn, account_id, Some("0.00"), "")
        })
        .unwrap();
}

fn seed_nav(store: &Store, code: &str, date: &str, nav_raw: i64) {
    store
        .with_write(|conn| {
            ledger::ensure_fund(conn, code, "测试基金")?;
            conn.execute(
                "INSERT INTO fund_nav(fund_id, nav_date, unit_nav_raw, source, fetched_at)
                 SELECT id, ?2, ?3, 'test', '1' FROM funds WHERE code=?1",
                rusqlite::params![code, date, nav_raw],
            )?;
            Ok::<(), crate::ledger::LedgerError>(())
        })
        .unwrap();
}

fn today() -> String {
    crate::finance::today_cn()
}

// ---------------------------------------------------------------------------
// 新增持仓（登记存量）
// ---------------------------------------------------------------------------

#[test]
fn holding_create_derives_cost_and_shares_without_cash_or_history() {
    let store = Store::open_in_memory().unwrap();
    let t = today();
    seed_nav(&store, "110022", &t, 12_345);
    // 市值 ¥1234.56、收益 -234.56（负收益）→ 成本 1469.12；净值 1.2345。
    let out = crate::otc_holding::holding_create(
        &store,
        "默认账户",
        "110022",
        "消费行业",
        123_456,
        -23_456,
        None,
        &t,
        false,
        "rid-hold-1",
        &t,
    )
    .unwrap();
    assert_eq!(out["sharesDerived"], true);
    // 份额 = 1234.56 / 1.2345 = 1000.0000。
    let (qty, cost, realized, as_of) = store
        .with_read(|conn| {
            let account = ledger::ensure_account(conn, "默认账户")?;
            let fund: i64 = conn.query_row("SELECT id FROM funds WHERE code='110022'", [], |r| r.get(0))?;
            let row: (i64, i64, i64, String) = conn
                .query_row(
                    "SELECT quantity_raw, cost_raw, realized_raw, as_of FROM positions WHERE account_id=?1 AND fund_id=?2",
                    rusqlite::params![account, fund],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .unwrap();
            let tx_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM transactions WHERE account_id=?1",
                    [account],
                    |r| r.get(0),
                )
                .unwrap();
            let derived: i64 = conn
                .query_row(
                    "SELECT derived_cost FROM opening_snapshots WHERE account_id=?1 AND fund_id=?2",
                    rusqlite::params![account, fund],
                    |r| r.get(0),
                )
                .unwrap();
            Ok::<_, crate::ledger::LedgerError>((row.0, row.1, row.2, (row.3, tx_count, derived)))
        })
        .unwrap();
    assert_eq!(qty, 10_000_486, "份额 1000.0486（半进位）");
    assert_eq!(cost, 146_912, "成本 ¥1469.12（分）");
    assert_eq!(realized, 0);
    assert_eq!(as_of.1, 0, "不生成任何历史交易");
    assert_eq!(as_of.2, 1, "推算留痕 derived_cost=1");
}

#[test]
fn holding_create_rejects_negative_cost_and_missing_nav() {
    let store = Store::open_in_memory().unwrap();
    let t = today();
    // 负成本：收益 > 市值。
    let err = crate::otc_holding::holding_create(
        &store, "A", "110022", "消费", 1_000, 2_000, None, &t, false, "r1", &t,
    )
    .unwrap_err();
    assert!(err.message().contains("负成本"), "{}", err.message());
    // 无净值且无实际份额 → 明确拒绝。
    let err = crate::otc_holding::holding_create(
        &store,
        "A",
        "160703",
        "十八国资",
        10_000,
        0,
        None,
        &t,
        false,
        "r2",
        &t,
    )
    .unwrap_err();
    assert!(err.message().contains("无可用净值"), "{}", err.message());
    // 无当日净值但有早期净值：未确认改用早期净值时拒绝。
    seed_nav(&store, "160703", "2026-01-30", 12_345);
    let err = crate::otc_holding::holding_create(
        &store,
        "A",
        "160703",
        "十八国资",
        10_000,
        0,
        None,
        &t,
        false,
        "r3",
        &t,
    )
    .unwrap_err();
    assert!(err.message().contains("无当日净值"), "{}", err.message());
    // 明确接受早期净值 → 成功，且使用早期净值。
    let out = crate::otc_holding::holding_create(
        &store,
        "A",
        "160703",
        "十八国资",
        10_000,
        0,
        None,
        &t,
        true,
        "r4",
        &t,
    )
    .unwrap();
    assert_eq!(out["sharesDerived"], true);
}

#[test]
fn holding_preview_never_creates_or_updates_an_account() {
    let store = Store::open_in_memory().unwrap();
    let t = today();
    seed_nav(&store, "110022", &t, 10_000);
    let before = store.with_read(Store::data_revision).unwrap();
    let preview = crate::otc_holding::holding_edit_preview(
        &store,
        "Preview only",
        "110022",
        10_000,
        -1_000,
        None,
        &t,
    )
    .unwrap();
    assert_eq!(preview["mode"], "new");
    assert_eq!(
        Fixed::parse(preview["cost"].as_str().unwrap(), 2)
            .unwrap()
            .raw(),
        11_000
    );
    let accounts: i64 = store
        .with_read(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM accounts WHERE name='Preview only'",
                [],
                |row| row.get(0),
            )
        })
        .unwrap();
    assert_eq!(accounts, 0);
    assert_eq!(store.with_read(Store::data_revision).unwrap(), before);
}

#[test]
fn holding_create_retry_replays_receipt_without_rewriting_later_changes() {
    let store = Store::open_in_memory().unwrap();
    let t = today();
    let save = |gain| {
        crate::otc_holding::holding_create(
            &store,
            "A",
            "110022",
            "Fund",
            10_000,
            gain,
            Some(10_000),
            &t,
            true,
            "holding-retry",
            &t,
        )
    };
    let first = save(-1_000).unwrap();
    let revision = store.with_read(Store::data_revision).unwrap();
    assert_eq!(save(-1_000).unwrap(), first);
    assert_eq!(store.with_read(Store::data_revision).unwrap(), revision);
    assert!(matches!(save(0), Err(ledger::LedgerError::Conflict(_))));
    store
        .with_write(|conn| conn.execute("UPDATE opening_snapshots SET cost_raw=20000", []))
        .unwrap();
    assert_eq!(save(-1_000).unwrap(), first);
    let cost: i64 = store
        .with_read(|conn| {
            conn.query_row("SELECT cost_raw FROM opening_snapshots", [], |row| {
                row.get(0)
            })
        })
        .unwrap();
    assert_eq!(cost, 20_000);
}

#[test]
fn holding_edit_opening_retry_is_atomic_and_preserves_newer_changes() {
    let store = Store::open_in_memory().unwrap();
    let date = today();
    seed_nav(&store, "110022", &date, 10_000);
    crate::otc_holding::holding_create(
        &store,
        "A",
        "110022",
        "消费",
        10_000,
        0,
        None,
        &date,
        true,
        "create-edit",
        &date,
    )
    .unwrap();
    let edit = |account, gain, request, version| {
        crate::otc_holding::holding_edit(
            &store,
            "opening",
            account,
            "110022",
            20_000,
            gain,
            Some(2_000_000),
            &date,
            "对账",
            request,
            true,
            Some(version),
            &date,
        )
    };
    let first = edit("A", 0, "opening-edit", 1).unwrap();
    let revision = store.with_read(Store::data_revision).unwrap();
    assert_eq!(edit("A", 0, "opening-edit", 1).unwrap(), first);
    assert_eq!(store.with_read(Store::data_revision).unwrap(), revision);
    assert!(matches!(
        edit("A", 1, "opening-edit", 1),
        Err(ledger::LedgerError::Conflict(_))
    ));
    assert!(matches!(
        edit("A", 0, "stale-edit", 1),
        Err(ledger::LedgerError::Conflict(_))
    ));
    assert!(edit("Missing", 0, "missing-edit", 1).is_err());
    assert_eq!(store.with_read(Store::data_revision).unwrap(), revision);
    edit("A", 1_000, "newer-edit", 2).unwrap();
    let revision = store.with_read(Store::data_revision).unwrap();
    assert_eq!(edit("A", 0, "opening-edit", 1).unwrap(), first);
    assert_eq!(store.with_read(Store::data_revision).unwrap(), revision);
    store.with_read(|conn| {
        let (cost, version): (i64,i64) = conn.query_row(
            "SELECT cost_raw,version FROM opening_snapshots", [], |row| Ok((row.get(0)?,row.get(1)?)),
        )?;
        assert_eq!((cost,version), (19_000,3));
        let audits: i64 = conn.query_row("SELECT COUNT(*) FROM opening_history", [], |row| row.get(0))?;
        let accounts: i64 = conn.query_row("SELECT COUNT(*) FROM accounts WHERE name='Missing'", [], |row| row.get(0))?;
        let failed: i64 = conn.query_row("SELECT COUNT(*) FROM app_meta WHERE key IN ('otc_holding_edit:stale-edit','otc_holding_edit:missing-edit')", [], |row| row.get(0))?;
        assert_eq!((audits,accounts,failed),(2,0,0));
        Ok::<_,ledger::LedgerError>(())
    }).unwrap();
}

#[test]
fn holding_edit_routes_opening_change_or_calibration() {
    let store = Store::open_in_memory().unwrap();
    let t = today();
    seed_nav(&store, "110022", &t, 12_345);
    crate::otc_holding::holding_create(
        &store, "A", "110022", "消费", 123_456, 23_456, None, &t, true, "r1", &t,
    )
    .unwrap();
    // 仅期初 → mode=opening。
    let preview =
        crate::otc_holding::holding_edit_preview(&store, "A", "110022", 130_000, 30_000, None, &t)
            .unwrap();
    assert_eq!(preview["mode"], "opening");
    // 后续交易 → mode=calibration。
    buy(
        &store, "A", "110022", "消费", "100.0000", "1.0000", &t, "tx-1",
    );
    let preview =
        crate::otc_holding::holding_edit_preview(&store, "A", "110022", 200_000, 10_000, None, &t)
            .unwrap();
    assert_eq!(preview["mode"], "calibration");
    let before_qty = preview["before"]["quantity"].as_str().unwrap().to_string();
    // 提交校准：份额（实际）1100 份、成本 190_000 分。
    let out = crate::otc_holding::holding_edit(
        &store,
        "calibration",
        "A",
        "110022",
        200_000,
        10_000,
        Some(11_000_000),
        &t,
        "渠道对账修正",
        "cal-1",
        true,
        None,
        &t,
    )
    .unwrap();
    assert_eq!(out["ok"], true);
    let revision = store.with_read(Store::data_revision).unwrap();
    let replay = crate::otc_holding::holding_edit(
        &store,
        "calibration",
        "A",
        "110022",
        200_000,
        10_000,
        Some(11_000_000),
        &t,
        "渠道对账修正",
        "cal-1",
        true,
        None,
        &t,
    )
    .unwrap();
    assert_eq!(replay, out);
    assert_eq!(store.with_read(Store::data_revision).unwrap(), revision);
    let (qty, cost, realized) = store
        .with_read(|conn| {
            let account = ledger::ensure_account(conn, "A")?;
            let fund: i64 = conn.query_row("SELECT id FROM funds WHERE code='110022'", [], |r| r.get(0))?;
            let row: (i64, i64, i64) = conn.query_row(
                "SELECT quantity_raw, cost_raw, realized_raw FROM positions WHERE account_id=?1 AND fund_id=?2",
                rusqlite::params![account, fund],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            Ok::<_, crate::ledger::LedgerError>(row)
        })
        .unwrap();
    assert_eq!(qty, 11_000_000, "校准后份额=1100.0000");
    assert_eq!(cost, 190_000, "校准后成本=¥1900.00");
    assert_eq!(realized, 0, "历史已实现收益不重算");
    let _ = before_qty;
    // 重复 request_id 幂等拒绝（不同内容冲突）。
    let err = crate::otc_holding::holding_edit(
        &store,
        "calibration",
        "A",
        "110022",
        200_000,
        10_000,
        Some(11_000_000),
        &t,
        "再次提交",
        "cal-1",
        true,
        None,
        &t,
    )
    .unwrap_err();
    assert!(matches!(err, crate::ledger::LedgerError::Conflict(_)));
}

// ---------------------------------------------------------------------------
// 加仓：占用 → 确认（现金恰扣一次）→ 取消释放
// ---------------------------------------------------------------------------

fn make_buy_draft(
    account: &str,
    code: &str,
    name: &str,
    amount: i64,
    rid: &str,
) -> crate::otc_ops::OrderDraft {
    crate::otc_ops::OrderDraft {
        op: crate::otc_ops::OP_BUY,
        account_name: account.into(),
        fund_code: code.into(),
        fund_name: name.into(),
        applied_date: today(),
        cutoff: "before_1500".into(),
        amount_minor: Some(amount),
        fee_minor: None,
        fee_known: false,
        shares_raw: None,
        target_code: None,
        target_name: None,
        plan_id: None,
        period_key: String::new(),
        request_id: rid.into(),
        note: String::new(),
    }
}

fn cash_minor(store: &Store, account: &str) -> i64 {
    store
        .with_read(|conn| {
            let account_id = ledger::ensure_account(conn, account)?;
            conn.query_row(
                "SELECT COALESCE(SUM(market_value_minor),0) FROM manual_assets
                 WHERE account_id=?1 AND product_type='cash'",
                [account_id],
                |r| r.get(0),
            )
            .map_err(|e| crate::ledger::LedgerError::Db(e.to_string()))
        })
        .unwrap()
}

#[test]
fn buy_order_reserves_then_confirms_cash_exactly_once() {
    let store = Store::open_in_memory().unwrap();
    seed_cash(&store, "A", "10000.00");
    let before = cash_minor(&store, "A");
    // 申请：现金未扣，占用已建。
    let out = crate::otc_ops::order_create(
        &store,
        &make_buy_draft("A", "110022", "消费", 100_000, "rid-buy-1"),
        &today(),
    )
    .unwrap();
    assert_eq!(out["cashReserved"], true);
    let _ = out["id"].as_i64().unwrap();
    assert_eq!(cash_minor(&store, "A"), before, "申请不扣现金");
    let held: i64 = store
        .with_read(|conn| {
            conn.query_row(
                "SELECT COALESCE(SUM(amount_minor),0) FROM cash_reservations WHERE status='held'",
                [],
                |r| r.get(0),
            )
        })
        .unwrap();
    assert_eq!(held, 100_000);
    // 确认：份额+净值 → 费用=总额−份额×净值；现金扣一次；占用释放。
    let id = out["id"].as_i64().unwrap();
    let confirm = crate::otc_ops::OrderConfirmInput {
        id,
        expected_version: 1,
        confirmed_shares_text: Some("800.0000".into()),
        confirmed_amount_text: None,
        fee_text: None,
        fee_known: false,
        confirmed_nav_date: Some(today()),
        confirmed_nav_text: Some("1.2300".into()),
    };
    crate::otc_ops::order_confirm(&store, &confirm, &today()).unwrap();
    // 份额 80000.0000 × 1.2300 = 98400.00 元 = 9_840_000 分 → 费用 1600 分。
    let (qty, cost) = store
        .with_read(|conn| {
            let account = ledger::ensure_account(conn, "A")?;
            let fund: i64 =
                conn.query_row("SELECT id FROM funds WHERE code='110022'", [], |r| r.get(0))?;
            let row: (i64, i64) = conn.query_row(
                "SELECT quantity_raw, cost_raw FROM positions WHERE account_id=?1 AND fund_id=?2",
                rusqlite::params![account, fund],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            Ok::<_, crate::ledger::LedgerError>(row)
        })
        .unwrap();
    assert_eq!(qty, 8_000_000);
    assert_eq!(cost, 100_000, "净申购 984.00 + 费用 16.00 = ¥1000.00 成本");
    assert_eq!(cash_minor(&store, "A"), before - 100_000, "现金恰扣一次");
    let held: i64 = store
        .with_read(|conn| {
            conn.query_row(
                "SELECT COALESCE(SUM(amount_minor),0) FROM cash_reservations WHERE status='held'",
                [],
                |r| r.get(0),
            )
        })
        .unwrap();
    assert_eq!(held, 0, "确认后占用释放");
    // 重复确认：Conflict。
    let err = crate::otc_ops::order_confirm(&store, &confirm, &today()).unwrap_err();
    assert!(matches!(err, crate::ledger::LedgerError::Conflict(_)));
}

#[test]
fn buy_order_cancel_releases_reservation() {
    let store = Store::open_in_memory().unwrap();
    seed_cash(&store, "A", "5000.00");
    let before = cash_minor(&store, "A");
    let out = crate::otc_ops::order_create(
        &store,
        &make_buy_draft("A", "110022", "消费", 100_000, "rid-buy-2"),
        &today(),
    )
    .unwrap();
    let id = out["id"].as_i64().unwrap();
    crate::otc_ops::order_cancel(&store, id, 1, "failed", "渠道申购失败").unwrap();
    let held: i64 = store
        .with_read(|conn| {
            conn.query_row(
                "SELECT COALESCE(SUM(amount_minor),0) FROM cash_reservations WHERE status='held'",
                [],
                |r| r.get(0),
            )
        })
        .unwrap();
    assert_eq!(held, 0, "失败释放占用");
    assert_eq!(cash_minor(&store, "A"), before);
    let status: String = store
        .with_read(|conn| {
            conn.query_row("SELECT status FROM fund_orders WHERE id=?1", [id], |r| {
                r.get(0)
            })
        })
        .unwrap();
    assert_eq!(status, "failed", "记录保留");
}

// ---------------------------------------------------------------------------
// 减仓：占用、超额拒绝、待到账、到账一次
// ---------------------------------------------------------------------------

fn make_sell_draft(
    account: &str,
    code: &str,
    name: &str,
    shares: &str,
    rid: &str,
) -> crate::otc_ops::OrderDraft {
    crate::otc_ops::OrderDraft {
        op: crate::otc_ops::OP_SELL,
        account_name: account.into(),
        fund_code: code.into(),
        fund_name: name.into(),
        applied_date: today(),
        cutoff: "after_1500".into(),
        amount_minor: None,
        fee_minor: None,
        fee_known: false,
        shares_raw: Some(Fixed::parse(shares, 4).unwrap().raw()),
        target_code: None,
        target_name: None,
        plan_id: None,
        period_key: String::new(),
        request_id: rid.into(),
        note: String::new(),
    }
}

#[test]
fn sell_order_occupancy_blocks_oversell_then_arrives_once() {
    let store = Store::open_in_memory().unwrap();
    seed_cash(&store, "A", "100.00");
    let t = today();
    // 已确认持仓 1000 份（成本 ¥900）。
    crate::otc_holding::holding_create(
        &store,
        "A",
        "110022",
        "消费",
        90_000,
        0,
        Some(10_000_000),
        &t,
        true,
        "o1",
        &t,
    )
    .unwrap();
    // 待确认减仓 400 份占用。
    let first = crate::otc_ops::order_create(
        &store,
        &make_sell_draft("A", "110022", "消费", "400.0000", "rid-sell-1"),
        &t,
    )
    .unwrap();
    // 可减 = 600 份；申请 700 → 拒绝。
    let err = crate::otc_ops::order_create(
        &store,
        &make_sell_draft("A", "110022", "消费", "700.0000", "rid-sell-2"),
        &t,
    )
    .unwrap_err();
    assert!(
        matches!(err, crate::ledger::LedgerError::Conflict(_)),
        "{}",
        err.message()
    );
    // 确认第一笔：300 份实际确认（部分成交）。
    let id = first["id"].as_i64().unwrap();
    crate::otc_ops::order_confirm(
        &store,
        &crate::otc_ops::OrderConfirmInput {
            id,
            expected_version: 1,
            confirmed_shares_text: Some("300.0000".into()),
            confirmed_amount_text: Some("330.00".into()),
            fee_text: Some("3.30".into()),
            fee_known: true,
            confirmed_nav_date: Some(t.clone()),
            confirmed_nav_text: Some("1.1000".into()),
        },
        &t,
    )
    .unwrap();
    // 持仓 700 份；现金未到账（receivable）。
    let (qty, cash) = store
        .with_read(|conn| {
            let account = ledger::ensure_account(conn, "A")?;
            let fund: i64 = conn.query_row("SELECT id FROM funds WHERE code='110022'", [], |r| r.get(0))?;
            let qty: i64 = conn.query_row(
                "SELECT quantity_raw FROM positions WHERE account_id=?1 AND fund_id=?2",
                rusqlite::params![account, fund],
                |r| r.get(0),
            )?;
            let cash: i64 = conn.query_row(
                "SELECT COALESCE(SUM(market_value_minor),0) FROM manual_assets WHERE account_id=?1 AND product_type='cash'",
                [account],
                |r| r.get(0),
            )?;
            Ok::<_, crate::ledger::LedgerError>((qty, cash))
        })
        .unwrap();
    assert_eq!(qty, 7_000_000, "确认后扣减持仓");
    assert_eq!(cash, 10_000, "待到账不进现金");
    // 到账：现金入一次；重复到账 Conflict。
    crate::otc_ops::order_arrival(&store, id, &t, "post", "").unwrap();
    assert_eq!(
        cash_minor(&store, "A"),
        10_000 + 32_670,
        "到账 ¥326.70（330−3.30 费用）"
    );
    let err = crate::otc_ops::order_arrival(&store, id, &t, "post", "").unwrap_err();
    assert!(matches!(err, crate::ledger::LedgerError::Conflict(_)));
}

// ---------------------------------------------------------------------------
// 转换：两腿、成本结转、现金不动
// ---------------------------------------------------------------------------

#[test]
fn convert_links_two_legs_and_moves_cost_without_cash() {
    let store = Store::open_in_memory().unwrap();
    seed_cash(&store, "A", "100.00");
    let t = today();
    crate::otc_holding::holding_create(
        &store,
        "A",
        "110022",
        "消费",
        90_000,
        0,
        Some(10_000_000),
        &t,
        true,
        "o1",
        &t,
    )
    .unwrap();
    let before_cash = cash_minor(&store, "A");
    let mut draft = make_sell_draft("A", "110022", "消费", "500.0000", "rid-conv-1");
    draft.op = "convert";
    draft.target_code = Some("005827".into());
    draft.target_name = Some("蓝筹混合".into());
    let out = crate::otc_ops::order_create(&store, &draft, &t).unwrap();
    let out_id = out["id"].as_i64().unwrap();
    let in_id = out["inId"].as_i64().unwrap();
    assert_eq!(out["conversionId"], out_id);
    for code in ["110022", "005827"] {
        let list = crate::otc_ops::orders_list(&store, "A", code).unwrap();
        let orders = list["orders"].as_array().unwrap();
        assert_eq!(
            orders.len(),
            2,
            "a conversion must expose both legs before a target position exists"
        );
        assert!(orders.iter().any(|order| order["id"] == out_id));
        assert!(orders.iter().any(|order| order["id"] == in_id));
    }
    // 转出未确认时转入不能先确认。
    let in_input = crate::otc_ops::OrderConfirmInput {
        id: in_id,
        expected_version: 1,
        confirmed_shares_text: Some("530.0000".into()),
        confirmed_amount_text: Some("539.28".into()),
        fee_text: Some("0".into()),
        fee_known: true,
        confirmed_nav_date: Some(t.clone()),
        confirmed_nav_text: Some("1.0175".into()),
    };
    let err = crate::otc_ops::order_confirm(&store, &in_input, &t).unwrap_err();
    assert!(err.message().contains("转出"), "{}", err.message());
    // 转出确认：500 份 × 1.0900 = 545.00 元。
    crate::otc_ops::order_confirm(
        &store,
        &crate::otc_ops::OrderConfirmInput {
            id: out_id,
            expected_version: 1,
            confirmed_shares_text: Some("500.0000".into()),
            confirmed_amount_text: Some("545.00".into()),
            fee_text: Some("5.45".into()),
            fee_known: true,
            confirmed_nav_date: Some(t.clone()),
            confirmed_nav_text: Some("1.0900".into()),
        },
        &t,
    )
    .unwrap();
    crate::otc_ops::order_confirm(&store, &in_input, &t).unwrap();
    let (out_qty, realized, in_qty, in_cost, cash) = store
        .with_read(|conn| {
            let account = ledger::ensure_account(conn, "A")?;
            let f1: i64 = conn.query_row("SELECT id FROM funds WHERE code='110022'", [], |r| r.get(0))?;
            let f2: i64 = conn.query_row("SELECT id FROM funds WHERE code='005827'", [], |r| r.get(0))?;
            let out_row: (i64, i64) = conn.query_row(
                "SELECT quantity_raw, realized_raw FROM positions WHERE account_id=?1 AND fund_id=?2",
                rusqlite::params![account, f1],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let in_row: (i64, i64) = conn.query_row(
                "SELECT quantity_raw, cost_raw FROM positions WHERE account_id=?1 AND fund_id=?2",
                rusqlite::params![account, f2],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let cash: i64 = conn.query_row(
                "SELECT COALESCE(SUM(market_value_minor),0) FROM manual_assets WHERE account_id=?1 AND product_type='cash'",
                [account],
                |r| r.get(0),
            )?;
            Ok::<_, crate::ledger::LedgerError>((out_row.0, out_row.1, in_row.0, in_row.1, cash))
        })
        .unwrap();
    assert_eq!(out_qty, 5_000_000, "转出后剩余 500 份");
    assert_eq!(
        realized,
        54_500 - 545 - 45_000,
        "已实现 = 545.00 − 5.45 费 − 450.00 成本（移动平均）"
    );
    assert_eq!(in_qty, 5_300_000, "目标并入 530 份");
    assert_eq!(in_cost, 53_928, "转入成本 = ¥539.28");
    assert_eq!(cash, before_cash, "转换全程现金不动");
}

// ---------------------------------------------------------------------------
// 定投：计划、期次、月末
// ---------------------------------------------------------------------------

fn plan_input(
    account: &str,
    code: &str,
    freq: &str,
    weekday: Option<i64>,
    monthday: Option<i64>,
) -> crate::otc_plans::PlanInput {
    crate::otc_plans::PlanInput {
        account_name: account.into(),
        fund_code: code.into(),
        fund_name: "消费".into(),
        amount_minor: 50_000,
        freq: freq.into(),
        weekday,
        monthday,
        start_date: today(),
        end_date: None,
        non_trading: "next".into(),
        note: String::new(),
    }
}

#[test]
fn plan_create_register_run_once_and_skip() {
    let store = Store::open_in_memory().unwrap();
    seed_cash(&store, "A", "10000.00");
    let t = today();
    let mut input = plan_input("A", "110022", "weekly", Some(3), None);
    let days0 = crate::assets::days_from_civil(&t).unwrap();
    let period =
        crate::assets::civil_from_days(days0 - (weekday_mon1_pub(days0) - 3).rem_euclid(7));
    input.start_date =
        crate::assets::civil_from_days(crate::assets::days_from_civil(&period).unwrap() - 21);
    crate::otc_plans::plan_create(&store, &input, &t).unwrap();
    let list = crate::otc_plans::plans_list(&store, "A", "110022", &t).unwrap();
    let plan_id = list["plans"][0]["id"].as_i64().unwrap();
    assert_eq!(
        list["plans"][0]["tradingDayVerified"], false,
        "节假日未判定"
    );
    // 期次登记买入 → 订单 pending；重复登记 Conflict。
    let out = crate::otc_plans::plan_run_register(
        &store,
        plan_id,
        &period,
        "buy",
        "before_1500",
        "rid-run-1",
        &t,
    )
    .unwrap();
    assert!(out["orderId"].as_i64().is_some());
    let err = crate::otc_plans::plan_run_register(
        &store,
        plan_id,
        &period,
        "buy",
        "before_1500",
        "rid-run-2",
        &t,
    )
    .unwrap_err();
    assert!(matches!(err, crate::ledger::LedgerError::Conflict(_)));
    // 跳过期次留痕（不同期次日）。
    let past = crate::assets::civil_from_days(crate::assets::days_from_civil(&period).unwrap() - 7);
    let earlier =
        crate::assets::civil_from_days(crate::assets::days_from_civil(&period).unwrap() - 14);
    crate::otc_plans::plan_run_register(
        &store,
        plan_id,
        &past,
        "skip",
        "before_1500",
        "rid-run-3",
        &t,
    )
    .unwrap();
    let runs = crate::otc_plans::plan_runs(&store, plan_id).unwrap();
    assert_eq!(runs["runs"].as_array().unwrap().len(), 2);
    // 暂停后不能登记。
    crate::otc_plans::plan_update(&store, plan_id, 1, "pause", None, &t).unwrap();
    let err = crate::otc_plans::plan_run_register(
        &store,
        plan_id,
        &earlier,
        "skip",
        "before_1500",
        "rid-run-4",
        &t,
    )
    .unwrap_err();
    assert!(err.message().contains("暂停"));
}

#[test]
fn plan_next_date_handles_month_end_clamp() {
    let store = Store::open_in_memory().unwrap();
    let t = today();
    let input = plan_input("A", "110022", "monthly", None, Some(31));
    crate::otc_plans::plan_create(&store, &input, &t).unwrap();
    let list = crate::otc_plans::plans_list(&store, "A", "110022", &t).unwrap();
    let next = list["plans"][0]["nextDate"].as_str().unwrap().to_string();
    // 下次执行日必须是原始周期日（或月末顺延/非交易日调整后）的有效日历日。
    assert!(crate::assets::is_valid_calendar_date(&next));
}

#[test]
fn convert_out_has_no_cash_arrival_but_sell_does() {
    let store = Store::open_in_memory().unwrap();
    seed_cash(&store, "A", "100.00");
    let t = today();
    crate::otc_holding::holding_create(
        &store,
        "A",
        "110022",
        "消费",
        90_000,
        0,
        Some(10_000_000),
        &t,
        true,
        "o1",
        &t,
    )
    .unwrap();
    let mut draft = make_sell_draft("A", "110022", "消费", "400.0000", "rid-arr-1");
    draft.op = "convert";
    draft.target_code = Some("005827".into());
    draft.target_name = Some("蓝筹".into());
    let out = crate::otc_ops::order_create(&store, &draft, &t).unwrap();
    let out_id = out["id"].as_i64().unwrap();
    crate::otc_ops::order_confirm(
        &store,
        &crate::otc_ops::OrderConfirmInput {
            id: out_id,
            expected_version: 1,
            confirmed_shares_text: Some("400.0000".into()),
            confirmed_amount_text: Some("440.00".into()),
            fee_text: Some("4.40".into()),
            fee_known: true,
            confirmed_nav_date: Some(t.clone()),
            confirmed_nav_text: Some("1.1000".into()),
        },
        &t,
    )
    .unwrap();
    // 转换转出腿资金直接过渡到转入份额，不进现金——登记到账必须拒绝。
    let err = crate::otc_ops::order_arrival(&store, out_id, &t, "post", "").unwrap_err();
    assert!(err.message().contains("减仓赎回"), "{}", err.message());
    assert_eq!(cash_minor(&store, "A"), 10_000, "转换不进现金");
}

#[test]
fn convert_create_is_idempotent_on_retry() {
    let store = Store::open_in_memory().unwrap();
    seed_cash(&store, "A", "100.00");
    let t = today();
    crate::otc_holding::holding_create(
        &store,
        "A",
        "110022",
        "消费",
        90_000,
        0,
        Some(10_000_000),
        &t,
        true,
        "o1",
        &t,
    )
    .unwrap();
    let mut draft = make_sell_draft("A", "110022", "消费", "1000.0000", "rid-conv-retry");
    draft.op = "convert";
    draft.target_code = Some("005827".into());
    draft.target_name = Some("蓝筹".into());
    let first = crate::otc_ops::order_create(&store, &draft, &t).unwrap();
    // 网络重试：同一 requestId 重复提交返回原转换（不建第二对腿）。
    let retry = crate::otc_ops::order_create(&store, &draft, &t).unwrap();
    assert_eq!(first["id"], retry["id"]);
    assert_eq!(first["inId"], retry["inId"]);
    draft.target_code = Some("012414".into());
    assert!(matches!(
        crate::otc_ops::order_create(&store, &draft, &t),
        Err(crate::ledger::LedgerError::Conflict(_))
    ));
    let count: i64 = store
        .with_read(|conn| {
            conn.query_row("SELECT COUNT(*) FROM fund_orders", [], |r| r.get(0))
                .map_err(|e| crate::ledger::LedgerError::Db(e.to_string()))
        })
        .unwrap();
    assert_eq!(count, 2, "重试不得新建第二对转换腿");
}

#[test]
fn buy_retry_rejects_changed_amount_or_account() {
    let store = Store::open_in_memory().unwrap();
    let t = today();
    let mut draft = make_buy_draft("A", "110022", "fund", 10000, "buy-retry");
    let first = crate::otc_ops::order_create(&store, &draft, &t).unwrap();
    assert_eq!(
        first["id"],
        crate::otc_ops::order_create(&store, &draft, &t).unwrap()["id"]
    );
    draft.amount_minor = Some(20000);
    assert!(matches!(
        crate::otc_ops::order_create(&store, &draft, &t),
        Err(crate::ledger::LedgerError::Conflict(_))
    ));
    draft.amount_minor = Some(10000);
    draft.account_name = "B".into();
    assert!(matches!(
        crate::otc_ops::order_create(&store, &draft, &t),
        Err(crate::ledger::LedgerError::Conflict(_))
    ));
}
