//! Program-owned news analysis policy, holdings projection and input budget.
//! Kept separate from the generic evidence assembler so the latter stays below
//! the module-size review threshold and the one-click contract remains testable.
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Program-owned analysis principles for the one-click news flow. These are
/// instructions, not user-authored strategies, and never enter E4.
pub(crate) const NEWS_ANALYSIS_PRINCIPLES: &[(&str, &str)] = &[
    (
        "relevance",
        "判断资讯直接影响具体持仓，还是仅属行业、政策或市场层面的间接影响；明确关联强弱。",
    ),
    (
        "holding-case",
        "判断资讯支持还是削弱原有持有依据，区分短期扰动与长期逻辑变化。",
    ),
    (
        "portfolio-risk",
        "结合实际可用持仓占比判断集中度、重复暴露和配置偏离；没有比例时不要编造。",
    ),
    (
        "event-response",
        "区分需要立即关注、等待进一步证据、暂不调整三类情况，允许明确建议暂不调整。",
    ),
    (
        "trend",
        "只有证据中存在可靠行情或净值趋势时才使用；没有数据不得编造技术信号。",
    ),
];

pub(crate) const NEWS_TEXT_LIMIT: usize = 6000;
pub(crate) const NEWS_SUMMARY_LIMIT: usize = 800;

/// Aggregate current security holdings without accounts or absolute amounts.
/// Missing valuations stay explicit; same security across accounts is merged.
pub(crate) fn news_holdings(summary: &crate::assets::AssetsSummary) -> (Vec<Value>, Value) {
    #[derive(Default)]
    struct Row {
        kind: String,
        market: String,
        code: String,
        name: String,
        value_minor: Option<i64>,
        as_of: Option<String>,
        stale: bool,
        missing: bool,
        risk_class: String,
    }
    let mut rows: BTreeMap<String, Row> = BTreeMap::new();
    let mut add = |key: String,
                   kind: &str,
                   market: &str,
                   code: &str,
                   name: &str,
                   value: Option<i64>,
                   as_of: Option<&str>,
                   stale: bool,
                   risk_class: &str| {
        let row = rows.entry(key).or_insert_with(|| Row {
            kind: kind.to_string(),
            market: market.to_string(),
            code: code.to_string(),
            name: name.to_string(),
            risk_class: risk_class.to_string(),
            ..Row::default()
        });
        row.value_minor = match (row.value_minor, value) {
            (Some(a), Some(b)) => Some(a.saturating_add(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
        if let Some(date) = as_of {
            if row.as_of.as_deref().is_none_or(|current| date > current) {
                row.as_of = Some(date.to_string());
            }
        }
        row.stale |= stale;
        row.missing |= value.is_none();
    };
    for fund in &summary.funds {
        if fund.quantity_raw <= 0 {
            continue;
        }
        let (kind, market, code) = crate::ledger::instrument_parts(&fund.fund_code).unwrap_or((
            fund.instrument_kind.as_str(),
            fund.market.as_str(),
            fund.fund_code.as_str(),
        ));
        add(
            format!("{kind}:{market}:{code}"),
            kind,
            market,
            code,
            &fund.fund_name,
            fund.value_minor,
            fund.price_as_of.as_deref().or(fund.nav_date.as_deref()),
            fund.stale,
            &fund.risk_class,
        );
    }
    for asset in &summary.manual {
        if asset.product_type == "cash" || asset.overlap_review || asset.legacy_overlap_distinct {
            continue;
        }
        add(
            format!("manual:{}", asset.instrument_key),
            "manual",
            "local",
            &asset.instrument_key,
            &asset.name,
            Some(asset.market_value_minor),
            Some(&asset.valued_at),
            asset.stale,
            &asset.risk_class,
        );
    }
    // Denominator is exactly the securities included below; cash and offline
    // assets cannot dilute the reported holding weights.
    let total: i64 = rows
        .values()
        .filter_map(|row| row.value_minor)
        .fold(0i64, |sum, value| sum.saturating_add(value));
    let mut holdings = Vec::with_capacity(rows.len());
    for row in rows.into_values() {
        let weight_ppm = row
            .value_minor
            .filter(|_| total > 0)
            .map(|value| ((value as i128 * 1_000_000) / total as i128) as i64);
        let quality = if row.missing || row.value_minor.is_none() {
            "missing_valuation"
        } else if row.stale {
            "stale"
        } else {
            "recorded"
        };
        holdings.push(json!({
            "kind": row.kind, "market": row.market, "code": row.code, "name": row.name,
            "riskClass": row.risk_class, "weightPpm": weight_ppm,
            "valueKnown": row.value_minor.is_some(), "asOf": row.as_of,
            "stale": row.stale, "dataQuality": quality,
        }));
    }
    holdings.sort_by(|a, b| {
        b["weightPpm"]
            .as_i64()
            .unwrap_or(-1)
            .cmp(&a["weightPpm"].as_i64().unwrap_or(-1))
            .then_with(|| {
                a["code"]
                    .as_str()
                    .unwrap_or("")
                    .cmp(b["code"].as_str().unwrap_or(""))
            })
    });
    let coverage = json!({
        "basis": "Current ledger positions, aggregated by security across accounts.",
        "weightBasis": "Known value of the included current security holdings; cash and offline assets are excluded from both numerator and denominator.",
        "holdingsCount": holdings.len(),
        "noHoldings": holdings.is_empty(),
        "excludes": ["account identity", "absolute amounts", "transactions", "personal notes"],
        "statement": if holdings.is_empty() {
            "No current security holdings are available; analyze the news without claiming personal exposure."
        } else {
            "Weights are portfolio-level percentages; unknown or stale valuations are explicit."
        },
    });
    (holdings, coverage)
}

pub(crate) fn take_chars(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

/// Keep the public 16000-character inference envelope without failing the
/// one-click flow. Lower-value evidence is removed first; selected news stays.
pub(crate) fn fit_news_budget(value: &mut Value) {
    let count = value.to_string().chars().count();
    let mut omitted: Vec<Value> = Vec::new();
    if count <= 16000 {
        value["budget"] =
            json!({"maxChars":16000,"sentChars":count,"truncated":false,"omitted":omitted});
        return;
    }
    let mut actions = 0;
    while value.to_string().chars().count() > 16000 && actions < 8 {
        let mut changed = false;
        for piece in value["evidence"].as_array_mut().into_iter().flatten() {
            if piece["id"] == "E5" && piece["quotes"].as_array().is_some_and(|q| !q.is_empty()) {
                let n = piece["quotes"].as_array().map_or(0, Vec::len);
                piece["quotes"] = json!([]);
                piece["closedTrend"] = Value::Null;
                piece["trendCoverage"] = json!(
                    "Omitted by the 16000-character input budget; no trend signal may be inferred."
                );
                omitted.push(
                    json!({"piece":"E5","field":"quotes","omittedCount":n,"reason":"input-budget"}),
                );
                changed = true;
                break;
            }
            if piece["id"] == "E3" && piece["events"].as_array().is_some_and(|e| !e.is_empty()) {
                let n = piece["events"].as_array().map_or(0, Vec::len);
                piece["events"] = json!([]);
                piece["coverageNotice"] = json!("Risk events omitted by the input budget.");
                omitted.push(
                    json!({"piece":"E3","field":"events","omittedCount":n,"reason":"input-budget"}),
                );
                changed = true;
                break;
            }
            if piece["id"] == "E1" {
                let mut n = 0;
                for field in ["financialGroups", "allocation"] {
                    if let Some(items) = piece[field]
                        .as_array_mut()
                        .filter(|items| !items.is_empty())
                    {
                        n += items.len();
                        *items = Vec::new();
                    }
                }
                if n > 0 {
                    piece["coverageNotice"] = json!("Portfolio aggregate detail omitted by the input budget; holdings and quality remain.");
                    omitted.push(json!({"piece":"E1","field":"allocation","omittedCount":n,"reason":"input-budget"}));
                    changed = true;
                    break;
                }
            }
        }
        if !changed {
            break;
        }
        actions += 1;
    }
    // If the envelope is still over budget, replace body text with its explicit
    // summary/title fallback rather than silently dropping the selected news.
    if value.to_string().chars().count() > 16000 {
        if let Some(items) = value["evidence"].as_array_mut() {
            for piece in items
                .iter_mut()
                .filter(|piece| piece["kind"] == "information")
            {
                if piece["contentSentChars"].as_u64().unwrap_or(0) > 0 {
                    piece["excerpt"] = json!("");
                    piece["contentSentChars"] = json!(0);
                    piece["fullTextRead"] = json!(false);
                    piece["contentTruncated"] = json!(true);
                    let has_summary = piece["summary"]
                        .as_str()
                        .is_some_and(|s| !s.trim().is_empty());
                    piece["basis"] = json!(if has_summary { "summary" } else { "title" });
                    piece["coverageNotice"] =
                        json!("正文因 16000 字符预算未发送；仅保留摘要/标题，模型未读全文。");
                    omitted.push(json!({"piece":piece["id"],"field":"excerpt","omittedCount":1,"reason":"input-budget"}));
                }
            }
        }
    }
    let sent = value.to_string().chars().count();
    value["budget"] = json!({"maxChars":16000,"sentChars":sent,"truncated":!omitted.is_empty(),"omitted":omitted});
}
