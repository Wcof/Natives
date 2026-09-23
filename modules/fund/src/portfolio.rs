//! portfolio 模块：持仓只读回放投影（审计 B6 / 实施方案 §7.2）。
//!
//! positions 由 opening_snapshots + transactions 按 (trade_date, id) 回放得出，
//! 是只读投影：`replay_position` 只算不写；`recompute_position` 在确认交易
//! 写事务内把回放结果落到 positions 表（供列表查询加速），二者同源不双权威。
//! 卖出按移动平均成本法扣成本；超卖已在 ledger 写入时拒绝。

use rusqlite::{params, Connection, OptionalExtension};

/// 成本/金额 raw 为 2 位（fixed amount），份额 raw 为 4 位。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Position {
    pub account_id: i64,
    pub fund_id: i64,
    pub quantity_raw: i64,
    pub cost_raw: i64,
    pub realized_raw: i64,
    /// 最近一次持仓变动的流水日期；期初快照则为 as_of。
    pub as_of: String,
    /// 成本是否为未知（期初快照未给成本且无法推算）。
    pub cost_unknown: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PortfolioError {
    Oversell {
        available_raw: i64,
        requested_raw: i64,
        date: String,
    },
    Db(String),
}

impl From<rusqlite::Error> for PortfolioError {
    fn from(e: rusqlite::Error) -> Self {
        PortfolioError::Db(e.to_string())
    }
}

/// 回放单个 (account, fund) 的持仓：期初快照 + 全部已确认流水。
/// 若从未有过任何快照或流水，返回 Ok(None)；
/// 若发生过交易但当前清仓（份额为 0），返回 Ok(Some(Position))，保留累计已实现收益。
/// 遇任何时刻超卖，严格返回 Err(PortfolioError::Oversell)，禁止 min 截断掩盖。
pub fn replay_position(
    conn: &Connection,
    account_id: i64,
    fund_id: i64,
) -> Result<Option<Position>, PortfolioError> {
    let mut quantity_raw: i64 = 0;
    let mut cost_raw: i64 = 0;
    let mut realized_raw: i64 = 0;
    let mut cost_unknown = false;
    let mut as_of = String::new();
    let mut has_history = false;

    // 期初快照（无流水建账）：数量、可选成本。
    let snapshot: Option<(i64, Option<i64>, i64, bool, String)> = conn
        .query_row(
            "SELECT quantity_raw, cost_raw, derived_cost, cost_raw IS NULL, as_of
             FROM opening_snapshots WHERE account_id = ?1 AND fund_id = ?2",
            params![account_id, fund_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;
    if let Some((qty, cost, _derived, _unknown, date)) = snapshot {
        has_history = true;
        quantity_raw = qty;
        if let Some(c) = cost {
            cost_raw = c;
        } else {
            cost_unknown = true;
        }
        as_of = date;
    }

    // 已确认流水按 (trade_date, id) 回放。
    let mut stmt = conn.prepare(
        "SELECT type, quantity_raw, amount_raw, fee_raw, trade_date
         FROM transactions
         WHERE account_id = ?1 AND fund_id = ?2 AND state = 'confirmed'
         ORDER BY trade_date, id",
    )?;
    let rows = stmt.query_map(params![account_id, fund_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, String>(4)?,
        ))
    })?;

    for row in rows {
        let (tx_type, q_raw, amount_raw, fee_raw, date) = row?;
        has_history = true;
        as_of = date.clone();
        if tx_type == "BUY" {
            if quantity_raw == 0 {
                // 此前已清仓或无历史持仓：新买入份额成本完全已知
                cost_unknown = false;
            }
            quantity_raw += q_raw;
            cost_raw += amount_raw + fee_raw;
            // 注意：若此前存在未知成本份额（quantity_raw > 0 && cost_unknown），
            // 后续买入不能将整体 cost_unknown 覆盖为 false，保持未知标记。
        } else {
            // SELL：严格检查超卖，禁止通过 min 截断掩盖超卖
            if q_raw > quantity_raw {
                return Err(PortfolioError::Oversell {
                    available_raw: quantity_raw,
                    requested_raw: q_raw,
                    date,
                });
            }
            let sell_raw = q_raw;
            if quantity_raw > 0 && !cost_unknown {
                // 扣除成本（2 位 raw）= cost_raw × sell_raw / total_raw，
                // 直接整数除法 ROUND_DOWN，不做中间每份成本舍入。
                let sold_cost: i128 =
                    (cost_raw as i128) * (sell_raw as i128) / (quantity_raw as i128);
                let sold_cost = sold_cost as i64;
                cost_raw -= sold_cost;
                // 已实现收益 = 卖出金额 − 费用 − 扣除成本。
                realized_raw += amount_raw - fee_raw - sold_cost;
            }
            quantity_raw -= sell_raw;
            if quantity_raw == 0 {
                cost_raw = 0;
                cost_unknown = false;
            }
        }
    }

    if !has_history {
        return Ok(None);
    }
    Ok(Some(Position {
        account_id,
        fund_id,
        quantity_raw,
        cost_raw,
        realized_raw,
        as_of,
        cost_unknown,
    }))
}

/// 把回放结果写入 positions 投影表（确认交易写事务内调用；O(n) 流水）。
/// 清仓时保留 0 份额行以持久化累计实现收益（B1 规范）。返回是否仍有在持份额。
pub fn recompute_position(
    conn: &Connection,
    account_id: i64,
    fund_id: i64,
) -> Result<bool, PortfolioError> {
    let position = replay_position(conn, account_id, fund_id)?;
    let Some(position) = position else {
        conn.execute(
            "DELETE FROM positions WHERE account_id = ?1 AND fund_id = ?2",
            params![account_id, fund_id],
        )?;
        return Ok(false);
    };
    conn.execute(
        "INSERT INTO positions(account_id, fund_id, quantity_raw, cost_raw, realized_raw, as_of, updated_at)
         VALUES(?1,?2,?3,?4,?5,?6,?6)
         ON CONFLICT(account_id, fund_id) DO UPDATE SET
           quantity_raw = excluded.quantity_raw,
           cost_raw = excluded.cost_raw,
           realized_raw = excluded.realized_raw,
           as_of = excluded.as_of,
           updated_at = excluded.updated_at",
        params![
            position.account_id,
            position.fund_id,
            position.quantity_raw,
            position.cost_raw,
            position.realized_raw,
            position.as_of,
        ],
    )?;
    Ok(position.quantity_raw > 0)
}

/// 持仓列表（来自 positions 投影表，含账户与基金信息）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PositionRow {
    pub account_id: i64,
    pub account_name: String,
    pub fund_id: i64,
    pub fund_code: String,
    pub fund_name: String,
    pub quantity_raw: i64,
    pub cost_raw: i64,
    pub realized_raw: i64,
    pub as_of: String,
}

/// 列出全部当前有在持份额的持仓（投影表；写入路径已保证与回放同步）。
pub fn list_positions(conn: &Connection) -> rusqlite::Result<Vec<PositionRow>> {
    let mut stmt = conn.prepare(
        "SELECT p.account_id, a.name, p.fund_id, f.code, f.name,
                p.quantity_raw, p.cost_raw, p.realized_raw, p.as_of
         FROM positions p
         JOIN accounts a ON a.id = p.account_id
         JOIN funds f ON f.id = p.fund_id
         WHERE p.quantity_raw > 0
         ORDER BY p.account_id, f.code",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(PositionRow {
                account_id: r.get(0)?,
                account_name: r.get(1)?,
                fund_id: r.get(2)?,
                fund_code: r.get(3)?,
                fund_name: r.get(4)?,
                quantity_raw: r.get(5)?,
                cost_raw: r.get(6)?,
                realized_raw: r.get(7)?,
                as_of: r.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 列出全部历史持仓（含已清仓 0 份额但保留累计收益的记录）。
pub fn list_all_positions(conn: &Connection) -> rusqlite::Result<Vec<PositionRow>> {
    let mut stmt = conn.prepare(
        "SELECT p.account_id, a.name, p.fund_id, f.code, f.name,
                p.quantity_raw, p.cost_raw, p.realized_raw, p.as_of
         FROM positions p
         JOIN accounts a ON a.id = p.account_id
         JOIN funds f ON f.id = p.fund_id
         ORDER BY p.account_id, f.code",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(PositionRow {
                account_id: r.get(0)?,
                account_name: r.get(1)?,
                fund_id: r.get(2)?,
                fund_code: r.get(3)?,
                fund_name: r.get(4)?,
                quantity_raw: r.get(5)?,
                cost_raw: r.get(6)?,
                realized_raw: r.get(7)?,
                as_of: r.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 历史市值回放（审计 §4.12 / 实施方案 B1）：给定账户与天数窗口，
/// 返回每日 {date, value_raw(2位), cost_raw(2位)}。
///
/// 算法：回放流水得每日 (share_raw, cost_raw)，只记操作日并向后填充
/// （操作日之前为 0）；每日价值 = Σ share × 当日或之前最近净值。
/// 缺净值的日期 value 为 None（不伪造完整总额，实施方案 B2 验收）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryPoint {
    pub date: String,
    /// 持仓市值（2 位 raw）；缺净值时为 None。
    pub value_raw: Option<i64>,
    /// 累计成本（2 位 raw）。
    pub cost_raw: i64,
}

/// 日期字符串比较用 YYYY-MM-DD；window 起点由调用方给定。
pub fn replay_history(
    conn: &Connection,
    account_id: i64,
    from_date: &str,
    to_date: &str,
) -> rusqlite::Result<Vec<HistoryPoint>> {
    // 收集窗口内涉及的 (fund_id) 与全部操作日序列。
    let mut stmt = conn.prepare(
        "SELECT trade_date, fund_id, type, quantity_raw, amount_raw, fee_raw
         FROM transactions
         WHERE account_id = ?1 AND state = 'confirmed' AND trade_date <= ?2
         ORDER BY trade_date, id",
    )?;
    let ops = stmt
        .query_map(params![account_id, to_date], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    // 每基金每日状态：只记录操作日，后续日期向前填充。
    // state: fund_id -> (share_raw, cost_raw, cost_unknown)
    let mut state: std::collections::HashMap<i64, (i64, i64, bool)> =
        std::collections::HashMap::new();
    let mut op_dates: Vec<String> = Vec::new();

    // 期初快照计入（as_of 及之后生效）。
    let mut snap_stmt = conn.prepare(
        "SELECT as_of, fund_id, quantity_raw, cost_raw
         FROM opening_snapshots WHERE account_id = ?1 AND as_of <= ?2",
    )?;
    let snaps = snap_stmt
        .query_map(params![account_id, to_date], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, Option<i64>>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (date, fund_id, qty, cost) in &snaps {
        let entry = state.entry(*fund_id).or_insert((0, 0, true));
        entry.0 += *qty;
        if let Some(c) = cost {
            entry.1 += *c;
            entry.2 = false;
        }
        op_dates.push(date.clone());
    }

    for (date, fund_id, tx_type, qty, amount, fee) in &ops {
        let entry = state.entry(*fund_id).or_insert((0, 0, true));
        if tx_type == "BUY" {
            if entry.0 == 0 {
                entry.2 = false;
            }
            entry.0 += *qty;
            entry.1 += amount + fee;
        } else {
            let sell = *qty;
            if entry.0 > 0 && !entry.2 {
                let sold: i128 = (entry.1 as i128) * (sell as i128) / (entry.0 as i128);
                entry.1 -= sold as i64;
            }
            entry.0 = entry.0.saturating_sub(sell);
            if entry.0 == 0 {
                entry.1 = 0;
                entry.2 = false;
            }
        }
        op_dates.push(date.clone());
    }
    op_dates.sort();
    op_dates.dedup();

    // 输出日序列：from_date..=to_date 的自然日（字符串序，YYYY-MM-DD 可比较）。
    let mut results: Vec<HistoryPoint> = Vec::new();
    let mut current = from_date.to_string();
    // 简单日历推进（每月按日历天数，YYYY-MM-DD 递增）。
    while current.as_str() <= to_date {
        // 向前填充：找出所有 as_of <= current 的基金状态快照。
        // 由于状态是单调演进的，重建“截至 current”的每日状态：
        // 为 O(操作数×天数)；窗口默认 30 天可接受，避免引入复杂索引。
        let mut day_state: std::collections::HashMap<i64, (i64, i64, bool)> =
            std::collections::HashMap::new();
        for (date, fund_id, qty, cost) in &snaps {
            if date.as_str() <= current.as_str() {
                let entry = day_state.entry(*fund_id).or_insert((0, 0, true));
                entry.0 += *qty;
                if let Some(c) = cost {
                    entry.1 += *c;
                    entry.2 = false;
                }
            }
        }
        for (date, fund_id, tx_type, qty, amount, fee) in &ops {
            if date.as_str() <= current.as_str() {
                let entry = day_state.entry(*fund_id).or_insert((0, 0, true));
                match tx_type.as_str() {
                    "BUY" => {
                        if entry.0 == 0 {
                            entry.2 = false;
                        }
                        entry.0 += *qty;
                        entry.1 += amount + fee;
                    }
                    _ => {
                        let sell = *qty;
                        if entry.0 > 0 && !entry.2 {
                            let sold: i128 = (entry.1 as i128) * (sell as i128) / (entry.0 as i128);
                            entry.1 -= sold as i64;
                        }
                        entry.0 = entry.0.saturating_sub(sell);
                        if entry.0 == 0 {
                            entry.1 = 0;
                            entry.2 = false;
                        }
                    }
                }
            }
        }

        // 每日成本恒累计。
        let day_cost: i64 = day_state.values().map(|(_, cost, _)| *cost).sum();
        // 每日市值：缺任一持仓净值 → None（不伪造）。
        let mut value: Option<i64> = Some(0);
        for (fund_id, (share_raw, _, cost_unknown)) in &day_state {
            if *share_raw == 0 {
                continue;
            }
            // 当日或之前最近净值。
            let nav: Option<i64> = conn
                .query_row(
                    "SELECT unit_nav_raw FROM fund_nav
                     WHERE fund_id = ?1 AND nav_date <= ?2
                     ORDER BY nav_date DESC LIMIT 1",
                    params![fund_id, current],
                    |r| r.get(0),
                )
                .optional()?;
            match nav {
                Some(nav_raw) if !cost_unknown => {
                    // share(4位) × nav(4位) → value(2位)：除以 10^(4+4-2)=10^6
                    // ROUND_DOWN。
                    let v: i128 = (*share_raw as i128) * (nav_raw as i128) / 1_000_000;
                    value = value.map(|acc| acc + v as i64);
                }
                _ => value = None,
            }
        }
        results.push(HistoryPoint {
            date: current.clone(),
            value_raw: value,
            cost_raw: day_cost,
        });
        current = next_day(&current);
    }
    Ok(results)
}

/// YYYY-MM-DD 加一天（纯日历，无外部依赖）。
fn next_day(date: &str) -> String {
    let bytes = date.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return date.to_string();
    }
    let mut y: i64 = date[0..4].parse().unwrap_or(2026);
    let mut m: i64 = date[5..7].parse().unwrap_or(1);
    let mut d: i64 = date[8..10].parse().unwrap_or(1);
    let days_in_month = |y: i64, m: i64| -> i64 {
        match m {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                    29
                } else {
                    28
                }
            }
            _ => 30,
        }
    };
    d += 1;
    if d > days_in_month(y, m) {
        d = 1;
        m += 1;
        if m > 12 {
            m = 1;
            y += 1;
        }
    }
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed::{Fixed, SCALE_SHARE};
    use crate::ledger::{
        ensure_account, ensure_fund, record_transaction, TransactionInput, STATE_CONFIRMED,
        TYPE_BUY, TYPE_SELL,
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
}
