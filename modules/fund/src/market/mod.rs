//! 行情板块（首页「行情」）：大盘指数 / 主题板块 / 板块 ETF。
//!
//! 数据全部来自公开上游实时接口，禁止 mock。统一走腾讯行情：
//! - A 股指数 + 美股指数 + ETF 实时行情：qt.gtimg.cn 批量接口
//!   （东财 push2 在 rustls 下因服务器不发送 TLS close_notify 而不可用，
//!   已实测并放弃；腾讯接口本仓库 TLS 栈验证可用）
//! - 行业板块排行：proxy.finance.qq.com mktHs/rank
//!
//! 板块→ETF 的对应关系是客观标的映射（代码常量），行情值每次实时拉取；
//! 上游请求失败 fail-closed：返回 APP_UPSTREAM 错误，绝不编造数值。

mod cache;
#[cfg(test)]
pub(crate) use cache::put as store_public_response;
pub(crate) use cache::read_success as read_public_response;
mod handlers;
mod quotes;
mod sectors;
mod smartbox;
mod util;

pub use handlers::{
    api_market_detail, api_market_etfs, api_market_indices, api_market_kline, api_market_minute,
    api_market_quotes, api_market_sector_stocks, api_market_sectors, api_market_stocks,
    api_market_themes,
};
pub(crate) use handlers::{fetch_kline_data, fetch_minute_points, fetch_quote_batch};
pub use sectors::SectorItem;
pub(crate) use sectors::{
    fetch_live_sector_stocks, fetch_tencent_sectors_by_type, sina_sector_node,
};
pub use smartbox::{api_market_suggest, normalize_symbol};
pub(crate) use util::{query_param, url_decode, urlencode};

pub(crate) fn cached_public_response(
    key: String,
    ttl: std::time::Duration,
    fetch: impl FnOnce() -> Result<String, (u16, String)>,
) -> Result<String, (u16, String)> {
    cache::cached(key, ttl, fetch)
}

pub(crate) fn unadjusted_history(symbol: &str) -> Result<serde_json::Value, (u16, String)> {
    let raw = cache::cached(
        format!("kline:{symbol}:day:bf"),
        std::time::Duration::from_secs(30),
        || handlers::kline_fetch_upstream(symbol, "day", "bf"),
    )?;
    serde_json::from_str(&raw).map_err(|_| (502, "Invalid price history".into()))
}

/// Evidence reuses an already fetched day chart; it never starts network I/O under a DB lock.
pub(crate) fn cached_closed_trend(
    market: &str,
    code: &str,
    today: &str,
) -> Option<serde_json::Value> {
    let (value, bars) = cached_closed_bars(market, code, today)?;
    let result = crate::trends::modules::trend::analyze_trend(&bars);
    Some(
        serde_json::json!({"symbol":format!("security:{market}:{code}"),"source":value["source"].as_str().unwrap_or("web.ifzq.gtimg.cn"),"adjustment":"qfq","period":"closed-day","asOf":bars.last()?.date,"algorithmVersion":1,"barCount":bars.len(),"direction":result.direction,"strength":result.strength,"stage":result.stage,"maArrangement":result.ma_arrangement,"signals":result.signals}),
    )
}

pub(crate) fn cached_closed_bars(
    market: &str,
    code: &str,
    today: &str,
) -> Option<(serde_json::Value, Vec<crate::trends::KlineBar>)> {
    let prefix = match market {
        "SSE" => "sh",
        "SZSE" => "sz",
        "BSE" => "bj",
        _ => return None,
    };
    let raw = cache::read_success(&format!("kline:{prefix}{code}:day:qfq"))
        .or_else(|| cache::read_success(&format!("opportunity:{prefix}{code}")))?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let mut bars = Vec::new();
    for candle in value["candles"].as_array()? {
        let date = candle["time"].as_str()?;
        if !crate::assets::is_valid_calendar_date(date) {
            return None;
        }
        if date >= today {
            continue;
        }
        let bar = crate::trends::KlineBar {
            date: date.into(),
            open: candle["open"].as_f64()?,
            close: candle["close"].as_f64()?,
            high: candle["high"].as_f64()?,
            low: candle["low"].as_f64()?,
            volume: candle["volume"].as_f64()?,
            amount: 0.0,
            pct: 0.0,
            turnover: 0.0,
        };
        bars.push(bar);
    }
    bars.sort_by(|a, b| a.date.cmp(&b.date));
    bars.dedup_by(|a, b| a.date == b.date);
    if bars.len() < 60 {
        return None;
    }
    if bars.len() > 250 {
        bars.drain(..bars.len() - 250);
    }
    // Remote qfq history can contain negative old prices; validate the actual analysis window.
    if bars.iter().any(|bar| {
        ![bar.open, bar.close, bar.high, bar.low, bar.volume]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.0)
            || bar.high < bar.low
    }) {
        return None;
    }
    Some((value, bars))
}

pub(crate) fn fetch_opportunity_history(symbol: &str) -> Result<String, (u16, String)> {
    handlers::kline_fetch_upstream(symbol, "day", "qfq")
}

pub(crate) fn save_opportunity_history(symbol: &str, body: String) -> Result<(), &'static str> {
    let mut value: serde_json::Value =
        serde_json::from_str(&body).map_err(|_| "invalid_history")?;
    let bars = value["candles"].as_array_mut().ok_or("invalid_history")?;
    if bars.len() > 251 {
        bars.drain(..bars.len() - 251);
    }
    let count = bars.len();
    value["count"] = count.into();
    cache::put(
        format!("opportunity:{symbol}"),
        std::time::Duration::from_secs(15 * 60),
        Ok(value.to_string()),
    );
    Ok(())
}

#[cfg(test)]
mod tests;
