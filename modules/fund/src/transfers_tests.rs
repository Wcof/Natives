//! 账户间持仓转移测试（R2 T03 / A15、A16）。

use crate::fixed::Fixed;
use crate::ledger::{self, LedgerError, TransactionInput, STATE_CONFIRMED, TYPE_BUY};
use crate::portfolio;
use crate::storage::Store;

fn today() -> String {
    crate::finance::today_cn()
}

fn revision(store: &Store) -> i64 {
    store
        .with_read(|conn| crate::storage::Store::data_revision(conn))
        .unwrap()
        .parse()
        .unwrap()
}

fn create_account(store: &Store, name: &str) -> i64 {
    store
        .with_write(|conn| ledger::ensure_account(conn, name))
        .unwrap()
}

/// 记账买入：返回 (fund_id)。
fn buy(store: &Store, account: &str, code: &str, qty: &str, price: &str, fee: &str, rid: &str) {
    let date = today();
    store
        .with_write(|conn| {
            ledger::record_transaction_tx(
                conn,
                &TransactionInput::manual(
                    account.into(),
                    code.into(),
                    "测试标的".into(),
                    TYPE_BUY.into(),
                    STATE_CONFIRMED.into(),
                    Fixed::parse(qty, 4).unwrap(),
                    Fixed::parse(price, 4).unwrap(),
                    Fixed::parse(fee, 2).unwrap(),
                    date,
                    rid.into(),
                ),
            )
        })
        .unwrap();
}

fn fund_id(store: &Store, code: &str) -> i64 {
    store
        .with_read(|conn| {
            conn.query_row("SELECT id FROM funds WHERE code=?1", [code], |r| {
                r.get::<_, i64>(0)
            })
            .map_err(|e| LedgerError::Db(e.to_string()))
        })
        .unwrap()
}

fn position(store: &Store, account: i64, fund: i64) -> Option<portfolio::Position> {
    store
        .with_read(|conn| portfolio::replay_position(conn, account, fund))
        .unwrap()
}

fn input(from: i64, to: i64, fund: i64, qty: &str, rid: &str) -> crate::transfers::TransferInput {
    crate::transfers::TransferInput {
        from_id: from,
        to_id: to,
        fund_id: fund,
        quantity: Fixed::parse(qty, 4).unwrap(),
        request_id: rid.into(),
    }
}

fn commit(
    store: &Store,
    input: &crate::transfers::TransferInput,
    rev: i64,
) -> Result<(i64, i64, bool), LedgerError> {
    let date = today();
    store.with_write(|conn| crate::transfers::commit(conn, input, rev, &date))
}

#[test]
fn transfer_moves_shares_and_cost_conserved() {
    let store = Store::open_in_memory().unwrap();
    let a = create_account(&store, "甲");
    let b = create_account(&store, "乙");
    buy(&store, "甲", "000001", "100", "1.2345", "5.55", "b1");
    let f = fund_id(&store, "000001");
    // 成本 = 100×1.2345(→123.45) + 5.55 = 129.00（12900 raw）。
    let before = position(&store, a, f).unwrap();
    assert_eq!((before.quantity_raw, before.cost_raw), (1_000_000, 12_900));

    let plan_rev = revision(&store);
    let out = commit(&store, &input(a, b, f, "50", "t1"), plan_rev).unwrap();
    let (out_id, in_id, created) = out;
    assert!(created);
    assert_ne!(out_id, in_id);
    let from = position(&store, a, f).unwrap();
    let to = position(&store, b, f).unwrap();
    assert_eq!(
        (from.quantity_raw, from.cost_raw, from.realized_raw),
        (500_000, 6_450, 0)
    );
    assert_eq!(
        (to.quantity_raw, to.cost_raw, to.realized_raw),
        (500_000, 6_450, 0)
    );
    // 成本守恒，现金/待核对零污染。
    assert_eq!(from.cost_raw + to.cost_raw, 12_900);
    let trade_cash: i64 = store
        .with_read(|conn| conn.query_row("SELECT COUNT(*) FROM trade_cash", [], |r| r.get(0)))
        .unwrap();
    assert_eq!(trade_cash, 0);
    assert!(revision(&store) > plan_rev);
}

#[test]
fn retry_same_request_id_does_not_double_settle() {
    let store = Store::open_in_memory().unwrap();
    let a = create_account(&store, "甲");
    let b = create_account(&store, "乙");
    buy(&store, "甲", "000001", "100", "1.0", "0", "b1");
    let f = fund_id(&store, "000001");
    let rev = revision(&store);
    let first = commit(&store, &input(a, b, f, "40", "t1"), rev).unwrap();
    assert!(first.2);
    // 重试携带陈旧 revision：幂等命中返回原两腿，不再结算（即使 revision 已过期）。
    let retry = commit(&store, &input(a, b, f, "40", "t1"), rev).unwrap();
    assert_eq!(retry, (first.0, first.1, false));
    assert_eq!(position(&store, a, f).unwrap().quantity_raw, 600_000);
    let legs: i64 = store
        .with_read(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM transactions WHERE source='transfer'",
                [],
                |r| r.get(0),
            )
        })
        .unwrap();
    assert_eq!(legs, 2);
}

#[test]
fn stale_expected_revision_is_rejected_atomically() {
    let store = Store::open_in_memory().unwrap();
    let a = create_account(&store, "甲");
    let b = create_account(&store, "乙");
    buy(&store, "甲", "000001", "100", "1.0", "0", "b1");
    let f = fund_id(&store, "000001");
    let stale = revision(&store);
    // 预览后另一写入推进修订。
    buy(&store, "甲", "000002", "10", "1.0", "0", "b2");
    let err = commit(&store, &input(a, b, f, "50", "t1"), stale).unwrap_err();
    assert!(matches!(err, LedgerError::Conflict(_)));
    // 无部分写：无任何 transfer 流水，持仓未变。
    let legs: i64 = store
        .with_read(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM transactions WHERE source='transfer'",
                [],
                |r| r.get(0),
            )
        })
        .unwrap();
    assert_eq!(legs, 0);
    assert_eq!(position(&store, a, f).unwrap().quantity_raw, 1_000_000);
}

#[test]
fn oversell_zero_quantity_and_self_transfer_rejected() {
    let store = Store::open_in_memory().unwrap();
    let a = create_account(&store, "甲");
    let b = create_account(&store, "乙");
    buy(&store, "甲", "000001", "10", "1.0", "0", "b1");
    let f = fund_id(&store, "000001");
    let rev = revision(&store);
    assert!(matches!(
        commit(&store, &input(a, b, f, "10.0001", "t1"), rev),
        Err(LedgerError::Oversell { .. })
    ));
    assert!(matches!(
        commit(&store, &input(a, b, f, "0", "t2"), rev),
        Err(LedgerError::Invalid(_))
    ));
    assert!(matches!(
        commit(&store, &input(a, a, f, "5", "t3"), rev),
        Err(LedgerError::Invalid(_))
    ));
    // 无持仓的转出账户（标的存在，但该账户未持有；建档推进修订，需刷新 rev）。
    let d = create_account(&store, "丁");
    let rev = revision(&store);
    let no_position = commit(&store, &input(d, b, f, "5", "t4"), rev);
    assert!(
        matches!(no_position, Err(LedgerError::NotFound(_))),
        "actual: {no_position:?}"
    );
}

#[test]
fn unknown_cost_position_rejected() {
    let store = Store::open_in_memory().unwrap();
    let a = create_account(&store, "甲");
    let b = create_account(&store, "乙");
    let date = today();
    store
        .with_write(|conn| {
            ledger::record_opening_tx(
                conn,
                &ledger::OpeningInput {
                    account_name: "甲".into(),
                    fund_code: "000001".into(),
                    fund_name: "测试标的".into(),
                    quantity: Fixed::parse("100", 4).unwrap(),
                    as_of: date.clone(),
                    total_cost: None,
                },
                &date,
            )
        })
        .unwrap();
    let f = fund_id(&store, "000001");
    let rev = revision(&store);
    assert!(matches!(
        commit(&store, &input(a, b, f, "50", "t1"), rev),
        Err(LedgerError::Invalid(_))
    ));
    // 真实 0 成本（非未知）允许转移（成本平移 0）。
    let c = create_account(&store, "丙");
    store
        .with_write(|conn| {
            ledger::record_opening_tx(
                conn,
                &ledger::OpeningInput {
                    account_name: "丙".into(),
                    fund_code: "000009".into(),
                    fund_name: "零成本".into(),
                    quantity: Fixed::parse("100", 4).unwrap(),
                    as_of: date,
                    total_cost: Some(Fixed::parse("0", 2).unwrap()),
                },
                &today(),
            )
        })
        .unwrap();
    let f9 = fund_id(&store, "000009");
    let rev = revision(&store);
    commit(&store, &input(c, b, f9, "50", "t2"), rev).unwrap();
    assert_eq!(position(&store, b, f9).unwrap().cost_raw, 0);
}

#[test]
fn archived_accounts_and_stock_lots_enforced() {
    let store = Store::open_in_memory().unwrap();
    let a = create_account(&store, "甲");
    let b = create_account(&store, "乙");
    store
        .with_write(|conn| crate::accounts::archive_account(conn, b, 1, "ar1"))
        .unwrap();
    buy(&store, "甲", "000001", "10", "1.0", "0", "b1");
    let f = fund_id(&store, "000001");
    let rev = revision(&store);
    assert!(matches!(
        commit(&store, &input(a, b, f, "5", "t1"), rev),
        Err(LedgerError::Invalid(_))
    ));
    // 股票整股约束（先刷新修订：失败提交不推进 revision）。
    buy(&store, "甲", "stock:SSE:600000", "100", "1.0", "0", "b2");
    let s = fund_id(&store, "stock:SSE:600000");
    let c = create_account(&store, "丙");
    let rev = revision(&store);
    assert!(matches!(
        commit(&store, &input(a, c, s, "50.5", "t2"), rev),
        Err(LedgerError::Invalid(_))
    ));
    commit(&store, &input(a, c, s, "50", "t3"), rev).unwrap();
    assert_eq!(position(&store, c, s).unwrap().quantity_raw, 500_000);
}

#[test]
fn transfer_onto_existing_position_accumulates_cost() {
    let store = Store::open_in_memory().unwrap();
    let a = create_account(&store, "甲");
    let b = create_account(&store, "乙");
    buy(&store, "甲", "000001", "100", "2.0", "0", "b1");
    buy(&store, "乙", "000001", "10", "1.0", "0", "b2");
    let f = fund_id(&store, "000001");
    let rev = revision(&store);
    commit(&store, &input(a, b, f, "50", "t1"), rev).unwrap();
    let to = position(&store, b, f).unwrap();
    // 乙原成本 10.00（1000 raw）+ 转入成本 100.00（10000 raw）。
    assert_eq!((to.quantity_raw, to.cost_raw), (600_000, 11_000));
}

#[test]
fn transfer_legs_cannot_be_deleted_or_corrected() {
    let store = Store::open_in_memory().unwrap();
    let a = create_account(&store, "甲");
    let b = create_account(&store, "乙");
    buy(&store, "甲", "000001", "100", "1.0", "0", "b1");
    let f = fund_id(&store, "000001");
    let rev = revision(&store);
    let (out_id, in_id, _) = commit(&store, &input(a, b, f, "30", "t1"), rev).unwrap();
    // 转账腿不能单独删除（会破坏成本守恒）。
    assert!(crate::corrections::preview_delete(&store, out_id)
        .map(|_| ())
        .is_err_and(|e| matches!(e, LedgerError::Invalid(_))));
    let input = crate::corrections::CorrectionInput {
        tx_id: in_id,
        quantity_raw: 10_000,
        price_raw: 1,
        fee_raw: 0,
        trade_date: today(),
        reason: "误录".into(),
        request_id: "c1".into(),
    };
    assert!(matches!(
        crate::corrections::preview_correction(&store, &input),
        Err(LedgerError::Invalid(_))
    ));
}

#[test]
fn preview_reports_plan_and_revision_before_any_write() {
    // mode/T04（A12）：预览返回业务类型口径的数量/成本/revision，不写状态；
    // 携带 dataRevision 供提交使用。
    let store = Store::open_in_memory().unwrap();
    let a = create_account(&store, "甲");
    let b = create_account(&store, "乙");
    buy(&store, "甲", "600519", "100", "10.0000", "0", "open-a");
    let f = fund_id(&store, "600519");
    let rev = revision(&store);
    let preview = store
        .with_read(|conn| crate::transfers::preview(conn, &input(a, b, f, "40", "p1")))
        .unwrap();
    // 100 股 @10.0000、零费用：成本 1000.00（amount scale2 → 100_000 raw）；
    // 转 40 股按成本平移 400.00（40_000 raw），剩余 600.00（60_000 raw）。
    assert_eq!(preview.from_quantity_raw, 600_000);
    assert_eq!(preview.from_cost_raw, 60_000);
    assert_eq!(preview.to_quantity_raw, 400_000);
    assert_eq!(preview.to_cost_raw, 40_000);
    assert_eq!(preview.transferred_cost_raw, 40_000);
    assert_eq!(preview.quantity_raw, 400_000);
    assert_eq!(preview.revision, rev);
    assert!(!preview.target_existing);
    // 预览不写状态：revision 不变、乙账户仍无持仓。
    assert_eq!(revision(&store), rev);
    assert!(position(&store, b, f).is_none());
    // 提交可用预览 revision 原子落账，重复请求不重复扣减。
    let (first, retry) = {
        let i = input(a, b, f, "40", "p1");
        let one = commit(&store, &input(a, b, f, "40", "p1"), preview.revision);
        let two = commit(&store, &i, preview.revision);
        (one, two)
    };
    assert!(first.unwrap().2);
    assert!(!retry.unwrap().2);
}
