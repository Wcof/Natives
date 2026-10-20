//! Public source identities projected from existing ledger authority.
/// A-share source identities only. Explicit foreign markets never collapse to a six-digit A-share code.
pub fn public_stock_code(symbol: &str) -> Option<String> {
    let key = crate::rules::normalize_symbol_key(symbol);
    let code = key.rsplit(':').next()?;
    if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let market = if code.starts_with('6') || code.starts_with('5') {
        "SSE"
    } else if code.starts_with('0') || code.starts_with('3') || code.starts_with('1') {
        "SZSE"
    } else if code.starts_with('4') || code.starts_with('8') || code.starts_with("92") {
        "BSE"
    } else {
        return None;
    };
    if key.contains(':') && key != format!("security:{market}:{code}") {
        return None;
    }
    Some(code.to_string())
}

/// 证券稳定身份（方案 03 §3）：A 股六位数字 → market=CN。
pub fn instrument_ref_for_code(code: &str) -> String {
    format!(
        "security:CN:stock:{}",
        public_stock_code(code).unwrap_or_else(|| code.to_string())
    )
}

/// 新闻关键词（可能是名称）的候选身份：同一关键词串规范化，不冒充已核实市场身份。
pub fn instrument_ref_for_keyword(keyword: &str) -> String {
    format!("keyword:{keyword}")
}

pub fn instrument_ref_for_fund(code: &str) -> String {
    format!("security:CN:fund:{code}")
}

/// 由 Store 中已登记的持仓投影出采集身份列表（基金与股票分开）。
/// 基金：funds 表已确认持仓的 code；股票：暂无股票主档（T04 前），
/// 允许从 watchlist/security 身份登记中取（scope=watchlist）。
pub fn collect_instruments(
    conn: &rusqlite::Connection,
) -> rusqlite::Result<(Vec<String>, Vec<String>)> {
    let mut funds = Vec::new();
    let mut stocks = Vec::new();
    let mut stmt=conn.prepare("SELECT DISTINCT f.instrument_kind,f.market,CASE WHEN f.display_code='' THEN f.code ELSE f.display_code END FROM funds f JOIN positions p ON p.fund_id=f.id WHERE p.quantity_raw>0")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    for row in rows {
        let (kind, market, code) = row?;
        if kind == "stock" {
            if let Some(code) = public_stock_code(&format!("security:{market}:{code}")) {
                stocks.push(code);
            }
        } else if market == "CN"
            || kind == "etf" && matches!(market.as_str(), "SSE" | "SZSE" | "BSE")
        {
            funds.push(code);
        }
    }
    let mut stmt=conn.prepare("SELECT instrument_key FROM manual_assets WHERE product_type IN ('stock','etf') AND market_value_minor>0 AND deleted_at IS NULL AND converted_ref IS NULL")?;
    for key in stmt.query_map([], |r| r.get::<_, String>(0))? {
        let key = key?;
        if let Some(code) = public_stock_code(&key) {
            stocks.push(code);
        }
    }
    funds.sort();
    funds.dedup();
    stocks.sort();
    stocks.dedup();
    Ok((funds, stocks))
}

/// 刷新全部启用来源（M05 语义：每来源独立记录状态；来源失败不影响其他来源；
/// 全部禁用 → 各来源 disabled，不偷偷回退）。

/// 持仓基金名称映射（新闻按名称搜索远好于代码，M01 供体经验）。
/// funds 表无名称列时返回空（新闻范围退化到代码搜索；不虚构名称）。
pub(crate) fn fund_names(c: &rusqlite::Connection) -> rusqlite::Result<Vec<String>> {
    let mut s=c.prepare("SELECT DISTINCT f.name FROM funds f JOIN positions p ON p.fund_id=f.id WHERE p.quantity_raw>0 AND f.name!='' AND f.market IN ('CN','SSE','SZSE','BSE') ORDER BY f.name LIMIT 50")?;
    let rows = s
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub(crate) fn fund_names_for_codes(
    c: &rusqlite::Connection,
    codes: &[String],
) -> rusqlite::Result<Vec<String>> {
    let mut names = Vec::new();
    let mut query=c.prepare("SELECT DISTINCT name FROM funds WHERE (display_code=?1 OR code=?1) AND instrument_kind IN ('fund','etf') AND market IN ('CN','SSE','SZSE','BSE') AND name!=''")?;
    for code in codes {
        names.extend(
            query
                .query_map([code], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?,
        );
    }
    names.sort();
    names.dedup();
    Ok(names)
}

pub(super) fn identity_batch(
    funds: &[String],
    stocks: &[String],
    offset: usize,
) -> (Vec<String>, Vec<String>, Option<usize>) {
    let total = funds.len() + stocks.len();
    let selected: Vec<_> = funds
        .iter()
        .map(|c| (true, c))
        .chain(stocks.iter().map(|c| (false, c)))
        .skip(offset)
        .take(50)
        .collect();
    let next = (offset + selected.len() < total).then_some(offset + selected.len());
    (
        selected
            .iter()
            .filter(|(fund, _)| *fund)
            .map(|(_, code)| (*code).clone())
            .collect(),
        selected
            .iter()
            .filter(|(fund, _)| !*fund)
            .map(|(_, code)| (*code).clone())
            .collect(),
        next,
    )
}
