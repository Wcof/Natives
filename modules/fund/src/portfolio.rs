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

impl std::fmt::Display for PortfolioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PortfolioError::Oversell {
                available_raw,
                requested_raw,
                date,
            } => write!(
                f,
                "oversell on {date}: available {available_raw}, requested {requested_raw}"
            ),
            PortfolioError::Db(message) => write!(f, "{message}"),
        }
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
             FROM opening_snapshots WHERE account_id = ?1 AND fund_id = ?2 AND deleted_at IS NULL",
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
         WHERE account_id = ?1 AND fund_id = ?2 AND state = 'confirmed' AND deleted_at IS NULL
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

    // 持仓校准锚点（N09）：按 (as_of_date, id) 顺序，在处理完 ≤as_of 的
    // 已确认流水后应用（同日锚点视为当日终态）；置份额与成本为校准值，
    // 不重算历史已实现收益，后续成本基线为校准值。多锚点按时间依次覆盖。
    let mut cal_stmt = conn.prepare(
        "SELECT as_of_date, quantity_raw, cost_minor
         FROM position_calibrations WHERE account_id = ?1 AND fund_id = ?2
         ORDER BY as_of_date, id",
    )?;
    let cals = cal_stmt
        .query_map(params![account_id, fund_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut cal_idx = 0usize;
    let apply_calibration = |cal: &(String, i64, i64),
                             quantity_raw: &mut i64,
                             cost_raw: &mut i64,
                             cost_unknown: &mut bool,
                             as_of: &mut String| {
        *quantity_raw = cal.1;
        *cost_raw = cal.2;
        *cost_unknown = false;
        if *quantity_raw == 0 {
            *cost_raw = 0;
        }
        if cal.0.as_str() > as_of.as_str() {
            *as_of = cal.0.clone();
        }
    };

    for row in rows {
        let (tx_type, q_raw, amount_raw, fee_raw, date) = row?;
        has_history = true;
        while cal_idx < cals.len() && cals[cal_idx].0.as_str() < date.as_str() {
            apply_calibration(
                &cals[cal_idx],
                &mut quantity_raw,
                &mut cost_raw,
                &mut cost_unknown,
                &mut as_of,
            );
            cal_idx += 1;
        }
        as_of = date.clone();
        if tx_type == "BUY" {
            if quantity_raw == 0 {
                // 此前已清仓或无历史持仓：新买入份额成本完全已知
                cost_unknown = false;
            }
            quantity_raw = quantity_raw
                .checked_add(q_raw)
                .ok_or_else(|| PortfolioError::Db("数量溢出".into()))?;
            cost_raw = cost_raw
                .checked_add(amount_raw)
                .and_then(|v| v.checked_add(fee_raw))
                .ok_or_else(|| PortfolioError::Db("成本溢出".into()))?;
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
                realized_raw = realized_raw
                    .checked_add(amount_raw)
                    .and_then(|v| v.checked_sub(fee_raw))
                    .and_then(|v| v.checked_sub(sold_cost))
                    .ok_or_else(|| PortfolioError::Db("收益溢出".into()))?;
            }
            quantity_raw -= sell_raw;
            if quantity_raw == 0 {
                cost_raw = 0;
                cost_unknown = false;
            }
        }
    }

    // 剩余锚点（日期 ≥ 最后一条流水）：按序应用，晚日期覆盖早日期。
    while cal_idx < cals.len() {
        has_history = true;
        apply_calibration(
            &cals[cal_idx],
            &mut quantity_raw,
            &mut cost_raw,
            &mut cost_unknown,
            &mut as_of,
        );
        cal_idx += 1;
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

/// 卡片/资产汇总的共享查询（ADR-0036 / 方案 05 §2）：当前在持 (account, fund)
/// 的回放真值，保留 `cost_unknown` 与 `as_of`。positions 投影表不持久化
/// cost_unknown，卡片与汇总不得直接求和 `positions.cost_raw` 冒充完整成本。
pub fn holdings_with_cost_status(conn: &Connection) -> rusqlite::Result<Vec<Position>> {
    let mut stmt =
        conn.prepare("SELECT DISTINCT account_id, fund_id FROM positions WHERE quantity_raw > 0")?;
    let pairs: Vec<(i64, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut holdings = Vec::with_capacity(pairs.len());
    for (account_id, fund_id) in pairs {
        let position = replay_position(conn, account_id, fund_id).map_err(|e| {
            rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
                Some(e.to_string()),
            )
        })?;
        if let Some(position) = position {
            if position.quantity_raw > 0 {
                holdings.push(position);
            }
        }
    }
    Ok(holdings)
}

/// 持仓页账户元数据（T04）：结构化 ID/版本/顺序/归档标记，按 (sort_order,id)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountMeta {
    pub id: i64,
    pub name: String,
    pub version: i64,
    pub sort_order: i64,
    pub archived: bool,
}

/// 持仓完整明细（T04 A19~A22）：回放真值（含 cost_unknown）+ 标的身份 +
/// 净值游标 + 待确认笔数，单读快照一致返回，避免 UI 多次请求间的撕裂读。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoldingDetail {
    pub account_id: i64,
    pub fund_id: i64,
    pub fund_code: String,
    pub fund_name: String,
    pub quantity_raw: i64,
    pub cost_raw: i64,
    pub realized_raw: i64,
    pub as_of: String,
    pub cost_unknown: bool,
    pub latest_nav_raw: Option<i64>,
    pub latest_nav_date: Option<String>,
    pub pending_count: i64,
}

/// 持仓页一次性快照：账户元数据（含归档，UI 自行过滤）与在持明细。
/// 明细以 `holdings_with_cost_status` 回放为权威，禁止用投影表冒充未知成本。
pub fn holdings_detail(
    conn: &Connection,
) -> rusqlite::Result<(Vec<AccountMeta>, Vec<HoldingDetail>)> {
    let mut stmt = conn.prepare(
        "SELECT id, name, version, sort_order, archived_at IS NOT NULL
         FROM accounts ORDER BY sort_order, id",
    )?;
    let accounts = stmt
        .query_map([], |r| {
            Ok(AccountMeta {
                id: r.get(0)?,
                name: r.get(1)?,
                version: r.get(2)?,
                sort_order: r.get(3)?,
                archived: r.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let holdings = holdings_with_cost_status(conn)?;

    let mut fund_stmt =
        conn.prepare("SELECT id, code, name, latest_nav_raw, latest_nav_date FROM funds")?;
    let funds: std::collections::HashMap<i64, (String, String, Option<i64>, Option<String>)> =
        fund_stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    (r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?),
                ))
            })?
            .collect::<Result<_, _>>()?;

    let mut pending_stmt = conn.prepare(
        "SELECT account_id, fund_id, COUNT(*) FROM transactions
         WHERE state = 'pending' AND deleted_at IS NULL
         GROUP BY account_id, fund_id",
    )?;
    let pending: std::collections::HashMap<(i64, i64), i64> = pending_stmt
        .query_map([], |r| Ok(((r.get(0)?, r.get(1)?), r.get(2)?)))?
        .collect::<Result<_, _>>()?;

    let mut rows = Vec::with_capacity(holdings.len());
    for h in &holdings {
        let Some((code, name, nav_raw, nav_date)) = funds.get(&h.fund_id) else {
            continue;
        };
        rows.push(HoldingDetail {
            account_id: h.account_id,
            fund_id: h.fund_id,
            fund_code: code.clone(),
            fund_name: name.clone(),
            quantity_raw: h.quantity_raw,
            cost_raw: h.cost_raw,
            realized_raw: h.realized_raw,
            as_of: h.as_of.clone(),
            cost_unknown: h.cost_unknown,
            latest_nav_raw: *nav_raw,
            latest_nav_date: nav_date.clone(),
            pending_count: pending
                .get(&(h.account_id, h.fund_id))
                .copied()
                .unwrap_or(0),
        });
    }
    Ok((accounts, rows))
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
         WHERE account_id = ?1 AND state = 'confirmed' AND deleted_at IS NULL AND trade_date <= ?2
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
         FROM opening_snapshots WHERE account_id = ?1 AND as_of <= ?2 AND deleted_at IS NULL",
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
mod tests;
