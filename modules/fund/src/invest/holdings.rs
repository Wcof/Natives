//! 持仓回放：把流水还原成数量、成本、盈亏。
//!
//! 这是整套账本的正确性中枢。设计约束：
//! - **移动加权平均**：卖出时按当时均价摊销成本，不追踪买入批次，不需要 lot tracking。
//! - **成本必须归零**：卖光时剩余成本精确归 0，不能残留 1 分（整数除法的经典陷阱）。
//! - **超卖拒绝**：任何时点数量为负说明流水有误，直接报错而不是静默截断。
//! - 全部用 `i128` 中间量，避免 `i64` 乘法溢出。

use super::model::{HoldingView, SnapshotPointView};
use crate::finance::FinanceError;
use crate::fixed::Fixed;
use crate::storage::Store;
use std::collections::{BTreeMap, HashMap};

const SCALE_QTY: u32 = 4;
const SCALE_AMOUNT: u32 = 2;

pub(crate) fn fmt_qty(raw: i64) -> String {
    Fixed::share(raw).to_string_value()
}

pub(crate) fn fmt_amount(raw: i64) -> String {
    Fixed::amount(raw).to_string_value()
}

/// 流水内部表示（数据库行）。
#[derive(Debug, Clone)]
pub struct FlowRow {
    pub id: i64,
    pub account_id: i64,
    pub instrument_id: i64,
    pub kind: String,
    pub quantity_raw: i64,
    pub price_raw: i64,
    pub amount_raw: i64,
    pub fee_raw: i64,
    pub split_from: Option<i64>,
    pub split_to: Option<i64>,
    pub trade_date: String,
    pub note: String,
}

/// 回放结果。`invested`/`recovered` 是累计值，用于计算净投入与日均收益。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplayState {
    /// 剩余数量（4 位定点）。
    pub quantity: i64,
    /// 剩余持仓成本（2 位定点）。
    pub cost: i64,
    /// 累计买入成本含费用。
    pub invested: i64,
    /// 累计回收 = 卖出净额 + 分红 − 独立费用。
    pub recovered: i64,
    pub opened_at: Option<String>,
}

impl ReplayState {
    /// 已实现盈亏 = 累计回收 − 已卖出部分的成本。
    pub fn realized(&self) -> i64 {
        self.recovered - (self.invested - self.cost)
    }

    /// 净投入 = 累计买入 − 累计回收。负数表示已回本。
    pub fn net_invested(&self) -> i64 {
        self.invested - self.recovered
    }
}

/// 按时间顺序回放一组流水。调用方必须保证 `rows` 已按 (trade_date, id) 升序。
pub fn replay(rows: &[FlowRow]) -> Result<ReplayState, String> {
    let mut state = ReplayState::default();
    for f in rows {
        match f.kind.as_str() {
            "BUY" => {
                state.quantity = state
                    .quantity
                    .checked_add(f.quantity_raw)
                    .ok_or("买入数量溢出")?;
                let delta = f.amount_raw.checked_add(f.fee_raw).ok_or("买入金额溢出")?;
                state.cost = state.cost.checked_add(delta).ok_or("持仓成本溢出")?;
                state.invested = state.invested.checked_add(delta).ok_or("累计投入溢出")?;
                if state.opened_at.is_none() {
                    state.opened_at = Some(f.trade_date.clone());
                }
            }
            "SELL" => {
                if f.quantity_raw > state.quantity {
                    return Err(format!(
                        "{} 卖出数量超过持仓（卖 {} > 持有 {}）",
                        f.trade_date,
                        fmt_qty(f.quantity_raw),
                        fmt_qty(state.quantity)
                    ));
                }
                // 卖光时直接清零，避免整数除法残留分位
                let removed = if f.quantity_raw == state.quantity {
                    state.cost
                } else if state.quantity > 0 {
                    ((state.cost as i128) * (f.quantity_raw as i128) / (state.quantity as i128))
                        as i64
                } else {
                    0
                };
                state.quantity -= f.quantity_raw;
                state.cost -= removed;
                if state.quantity == 0 {
                    state.cost = 0;
                }
                let net = f.amount_raw - f.fee_raw;
                state.recovered = state.recovered.checked_add(net).ok_or("卖出金额溢出")?;
            }
            "DIVIDEND" => {
                state.recovered = state
                    .recovered
                    .checked_add(f.amount_raw)
                    .ok_or("分红金额溢出")?;
            }
            "SPLIT" => {
                let from = f.split_from.ok_or("拆股缺少 split_from")?;
                let to = f.split_to.ok_or("拆股缺少 split_to")?;
                if from <= 0 || to <= 0 {
                    return Err("拆股比例必须为正整数".to_string());
                }
                state.quantity = ((state.quantity as i128) * (to as i128) / (from as i128)) as i64;
            }
            "FEE" => {
                state.recovered = state
                    .recovered
                    .checked_sub(f.amount_raw)
                    .ok_or("费用金额溢出")?;
            }
            other => return Err(format!("未知流水类型：{other}")),
        }
    }
    Ok(state)
}

/// 公历日期转序号，用于计算持有天数（含闰年规则）。供 backfill 复用。
pub(crate) fn ymd_to_ordinal(date: &str) -> Option<i64> {
    let mut parts = date.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let d: i64 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let a = (14 - m) / 12;
    let y2 = y + 4800 - a;
    let m2 = m + 12 * a - 3;
    Some(d + (153 * m2 + 2) / 5 + 365 * y2 + y2 / 4 - y2 / 100 + y2 / 400 - 32045)
}

/// 持有天数：含首日，最小 1。
pub fn held_days(opened_at: &str, today: &str) -> i64 {
    match (ymd_to_ordinal(opened_at), ymd_to_ordinal(today)) {
        (Some(a), Some(b)) => (b - a + 1).max(1),
        _ => 1,
    }
}

/// 读全部流水，按账户+标的+日期排序后分组。
pub(crate) fn load_flows(
    store: &Store,
) -> Result<BTreeMap<(i64, i64), Vec<FlowRow>>, FinanceError> {
    store.with_read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, account_id, instrument_id, type, quantity_raw, price_raw,
                    amount_raw, fee_raw, split_from, split_to, trade_date, note
             FROM flows
             ORDER BY account_id, instrument_id, trade_date, id",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(FlowRow {
                    id: r.get(0)?,
                    account_id: r.get(1)?,
                    instrument_id: r.get(2)?,
                    kind: r.get(3)?,
                    quantity_raw: r.get(4)?,
                    price_raw: r.get(5)?,
                    amount_raw: r.get(6)?,
                    fee_raw: r.get(7)?,
                    split_from: r.get(8)?,
                    split_to: r.get(9)?,
                    trade_date: r.get(10)?,
                    note: r.get(11)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut grouped: BTreeMap<(i64, i64), Vec<FlowRow>> = BTreeMap::new();
        for row in rows {
            grouped
                .entry((row.account_id, row.instrument_id))
                .or_default()
                .push(row);
        }
        Ok(grouped)
    })
}

pub(crate) struct InstrumentMeta {
    pub(crate) code: String,
    pub(crate) name: String,
    asset_class: String,
    pub(crate) market: String,
    currency: String,
    group_code: Option<String>,
    share_class: Option<String>,
    latest_price: Option<i64>,
    latest_price_at: Option<String>,
    price_stale: bool,
}

pub(crate) fn load_instrument_meta_map(
    store: &Store,
) -> Result<HashMap<i64, InstrumentMeta>, FinanceError> {
    store.with_read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, code, name, asset_class, market, currency, group_code, share_class,
                    latest_price_raw, latest_price_at, price_stale
             FROM instruments",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    InstrumentMeta {
                        code: r.get(1)?,
                        name: r.get(2)?,
                        asset_class: r.get(3)?,
                        market: r.get(4)?,
                        currency: r.get(5)?,
                        group_code: r.get(6)?,
                        share_class: r.get(7)?,
                        latest_price: r.get(8)?,
                        latest_price_at: r.get(9)?,
                        price_stale: r.get::<_, i64>(10)? != 0,
                    },
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows.into_iter().collect())
    })
}

fn load_account_names(store: &Store) -> Result<HashMap<i64, String>, FinanceError> {
    store.with_read(|conn| {
        let mut stmt = conn.prepare("SELECT id, name FROM accounts")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows.into_iter().collect())
    })
}

/// 全部持仓（仅数量非零）。数量、成本、盈亏均由此函数派生，不接受外部写入。
pub fn list_holdings(store: &Store) -> Result<Vec<HoldingView>, FinanceError> {
    let grouped = load_flows(store)?;
    let meta = load_instrument_meta_map(store)?;
    let accounts = load_account_names(store)?;
    let today = crate::assets::today_iso();
    let mut out = Vec::new();

    for ((account_id, instrument_id), rows) in grouped {
        let state = replay(&rows).map_err(FinanceError::Invalid)?;
        if state.quantity == 0 {
            continue;
        }
        let Some(inst) = meta.get(&instrument_id) else {
            continue;
        };
        let account_name = accounts.get(&account_id).cloned().unwrap_or_default();
        let avg_cost = if state.quantity > 0 {
            ((state.cost as i128) * 10_000 / (state.quantity as i128)) as i64
        } else {
            0
        };
        // 量纲：数量 4 位 × 价格 4 位 = 8 位，转成金额的 2 位需除以 10^6
        let market_value = inst
            .latest_price
            .map(|p| ((state.quantity as i128) * (p as i128) / 1_000_000) as i64);
        let unrealized = market_value.map(|v| v - state.cost);
        let unrealized_ratio = match (unrealized, state.cost) {
            (Some(u), c) if c > 0 => {
                Some(((u as f64) / (c as f64) * 100.0 * 100.0).round() / 100.0)
            }
            _ => None,
        };
        let days = state
            .opened_at
            .as_deref()
            .map(|d| held_days(d, &today))
            .unwrap_or(0);
        let daily_gain = market_value.and_then(|v| {
            if days <= 0 {
                return None;
            }
            let gain = v - state.net_invested();
            Some(((gain as i128) / (days as i128)) as i64)
        });

        out.push(HoldingView {
            account_id,
            account_name,
            instrument_id,
            code: inst.code.clone(),
            name: inst.name.clone(),
            asset_class: inst.asset_class.clone(),
            market: inst.market.clone(),
            currency: inst.currency.clone(),
            group_code: inst.group_code.clone(),
            share_class: inst.share_class.clone(),
            quantity: fmt_qty(state.quantity),
            avg_cost: fmt_amount(avg_cost),
            cost: fmt_amount(state.cost),
            price: inst.latest_price.map(fmt_qty),
            price_at: inst.latest_price_at.clone(),
            market_value: market_value.map(fmt_amount),
            unrealized: unrealized.map(fmt_amount),
            unrealized_ratio,
            realized: fmt_amount(state.realized()),
            held_days: days,
            daily_gain: daily_gain.map(fmt_amount),
            price_stale: inst.price_stale,
            opened_at: state.opened_at.clone(),
        });
    }

    // 稳定排序：先类别，再市场，再代码，最后账户
    out.sort_by(|a, b| {
        (
            &a.asset_class,
            &a.market,
            &a.code,
            &a.account_name,
            &a.account_id,
        )
            .cmp(&(
                &b.asset_class,
                &b.market,
                &b.code,
                &b.account_name,
                &b.account_id,
            ))
    });
    Ok(out)
}

/// 已落库的快照序列（日期升序）。
pub fn list_snapshots(store: &Store, limit: i64) -> Result<Vec<SnapshotPointView>, FinanceError> {
    store.with_read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT snapshot_date, net_worth_raw, cost_raw
             FROM snapshots ORDER BY snapshot_date DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map([limit], |r| {
                Ok(SnapshotPointView {
                    date: r.get(0)?,
                    net_worth: fmt_amount(r.get(1)?),
                    cost: fmt_amount(r.get(2)?),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows.into_iter().rev().collect())
    })
}

/// 写一条快照（同日覆盖）。返回是否为新写入。
pub fn upsert_snapshot(
    store: &Store,
    date: &str,
    net_worth: i64,
    cost: i64,
    realized: i64,
    stale: bool,
) -> Result<(), FinanceError> {
    let ts = crate::finance::now_secs();
    store.with_write(|conn| {
        conn.execute(
            "INSERT INTO snapshots(snapshot_date, currency, net_worth_raw, cost_raw,
                                   realized_raw, stale, created_at)
             VALUES(?1,'CNY',?2,?3,?4,?5,?6)
             ON CONFLICT(snapshot_date) DO UPDATE SET
               net_worth_raw=excluded.net_worth_raw,
               cost_raw=excluded.cost_raw,
               realized_raw=excluded.realized_raw,
               stale=excluded.stale",
            rusqlite::params![date, net_worth, cost, realized, stale as i64, ts],
        )?;
        Ok(())
    })
}

/// 写一条快照明细（同日同持仓覆盖）。
pub fn upsert_snapshot_item(
    store: &Store,
    date: &str,
    holding: &HoldingView,
    market_value: i64,
    price: i64,
) -> Result<(), FinanceError> {
    store.with_write(|conn| {
        conn.execute(
            "INSERT INTO snapshot_items(snapshot_date, account_id, instrument_id,
                                        asset_class, market, quantity_raw, price_raw,
                                        market_value_raw, cost_raw)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(snapshot_date, account_id, instrument_id) DO UPDATE SET
               asset_class=excluded.asset_class, market=excluded.market,
               quantity_raw=excluded.quantity_raw, price_raw=excluded.price_raw,
               market_value_raw=excluded.market_value_raw, cost_raw=excluded.cost_raw",
            rusqlite::params![
                date,
                holding.account_id,
                holding.instrument_id,
                holding.asset_class,
                holding.market,
                parse_qty_raw(&holding.quantity),
                price,
                market_value,
                parse_amount_raw(&holding.cost)
            ],
        )?;
        Ok(())
    })
}

fn parse_qty_raw(text: &str) -> i64 {
    Fixed::parse(text, SCALE_QTY).map(|f| f.raw()).unwrap_or(0)
}

fn parse_amount_raw(text: &str) -> i64 {
    Fixed::parse(text, SCALE_AMOUNT)
        .map(|f| f.raw())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flow(kind: &str, qty: i64, amount: i64, fee: i64) -> FlowRow {
        FlowRow {
            id: 0,
            account_id: 1,
            instrument_id: 1,
            kind: kind.to_string(),
            quantity_raw: qty,
            price_raw: 0,
            amount_raw: amount,
            fee_raw: fee,
            split_from: None,
            split_to: None,
            trade_date: "2026-01-01".to_string(),
            note: String::new(),
        }
    }

    #[test]
    fn weighted_average_cost_and_realized_profit() {
        // 买 100 股 @10（含费 5）→ 买 100 股 @20 → 卖 100 股 @30
        // 均价 = 3005 ÷ 200 = 15.025；卖掉一半摊销 1502.50
        let rows = vec![
            flow("BUY", 1_000_000, 1_000_00, 5_00),
            flow("BUY", 1_000_000, 2_000_00, 0),
            flow("SELL", 1_000_000, 3_000_00, 0),
        ];
        let s = replay(&rows).unwrap();
        assert_eq!(s.quantity, 1_000_000);
        assert_eq!(s.cost, 1_502_50, "剩余成本应为 3005 的一半");
        // 已实现 = 3000 − (3005 − 1502.5) = 1497.5
        assert_eq!(s.realized(), 1_497_50);
        // 净投入 = 3005 − 3000 = 5（只剩手续费）
        assert_eq!(s.net_invested(), 5_00);
    }

    #[test]
    fn selling_everything_zeroes_cost_exactly() {
        // 三笔买入制造非整除成本，卖光后成本必须精确归零
        let rows = vec![
            flow("BUY", 3_000_000, 1_000_00, 0),
            flow("SELL", 1_000_000, 400_00, 0),
            flow("SELL", 2_000_000, 800_00, 0),
        ];
        let s = replay(&rows).unwrap();
        assert_eq!(s.quantity, 0);
        assert_eq!(s.cost, 0, "卖光后不得残留成本分位");
    }

    #[test]
    fn split_doubles_quantity_and_halves_average_cost() {
        let mut buy = flow("BUY", 1_000_000, 1_000_00, 0);
        buy.trade_date = "2026-01-01".into();
        let mut split = flow("SPLIT", 0, 0, 0);
        split.split_from = Some(1);
        split.split_to = Some(2);
        split.trade_date = "2026-02-01".into();
        let s = replay(&[buy, split]).unwrap();
        assert_eq!(s.quantity, 2_000_000);
        assert_eq!(s.cost, 1_000_00, "拆股不应改变总成本");
    }

    #[test]
    fn dividend_does_not_change_quantity_but_increases_recovery() {
        let rows = vec![
            flow("BUY", 1_000_000, 1_000_00, 0),
            flow("DIVIDEND", 0, 50_00, 0),
        ];
        let s = replay(&rows).unwrap();
        assert_eq!(s.quantity, 1_000_000);
        assert_eq!(s.recovered, 50_00);
        assert_eq!(s.net_invested(), 950_00);
    }

    #[test]
    fn oversell_is_rejected() {
        let rows = vec![
            flow("BUY", 1_000_000, 1_000_00, 0),
            flow("SELL", 2_000_000, 2_000_00, 0),
        ];
        assert!(replay(&rows).is_err(), "超卖必须被拒绝而不是静默截断");
    }

    #[test]
    fn held_days_counts_inclusive_and_handles_leap_year() {
        assert_eq!(held_days("2026-01-01", "2026-01-01"), 1);
        assert_eq!(held_days("2026-01-01", "2026-01-02"), 2);
        // 2024 是闰年，2 月有 29 天
        assert_eq!(held_days("2024-02-28", "2024-03-01"), 3);
        assert_eq!(held_days("2026-02-28", "2026-03-01"), 2);
    }
}
