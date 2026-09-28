//! 缠论趋势分析（A1 决策「趋势引擎 Rust 移植」的 P0 阶段）。
//!
//! 1:1 移植自 `engine/analysis/chanlun_daily.py` 与 `engine/analysis/chanlun_minute.py`，
//! 含 JSON 序列化契约（`daily_result_to_dict` / `signals_to_dict`）。算法语义以
//! Python 输出为真值，由 `testdata/` 冻结 fixture + 黄金 JSON 的 A/B 深度相等
//! 测试逐字节校验（`tests.rs`）。
//!
//! 移植铁律（与 P0 范围一并生效）：
//! - 两个 MACD 实现不可混用：日线 `calc_daily_macd`（EMA12/26 在索引 12/26 前恒为
//!   SMA 值、DEA=SMA(9) 种子且递推式写法不同）与分钟 `_ema_sma_seed`（`x*k+prev*(1-k)`
//!   写法）浮点运算顺序不同，位模式不可互换，各自独立实现，禁止抽共享；
//! - 数值精度：价格 round 2 位、分钟 macd_area round 4 位、分钟 macd_bar round 6 位、
//!   confidence 为整数；round 采用 Python 语义（对二进制精确值做十进制半偶舍入）；
//! - 信号输出顺序：日线 = 一类买卖点按日期交错 + 二买 + 二卖 + 三卖 + 三买；
//!   分钟只有一类买卖点（按笔序）；
//! - `sum()` 作用在空切片上返回 int 0（分钟跨交易日时间标签重复时可触发），
//!   序列化时输出整数 0 而非 0.0（见 [`macd_area_value`]）。
//!
//! P0 不接线：`/api/trends/*` 仍由 `api/mod.rs` 反代 8795，路由接入属 P3。

pub mod chanlun_daily;
pub mod chanlun_minute;
pub mod engine;
pub mod indicators;
pub mod modules;
pub mod postprocess;
pub mod serialize;

#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

/// 归一化单根 K 线对象（对应 Python `Kline`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KlineBar {
    pub date: String,
    pub open: f64,
    pub close: f64,
    pub high: f64,
    pub low: f64,
    pub volume: f64,
    #[serde(default)]
    pub amount: f64,
    #[serde(default)]
    pub pct: f64,
    #[serde(default)]
    pub turnover: f64,
}

/// 实时盘口数据输入（对应 Python `Quote`）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QuoteInput {
    pub symbol: String,
    pub name: String,
    pub price: f64,
    pub pct: f64,
    pub change: f64,
    pub high: f64,
    pub low: f64,
    pub open: f64,
    pub pre_close: f64,
    pub volume: f64,
    pub amount: f64,
    pub turnover: f64,
    #[serde(default)]
    pub timestamp: String,
}

/// 资金流数据输入（对应 Python `FundFlow`）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FundFlowInput {
    pub date: String,
    pub main_net: f64,
    pub super_large_net: f64,
    pub large_net: f64,
    pub medium_net: f64,
    pub small_net: f64,
    #[serde(default)]
    pub main_pct: f64,
}

/// 日线/周线缠论分析的 K 线输入。
///
/// 六个序列与 engine `handle_chanlun_daily` 从 `fetch_kline` 展开的
/// `dates/opens/closes/highs/lows/volumes` 一一对应（等长）。
pub struct KlineInput {
    pub dates: Vec<String>,
    pub opens: Vec<f64>,
    pub closes: Vec<f64>,
    pub highs: Vec<f64>,
    pub lows: Vec<f64>,
    pub volumes: Vec<f64>,
}

impl KlineInput {
    /// 转换为 `Vec<KlineBar>`，自动补齐涨跌幅 `pct`（round 2 位）。
    pub fn to_kline_bars(&self) -> Vec<KlineBar> {
        let n = self.closes.len();
        let mut bars = Vec::with_capacity(n);
        for i in 0..n {
            let close = self.closes.get(i).copied().unwrap_or(0.0);
            let pct = if i > 0 && self.closes[i - 1] != 0.0 {
                py_round((close - self.closes[i - 1]) / self.closes[i - 1] * 100.0, 2)
            } else {
                0.0
            };
            bars.push(KlineBar {
                date: self.dates.get(i).cloned().unwrap_or_default(),
                open: self.opens.get(i).copied().unwrap_or(0.0),
                close,
                high: self.highs.get(i).copied().unwrap_or(0.0),
                low: self.lows.get(i).copied().unwrap_or(0.0),
                volume: self.volumes.get(i).copied().unwrap_or(0.0),
                amount: 0.0,
                pct,
                turnover: 0.0,
            });
        }
        bars
    }
}

/// Python `round(x, n)` 语义：对二进制精确值按十进制舍入（半偶舍入）后转回 f64。
///
/// CPython 通过 `round(x, n)` 对浮点数的精确二进制值做正确十进制舍入（2.675 → 2.67）。
/// Rust 的 `{:.n$}` 格式化同样是按二进制精确值展开十进制并半偶舍入，格式化再解析
/// 即与 Python 逐位一致。
pub(crate) fn py_round(x: f64, ndigits: usize) -> f64 {
    if !x.is_finite() {
        return x;
    }
    format!("{:.*}", ndigits, x).parse::<f64>().unwrap_or(x)
}

/// Python `round(x)` 语义：返回整数（半偶舍入，round(2.5)=2、round(3.5)=4）。
pub(crate) fn py_round_i(x: f64) -> i64 {
    if !x.is_finite() {
        return 0;
    }
    x.round_ties_even() as i64
}

/// 笔 MACD 面积的序列化值。
///
/// Python `sum()` 作用在空切片上返回 `int 0`（分钟数据跨交易日时间标签重复、
/// start→end 索引倒挂时出现），`round(int 0, n)` 仍是 `int`，JSON 输出 `0`；
/// 其余情况为浮点，按 `digits` 位舍入。
pub(crate) fn macd_area_value(area: f64, int_zero: bool, digits: usize) -> serde_json::Value {
    if int_zero {
        serde_json::Value::Number(serde_json::Number::from(0i64))
    } else {
        serde_json::json!(py_round(area, digits))
    }
}

/// 处理 `/api/trends/*` 路由请求（进程内纯 Rust 计算，替代原 Python 8795 伴生子进程）。
pub fn handle_trends_request(
    request: &app_runtime_core::http::HttpRequest,
    route: &str,
) -> Result<String, (u16, String)> {
    let subpath = route.strip_prefix("/api/trends").unwrap_or(route);
    let subpath = if subpath.is_empty() { "/" } else { subpath };

    match subpath {
        "/health" => Ok("{\"status\":\"ok\"}".into()),
        "/quote" => crate::market::api_market_detail(request),
        "/kline" => crate::market::api_market_kline(request),
        "/minute" => crate::market::api_market_minute(request),
        "/search" => crate::market::api_market_suggest(request),
        "/realtime_flow" => {
            let symbol = crate::market::url_decode(
                &crate::market::query_param(&request.path, "symbol")
                    .unwrap_or_default(),
            );
            Ok(format!(
                "{{\"symbol\":{},\"flows\":[],\"summary\":{{}},\"time_range\":\"\"}}",
                serde_json::json!(symbol)
            ))
        }
        "/scan" => {
            Ok("{\"status\":\"idle\",\"stage\":\"未运行\",\"progress\":0,\"total\":0,\"scanned\":0,\"found\":0,\"results\":[],\"error\":null,\"elapsed\":0}".into())
        }
        "/chanlun_daily" => {
            let raw_symbol = crate::market::query_param(&request.path, "symbol")
                .ok_or_else(|| (400, "{\"error\":\"缺少symbol参数\"}".into()))?;
            let symbol = crate::market::url_decode(&raw_symbol);
            let period = crate::market::query_param(&request.path, "period")
                .unwrap_or_else(|| "day".to_string());
            let count = crate::market::query_param(&request.path, "count")
                .and_then(|c| c.parse::<usize>().ok())
                .unwrap_or(250);

            let mut bars = crate::market::fetch_kline_bars(&symbol, &period, "qfq")?;
            if bars.len() < 10 {
                return Ok(format!("{{\"error\":\"K线数据不足（仅{}根）\"}}", bars.len()));
            }
            if bars.len() > count {
                let start = bars.len() - count;
                bars = bars[start..].to_vec();
            }

            let input = KlineInput {
                dates: bars.iter().map(|b| b.date.clone()).collect(),
                opens: bars.iter().map(|b| b.open).collect(),
                closes: bars.iter().map(|b| b.close).collect(),
                highs: bars.iter().map(|b| b.high).collect(),
                lows: bars.iter().map(|b| b.low).collect(),
                volumes: bars.iter().map(|b| b.volume).collect(),
            };
            let result = chanlun_daily::analyze_chanlun_daily(&input);
            let dict = chanlun_daily::daily_result_to_dict(&result);
            Ok(dict.to_string())
        }
        "/chanlun_minute" => {
            let raw_symbol = crate::market::query_param(&request.path, "symbol")
                .ok_or_else(|| (400, "{\"error\":\"缺少symbol参数\"}".into()))?;
            let symbol = crate::market::url_decode(&raw_symbol);
            let norm = crate::market::normalize_symbol(&symbol);
            let (times, prices, volumes) = crate::market::fetch_minute_points(&norm)?;
            if prices.is_empty() {
                return Ok("{\"error\":\"获取分时数据失败\"}".into());
            }
            let result = chanlun_minute::analyze_chanlun_minute(&times, &prices, &volumes);
            let dict = chanlun_minute::signals_to_dict(&result);
            Ok(dict.to_string())
        }
        "/analyze" => {
            let raw_symbol = crate::market::query_param(&request.path, "symbol")
                .ok_or_else(|| (400, "{\"error\":\"缺少symbol参数\"}".into()))?;
            let symbol = crate::market::url_decode(&raw_symbol);
            let period = crate::market::query_param(&request.path, "period")
                .unwrap_or_else(|| "day".to_string());
            let norm = crate::market::normalize_symbol(&symbol);

            let mut bars = crate::market::fetch_kline_bars(&symbol, &period, "qfq")?;
            if bars.len() < 10 {
                return Ok(format!("{{\"error\":\"K线数据不足（仅{}根）\"}}", bars.len()));
            }
            if bars.len() > 250 {
                let start = bars.len() - 250;
                bars = bars[start..].to_vec();
            }

            let quote = match crate::market::api_market_detail(request) {
                Ok(json_str) => {
                    serde_json::from_str::<serde_json::Value>(&json_str).ok().map(|v| {
                        QuoteInput {
                            symbol: norm.clone(),
                            name: v["name"].as_str().unwrap_or("").to_string(),
                            price: v["price"].as_f64().unwrap_or(0.0),
                            pct: v["changePct"].as_f64().unwrap_or(0.0),
                            change: v["change"].as_f64().unwrap_or(0.0),
                            high: v["high"].as_f64().unwrap_or(0.0),
                            low: v["low"].as_f64().unwrap_or(0.0),
                            open: v["open"].as_f64().unwrap_or(0.0),
                            pre_close: v["prevClose"].as_f64().unwrap_or(0.0),
                            volume: v["volume"].as_f64().unwrap_or(0.0),
                            amount: v["amount"].as_f64().unwrap_or(0.0),
                            turnover: v["turnover"].as_f64().unwrap_or(0.0),
                            timestamp: String::new(),
                        }
                    })
                }
                Err(_) => None,
            };

            let mut result = engine::run_analysis(&bars, quote.as_ref(), None, None);
            postprocess::apply_signal_optimization(&mut result, &bars, quote.as_ref());
            let signal_data = serialize::signal_to_dict(&result);

            let recent_klines_start = if bars.len() > 120 { bars.len() - 120 } else { 0 };
            let klines_val: Vec<serde_json::Value> = bars[recent_klines_start..]
                .iter()
                .map(serialize::kline_to_dict)
                .collect();

            let quote_val = quote.as_ref().map(serialize::quote_to_dict);
            let name = quote.as_ref().map(|q| q.name.clone()).unwrap_or_default();

            let resp = serde_json::json!({
                "symbol": symbol,
                "name": name,
                "quote": quote_val,
                "signal": signal_data,
                "klines": klines_val,
                "flows": [],
                "market_env": "",
                "breadth": null,
            });
            Ok(resp.to_string())
        }
        _ => Err((404, format!("{{\"error\":\"未知趋势接口: {}\"}}", subpath))),
    }
}
