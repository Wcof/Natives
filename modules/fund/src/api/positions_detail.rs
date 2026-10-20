//! 持仓页完整快照实现（自 positions.rs 拆分，控制单文件规模）：
//! 回放真值 + 身份/场馆 + 净值游标 + 待确认数 + 账户元数据与 dataRevision
//! （供编辑/转账 dialog 的 expected revision 使用），以及当日/期间收益展示指标。

use super::{api_error, json_str, query_param};
use crate::fixed::Fixed;
use crate::ledger;
use crate::portfolio;
use crate::storage::Store;
use rusqlite::{Connection, OptionalExtension};

/// 场外持仓展示指标的净值基线（全部来自本地 fund_nav，不临时外呼）。
#[derive(Default)]
struct OtcNavSeries {
    prev_raw: Option<i64>,
    growth_raw: Option<i64>,
    same_source: bool,
    week_raw: Option<i64>,
    month_raw: Option<i64>,
    year_raw: Option<i64>,
}

const NAV_VISIBLE: &str = "(unit_nav_raw > 0 OR (unit_nav_raw = 0 AND source = 'user_confirmed'))";

/// 兼容原快照调用的测试入口。旧扁平指标保留兼容；页面收益使用 income：
/// 按真实历史数量、买卖费用和期间资金流计算金额与投入基数收益率。
/// 场外使用可见净值，场内使用实际报价及未复权日线；缺失基线显式为 null。
#[cfg(test)]
pub(super) fn positions_detail_with_quotes(
    store: &Store,
    path: &str,
    fetch: impl FnMut(&[String]) -> Option<serde_json::Value>,
) -> Result<String, (u16, String)> {
    positions_detail_with_sources(store, path, fetch, |_| None)
}

pub(super) fn positions_detail_with_sources(
    store: &Store,
    path: &str,
    mut fetch: impl FnMut(&[String]) -> Option<serde_json::Value>,
    mut history: impl FnMut(&str) -> Option<serde_json::Value>,
) -> Result<String, (u16, String)> {
    let _budget = crate::network::limit_duration(std::time::Duration::from_secs(20));
    let include_archived = query_param(path, "includeArchived").as_deref() == Some("1");
    let today = crate::finance::today_cn();
    let today_days = crate::assets::days_from_civil(&today).unwrap_or(0);
    // 周一为一周起点（北京时间）；weekday: (days+4)%7 → 0=周日。
    let days_since_monday = (((today_days + 4) % 7) + 6) % 7;
    let week_start = crate::assets::civil_from_days(today_days - days_since_monday);
    let month_start = format!("{}-01", &today[..7]);
    let year_start = format!("{}-01-01", &today[..4]);
    let (accounts, rows, mut collected, revision, realized, otc_navs, income_books, mut income_prices, income_realized) = store
        .with_snapshot(|conn| -> Result<_, crate::assets::AssetError> {
            let (accounts, rows) = portfolio::holdings_detail(conn)?;
            let revision = Store::data_revision(conn)?;
            let collected = crate::assets::collect(conn, &today)?;
            let mut realized = Vec::new();
            let mut income_realized=std::collections::HashMap::new();
            for (account, fund, value) in crate::securities_income::realized_rows(conn).map_err(|e|crate::assets::AssetError::Invalid(e.message()))? {
                income_realized.insert((account,fund),value);
                let code: String = conn.query_row("SELECT code FROM funds WHERE id=?1",[fund],|r|r.get(0))?;
                let kind = ledger::instrument_parts(&code).map(|p|p.0).unwrap_or("fund");
                // DES-D01：历史已实现行补真实 instrumentKind/venue；旧 kind
                // 字段保持 stock/fund 兼容语义，不动旧 caller。
                let (ikind, _, _) = ledger::instrument_parts(&code).unwrap_or(("fund", "CN", ""));
                realized.push(serde_json::json!({"accountId":account,"kind":if kind=="stock"{"stock"}else{"fund"},"instrumentKind":ikind,"venue":if ikind=="fund"{"otc"}else{"exchange"},"value":value.map(|v|Fixed::amount(v).to_string_value())}));
            }
            // 场外净值序列：最新两行（当日涨跌）与周/月/年起点前最后净值。
            let mut otc_navs: std::collections::HashMap<i64, OtcNavSeries> =
                std::collections::HashMap::new();
            let mut income_books=std::collections::HashMap::new();
            let mut income_prices=std::collections::HashMap::new();
            for row in &rows {
                income_books.insert((row.account_id,row.fund_id),super::positions_income::load(conn,row.account_id,row.fund_id)?);
            }
            let otc_fund_ids: std::collections::BTreeSet<i64> = rows
                .iter()
                .filter_map(|h| {
                    let (kind, _, _) = ledger::instrument_parts(&h.fund_code).ok()?;
                    (kind == "fund").then_some(h.fund_id)
                })
                .collect();
            for fund_id in otc_fund_ids {
                income_prices.insert(fund_id,super::positions_income::nav_prices(conn,fund_id,&today)?);
                let mut series = OtcNavSeries::default();
                let mut stmt = conn.prepare(&format!(
                    "SELECT unit_nav_raw,source,daily_growth_raw FROM fund_nav WHERE fund_id = ?1 AND nav_date <= ?2 AND {NAV_VISIBLE} ORDER BY nav_date DESC LIMIT 2"
                ))?;
                let navs = stmt
                    .query_map(rusqlite::params![fund_id, today], |r| Ok((r.get::<_, i64>(0)?,r.get::<_,String>(1)?,r.get::<_,Option<i64>>(2)?)))?
                    .collect::<Result<Vec<_>, _>>()?;
                if navs.len() > 1 {
                    series.prev_raw = Some(navs[1].0);
                    series.same_source=navs[0].1==navs[1].1;
                }
                series.growth_raw=navs.first().and_then(|row|row.2);
                let baseline = |before: &str, conn: &Connection| -> rusqlite::Result<Option<i64>> {
                    conn.query_row(
                        &format!(
                            "SELECT unit_nav_raw FROM fund_nav WHERE fund_id = ?1 AND nav_date < ?2 AND {NAV_VISIBLE} ORDER BY nav_date DESC LIMIT 1"
                        ),
                        rusqlite::params![fund_id, before],
                        |r| r.get(0),
                    )
                    .optional()
                };
                series.week_raw = baseline(&week_start, conn)?;
                series.month_raw = baseline(&month_start, conn)?;
                series.year_raw = baseline(&year_start, conn)?;
                otc_navs.insert(fund_id, series);
            }
            Ok((accounts, rows, collected, revision, realized, otc_navs, income_books, income_prices, income_realized))
        })
        .map_err(|e| api_error(500, e.code(), &e.message()))?;
    if query_param(path, "refresh").as_deref() == Some("1") {
        crate::nav::warm_estimates(&collected.funds, &today);
    }
    let account_json: Vec<String> = accounts
        .iter()
        .filter(|a| include_archived || !a.archived)
        .map(|a| {
            format!(
                "{{\"id\":{},\"name\":{},\"version\":{},\"sortOrder\":{},\"archived\":{}}}",
                a.id,
                json_str(&a.name),
                a.version,
                a.sort_order,
                a.archived
            )
        })
        .collect();
    let active_ids: std::collections::HashSet<i64> = accounts
        .iter()
        .filter(|a| include_archived || !a.archived)
        .map(|a| a.id)
        .collect();
    // 网络查询在释放账本锁之后进行；实时估值是展示快照，不写回期初或现金。
    let mut quotes = serde_json::Map::new();
    let mut quote_inputs: std::collections::BTreeMap<String, crate::assets::QuoteInput> =
        std::collections::BTreeMap::new();
    let mut quote_failed = false;
    let fetched_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    if query_param(path, "refresh").as_deref() == Some("1") {
        // DES-D01：kind 扩展 etf/otc；旧 fund 保留「合并基金」语义（场内 ETF
        // 拉行情 + 场外不拉股票报价）。otc 只返回场外行，不调用股票报价（NAV
        // 走真实同步 owner）。未知值仍拒绝。
        let kind = query_param(path, "kind").unwrap_or_else(|| "stock".into());
        if !["stock", "fund", "all", "etf", "otc"].contains(&kind.as_str()) {
            return Err(api_error(400, "LEDGER_INVALID", "标的类型无效"));
        }
        let symbols: std::collections::BTreeSet<String> = rows
            .iter()
            .filter(|h| active_ids.contains(&h.account_id))
            .filter_map(|h| {
                let (k, m, c) = ledger::instrument_parts(&h.fund_code).ok()?;
                // "all" 不按类别过滤：股票与场内 ETF 行情一并刷新。
                // stock→kind=stock；etf→kind=etf；fund(旧)→非股票全部（含 etf）。
                let in_scope = match kind.as_str() {
                    "all" => true,
                    "stock" => k == "stock",
                    "etf" => k == "etf",
                    "fund" => k != "stock",
                    // otc 无交易所行情；NAV 由独立同步 owner 负责。
                    _ => false,
                };
                if !in_scope {
                    return None;
                }
                let prefix = match (k, m) {
                    ("stock" | "etf", "SSE") => "sh",
                    ("stock" | "etf", "SZSE") => "sz",
                    ("stock" | "etf", "BSE") => "bj",
                    _ => return None,
                };
                Some(format!("{prefix}{c}"))
            })
            .collect();
        let symbols: Vec<_> = symbols.into_iter().collect();
        // 单次最多十批；超限保留缺口，不假称完整覆盖。
        quote_failed = symbols.len() > 600;
        for batch in symbols.chunks(60).take(10) {
            match fetch(batch) {
                Some(value) => {
                    for q in value["quotes"].as_array().into_iter().flatten() {
                        let Some(code) = q["code"].as_str() else {
                            continue;
                        };
                        quotes.insert(code.into(), q.clone());
                        if let Some(price) =
                            q["price"].as_f64().filter(|p| p.is_finite() && *p >= 0.0)
                        {
                            if let Ok(raw) = Fixed::parse(&price.to_string(), 4) {
                                quote_inputs.insert(
                                    code.into(),
                                    crate::assets::QuoteInput {
                                        price_raw: raw.raw(),
                                        change_raw: q["change"]
                                            .as_f64()
                                            .and_then(|v| Fixed::parse(&v.to_string(), 4).ok())
                                            .map(|v| v.raw()),
                                        as_of: q["asOf"].as_str().map(String::from),
                                        source: "market-api".into(),
                                    },
                                );
                            }
                        }
                    }
                }
                None => quote_failed = true,
            }
        }
        // 未复权日线只在此显式刷新、锁外有界读取；缺历史不影响当前市值。
        for symbol in symbols.iter().take(10) {
            if let Some(series) = history(symbol) {
                let prices = super::positions_income::market_prices(&series, &today);
                for row in rows.iter().filter(|row| {
                    ledger::instrument_parts(&row.fund_code).is_ok_and(|(_, market, code)| {
                        let prefix = match market {
                            "SSE" => "sh",
                            "SZSE" => "sz",
                            "BSE" => "bj",
                            _ => "",
                        };
                        format!("{prefix}{code}") == *symbol
                    })
                }) {
                    income_prices.insert(row.fund_id, prices.clone());
                }
            }
        }
    }
    let cached_batch = if query_param(path, "refresh").as_deref() != Some("1") {
        let (inputs, batch) = store
            .with_read(|conn| {
                Ok::<_, rusqlite::Error>((
                    crate::valuation::cached_quotes(conn),
                    crate::valuation::read_valuation_cache(conn),
                ))
            })
            .map_err(|_| api_error(500, "LEDGER_DB", "Valuation cache read failed"))?;
        if let Some(inputs) = inputs {
            for (code, input) in &inputs {
                quotes.insert(code.clone(),serde_json::json!({"code":code,"price":input.price_raw as f64/10000.0,"asOf":input.as_of,"source":input.source}));
            }
            quote_inputs = inputs;
        }
        batch
    } else {
        None
    };
    // 当前报价只放入其真实业务日期，不能用查询日期伪造净值/行情日期。
    for row in &rows {
        let Ok((kind, market, code)) = ledger::instrument_parts(&row.fund_code) else {
            continue;
        };
        if kind == "fund" {
            continue;
        }
        let prefix = match market {
            "SSE" => "sh",
            "SZSE" => "sz",
            "BSE" => "bj",
            _ => "",
        };
        if let Some(q) = quote_inputs.get(&format!("{prefix}{code}")) {
            if let Some(date) = q
                .as_of
                .as_deref()
                .and_then(|d| d.get(..10))
                .filter(|d| crate::assets::is_valid_calendar_date(d) && *d <= today.as_str())
            {
                let prices = income_prices.entry(row.fund_id).or_default();
                prices.retain(|d, _| d.as_str() <= date);
                prices.insert(date.to_string(), q.price_raw);
            }
        }
    }
    let current_revision = store
        .with_read(Store::data_revision)
        .map_err(|_| api_error(500, "LEDGER_DB", "数据版本读取失败"))?;
    if current_revision != revision {
        return Err(api_error(409, "FIN_CONFLICT", "数据已变化，请刷新后重试"));
    }
    // N04 统一估值：与首页同一规则（场内数量×行情价、场外份额×净值）；
    // 同一估值批次身份写入共享缓存供空间卡等只读消费方使用。
    crate::assets::apply_quotes(&mut collected, &quote_inputs, &today);
    let mut summary = crate::assets::finalize(collected, &today)
        .map_err(|e| api_error(500, e.code(), &e.message()))?;
    let valuation_batch = if query_param(path, "refresh").as_deref() == Some("1") {
        let batch = crate::assets::valuation_batch(&revision, fetched_at, &summary);
        summary.valuation_batch = batch.clone();
        summary.valuation_fetched_at = Some(fetched_at);
        summary.valuation_stale = false;
        let _ = store.with_read(|conn| -> Result<(), rusqlite::Error> {
            crate::assets::store_valuation_cache(
                conn,
                &quote_inputs,
                fetched_at,
                &revision,
                &batch,
                quote_failed,
            );
            Ok(())
        });
        batch
    } else {
        cached_batch
            .map(|batch| {
                let id = if batch.revision == revision {
                    batch.batch
                } else {
                    crate::assets::valuation_batch(&revision, batch.fetched_at, &summary)
                };
                summary.valuation_batch = id.clone();
                summary.valuation_fetched_at = Some(batch.fetched_at);
                id
            })
            .unwrap_or_default()
    };
    let valuations = &summary.funds;
    let (income_revision, daily_facts) = store
        .with_snapshot(|conn| {
            let version = Store::data_revision(conn)?;
            let facts = summary
                .funds
                .iter()
                .map(|row| {
                    crate::securities_income::daily(conn, row, &today)
                        .map(|fact| ((row.account_id, row.fund_id), fact))
                })
                .collect::<Result<std::collections::HashMap<_, _>, crate::assets::AssetError>>()?;
            Ok::<_, crate::assets::AssetError>((version, facts))
        })
        .map_err(|e| api_error(500, e.code(), &e.message()))?;
    if income_revision != revision {
        return Err(api_error(409, "FIN_CONFLICT", "Refresh after data changed"));
    }

    // N09 场外在途/待确认聚合（单次分组查询；fund_orders 为用户操作记录，量级有界）。
    let otc_pending: std::collections::HashMap<(i64, i64), (i64, i64, i64)> = {
        let mut map = std::collections::HashMap::new();
        store
            .with_read(|conn| -> Result<(), rusqlite::Error> {
                let mut stmt = conn.prepare(
                    "SELECT account_id, fund_id,
                        COALESCE(SUM(CASE WHEN status='pending' AND op='buy' THEN amount_minor ELSE 0 END),0),
                        COALESCE(SUM(CASE WHEN status='pending' AND op IN ('sell','convert_out') THEN shares_raw ELSE 0 END),0),
                        COALESCE(SUM(CASE WHEN status='confirmed' AND cash_arrival IS NULL AND op IN ('sell','convert_out') THEN confirmed_amount_minor ELSE 0 END),0)
                     FROM fund_orders GROUP BY account_id, fund_id",
                )?;
                let rows = stmt.query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, i64>(3)?,
                        r.get::<_, i64>(4)?,
                    ))
                })?;
                for row in rows {
                    let (a, f, buy, sell_shares, transit) = row?;
                    map.insert((a, f), (buy, sell_shares, transit));
                }
                Ok(())
            })
            .unwrap_or(());
        map
    };
    let holding_json: Vec<String> = rows
        .iter()
        .filter(|h| active_ids.contains(&h.account_id))
        .map(|h| {
            let (kind, market, code) =
                ledger::instrument_parts(&h.fund_code).unwrap_or(("fund", "CN", h.fund_code.as_str()));
            // venue：场外基金（裸 6 位代码）为 otc；股票/场内 ETF 走交易所行情。
            let venue = if kind == "fund" { "otc" } else { "exchange" };
            let quote_symbol = match (kind, market) {
                ("stock" | "etf", "SSE") => Some(format!("sh{code}")),
                ("stock" | "etf", "SZSE") => Some(format!("sz{code}")),
                ("stock" | "etf", "BSE") => Some(format!("bj{code}")),
                _ => None,
            };
            let valuation = valuations.iter().find(|v| v.account_id == h.account_id && v.fund_id == h.fund_id);
            let nav = match (valuation.and_then(|v| v.nav_raw), valuation.and_then(|v| v.nav_date.as_ref())) {
                (Some(raw), Some(date)) => Some(format!(
                    "{{\"nav\":{},\"date\":{},\"dailyGrowthPct\":{}}}",
                    json_str(&Fixed::nav(raw).to_string_value()),
                    json_str(date),
                    otc_navs.get(&h.fund_id).and_then(|s|s.growth_raw).map(|raw|json_str(&Fixed::nav(raw).to_string_value())).unwrap_or_else(||"null".into())
                )),
                _ => None,
            };
            let average_cost = if h.cost_unknown { None } else {Fixed::amount(h.cost_raw).checked_div_to(&Fixed::share(h.quantity_raw),4)};
            let quote = quote_symbol.as_ref().and_then(|s| quotes.get(s));
            // N04：市值与质量来自统一估值行（与首页/空间卡同源同批次规则）。
            let value = valuation.and_then(|v| v.value_minor).map(Fixed::amount);
            let quality = match venue {
                "otc" => valuation
                    .map(|v| {
                        v.value_gap
                            .clone()
                            .unwrap_or_else(|| if v.stale { "stale".into() } else { "complete".into() })
                    })
                    .unwrap_or_else(|| "incomplete".into()),
                _ => valuation
                    .map(|v| match v.value_gap.as_deref() {
                        None if v.stale => "stale".to_string(),
                        None => "complete".to_string(),
                        Some("missing_quote") => "incomplete".to_string(),
                        Some(other) => other.to_string(),
                    })
                    .unwrap_or_else(|| "incomplete".into()),
            };
            // 当日/期间收益展示指标：份额 × 价格（净值）变动，全部现算。
            let pct_2dp = |now: i128, base: i128| -> f64 { ((now - base) as f64 / base as f64 * 10000.0).round() / 100.0 };
            let pnl_minor = |diff_raw: i128| -> Option<i64> { i64::try_from(h.quantity_raw as i128 * diff_raw / 1_000_000).ok() };
            let nav_now = valuation.and_then(|v| v.nav_raw);
            let series = if venue == "otc" { otc_navs.get(&h.fund_id) } else { None };
            let daily_fact = daily_facts.get(&(h.account_id,h.fund_id));
            let day_pnl = daily_fact.and_then(|f|f["amount"].as_str()).and_then(|v|Fixed::parse(v,2).ok()).map(|v|v.raw());
            let day_pct = if venue=="otc"&&valuation.and_then(|row|row.nav_date.as_deref())==Some(today.as_str()) {
                series.and_then(|s|s.growth_raw).map(|raw|raw as f64/10000.0).or_else(||nav_now.zip(series.filter(|s|s.same_source).and_then(|s|s.prev_raw)).filter(|(_,base)|*base>0).map(|(now,base)|pct_2dp(now as i128,base as i128)))
            }else{daily_fact.and_then(|f|f["rateBps"].as_str()).and_then(|v|v.parse::<i64>().ok()).map(|v|v as f64/100.0)};
            let period_pnl = |base: Option<i64>| -> Option<i64> {
                match (nav_now, base) {
                    (Some(now), Some(b)) => pnl_minor(now as i128 - b as i128),
                    _ => None,
                }
            };
            let week_pnl = period_pnl(series.and_then(|s| s.week_raw));
            let month_pnl = period_pnl(series.and_then(|s| s.month_raw));
            let year_pct = match (nav_now, series.and_then(|s| s.year_raw)) {
                (Some(now), Some(b)) if b > 0 => Some(pct_2dp(now as i128, b as i128)),
                _ => None,
            };
            let book=income_books.get(&(h.account_id,h.fund_id));
            let holding_days=book.and_then(|b|b.holding_days(&today));
            let mut income=book.map(|b| b.project(&today,income_prices.get(&h.fund_id).unwrap_or(&std::collections::BTreeMap::new()),
                value.map(|v|v.raw()),if h.cost_unknown {None}else{Some(h.cost_raw)},
                income_realized.get(&(h.account_id,h.fund_id)).copied().flatten())).unwrap_or(serde_json::Value::Null);
            if !matches!(income["day"]["reason"].as_str(),Some("calibration_boundary"|"record_limit")){if let Some(fact)=daily_fact { income["day"]=fact.clone(); }}
            let estimate=valuation.and_then(|row|crate::nav::cached_estimate(row,&today));
            if let (Some(row),Some(estimate))=(valuation,estimate.as_ref()){income["estimatedDay"]=crate::nav::estimated_income(row,estimate,&today);}
            let json_minor = |v: Option<i64>| -> String {
                v.map(|m| json_str(&Fixed::amount(m).to_string_value())).unwrap_or_else(|| "null".into())
            };
            let otc = otc_pending
                .get(&(h.account_id, h.fund_id))
                .copied()
                .unwrap_or((0, 0, 0));
            format!(
                "{{\"accountId\":{},\"fundId\":{},\"fundCode\":{},\"fundName\":{},\"instrumentKind\":{},\"market\":{},\"venue\":{},\"quoteSymbol\":{},\"quantity\":{},\"cost\":{},\"realized\":{},\"asOf\":{},\"costUnknown\":{},\"averageCost\":{},\"nav\":{},\"pendingCount\":{},\"marketValue\":{},\"valuationQuality\":{},\"quote\":{},\"dayPct\":{},\"dayPnl\":{},\"weekPnl\":{},\"monthPnl\":{},\"yearPct\":{},\"holdingDays\":{},\"otcPendingBuyMinor\":{},\"otcPendingSellShares\":{},\"otcInTransitMinor\":{}}}",
                h.account_id,
                h.fund_id,
                json_str(code),
                json_str(&h.fund_name),
                json_str(kind),
                json_str(market),
                json_str(venue),
                match &quote_symbol {
                    Some(s) => json_str(s),
                    None => "null".into(),
                },
                json_str(&Fixed::nav(h.quantity_raw).to_string_value()),
                json_str(&Fixed::amount(h.cost_raw).to_string_value()),
                json_str(&Fixed::amount(h.realized_raw).to_string_value()),
                json_str(&h.as_of),
                h.cost_unknown,
                average_cost.map(|v|json_str(&v.to_string_value())).unwrap_or_else(||"null".into()),
                nav.unwrap_or_else(|| "null".into()),
                h.pending_count,
                value.map(|v| json_str(&v.to_string_value())).unwrap_or_else(|| "null".into()),
                json_str(&quality),
                quote.map(|q| q.to_string()).unwrap_or_else(|| "null".into()),
                day_pct.map(|v| format!("{v:.2}")).unwrap_or_else(|| "null".into()),
                json_minor(day_pnl),
                json_minor(week_pnl),
                json_minor(month_pnl),
                year_pct.map(|v| format!("{v:.2}")).unwrap_or_else(|| "null".into()),
                holding_days.map(|d| d.to_string()).unwrap_or_else(|| "null".into()),
                otc.0,
                if otc.1 == 0 { "null".to_string() } else { Fixed::share(otc.1).to_string_value() },
                if otc.2 == 0 { "null".to_string() } else { Fixed::amount(otc.2).to_string_value() },
            )
            .trim_end_matches('}').to_string()+",\"income\":"+&income.to_string()+",\"estimate\":"+&estimate.unwrap_or(serde_json::Value::Null).to_string()+"}"
        })
        .collect();
    Ok(format!(
        "{{\"dataRevision\":{},\"valuationBatch\":{},\"accounts\":[{}],\"holdings\":[{}],\"quoteFailed\":{},\"realized\":{}}}",
        json_str(&revision),
        json_str(&valuation_batch),
        account_json.join(","),
        holding_json.join(","),
        quote_failed,
        serde_json::Value::Array(realized.into_iter().filter(|r|active_ids.contains(&r["accountId"].as_i64().unwrap_or(0))).collect())
    ))
}
