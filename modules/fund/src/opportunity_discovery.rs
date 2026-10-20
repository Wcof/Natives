//! 机会市场发现有界池（v23）。
//!
//! 契约：plan/11 R2.6——补供体相关市场发现来源及有界池，明确在持、自选、市场
//! 发现的覆盖范围和身份；不能把当前 100 项持仓／自选池称全市场扫描，也不能以
//! 无限全市场扫描替代本地生命周期规则。
//!
//! 供体对照（MIT，commit 00b345b）：`market_scan` 候选源来自常驻扫描；本实现
//! 不建常驻 Agent，发现只由用户显式刷新触发（复用既有刷新 owner／提交门／截止
//! 时间），来源为已收盘板块成分股快照（腾讯板块排行 → 新浪领涨成分股），每批
//! ≤10 只、留存表 ≤50 只、超限淘汰最旧。投影只读本表，绝不联网；冷启动（未
//! 刷新过）时发现池为空，coverage 如实标注。

use crate::finance::FinanceError;
use rusqlite::{params, Connection};
use serde_json::{json, Value};

/// 留存上限：超过淘汰最旧（discovered_at 升序，再按 symbol 稳定排序）。
pub(crate) const RETENTION: i64 = 50;
/// 单次刷新最多登记的发现数。
pub(crate) const SEED_BATCH: usize = 10;
/// 投影一次消费的发现上限（与留存上限独立，投影端再做有界）。
pub(crate) const PROJECTION_LIMIT: i64 = 30;

#[derive(Clone, Debug)]
pub(crate) struct DiscoveryEntry {
    pub symbol: String,
    pub market: &'static str,
    pub kind: &'static str,
    pub code: String,
    pub name: String,
    pub source: String,
    pub change_pct_raw: i64,
}

/// 规范化发现行：仅接受沪深京 A 股／场内基金符号（sh/sz/bj 前缀 + 6 位数字），
/// 市场与代码由前缀派生；非法行静默跳过（上游数据不可信时宁可少收）。
pub(crate) fn parse_symbol(
    symbol: &str,
    name: &str,
    source: &str,
    change_pct_raw: i64,
) -> Option<DiscoveryEntry> {
    let (market, code) = match symbol.get(..2) {
        Some("sh") => ("SSE", &symbol[2..]),
        Some("sz") => ("SZSE", &symbol[2..]),
        Some("bj") => ("BSE", &symbol[2..]),
        _ => return None,
    };
    if code.len() != 6
        || !code.bytes().all(|b| b.is_ascii_digit())
        || name.is_empty()
        || name.chars().count() > 64
    {
        return None;
    }
    // 板块成分股以股票为主；场内基金代码段显式标注 kind，其余归股票。
    let kind = if ["51", "56", "58", "15", "16"].contains(&&code[..2]) {
        "etf"
    } else {
        "stock"
    };
    Some(DiscoveryEntry {
        symbol: symbol.to_string(),
        market,
        kind,
        code: code.to_string(),
        name: name.to_string(),
        source: source.chars().take(64).collect(),
        change_pct_raw,
    })
}

/// 登记一批发现（调用方已持有写事务）；同 symbol 覆盖更新，随后按留存上限
/// 淘汰最旧。返回实际新增数。
pub(crate) fn seed(conn: &Connection, entries: &[DiscoveryEntry]) -> Result<usize, FinanceError> {
    let mut inserted = 0usize;
    for e in entries {
        conn.execute(
            "INSERT INTO opportunity_discovery(symbol,market,kind,name,source,change_pct_raw,discovered_at) VALUES(?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(symbol) DO UPDATE SET name=excluded.name, source=excluded.source, change_pct_raw=excluded.change_pct_raw, discovered_at=excluded.discovered_at",
            params![e.symbol, e.market, e.kind, e.name, e.source, e.change_pct_raw, crate::finance::now_secs()],
        )?;
        inserted += 1;
    }
    conn.execute(
        "DELETE FROM opportunity_discovery WHERE symbol NOT IN (
            SELECT symbol FROM opportunity_discovery ORDER BY discovered_at DESC, symbol DESC LIMIT ?1)",
        [RETENTION],
    )?;
    Ok(inserted)
}

/// 投影消费：按涨幅降序的有界发现行。
pub(crate) fn projection_entries(conn: &Connection) -> Result<Vec<DiscoveryEntry>, FinanceError> {
    let mut stmt = conn.prepare(
        "SELECT symbol,market,kind,name,source,change_pct_raw FROM opportunity_discovery ORDER BY change_pct_raw DESC, symbol ASC LIMIT ?1",
    )?;
    let rows = stmt.query_map([PROJECTION_LIMIT], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, i64>(5)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (symbol, market, kind, name, source, change_pct_raw) = row?;
        // 市场列在写入时已校验；读取端仍按 parse_symbol 的派生逻辑核对一致性。
        if let Some(e) = parse_symbol(&symbol, &name, &source, change_pct_raw) {
            if e.market != market || e.kind != kind {
                continue;
            }
            out.push(e);
        }
    }
    Ok(out)
}

/// 投影身份：与 opportunities::identity 同构（股票/ETF 用 security 命名空间）。
pub(crate) fn identity(e: &DiscoveryEntry) -> String {
    format!("security:{}:{}", e.market, e.code)
}

/// 投影 JSON 行（池标记为 market，不算持仓也不算自选）。
pub(crate) fn pool_row(e: &DiscoveryEntry) -> Value {
    json!({
        "securityId": identity(e),
        "name": e.name,
        "kind": e.kind,
        "market": e.market,
        "code": e.code,
        "pool": "market",
        "isHolding": false,
        "discoverySource": e.source,
    })
}

/// 市场发现种子（v23）：板块排行取首个领涨概念板块，再取其领涨成分股，
/// 解析为沪深京符号后一次写事务登记；任何上游/存储失败返回空，绝不致命。
pub(crate) fn seed_from_upstream(
    store: &std::sync::Arc<crate::storage::Store>,
) -> Vec<DiscoveryEntry> {
    let sectors = match crate::market::fetch_tencent_sectors_by_type("concept") {
        Ok(mut s) => {
            s.sort_by(|a, b| {
                b.change_pct
                    .partial_cmp(&a.change_pct)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            s
        }
        Err(_) => return vec![],
    };
    let Some(top) = sectors.first() else {
        return vec![];
    };
    let quotes =
        match crate::market::fetch_live_sector_stocks(crate::market::sina_sector_node(&top.name)) {
            Ok(q) => q,
            Err(_) => return vec![],
        };
    let entries: Vec<_> = quotes
        .iter()
        .filter_map(|q| {
            crate::opportunity_discovery::parse_symbol(
                &q.code,
                &q.name,
                &top.name,
                (q.change_pct.unwrap_or(0.0) * 100.0) as i64,
            )
        })
        .take(crate::opportunity_discovery::SEED_BATCH)
        .collect();
    if entries.is_empty() {
        return vec![];
    }
    match store.with_write(|c| crate::opportunity_discovery::seed(c, &entries)) {
        Ok(_) => entries,
        Err(_) => vec![],
    }
}

/// 发现行转刷新目标（与快照条目同形状，fetcher 直接可消费）。
pub(crate) fn target_rows(entries: &[DiscoveryEntry]) -> Vec<Value> {
    entries
        .iter()
        .map(crate::opportunity_discovery::pool_row)
        .collect()
}

#[cfg(test)]
#[path = "opportunity_discovery_tests.rs"]
mod tests;
