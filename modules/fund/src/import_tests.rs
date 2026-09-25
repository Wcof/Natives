use super::*;

const TX_CSV: &str = "account,fund_code,trade_date,type,quantity,price,fee,external_id\n\
    默认账户,000001,2026-01-05,BUY,100,1.00,1.00,ext-1\n\
    默认账户,000001,2026-01-06,BUY,50,1.20,0.50,ext-2\n\
    默认账户,000001,2026-01-07,SELL,60,1.30,0.50,ext-3\n";

const SNAP_CSV: &str = "account,fund_code,as_of,quantity,market_value,earnings\n\
    券商,000009,2026-01-01,200,215.00,15.00\n\
    券商,000010,2026-01-01,100\n";

#[test]
fn preview_tx_csv_valid() {
    let p = preview(TEMPLATE_TX, TX_CSV).unwrap();
    assert_eq!(p.total_rows, 3);
    assert_eq!(p.error_rows, 0);
    assert_eq!(p.lines[0].amount_raw, Some(100_00));
    assert_eq!(p.lines[2].kind, "SELL");
}

#[test]
fn preview_rejects_bad_rows() {
    let csv = "account,fund_code,trade_date,type,quantity,price\n\
        A,000001,2026-01-05,BUY,10,1.00\n\
        ,000001,2026-01-05,BUY,10,1.00\n\
        A,abc,2026-01-05,BUY,10,1.00\n\
        A,000001,2026/01/05,BUY,10,1.00\n";
    let p = preview(TEMPLATE_TX, csv).unwrap();
    assert_eq!(p.total_rows, 4);
    assert_eq!(p.valid_rows, 1);
    assert_eq!(p.error_rows, 3);
}

#[test]
fn preview_snapshot_derived_and_unknown_cost() {
    let p = preview(TEMPLATE_SNAPSHOT, SNAP_CSV).unwrap();
    assert_eq!(p.valid_rows, 2);
    // 000009：市值 215 − 收益 15 = 200 推算成本。
    assert_eq!(p.lines[0].amount_raw, Some(200_00));
    assert_eq!(p.lines[0].cost_state, "derived");
    // 000010：无成本也无市值 → 未知成本。
    assert_eq!(p.lines[1].cost_state, "unknown");
    assert!(p.lines[1].amount_raw.is_none());
}

#[test]
fn commit_tx_idempotent_and_replay() {
    let store = Store::open_in_memory().unwrap();
    let p = preview(TEMPLATE_TX, TX_CSV).unwrap();
    let first = commit(&store, "yjb", TEMPLATE_TX, "hash1", "params1", &p).unwrap();
    assert_eq!(first.rows_imported, 3);
    assert!(!first.replayed);
    // 持仓正确（黄金用例数值）。
    let position = store
        .with_read(|c| crate::portfolio::replay_position(c, 1, 1))
        .unwrap()
        .unwrap();
    assert_eq!(position.quantity_raw, 900_000);
    assert_eq!(position.cost_raw, 96_90);
    // 精确重复批次：返回原结果不再次入账。
    let second = commit(&store, "yjb", TEMPLATE_TX, "hash1", "params1", &p).unwrap();
    assert!(second.replayed);
    assert_eq!(second.rows_imported, 3);
    // 重复导入不倍增持仓。
    let position = store
        .with_read(|c| crate::portfolio::replay_position(c, 1, 1))
        .unwrap()
        .unwrap();
    assert_eq!(position.quantity_raw, 900_000);
}

#[test]
fn commit_external_id_dedup_across_batches() {
    let store = Store::open_in_memory().unwrap();
    let p = preview(TEMPLATE_TX, TX_CSV).unwrap();
    commit(&store, "yjb", TEMPLATE_TX, "hash1", "params1", &p).unwrap();
    // 另一批次，但 external_id 相同 → skip 不倍增。
    let first = commit(&store, "other", TEMPLATE_TX, "hash2", "params2", &p).unwrap();
    assert_eq!(first.rows_skipped, 3);
    assert_eq!(first.rows_imported, 0);
    let position = store
        .with_read(|c| crate::portfolio::replay_position(c, 1, 1))
        .unwrap()
        .unwrap();
    assert_eq!(position.quantity_raw, 900_000);
}

#[test]
fn commit_snapshot_then_tx_conflict() {
    let store = Store::open_in_memory().unwrap();
    let sp = preview(TEMPLATE_SNAPSHOT, SNAP_CSV).unwrap();
    let receipt = commit(&store, "snap", TEMPLATE_SNAPSHOT, "h", "p", &sp).unwrap();
    assert_eq!(receipt.rows_imported, 2);
    // 快照后持仓存在且成本未知的一只显示 unknown。
    let positions = store
        .with_read(|c| crate::portfolio::list_positions(c))
        .unwrap();
    assert_eq!(positions.len(), 2);
    // 再用交易模板导入同账户/基金：快照不阻止交易（快照建账后可继续记账）。
    let tx_csv = "account,fund_code,trade_date,type,quantity,price\n券商,000009,2026-02-01,BUY,10,1.00\n";
    let tp = preview(TEMPLATE_TX, tx_csv).unwrap();
    let receipt = commit(&store, "t2", TEMPLATE_TX, "h2", "p2", &tp).unwrap();
    assert_eq!(receipt.rows_imported, 1);
}

#[test]
fn csv_quotes_and_bom() {
    let csv = "\u{feff}account,fund_code,trade_date,type,quantity,price\n\
        \"我的,账户\",000001,2026-01-05,BUY,10,\"1.00\"\n";
    let p = preview(TEMPLATE_TX, csv).unwrap();
    assert_eq!(p.error_rows, 0);
    assert_eq!(p.lines[0].account, "我的,账户");
}

#[test]
fn commit_tx_rolls_back_whole_batch_on_oversell() {
    let store = Store::open_in_memory().unwrap();
    // 第 1 行：买入 10 份
    // 第 2 行：卖出 20 份（超卖错误！）
    let csv = "account,fund_code,trade_date,type,quantity,price,fee\n\
        A,000001,2026-01-05,BUY,10,1.00,0\n\
        A,000001,2026-01-06,SELL,20,1.00,0\n";
    let p = preview(TEMPLATE_TX, csv).unwrap();
    assert_eq!(p.valid_rows, 2);
    let err = commit(&store, "batch-err", TEMPLATE_TX, "h_err", "p_err", &p).unwrap_err();
    assert!(matches!(err, ImportError::Invalid(_)));

    // 验证整批回滚：第 1 行买入也不得遗留入库，持仓为空
    let position = store
        .with_read(|c| crate::portfolio::replay_position(c, 1, 1))
        .unwrap();
    assert!(position.is_none(), "整批导入遇任何行错误必须原子回滚");

    let tx_count: i64 = store
        .with_read(|c| c.query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0)))
        .unwrap();
    assert_eq!(tx_count, 0, "事务回滚后 transactions 表必须为 0 条");
}
