//! 汇率刷新：只拉**实际用到的**币种，不维护全量汇率表。
//!
//! 双源兜底：
//! 1. 新浪外汇 `fx_s{base}cny`，实时，需 Referer 头
//! 2. open.er-api.com，日频，无需凭据
//!
//! 都失败时**不写库**，让 `overview` 继续把外币标为「未折算」——
//! 用 1:1 或过期汇率冒充精确值，比不显示更糟。
//!
//! USDT 没有直接报价源，按 USD 近似。差额通常在千分之几，
//! 且 `fx_rates.source` 会记成 `usd-proxy`，事后可追溯。

use crate::finance::{now_secs, FinanceError};
use crate::storage::Store;
use std::collections::BTreeSet;

const RATE_SCALE: f64 = 1_000_000.0;

#[derive(Debug, Default, serde::Serialize)]
pub struct FxRefreshResult {
    pub updated: usize,
    /// 成功刷新的币种对，如 `USD/CNY`。
    pub pairs: Vec<String>,
    pub errors: Vec<String>,
}

/// 用到的外币（排除本币 CNY）。
fn required_bases(store: &Store) -> Result<BTreeSet<String>, FinanceError> {
    store.with_read(|conn| {
        let mut out = BTreeSet::new();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT currency FROM instruments WHERE archived_at IS NULL
             UNION
             SELECT DISTINCT currency FROM accounts WHERE archived_at IS NULL",
        )?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for row in rows {
            let code = row?.trim().to_uppercase();
            if !code.is_empty() && code != "CNY" {
                out.insert(code);
            }
        }
        Ok(out)
    })
}

fn decode_gbk(bytes: &[u8]) -> String {
    let (text, _, _) = encoding_rs::GBK.decode(bytes);
    text.into_owned()
}

/// 新浪外汇：返回中间价（买卖均价的近似）。
fn fetch_sina(base: &str) -> Result<f64, String> {
    // USDT 按 USD 近似
    let quote_base = if base == "USDT" { "USD" } else { base };
    let url = format!(
        "https://hq.sinajs.cn/list=fx_s{}cny",
        quote_base.to_lowercase()
    );
    let response = crate::network::client()
        .get(&url)
        .set("Referer", "https://finance.sina.com.cn")
        .timeout(std::time::Duration::from_secs(8))
        .call()
        .map_err(|e| format!("sina fx: {e}"))?;
    let mut bytes = Vec::new();
    use std::io::Read;
    response
        .into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| format!("sina fx read: {e}"))?;
    if bytes.len() > 256 * 1024 {
        return Err("sina fx: response too large".into());
    }
    let text = decode_gbk(&bytes);
    let payload = text
        .split('"')
        .nth(1)
        .ok_or_else(|| "sina fx: empty payload".to_string())?;
    let fields: Vec<&str> = payload.split(',').collect();
    if fields.len() < 3 {
        return Err("sina fx: unexpected shape".into());
    }
    let bid: f64 = fields[1].trim().parse().map_err(|_| "sina fx: bad bid")?;
    let ask: f64 = fields[2].trim().parse().map_err(|_| "sina fx: bad ask")?;
    if bid <= 0.0 || ask <= 0.0 {
        return Err("sina fx: non-positive rate".into());
    }
    Ok((bid + ask) / 2.0)
}

/// er-api 兜底：以 USD 为基准一次性取全表，再换算到 CNY。
fn fetch_erapi(bases: &[String]) -> Result<Vec<(String, f64)>, String> {
    let response = crate::network::client()
        .get("https://open.er-api.com/v6/latest/USD")
        .timeout(std::time::Duration::from_secs(8))
        .call()
        .map_err(|e| format!("er-api: {e}"))?;
    let value: serde_json::Value = response
        .into_json()
        .map_err(|e| format!("er-api json: {e}"))?;
    let rates = value
        .get("rates")
        .and_then(|v| v.as_object())
        .ok_or_else(|| "er-api: missing rates".to_string())?;
    let cny = rates
        .get("CNY")
        .and_then(|v| v.as_f64())
        .ok_or_else(|| "er-api: missing CNY".to_string())?;
    let mut out = Vec::new();
    for base in bases {
        let quote_base = if base == "USDT" { "USD" } else { base.as_str() };
        if quote_base == "USD" {
            out.push((base.clone(), cny));
            continue;
        }
        if let Some(rate) = rates.get(quote_base).and_then(|v| v.as_f64()) {
            if rate > 0.0 {
                out.push((base.clone(), cny / rate));
            }
        }
    }
    if out.is_empty() {
        return Err("er-api: no usable pair".into());
    }
    Ok(out)
}

/// 刷新所有在用外币兑 CNY 的汇率。
pub fn refresh_all(store: &Store) -> Result<FxRefreshResult, FinanceError> {
    let bases = required_bases(store)?;
    let mut result = FxRefreshResult::default();
    if bases.is_empty() {
        return Ok(result);
    }

    let list: Vec<String> = bases.iter().cloned().collect();
    // 先逐币种走新浪；失败的汇总后交给 er-api 一次性兜底
    let mut resolved: Vec<(String, f64, &str)> = Vec::new();
    let mut pending: Vec<String> = Vec::new();
    for base in &list {
        match fetch_sina(base) {
            Ok(rate) => resolved.push((
                base.clone(),
                rate,
                if base == "USDT" {
                    "sina-usd-proxy"
                } else {
                    "sina"
                },
            )),
            Err(e) => {
                if result.errors.len() < 3 {
                    result.errors.push(format!("{base}: {e}"));
                }
                pending.push(base.clone());
            }
        }
    }
    if !pending.is_empty() {
        match fetch_erapi(&pending) {
            Ok(rows) => {
                for (base, rate) in rows {
                    // 先算 source，避免在 push 里同时借用已 move 的 base
                    let source = if base == "USDT" {
                        "er-api-usd-proxy"
                    } else {
                        "er-api"
                    };
                    resolved.push((base, rate, source));
                }
            }
            Err(e) => {
                if result.errors.len() < 3 {
                    result.errors.push(format!("er-api 兜底失败: {e}"));
                }
            }
        }
    }

    let today = crate::assets::today_iso();
    let ts = now_secs();
    let mut written = Vec::new();
    store.with_write(|conn| {
        for (base, rate, source) in &resolved {
            if *rate <= 0.0 {
                continue;
            }
            let raw = (rate * RATE_SCALE).round() as i64;
            conn.execute(
                "INSERT INTO fx_rates(base, quote, rate_raw, rate_date, source, fetched_at)
                 VALUES(?1,'CNY',?2,?3,?4,?5)
                 ON CONFLICT(base, quote, rate_date) DO UPDATE SET
                   rate_raw=excluded.rate_raw, source=excluded.source,
                   fetched_at=excluded.fetched_at",
                rusqlite::params![base, raw, today, source, ts],
            )?;
            written.push(format!("{base}/CNY"));
        }
        Ok::<(), FinanceError>(())
    })?;

    result.updated = written.len();
    result.pairs = written;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_bases_excludes_cny_and_dedupes() {
        let store = Store::open_in_memory().unwrap();
        store
            .with_write(|c| {
                c.execute(
                    "INSERT INTO accounts(name,channel,currency,created_at,updated_at)
                     VALUES('A','domestic_broker','CNY','0','0'),
                           ('B','overseas_broker','USD','0','0'),
                           ('C','overseas_broker','usd','0','0')",
                    [],
                )?;
                Ok::<(), rusqlite::Error>(())
            })
            .unwrap();
        let bases = required_bases(&store).unwrap();
        // CNY 排除；USD 与 usd 归一后只留一个
        assert_eq!(bases.into_iter().collect::<Vec<_>>(), vec!["USD"]);
    }

    /// 手动验证上游：`cargo test -p fund-module live_fx -- --ignored`
    #[test]
    #[ignore = "需要网络：手动验证汇率上游"]
    fn live_fx_sources_return_positive_rates() {
        let usd = fetch_sina("USD").expect("新浪 USD 应可达");
        assert!(usd > 1.0 && usd < 20.0, "USD/CNY 应在合理区间：{usd}");
        let hkd = fetch_sina("HKD").expect("新浪 HKD 应可达");
        assert!(hkd > 0.1 && hkd < 3.0, "HKD/CNY 应在合理区间：{hkd}");
        let rows = fetch_erapi(&["USD".to_string(), "HKD".to_string()]).expect("er-api 应可达");
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|(_, r)| *r > 0.0));
    }
}
