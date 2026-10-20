//! ledger 测试（拆分自 ledger.rs，仅移动，无逻辑变更）。

#![cfg(test)]

use super::{
    ensure_account, ensure_fund, record_transaction, LedgerError, TransactionInput,
    STATE_CONFIRMED, STATE_PENDING, TYPE_BUY, TYPE_SELL,
};
use crate::fixed::{Fixed, SCALE_SHARE};
use crate::storage::Store;

fn buy(
    store: &Store,
    account: &str,
    code: &str,
    qty: &str,
    price: &str,
    fee: &str,
    date: &str,
    rid: &str,
) -> (i64, bool) {
    record_transaction(
        store,
        &TransactionInput::manual(
            account.into(),
            code.into(),
            "测试基金".into(),
            TYPE_BUY.into(),
            STATE_CONFIRMED.into(),
            Fixed::parse(qty, SCALE_SHARE).unwrap(),
            Fixed::parse(price, SCALE_SHARE).unwrap(),
            Fixed::parse(fee, 2).unwrap(),
            date.into(),
            rid.into(),
        ),
    )
    .unwrap()
}

fn sell(
    store: &Store,
    account: &str,
    code: &str,
    qty: &str,
    price: &str,
    fee: &str,
    date: &str,
    rid: &str,
) -> Result<(i64, bool), LedgerError> {
    record_transaction(
        store,
        &TransactionInput::manual(
            account.into(),
            code.into(),
            "测试基金".into(),
            TYPE_SELL.into(),
            STATE_CONFIRMED.into(),
            Fixed::parse(qty, SCALE_SHARE).unwrap(),
            Fixed::parse(price, SCALE_SHARE).unwrap(),
            Fixed::parse(fee, 2).unwrap(),
            date.into(),
            rid.into(),
        ),
    )
}

/// 实施方案 §7.2 黄金用例（B-G2）。
#[test]
fn golden_case_moving_average_cost() {
    let store = Store::open_in_memory().unwrap();
    store
        .with_write(|conn| -> Result<(), LedgerError> {
            ensure_account(conn, "默认账户")?;
            ensure_fund(conn, "000001", "测试基金")?;
            Ok(())
        })
        .unwrap();

    buy(
        &store,
        "默认账户",
        "000001",
        "100",
        "1.00",
        "1.00",
        "2026-01-05",
        "r1",
    );
    buy(
        &store,
        "默认账户",
        "000001",
        "50",
        "1.20",
        "0.50",
        "2026-01-06",
        "r2",
    );
    sell(
        &store,
        "默认账户",
        "000001",
        "60",
        "1.30",
        "0.50",
        "2026-01-07",
        "r3",
    )
    .unwrap();

    let position = store
        .with_read(|conn| crate::portfolio::replay_position(conn, 1, 1))
        .unwrap()
        .unwrap();
    // 剩余份额 90、成本 96.90
    assert_eq!(position.quantity_raw, 900_000);
    assert_eq!(position.cost_raw, 96_90);
    assert_eq!(position.realized_raw, 12_90);
}

#[test]
fn oversell_rejected_at_write_time() {
    let store = Store::open_in_memory().unwrap();
    store
        .with_write(|conn| -> Result<(), LedgerError> {
            ensure_account(conn, "A")?;
            ensure_fund(conn, "000002", "X")?;
            Ok(())
        })
        .unwrap();
    buy(&store, "A", "000002", "10", "1.00", "0", "2026-01-05", "b1");
    let err = sell(&store, "A", "000002", "11", "1.00", "0", "2026-01-06", "s1").unwrap_err();
    assert!(matches!(
        err,
        LedgerError::Oversell {
            available_raw: 100_000
        }
    ));
    // 数据未被污染：持仓仍是 10。
    let position = store
        .with_read(|c| crate::portfolio::replay_position(c, 1, 1))
        .unwrap()
        .unwrap();
    assert_eq!(position.quantity_raw, 100_000);
}

#[test]
fn same_request_id_same_payload_is_idempotent() {
    let store = Store::open_in_memory().unwrap();
    store
        .with_write(|conn| -> Result<(), LedgerError> {
            ensure_account(conn, "A")?;
            ensure_fund(conn, "000003", "X")?;
            Ok(())
        })
        .unwrap();
    let (first, created) = buy(
        &store,
        "A",
        "000003",
        "10",
        "1.00",
        "0",
        "2026-01-05",
        "same",
    );
    assert!(created);
    let (second, created_again) = buy(
        &store,
        "A",
        "000003",
        "10",
        "1.00",
        "0",
        "2026-01-05",
        "same",
    );
    assert!(!created_again);
    assert_eq!(first, second);
    // 重复提交不倍增持仓。
    let position = store
        .with_read(|c| crate::portfolio::replay_position(c, 1, 1))
        .unwrap()
        .unwrap();
    assert_eq!(position.quantity_raw, 100_000);
}

#[test]
fn same_request_id_different_payload_conflicts() {
    let store = Store::open_in_memory().unwrap();
    store
        .with_write(|conn| -> Result<(), LedgerError> {
            ensure_account(conn, "A")?;
            ensure_fund(conn, "000004", "X")?;
            Ok(())
        })
        .unwrap();
    buy(
        &store,
        "A",
        "000004",
        "10",
        "1.00",
        "0",
        "2026-01-05",
        "dup",
    );
    // buy 助手内部 unwrap，直接用 record_transaction 验证冲突路径。
    let err = record_transaction(
        &store,
        &TransactionInput::manual(
            "A".into(),
            "000004".into(),
            "X".into(),
            TYPE_BUY.into(),
            STATE_CONFIRMED.into(),
            Fixed::parse("20", SCALE_SHARE).unwrap(),
            Fixed::parse("1.00", SCALE_SHARE).unwrap(),
            Fixed::zero(2),
            "2026-01-05".into(),
            "dup".into(),
        ),
    )
    .map(|_| ())
    .unwrap_err();
    assert!(matches!(err, LedgerError::Conflict(_)));
}

#[test]
fn same_day_multiple_buys_allowed() {
    let store = Store::open_in_memory().unwrap();
    store
        .with_write(|conn| -> Result<(), LedgerError> {
            ensure_account(conn, "A")?;
            ensure_fund(conn, "000005", "X")?;
            Ok(())
        })
        .unwrap();
    buy(&store, "A", "000005", "10", "1.00", "0", "2026-01-05", "d1");
    buy(&store, "A", "000005", "10", "1.00", "0", "2026-01-05", "d2");
    let position = store
        .with_read(|c| crate::portfolio::replay_position(c, 1, 1))
        .unwrap()
        .unwrap();
    assert_eq!(position.quantity_raw, 200_000);
}

#[test]
fn pending_transaction_does_not_change_position() {
    let store = Store::open_in_memory().unwrap();
    store
        .with_write(|conn| -> Result<(), LedgerError> {
            ensure_account(conn, "A")?;
            ensure_fund(conn, "000006", "X")?;
            Ok(())
        })
        .unwrap();
    record_transaction(
        &store,
        &TransactionInput::manual(
            "A".into(),
            "000006".into(),
            "X".into(),
            TYPE_BUY.into(),
            STATE_PENDING.into(),
            Fixed::parse("10", SCALE_SHARE).unwrap(),
            Fixed::parse("1.00", SCALE_SHARE).unwrap(),
            Fixed::zero(2),
            "2026-01-05".into(),
            "p1".into(),
        ),
    )
    .unwrap();
    let position = store
        .with_read(|c| crate::portfolio::replay_position(c, 1, 1))
        .unwrap();
    assert!(position.is_none(), "待确认交易不得改变持仓");
}

#[test]
fn invalid_input_rejected() {
    let store = Store::open_in_memory().unwrap();
    // 负费用在写入校验时拒绝（精度超限由 Fixed::parse 在调用方拒绝，
    // 已在 fixed 测试覆盖；这里覆盖 ledger 自身校验）。
    let err = record_transaction(
        &store,
        &TransactionInput::manual(
            "A".into(),
            "000007".into(),
            "X".into(),
            TYPE_BUY.into(),
            STATE_CONFIRMED.into(),
            Fixed::parse("10", SCALE_SHARE).unwrap(),
            Fixed::parse("1.00", SCALE_SHARE).unwrap(),
            Fixed::parse("-1.00", 2).unwrap(),
            "2026-01-05".into(),
            "p2".into(),
        ),
    )
    .map(|_| ())
    .unwrap_err();
    assert!(matches!(err, LedgerError::Invalid(_)));
}

#[test]
fn timeline_oversell_rejected_at_write_time() {
    let store = Store::open_in_memory().unwrap();
    store
        .with_write(|conn| -> Result<(), LedgerError> {
            ensure_account(conn, "A")?;
            ensure_fund(conn, "000008", "X")?;
            Ok(())
        })
        .unwrap();
    // 1. 2026-01-05 买入 10 份
    buy(&store, "A", "000008", "10", "1.00", "0", "2026-01-05", "b1");
    // 2. 2026-01-10 卖出 10 份（此时剩余 0 份）
    sell(&store, "A", "000008", "10", "1.00", "0", "2026-01-10", "s1").unwrap();

    // 3. 倒签在 2026-01-08 卖出 5 份：如果允许插入，会导致 2026-01-10 那笔卖出超卖（时序回放出现负份额）
    let err = sell(&store, "A", "000008", "5", "1.00", "0", "2026-01-08", "s2").unwrap_err();
    assert!(matches!(err, LedgerError::Oversell { .. }));

    // 验证原子回滚，流水库中没有这笔 s2，持仓保持正确
    let tx_count: i64 = store
        .with_read(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM transactions WHERE request_id = 's2'",
                [],
                |r| r.get(0),
            )
        })
        .unwrap();
    assert_eq!(tx_count, 0, "时序超卖事务必须完整回滚");
}

// ---------------------------------------------------------------------------
// 期初登记（方案 01 §5.1）与 pending 确认（方案 03 §2）。
// ---------------------------------------------------------------------------

fn opening(
    store: &Store,
    account: &str,
    code: &str,
    qty: &str,
    as_of: &str,
    total_cost: Option<&str>,
) -> Result<i64, LedgerError> {
    super::record_opening(
        store,
        &super::OpeningInput {
            account_name: account.into(),
            fund_code: code.into(),
            fund_name: "期初基金".into(),
            quantity: Fixed::parse(qty, SCALE_SHARE).unwrap(),
            as_of: as_of.into(),
            total_cost: total_cost.map(|c| Fixed::parse(c, 2).unwrap()),
        },
        "2026-10-02",
    )
}

#[test]
fn opening_registration_replays_into_position() {
    let store = Store::open_in_memory().unwrap();
    opening(
        &store,
        "账户A",
        "000001",
        "1000",
        "2026-09-30",
        Some("10000.00"),
    )
    .unwrap();
    // 期初直接形成持仓。
    let pos = store
        .with_read(|conn| crate::portfolio::replay_position(conn, 1, 1))
        .unwrap()
        .unwrap();
    assert_eq!(pos.quantity_raw, 10_000_000); // 1000 份 × 10^4。
    assert_eq!(pos.cost_raw, 1_000_000); // 10000 元 × 10^2。
    assert!(!pos.cost_unknown);
    // 期初后可正常交易并回放。
    buy(
        &store,
        "账户A",
        "000001",
        "100",
        "2.00",
        "0.00",
        "2026-10-01",
        "r1",
    );
    let pos = store
        .with_read(|conn| crate::portfolio::replay_position(conn, 1, 1))
        .unwrap()
        .unwrap();
    assert_eq!(pos.quantity_raw, 11_000_000);
}

#[test]
fn opening_duplicate_and_post_tx_rejected() {
    let store = Store::open_in_memory().unwrap();
    opening(&store, "账户A", "000001", "1000", "2026-09-30", None).unwrap();
    // 账户+标的不能重复登记期初。
    assert!(matches!(
        opening(&store, "账户A", "000001", "500", "2026-09-30", None),
        Err(LedgerError::Conflict(_))
    ));
    // 已有流水后不能补登期初。
    let store2 = Store::open_in_memory().unwrap();
    buy(
        &store2,
        "账户B",
        "000002",
        "100",
        "1.00",
        "0.00",
        "2026-10-01",
        "r1",
    );
    assert!(matches!(
        opening(&store2, "账户B", "000002", "500", "2026-09-30", None),
        Err(LedgerError::Conflict(_))
    ));
}

#[test]
fn opening_validation_rejects_bad_input() {
    let store = Store::open_in_memory().unwrap();
    // 零/负数量。
    assert!(matches!(
        opening(&store, "账户A", "000001", "0", "2026-09-30", None),
        Err(LedgerError::Invalid(_))
    ));
    // 非法日历日。
    assert!(matches!(
        opening(&store, "账户A", "000001", "10", "2026-02-31", None),
        Err(LedgerError::Invalid(_))
    ));
    // 未来日期。
    assert!(matches!(
        opening(&store, "账户A", "000001", "10", "2026-10-03", None),
        Err(LedgerError::Invalid(_))
    ));
    // 未知成本显式进入 cost_unknown，不当作 0。
    opening(&store, "账户A", "000001", "10", "2026-09-30", None).unwrap();
    let pos = store
        .with_read(|conn| crate::portfolio::replay_position(conn, 1, 1))
        .unwrap()
        .unwrap();
    assert!(pos.cost_unknown, "缺省总成本必须是未知成本状态");
}

#[test]
fn pending_confirm_is_one_shot_and_idempotent() {
    let store = Store::open_in_memory().unwrap();
    // pending 交易不参与回放。
    let (tid, _) = record_transaction(
        &store,
        &TransactionInput::manual(
            "账户C".into(),
            "000003".into(),
            "测试基金".into(),
            TYPE_BUY.into(),
            STATE_PENDING.into(),
            Fixed::parse("100", SCALE_SHARE).unwrap(),
            Fixed::parse("1.00", SCALE_SHARE).unwrap(),
            Fixed::zero(2),
            "2026-10-01".into(),
            "p1".into(),
        ),
    )
    .unwrap();
    let none = store
        .with_read(|conn| crate::portfolio::replay_position(conn, 1, 1))
        .unwrap();
    assert!(none.is_none(), "pending 交易不产生持仓");
    // 确认生效一次。
    let created = super::confirm_transaction(&store, tid).unwrap();
    assert!(created);
    let pos = store
        .with_read(|conn| crate::portfolio::replay_position(conn, 1, 1))
        .unwrap()
        .unwrap();
    assert_eq!(pos.quantity_raw, 1_000_000); // 100 份 × 10^4。
                                             // 重复确认幂等。
    assert!(!super::confirm_transaction(&store, tid).unwrap());
    // 不存在交易 404。
    assert!(matches!(
        super::confirm_transaction(&store, 999),
        Err(LedgerError::NotFound(_))
    ));
    // 确认后删除 → 再确认拒绝（软删除保护）。
    crate::corrections::delete_transaction(&store, tid, "误录", "2026-10-02").unwrap();
    assert!(matches!(
        super::confirm_transaction(&store, tid),
        Err(LedgerError::Invalid(_))
    ));
}
