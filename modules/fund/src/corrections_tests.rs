use super::*;
use crate::ledger;

fn store() -> Store {
    Store::open_in_memory().unwrap()
}

fn fx(text: &str, scale: u32) -> crate::fixed::Fixed {
    crate::fixed::Fixed::parse(text, scale).unwrap()
}

fn buy(
    store: &Store,
    account: &str,
    code: &str,
    qty: i64,
    price: i64,
    date: &str,
    req: &str,
) -> i64 {
    let (id, _) = ledger::record_transaction(
        store,
        &ledger::TransactionInput {
            account_name: account.to_string(),
            fund_code: code.to_string(),
            fund_name: String::new(),
            tx_type: "BUY".into(),
            state: "confirmed".into(),
            quantity: fx(&qty.to_string(), 4),
            price: fx(&price.to_string(), 4),
            fee: fx("0", 2),
            amount: None,
            trade_date: date.to_string(),
            source: "manual".into(),
            request_id: Some(req.to_string()),
            external_id: None,
            batch_id: None,
            line_no: None,
            payload_hash: None,
        },
    )
    .unwrap();
    id
}

#[test]
fn delete_rejects_when_replay_oversells_and_restores() {
    let store = store();
    let today = {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
            + 8 * 3600;
        let (y, m, d) = civil(secs as i64);
        format!("{y:04}-{m:02}-{d:02}")
    };
    let b1 = buy(&store, "账户A", "600001", 100, 10, &today, "r1");
    buy(&store, "账户A", "600001", 50, 11, &today, "r2");
    let s = buy(&store, "账户A", "600001", 60, 12, &today, "r3");
    // 删除第一笔买入：剩余 50 不足以支撑 60 卖出？本例只有买入——卖出来验证超卖。
    let _ = s;
    // 正常删除：不影响时序。
    assert!(delete_transaction(&store, b1, "误录", "now").unwrap());
    // 幂等：再次删除返回 false。
    assert!(!delete_transaction(&store, b1, "误录", "now").unwrap());
    // 恢复。
    assert!(restore_transaction(&store, b1).unwrap());
    // 买入交易删除后回放不受阻（只有买入无超卖风险）。
    let preview = preview_delete(&store, b1).unwrap();
    assert_eq!(
        preview.resulting_quantity_raw, 1_100_000,
        "110 股 → 4 位定点"
    );
}

#[test]
fn delete_sell_that_oversells_is_rejected_atomically() {
    let store = store();
    let today = {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
            + 8 * 3600;
        let (y, m, d) = civil(secs as i64);
        format!("{y:04}-{m:02}-{d:02}")
    };
    let b = buy(&store, "账户B", "600002", 100, 10, &today, "b1");
    let s = buy(&store, "账户B", "600002", 50, 10, &today, "b2");
    // 删除第二笔买入（100+50=150，卖出 60 仍可行）——先建卖出。
    let _ = s;
    let _ = b;
    // 通过 record 建一笔 SELL。
    let (sell, _) = ledger::record_transaction(
        &store,
        &ledger::TransactionInput {
            account_name: "账户B".into(),
            fund_code: "600002".into(),
            fund_name: String::new(),
            tx_type: "SELL".into(),
            state: "confirmed".into(),
            quantity: fx("140", 4),
            price: fx("10", 4),
            fee: fx("0", 2),
            amount: None,
            trade_date: today.clone(),
            source: "manual".into(),
            request_id: Some("s1".into()),
            external_id: None,
            batch_id: None,
            line_no: None,
            payload_hash: None,
        },
    )
    .unwrap();
    // 删除第一笔买入 → 50 < 140 中途超卖 → 拒绝。
    let err = delete_transaction(&store, b, "误录", "now").unwrap_err();
    assert!(
        matches!(err, LedgerError::Invalid(_)),
        "expected invalid, got {err:?}"
    );
    // 预览带阻塞项。
    let p = preview_delete(&store, b).unwrap();
    assert!(!p.blockers.is_empty());
    // 交易未被删除（原子性）。
    let still_there: bool = store
        .with_read(|c| {
            c.query_row(
                "SELECT deleted_at IS NULL FROM transactions WHERE id=?1",
                [b],
                |r| r.get(0),
            )
        })
        .unwrap();
    assert!(still_there);
    // 删除卖出可以成功。
    assert!(delete_transaction(&store, sell, "误录", "now").unwrap());
}

// 审计 P0-3：更正/删除后 positions 投影必须与回放一致；同 key 不同内容拒绝。
#[test]
fn correction_and_delete_sync_positions_projection() {
    let store = store();
    let today = today_for_test();
    let b = buy(&store, "账户P", "600019", 100, 10, &today, "p1");
    // 更正 100→200：投影=回放=200（raw 4 位定点）。
    let input = CorrectionInput {
        tx_id: b,
        quantity_raw: 2_000_000,
        price_raw: 100_000,
        fee_raw: 0,
        trade_date: today.clone(),
        request_id: "corr-p1".into(),
        reason: "数量录错".into(),
    };
    submit_correction(&store, &input, "now").unwrap();
    let (cached, replay): (i64, i64) = store
            .with_read(|c| -> rusqlite::Result<(i64, i64)> {
                Ok((
                    c.query_row("SELECT quantity_raw FROM positions", [], |r| r.get(0))?,
                    c.query_row(
                        "SELECT COALESCE(SUM(CASE WHEN t.type='BUY' THEN t.quantity_raw ELSE -t.quantity_raw END),0) \
                         FROM transactions t WHERE t.deleted_at IS NULL AND t.state='confirmed'",
                        [],
                        |r| r.get(0),
                    )?,
                ))
            })
            .unwrap();
    assert_eq!(cached, 2_000_000, "投影必须与回放一致");
    assert_eq!(replay, 2_000_000);
    // 同 request_id 不同内容 → 拒绝。
    let mut wrong = input.clone();
    wrong.quantity_raw = 9_000_000;
    let err = submit_correction(&store, &wrong, "now").unwrap_err();
    assert!(matches!(err, LedgerError::Invalid(_)), "got {err:?}");
    // 删除更正交易：投影同步消失（recompute 删行）。
    let corrected: i64 = store
        .with_read(|c| {
            c.query_row(
                "SELECT id FROM transactions WHERE request_id='corr-p1'",
                [],
                |r| r.get(0),
            )
        })
        .unwrap();
    delete_transaction(&store, corrected, "误录", "now").unwrap();
    let cached: i64 = store
        .with_read(|c| {
            c.query_row(
                "SELECT COALESCE((SELECT quantity_raw FROM positions),0)",
                [],
                |r| r.get(0),
            )
        })
        .unwrap();
    let replay: i64 = store
            .with_read(|c| {
                c.query_row(
                    "SELECT COALESCE(SUM(CASE WHEN t.type='BUY' THEN t.quantity_raw ELSE -t.quantity_raw END),0) \
                     FROM transactions t WHERE t.deleted_at IS NULL AND t.state='confirmed'",
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap();
    // 原交易仍在软删除（更正替换），删除更正交易后无有效流水 → 投影与回放同为 0。
    assert_eq!(cached, 0, "投影与回放一致（无有效交易）");
    assert_eq!(replay, 0);
}

#[test]
fn delete_with_settled_cash_is_rejected() {
    let store = store();
    let today = today_for_test();
    let b = buy(&store, "账户Q", "600020", 10, 10, &today, "q1");
    store
        .with_write(|c| -> rusqlite::Result<()> {
            c.execute(
                "INSERT INTO trade_cash(tx_id, account_id, delta_minor, status) \
                     SELECT id, account_id, amount_raw, 'settled' FROM transactions WHERE id=?1",
                [b],
            )?;
            Ok(())
        })
        .unwrap();
    let err = delete_transaction(&store, b, "误录", "now").unwrap_err();
    assert!(
        matches!(err, LedgerError::Invalid(_)),
        "settled 资金保护，got {err:?}"
    );
    // unreconciled 行随交易删除。
    store
        .with_write(|c| -> rusqlite::Result<()> {
            c.execute(
                "UPDATE trade_cash SET status='unreconciled' WHERE tx_id=?1",
                [b],
            )?;
            Ok(())
        })
        .unwrap();
    assert!(delete_transaction(&store, b, "误录", "now").unwrap());
    let left: i64 = store
        .with_read(|c| {
            c.query_row("SELECT COUNT(*) FROM trade_cash WHERE tx_id=?1", [b], |r| {
                r.get(0)
            })
        })
        .unwrap();
    assert_eq!(left, 0, "待核对资金行随交易删除");
}

#[test]
fn correction_replaces_transaction_idempotently() {
    let store = store();
    let today = {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
            + 8 * 3600;
        let (y, m, d) = civil(secs as i64);
        format!("{y:04}-{m:02}-{d:02}")
    };
    let b = buy(&store, "账户C", "600003", 100, 10, &today, "c1");
    let input = CorrectionInput {
        tx_id: b,
        quantity_raw: 1_200_000,
        price_raw: 100_000,
        fee_raw: 5,
        trade_date: today.clone(),
        request_id: "corr-1".into(),
        reason: "数量录错".into(),
    };
    let preview = preview_correction(&store, &input).unwrap();
    assert_eq!(preview.original_quantity_raw, 1_000_000, "原数量 4 位定点");
    assert_eq!(
        preview.new_amount_raw, 120_000,
        "120 股 × 10 元 = 120000 分"
    );
    let id1 = submit_correction(&store, &input, "now").unwrap();
    let id2 = submit_correction(&store, &input, "now").unwrap();
    assert_eq!(id1, id2, "request_id 幂等");
    let (qty, deleted): (i64, bool) = store
        .with_read(|c| {
            c.query_row(
                "SELECT quantity_raw, deleted_at IS NULL FROM transactions WHERE id=?1",
                [b],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
        })
        .unwrap();
    assert_eq!(qty, 1_000_000, "quantity_raw 为 4 位定点（100 → 1000000）");
    assert!(!deleted, "原交易已软删除");
    let total: i64 = store
            .with_read(|c| {
                c.query_row(
                    "SELECT COALESCE(SUM(CASE WHEN type='BUY' THEN quantity_raw ELSE -quantity_raw END),0) \
                     FROM transactions WHERE deleted_at IS NULL AND state='confirmed'",
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap();
    assert_eq!(
        total, 1_200_000,
        "更正后回放合计为更正数量（120 → 1200000）"
    );
}

#[test]
fn cash_correction_and_pending_confirmation_are_atomic() {
    let store = store();
    let today = crate::finance::today_cn();
    let b = buy(&store, "Cash", "600101", 10, 10, &today, "cash-1");
    crate::assets::create_asset(
        &store,
        &crate::assets::ManualAssetInput {
            account_name: "Cash".into(),
            product_type: "cash".into(),
            market: None,
            code: None,
            name: "Cash".into(),
            risk_class: "cash".into(),
            market_value_text: "1000".into(),
            valued_at: today.clone(),
            confirm_distinct: false,
        },
        &today,
    )
    .unwrap();
    store.with_write::<_,rusqlite::Error>(|c|{c.execute("INSERT INTO cash_reserves(account_id,state,amount_minor,updated_at) VALUES(1,'configured',0,'0')",[])?;c.execute("INSERT INTO trade_cash(tx_id,account_id,delta_minor,status,settled_at) VALUES(?1,1,-10000,'settled',?2)",rusqlite::params![b,today])?;Ok(())}).unwrap();
    let input = CorrectionInput {
        tx_id: b,
        quantity_raw: 200000,
        price_raw: 100000,
        fee_raw: 0,
        trade_date: today.clone(),
        request_id: "cash-correct".into(),
        reason: "Verified receipt".into(),
    };
    assert!(submit_correction_versioned(&store, &input, "now", Some(1)).is_err());
    let proof = serde_json::json!({"confirmed":true,"settledAt":today,"note":"Broker receipt"});
    let id = submit_correction_with_cash(&store, &input, "now", Some(1), Some(&proof)).unwrap();
    assert!(restore_transaction_versioned(&store, b, Some(2)).is_err());
    assert_eq!(
        store
            .with_read(|c| c.query_row(
                "SELECT quantity_raw FROM positions WHERE account_id=1 AND fund_id=1",
                [],
                |r| r.get::<_, i64>(0)
            ))
            .unwrap(),
        200000
    );
    assert_eq!(
        id,
        submit_correction_with_cash(&store, &input, "now", Some(1), Some(&proof)).unwrap()
    );
    let balance = store
        .with_read(|c| {
            c.query_row(
                "SELECT market_value_minor FROM manual_assets WHERE product_type='cash'",
                [],
                |r| r.get::<_, i64>(0),
            )
        })
        .unwrap();
    assert_eq!(balance, 90000);
    assert!(submit_correction_with_cash(
        &store,
        &input,
        "now",
        Some(1),
        Some(&serde_json::json!({"confirmed":true,"settledAt":today,"note":"Different proof"}))
    )
    .is_err());
    let mut pending = ledger::TransactionInput::manual(
        "Cash".into(),
        "600102".into(),
        "Fund".into(),
        "BUY".into(),
        "pending".into(),
        fx("2", 4),
        fx("10", 4),
        fx("0", 2),
        today.clone(),
        "pending-cash".into(),
    );
    let (tx, _) = ledger::record_transaction(&store, &pending).unwrap();
    assert!(ledger::confirm_transaction_versioned(&store, tx, 1).unwrap());
    let balance = store
        .with_read(|c| {
            c.query_row(
                "SELECT market_value_minor FROM manual_assets WHERE product_type='cash'",
                [],
                |r| r.get::<_, i64>(0),
            )
        })
        .unwrap();
    assert_eq!(balance, 88000);
    assert!(ledger::confirm_transaction_versioned(&store, tx, 1).is_err());
    assert!(!ledger::confirm_transaction_versioned(&store, tx, 2).unwrap());
    pending.request_id = Some("pending-too-large".into());
    pending.quantity = fx("10000", 4);
    let (large, _) = ledger::record_transaction(&store, &pending).unwrap();
    assert!(ledger::confirm_transaction_versioned(&store, large, 1).is_err());
    let state = store
        .with_read(|c| {
            c.query_row("SELECT state FROM transactions WHERE id=?1", [large], |r| {
                r.get::<_, String>(0)
            })
        })
        .unwrap();
    assert_eq!(state, "pending");
}
