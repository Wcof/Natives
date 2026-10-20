//! N04 统一估值管线：集合段（collect）→ 行情统一（apply_quotes）→ 聚合段
//! （finalize），以及估值批次身份与进程内共享批次缓存。唯一聚合权威；
//! 纯账本路径（assets::assets_summary）与行情路径共用同一段聚合代码。

use crate::assets::{
    add_minor, finalize_summary, find_legacy_overlap, fund_valuation_row, is_stale,
    touch_valuation_dates, AssetError, AssetsSummary, FundValuationRow, ManualAssetRow,
};
use crate::ledger;
use crate::portfolio;
use crate::storage::Store;
use rusqlite::Connection;

/// 读段（N04）：同一读快照内只做查询，不做聚合；行情在锁外获取后经
/// [`apply_quotes`] 统一进估值，再 [`finalize`] 聚合。两条路径共用同一聚合。
pub struct Collected {
    pub funds: Vec<FundValuationRow>,
    pub manual: Vec<ManualAssetRow>,
    /// (value_minor, valued_at, risk_class, receivable)
    pub offline: Vec<(i64, String, String, i64)>,
    pub pending_trades: i64,
}

/// 已记录投资资产集合段：只查询账本真值（在持、人工、线下、待确认流水）。
pub fn collect(conn: &Connection, today: &str) -> Result<Collected, AssetError> {
    let mut collected = Collected {
        funds: Vec::new(),
        manual: Vec::new(),
        offline: Vec::new(),
        pending_trades: 0,
    };

    // 1. 在持基金（回放真值，含 cost_unknown）。
    let holdings = portfolio::holdings_with_cost_status(conn)?;
    for position in &holdings {
        collected
            .funds
            .push(fund_valuation_row(conn, position, today)?);
    }

    // 2. 人工资产。
    let mut stmt = conn.prepare(
        "SELECT m.id, m.account_id, a.name, m.product_type, m.instrument_key, m.name,
                m.risk_class, m.market_value_minor, m.valued_at, m.legacy_overlap_distinct
         FROM manual_assets m
         JOIN accounts a ON a.id = m.account_id
         WHERE m.deleted_at IS NULL AND m.converted_ref IS NULL
         ORDER BY m.account_id, m.product_type, m.id",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, i64>(7)?,
                r.get::<_, String>(8)?,
                r.get::<_, i64>(9)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (
        id,
        account_id,
        account_name,
        product_type,
        key,
        name,
        risk_class,
        value_minor,
        valued_at,
        distinct,
    ) in rows
    {
        // 聚合时统一扫描疑似重复：旧基金持仓在人工记录之后出现同样触发。
        let mut overlap_review = false;
        if product_type == "stock" || product_type == "etf" {
            if let Some(code) = key.rsplit(':').next() {
                if distinct == 0 && find_legacy_overlap(conn, account_id, code)? {
                    overlap_review = true;
                }
            }
        }
        let stale = is_stale(&valued_at, today);
        collected.manual.push(ManualAssetRow {
            id,
            account_id,
            account_name,
            product_type,
            instrument_key: key,
            name,
            risk_class,
            market_value_minor: value_minor,
            valued_at,
            stale,
            overlap_review,
            legacy_overlap_distinct: distinct == 1,
        });
    }

    // 3. 线下投资资产（审计 P0-2）：实物黄金/定存/理财/其他手工估值。
    {
        let mut stmt = conn.prepare(
            "SELECT value_minor, valued_at, risk_class, \
             COALESCE((SELECT SUM(h.amount_minor - h.fee_minor) FROM offline_exit_history h \
                       WHERE h.asset_id = o.id AND h.settled = 0), 0) \
             FROM offline_assets o WHERE deleted_at IS NULL",
        )?;
        let offline_rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        collected.offline = offline_rows;
    }

    collected.pending_trades = conn.query_row(
        "SELECT COUNT(*) FROM trade_cash x JOIN transactions t ON t.id=x.tx_id WHERE x.status!='settled' AND t.deleted_at IS NULL",
        [],
        |r| r.get(0),
    )?;
    Ok(collected)
}

/// 写段（N04）：从集合数据聚合全部计数与金额。单一权威聚合，
/// 行情统一与非行情路径共用，禁止各接口独立重算。
pub fn finalize(collected: Collected, today: &str) -> Result<AssetsSummary, AssetError> {
    let mut summary = AssetsSummary::default();

    // 1. 在持基金：统一估值行（行情或净值）聚合。
    for row in &collected.funds {
        if row.risk_class == "unknown" {
            summary.unknown_class_count += 1;
        }
        touch_valuation_dates(&mut summary, price_date(&row.price_as_of));
        match row.value_gap.as_deref() {
            None => {
                summary.priced_count += 1;
                summary.quote_count += 1;
                summary.fund_value_minor = Some(add_minor(
                    summary.fund_value_minor.unwrap_or(0),
                    row.value_minor.unwrap_or(0),
                    "基金现值合计",
                )?);
            }
            Some("missing_quote") => summary.missing_quote_count += 1,
            Some("missing_nav") => summary.missing_price_count += 1,
            Some("currency_unknown") => summary.currency_unconfirmed_count += 1,
            Some(_) => summary.currency_unsupported_count += 1,
        }
        if row.stale {
            summary.old_valuation_count += 1;
        }
        if row.cost_unknown {
            summary.unknown_cost_count += 1;
        }
        if row.value_minor.is_some() {
            if row.cost_unknown {
                summary.floating_unknown_cost_count += 1;
            } else {
                let delta = (row.value_minor.unwrap_or(0) as i128) - (row.cost_raw as i128);
                let delta = i64::try_from(delta)
                    .map_err(|_| AssetError::Overflow(format!("浮动收益溢出：{delta}")))?;
                summary.floating_minor = Some(add_minor(
                    summary.floating_minor.unwrap_or(0),
                    delta,
                    "持仓浮动收益",
                )?);
            }
        }
    }

    // 2. 人工资产。
    for row in &collected.manual {
        if row.risk_class == "unknown" {
            summary.unknown_class_count += 1;
        }
        if row.overlap_review {
            summary.overlap_review_count += 1;
        }
        if row.stale {
            summary.old_valuation_count += 1;
        }
        touch_valuation_dates(&mut summary, Some(&row.valued_at));
        if !row.overlap_review {
            // 人工资产有用户声明的估值（零元也算已计价）。
            summary.priced_count += 1;
            summary.manual_value_minor = Some(add_minor(
                summary.manual_value_minor.unwrap_or(0),
                row.market_value_minor,
                "人工资产合计",
            )?);
        }
    }

    // 3. 线下投资资产：估值计入 A；退出余值 0 行保留计数但不计价值；
    // 未到账应收独立统计。
    for (value_minor, valued_at, risk_class, receivable) in &collected.offline {
        summary.offline_count += 1;
        summary.has_entries = true;
        if risk_class == "unknown" {
            summary.unknown_class_count += 1;
        }
        if is_stale(valued_at, today) {
            summary.old_valuation_count += 1;
        }
        touch_valuation_dates(&mut summary, Some(valued_at));
        summary.priced_count += 1;
        summary.offline_value_minor = Some(add_minor(
            summary.offline_value_minor.unwrap_or(0),
            *value_minor,
            "线下资产合计",
        )?);
        if *receivable > 0 {
            summary.offline_receivable_minor = Some(add_minor(
                summary.offline_receivable_minor.unwrap_or(0),
                *receivable,
                "线下退出应收合计",
            )?);
        }
    }

    summary.missing_price_count += collected.pending_trades as usize;
    summary.funds = collected.funds;
    summary.manual = collected.manual;
    finalize_summary(&mut summary)?;
    Ok(summary)
}

/// 价格业务时间的日期部分（行情 asOf 含时分秒，净值日期本身即日期）。
fn price_date(price_as_of: &Option<String>) -> Option<&str> {
    price_as_of.as_deref().map(|s| &s[..s.len().min(10)])
}

// ---------------------------------------------------------------------------
// N04 统一估值：行情接入、批次身份与共享估值缓存
// ---------------------------------------------------------------------------

/// 单一标的的行情输入（价格 4 位定点；asOf 为行情业务时间）。
#[derive(Debug, Clone)]
pub struct QuoteInput {
    pub price_raw: i64,
    pub change_raw: Option<i64>,
    pub as_of: Option<String>,
    pub source: String,
}

/// 把行情统一进集合数据：场内股票/ETF 用 数量×行情价；场外基金保持净值。
/// 返回成功计价的场内项数。价格变化不推进账本 revision，批次身份由
/// [`valuation_batch`] 另行包含价格来源/业务时间/实际值。
pub fn apply_quotes(
    collected: &mut Collected,
    quotes: &std::collections::BTreeMap<String, QuoteInput>,
    today: &str,
) -> usize {
    let mut matched = 0usize;
    for row in &mut collected.funds {
        let Ok((kind, market, code)) = ledger::instrument_parts(&row.fund_code) else {
            continue;
        };
        row.instrument_kind = kind.to_string();
        row.market = market.to_string();
        if kind == "fund" {
            // 场外基金：净值口径不变；人工确认净值保留来源与日期。
            row.value_source = if row.nav_raw.is_some() { "nav" } else { "none" }.to_string();
            row.price_raw = row.nav_raw;
            row.price_as_of = row.nav_date.clone();
            continue;
        }
        let prefix = match (kind, market) {
            ("stock" | "etf", "SSE") => "sh",
            ("stock" | "etf", "SZSE") => "sz",
            ("stock" | "etf", "BSE") => "bj",
            _ => "",
        };
        let Some(input) = quotes.get(&format!("{prefix}{code}")) else {
            row.value_minor = None;
            row.value_gap = Some("missing_quote".into());
            row.value_source = "none".into();
            row.price_raw = None;
            row.price_as_of = None;
            row.stale = false;
            continue;
        };
        let product = (row.quantity_raw as i128) * (input.price_raw as i128);
        match i64::try_from(product / 1_000_000) {
            Ok(minor) => {
                row.value_minor = Some(minor);
                row.value_gap = None;
                matched += 1;
            }
            Err(_) => {
                row.value_minor = None;
                row.value_gap = Some("overflow".into());
            }
        }
        row.value_source = "quote".into();
        row.price_raw = Some(input.price_raw);
        row.day_change_raw = input.change_raw;
        row.price_as_of = input.as_of.clone();
        row.stale = input
            .as_of
            .as_deref()
            .map(|s| &s[..s.len().min(10)] != today)
            .unwrap_or(true);
    }
    matched
}

/// 场内在持的行情代码集合（有界获取的输入身份）。
pub fn quote_symbols(collected: &Collected) -> Vec<String> {
    let mut symbols = std::collections::BTreeSet::new();
    for row in &collected.funds {
        let Ok((kind, market, code)) = ledger::instrument_parts(&row.fund_code) else {
            continue;
        };
        let prefix = match (kind, market) {
            ("stock" | "etf", "SSE") => "sh",
            ("stock" | "etf", "SZSE") => "sz",
            ("stock" | "etf", "BSE") => "bj",
            _ => continue,
        };
        symbols.insert(format!("{prefix}{code}"));
    }
    symbols.into_iter().collect()
}

/// 估值批次身份：账本 revision ＋ 各标的（来源｜业务时间｜实际价格值）的
/// 规范化序；价格变化不推进 revision 时批次仍可区分。
pub fn valuation_batch(revision: &str, fetched_at: i64, summary: &AssetsSummary) -> String {
    let mut entries: Vec<String> = summary
        .funds
        .iter()
        .filter_map(|row| {
            row.price_raw.map(|price| {
                format!(
                    "{}|{}|{price}|{}|{:?}",
                    row.fund_code,
                    row.value_source,
                    row.price_as_of.as_deref().unwrap_or("-"),
                    row.day_change_raw
                )
            })
        })
        .collect();
    entries.sort();
    app_runtime_core::sha256_hex(
        format!("v1|{revision}|{fetched_at}|{}", entries.join(";")).as_bytes(),
    )
}

/// 共享估值批次缓存（进程内静态；按连接地址区分 Store 实例，测试互不污染）：
/// 首页/持仓刷新写入，空间卡、目标、配置等只读消费方读取，避免逐卡联网或
/// 第二行情权威。缓存写入不推进账本 revision；进程重启后由首次刷新重建。
#[derive(Debug, Clone)]
pub struct CachedValuation {
    pub batch: String,
    pub fetched_at: i64,
    pub revision: String,
}

struct SharedValuationBatch {
    store_id: usize,
    quotes: std::collections::BTreeMap<String, QuoteInput>,
    fetched_at: i64,
    revision: String,
    batch: String,
    /// 上游获取是否失败（批次不完整时的显式标记）。
    #[allow(dead_code)]
    failed: bool,
}

static SHARED_VALUATION: std::sync::Mutex<Option<SharedValuationBatch>> =
    std::sync::Mutex::new(None);

fn connection_id(conn: &Connection) -> usize {
    conn as *const Connection as usize
}

pub fn store_valuation_cache(
    conn: &Connection,
    quotes: &std::collections::BTreeMap<String, QuoteInput>,
    fetched_at: i64,
    revision: &str,
    batch: &str,
    failed: bool,
) {
    let entry = SharedValuationBatch {
        store_id: connection_id(conn),
        quotes: quotes.clone(),
        fetched_at,
        revision: revision.to_string(),
        batch: batch.to_string(),
        failed,
    };
    if let Ok(mut guard) = SHARED_VALUATION.lock() {
        *guard = Some(entry);
    }
}

fn cache_applicable(conn: &Connection, entry: &SharedValuationBatch) -> bool {
    // Quotes contain identity and price, never old quantities; callers revalue
    // current holdings and rebuild the batch when the ledger revision changes.
    entry.store_id == connection_id(conn)
}

pub(crate) fn read_valuation_cache(conn: &Connection) -> Option<CachedValuation> {
    let guard = SHARED_VALUATION.lock().ok()?;
    let entry = guard.as_ref()?;
    if !cache_applicable(conn, entry) {
        return None;
    }
    Some(CachedValuation {
        batch: entry.batch.clone(),
        fetched_at: entry.fetched_at,
        revision: entry.revision.clone(),
    })
}

pub(crate) fn cached_quotes(
    conn: &Connection,
) -> Option<std::collections::BTreeMap<String, QuoteInput>> {
    let guard = SHARED_VALUATION.lock().ok()?;
    let entry = guard.as_ref()?;
    if !cache_applicable(conn, entry) {
        return None;
    }
    Some(entry.quotes.clone())
}

/// 只读消费方的统一汇总：账本净值＋最近共享行情批次（同进程同源，带过期态）。
pub fn summary_with_cached_quotes(
    conn: &Connection,
    today: &str,
) -> Result<(AssetsSummary, Option<CachedValuation>), AssetError> {
    let mut collected = collect(conn, today)?;
    let cached = read_valuation_cache(conn);
    if cached.is_some() {
        if let Some(quotes) = cached_quotes(conn) {
            apply_quotes(&mut collected, &quotes, today);
        }
    }
    let mut summary = finalize(collected, today)?;
    if let Some(cached) = &cached {
        let revision = Store::data_revision(conn)?;
        summary.valuation_batch = if cached.revision == revision {
            cached.batch.clone()
        } else {
            valuation_batch(&revision, cached.fetched_at, &summary)
        };
        summary.valuation_fetched_at = Some(cached.fetched_at);
        summary.valuation_stale = cached.fetched_at / 86400
            != crate::finance::now_secs().parse::<i64>().unwrap_or(0) / 86400;
    }
    Ok((summary, cached))
}
