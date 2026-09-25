//! portfolio 测试（拆分自 portfolio.rs，仅移动，无逻辑变更）。

#![cfg(test)]

use super::{
    list_all_positions, list_positions, recompute_position, replay_history, replay_position,
    PortfolioError,
};
use crate::fixed::{Fixed, SCALE_SHARE};
use crate::ledger::{
    ensure_account, ensure_fund, record_transaction, TransactionInput, STATE_CONFIRMED, TYPE_BUY,
    TYPE_SELL,
};
use crate::storage::Store;

    fn tx(
        store: &Store,
        code: &str,
        t: &str,
        qty: &str,
        price: &str,
        fee: &str,
        date: &str,
        rid: &str,
    ) {
        record_transaction(
            store,
            &TransactionInput::manual(
                "A".into(),
                code.into(),
                "X".into(),
                t.into(),
                STATE_CONFIRMED.into(),
                Fixed::parse(qty, SCALE_SHARE).unwrap(),
                Fixed::parse(price, SCALE_SHARE).unwrap(),
                Fixed::parse(fee, 2).unwrap(),
                date.into(),
                rid.into(),
            ),
        )
        .unwrap();
    }

    fn setup(store: &Store, code: &str) {
        store
            .with_write(|conn| -> Result<(), crate::ledger::LedgerError> {
                ensure_account(conn, "A")?;
                ensure_fund(conn, code, "X")?;
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn clearing_position_preserves_realized_and_marks_zero_share() {
        let store = Store::open_in_memory().unwrap();
        setup(&store, "000010");
        tx(
            &store,
            "000010",
            TYPE_BUY,
            "10",
            "1.00",
            "0",
            "2026-01-05",
            "b1",
        );
        tx(
            &store,
            "000010",
            TYPE_SELL,
            "10",
            "1.10",
            "0",
            "2026-01-06",
            "s1",
        );
        let active = store
            .with_write(|conn| recompute_position(conn, 1, 1))
            .unwrap();
        assert!(!active, "清仓后 active 应为 false");
        let row: (i64, i64, i64) = store
            .with_read(|conn| {
                conn.query_row(
                    "SELECT quantity_raw, cost_raw, realized_raw FROM positions WHERE account_id = 1 AND fund_id = 1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
            })
            .unwrap();
        // 清仓后 positions 表保留 0 份额，且累计已实现收益 1.00 不丢失
        assert_eq!(row.0, 0);
        assert_eq!(row.1, 0);
        assert_eq!(row.2, 1_00);

        // list_positions 只返回在持份额 > 0 的持仓
        let active_rows = store.with_read(|conn| list_positions(conn)).unwrap();
        assert_eq!(active_rows.len(), 0);

        // list_all_positions 包含已清仓记录
        let all_rows = store.with_read(|conn| list_all_positions(conn)).unwrap();
        assert_eq!(all_rows.len(), 1);
        assert_eq!(all_rows[0].quantity_raw, 0);
        assert_eq!(all_rows[0].realized_raw, 1_00);

        // 回放同样保留 0 份额与收益
        let replayed = store
            .with_read(|conn| replay_position(conn, 1, 1))
            .unwrap()
            .unwrap();
        assert_eq!(replayed.quantity_raw, 0);
        assert_eq!(replayed.cost_raw, 0);
        assert_eq!(replayed.realized_raw, 1_00);
        assert_eq!(replayed.cost_unknown, false);
    }

    #[test]
    fn cost_unknown_preserved_on_subsequent_buys_until_cleared() {
        let store = Store::open_in_memory().unwrap();
        setup(&store, "000099");
        // 插入期初未知成本快照（cost_raw 为 NULL）
        store
            .with_write(|conn| -> rusqlite::Result<()> {
                conn.execute(
                    "INSERT INTO opening_snapshots(account_id, fund_id, as_of, quantity_raw, cost_raw, derived_cost, created_at)
                     VALUES(1, 1, '2026-01-01', 1000000, NULL, 0, '2026-01-01')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        let pos1 = store
            .with_read(|conn| replay_position(conn, 1, 1))
            .unwrap()
            .unwrap();
        assert_eq!(pos1.quantity_raw, 1000000);
        assert!(pos1.cost_unknown, "期初未知成本");

        // 后续买入 50 份
        tx(
            &store,
            "000099",
            TYPE_BUY,
            "50",
            "1.50",
            "0",
            "2026-01-05",
            "b1",
        );

        let pos2 = store
            .with_read(|conn| replay_position(conn, 1, 1))
            .unwrap()
            .unwrap();
        assert_eq!(pos2.quantity_raw, 1500000);
        assert!(
            pos2.cost_unknown,
            "既有未知成本份额未清仓前，整体 cost_unknown 仍须为 true"
        );

        // 全部卖出 150 份（清仓）
        tx(
            &store,
            "000099",
            TYPE_SELL,
            "150",
            "2.00",
            "0",
            "2026-01-06",
            "s1",
        );

        let pos3 = store
            .with_read(|conn| replay_position(conn, 1, 1))
            .unwrap()
            .unwrap();
        assert_eq!(pos3.quantity_raw, 0);
        assert_eq!(pos3.cost_unknown, false);

        // 清仓后再次买入 20 份
        tx(
            &store,
            "000099",
            TYPE_BUY,
            "20",
            "2.10",
            "0",
            "2026-01-07",
            "b2",
        );

        let pos4 = store
            .with_read(|conn| replay_position(conn, 1, 1))
            .unwrap()
            .unwrap();
        assert_eq!(pos4.quantity_raw, 200000);
        assert_eq!(
            pos4.cost_unknown, false,
            "此前未知成本份额已清空，新买入成本已知"
        );
    }

    #[test]
    fn replay_rejects_oversell_without_truncation() {
        let store = Store::open_in_memory().unwrap();
        setup(&store, "000088");
        tx(
            &store,
            "000088",
            TYPE_BUY,
            "10",
            "1.00",
            "0",
            "2026-01-05",
            "b1",
        );
        // 直接绕过 ledger 手工插一条超卖流水测试回放层防御
        store
            .with_write(|conn| -> rusqlite::Result<()> {
                conn.execute(
                    "INSERT INTO transactions(account_id, fund_id, type, state, quantity_raw, price_raw, amount_raw, fee_raw, trade_date, source, created_at)
                     VALUES(1, 1, 'SELL', 'confirmed', 150000, 10000, 1500, 0, '2026-01-06', 'test', 'x')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        let err = store
            .with_read(|conn| replay_position(conn, 1, 1))
            .unwrap_err();
        assert!(matches!(
            err,
            PortfolioError::Oversell {
                available_raw: 100000,
                requested_raw: 150000,
                ..
            }
        ));
    }

    #[test]
    fn projection_matches_replay_after_series() {
        let store = Store::open_in_memory().unwrap();
        setup(&store, "000011");
        tx(
            &store,
            "000011",
            TYPE_BUY,
            "100",
            "1.00",
            "1.00",
            "2026-01-05",
            "b1",
        );
        tx(
            &store,
            "000011",
            TYPE_BUY,
            "50",
            "1.20",
            "0.50",
            "2026-01-06",
            "b2",
        );
        tx(
            &store,
            "000011",
            TYPE_SELL,
            "60",
            "1.30",
            "0.50",
            "2026-01-07",
            "s1",
        );
        store
            .with_write(|conn| recompute_position(conn, 1, 1))
            .unwrap();
        let projected: (i64, i64, i64) = store
            .with_read(|conn| {
                conn.query_row(
                    "SELECT quantity_raw, cost_raw, realized_raw FROM positions",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
            })
            .unwrap();
        let replayed = store
            .with_read(|conn| replay_position(conn, 1, 1))
            .unwrap()
            .unwrap();
        assert_eq!(
            projected,
            (
                replayed.quantity_raw,
                replayed.cost_raw,
                replayed.realized_raw
            )
        );
        assert_eq!(projected.0, 900_000);
        assert_eq!(projected.1, 96_90);
        assert_eq!(projected.2, 12_90);
    }

    #[test]
    fn history_replay_fills_and_marks_missing_nav() {
        let store = Store::open_in_memory().unwrap();
        setup(&store, "000012");
        tx(
            &store,
            "000012",
            TYPE_BUY,
            "100",
            "1.00",
            "0",
            "2026-01-05",
            "b1",
        );
        // 写入 2026-01-06 净值 1.10（1月5日无净值）。
        store
            .with_write(|conn| -> rusqlite::Result<()> {
                conn.execute(
                    "INSERT INTO fund_nav(fund_id, nav_date, unit_nav_raw, source, fetched_at)
                     VALUES(1, '2026-01-06', 11000, 'test', '0')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let points = store
            .with_read(|conn| replay_history(conn, 1, "2026-01-04", "2026-01-07"))
            .unwrap();
        assert_eq!(points.len(), 4);
        // 01-04：无操作、无持仓。
        assert_eq!(points[0].date, "2026-01-04");
        assert_eq!(points[0].cost_raw, 0);
        // 01-05：有持仓但缺净值 → value None（不伪造）。
        assert_eq!(points[1].date, "2026-01-05");
        assert_eq!(points[1].value_raw, None);
        assert_eq!(points[1].cost_raw, 100_00);
        // 01-06：净值 1.10 → 市值 110.00。
        assert_eq!(points[2].value_raw, Some(110_00));
        assert_eq!(points[2].cost_raw, 100_00);
        // 01-07：向前填充同 01-06。
        assert_eq!(points[3].value_raw, Some(110_00));
    }

    #[test]
    fn list_positions_returns_rows_with_names() {
        let store = Store::open_in_memory().unwrap();
        setup(&store, "000013");
        tx(
            &store,
            "000013",
            TYPE_BUY,
            "10",
            "1.00",
            "0",
            "2026-01-05",
            "b1",
        );
        let rows = store.with_read(|conn| list_positions(conn)).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].account_name, "A");
        assert_eq!(rows[0].fund_code, "000013");
        assert_eq!(rows[0].quantity_raw, 100_000);
        assert_eq!(rows[0].cost_raw, 10_00);
    }
