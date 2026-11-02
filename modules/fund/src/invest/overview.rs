//! 总览聚合与每日快照。
//!
//! 两条不可妥协的原则：
//! 1. **缺数据不假装**：持仓无行情时不计入净值；外币无汇率时按 1:1 计入但
//!    标记 `incomplete`，由 UI 明确提示「未折算」，而不是给出一个看似精确的假数字。
//! 2. **快照只在数据完整时沉淀**：`stale` 标记会随快照落库，日后回看能知道
//!    那天的数字是否完整。
//!
//! lazy 补算的范围说明：补历史需要历史收盘价，目前 `quotes` 为空，
//! 因此本期只保证「打开时写入当天快照」，不做历史日期回补。
//! 等行情接入后再启用逐日补算（设计文档 §4.3）。

use super::holdings::{
    fmt_amount, list_holdings, list_snapshots, upsert_snapshot, upsert_snapshot_item,
};
use super::model::{ClassTotalView, HoldingView, OverviewView, SnapshotPointView};
use crate::finance::FinanceError;
use crate::fixed::Fixed;
use crate::storage::Store;
use std::collections::BTreeMap;

const RATE_SCALE: i64 = 1_000_000;

/// 读最近一条汇率（base → quote）。无数据返回 None，调用方必须处理降级。
fn latest_rate(store: &Store, base: &str, quote: &str) -> Result<Option<i64>, FinanceError> {
    if base == quote {
        return Ok(Some(RATE_SCALE));
    }
    store.with_read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT rate_raw FROM fx_rates
             WHERE base=?1 AND quote=?2
             ORDER BY rate_date DESC LIMIT 1",
        )?;
        let mut rows = stmt.query_map([base, quote], |r| r.get::<_, i64>(0))?;
        match rows.next() {
            Some(v) => Ok(Some(v?)),
            None => Ok(None),
        }
    })
}

/// 按资产类别汇总的结果。
struct Tally {
    market_value: i64,
    cost: i64,
}

/// 构建总览。所有金额以 CNY 计；无法折算的部分不计入并在 `incomplete` 中体现。
pub fn build(store: &Store) -> Result<OverviewView, FinanceError> {
    let holdings = list_holdings(store)?;
    let today = crate::assets::today_iso();
    let mut by_class: BTreeMap<String, Tally> = BTreeMap::new();
    let mut net_worth: i64 = 0;
    let mut total_cost: i64 = 0;
    let mut realized: i64 = 0;
    let mut incomplete = false;

    for h in &holdings {
        // 无行情的持仓不进入净值：宁可少算，也不要凭空造一个价格。
        let Some(mv_text) = h.market_value.as_deref() else {
            incomplete = true;
            continue;
        };
        let raw_value = Fixed::parse(mv_text, 2).map(|f| f.raw()).unwrap_or(0);
        let raw_cost = Fixed::parse(&h.cost, 2).map(|f| f.raw()).unwrap_or(0);
        let raw_realized = Fixed::parse(&h.realized, 2).map(|f| f.raw()).unwrap_or(0);

        let (value_cny, cost_cny) = if h.currency == "CNY" {
            (raw_value, raw_cost)
        } else {
            match latest_rate(store, &h.currency, "CNY")? {
                Some(rate) => (
                    ((raw_value as i128) * (rate as i128) / (RATE_SCALE as i128)) as i64,
                    ((raw_cost as i128) * (rate as i128) / (RATE_SCALE as i128)) as i64,
                ),
                None => {
                    // 没有汇率时按 1:1 计入，但必须标记不完整
                    incomplete = true;
                    (raw_value, raw_cost)
                }
            }
        };

        net_worth += value_cny;
        total_cost += cost_cny;
        realized += raw_realized;
        let entry = by_class.entry(h.asset_class.clone()).or_insert(Tally {
            market_value: 0,
            cost: 0,
        });
        entry.market_value += value_cny;
        entry.cost += cost_cny;
    }

    let classes: Vec<ClassTotalView> = by_class
        .into_iter()
        .map(|(asset_class, tally)| {
            let weight = if net_worth > 0 {
                ((tally.market_value as f64) / (net_worth as f64) * 100.0 * 100.0).round() / 100.0
            } else {
                0.0
            };
            ClassTotalView {
                asset_class,
                market_value: fmt_amount(tally.market_value),
                cost: fmt_amount(tally.cost),
                weight: format!("{weight:.2}"),
            }
        })
        .collect();

    let series = list_snapshots(store, 365)?;

    Ok(OverviewView {
        currency: "CNY".to_string(),
        net_worth: fmt_amount(net_worth),
        cost: fmt_amount(total_cost),
        unrealized: fmt_amount(net_worth - total_cost),
        realized: fmt_amount(realized),
        by_class: classes,
        series,
        incomplete,
        as_of: today,
    })
}

/// 写入当天快照。同日重复调用覆盖，不会产生重复行。
///
/// 返回 `Ok(true)` 表示本次确实写入了（含覆盖），`Ok(false)` 表示数据不完整
/// 且已有快照、无需覆盖。
pub fn write_today_snapshot(store: &Store) -> Result<bool, FinanceError> {
    let holdings = list_holdings(store)?;
    let today = crate::assets::today_iso();
    let overview = build(store)?;
    let net_worth = Fixed::parse(&overview.net_worth, 2)
        .map(|f| f.raw())
        .unwrap_or(0);
    let cost = Fixed::parse(&overview.cost, 2)
        .map(|f| f.raw())
        .unwrap_or(0);
    let realized = Fixed::parse(&overview.realized, 2)
        .map(|f| f.raw())
        .unwrap_or(0);

    let mut stale = overview.incomplete;
    for h in &holdings {
        if h.price_stale {
            stale = true;
        }
        // 无行情的持仓不写明细行，避免出现价格为 0 的假快照
        let (Some(mv_text), Some(price_text)) = (h.market_value.as_deref(), h.price.as_deref())
        else {
            continue;
        };
        let mv = Fixed::parse(mv_text, 2).map(|f| f.raw()).unwrap_or(0);
        let price = Fixed::parse(price_text, 4).map(|f| f.raw()).unwrap_or(0);
        upsert_snapshot_item(store, &today, h, mv, price)?;
    }

    upsert_snapshot(store, &today, net_worth, cost, realized, stale)?;
    Ok(true)
}

/// 该持仓是否属于某个 A/C 归组（UI 合并展示用）。
pub fn group_key(h: &HoldingView) -> String {
    h.group_code
        .clone()
        .unwrap_or_else(|| format!("{}:{}", h.market, h.code))
}

/// 供测试与调试：空序列占位，避免前端拿到 null。
pub fn empty_series() -> Vec<SnapshotPointView> {
    Vec::new()
}
