//! market 测试（拆分自 market.rs，按归属分散；仅移动，无逻辑变更）。

#![cfg(test)]

use super::handlers::api_market_quotes;
use super::quotes::{quote_json, Quote, THEME_ETFS};
use app_runtime_core::http::HttpRequest;

#[test]
fn theme_etfs_cover_required_themes() {
    for theme in [
        "PCB概念",
        "存储芯片",
        "半导体封装",
        "光学光电子",
        "CPU处理器",
    ] {
        assert!(
            THEME_ETFS
                .iter()
                .any(|(t, etfs)| **t == *theme && !etfs.is_empty()),
            "theme {theme} must map to real ETF symbols"
        );
    }
}

#[test]
fn quote_json_renders_null_for_missing() {
    let quotes = vec![Quote {
        as_of: None,
        code: "512880".into(),
        name: "证券ETF".into(),
        price: Some(1.054),
        change_pct: Some(0.67),
        change: None,
        volume: None,
        turnover: None,
        amplitude: None,
        volume_ratio: None,
    }];
    let body = quote_json(&quotes);
    assert!(body.contains("\"price\":1.05"));
    assert!(body.contains("\"change\":null"));
}

#[test]
fn market_quotes_rejects_bad_codes() {
    let request =
        HttpRequest::for_internal("GET", "/api/market/theme/etfs?codes=../etc", Vec::new());
    let result = api_market_quotes(&request);
    assert_eq!(result.unwrap_err().0, 400);
}

#[test]
fn test_kline_and_detail_live() {
    let req_kline = HttpRequest::for_internal(
        "GET",
        "/api/market/kline?symbol=sh600900&period=day",
        Vec::new(),
    );
    let res_kline = super::handlers::api_market_kline(&req_kline);
    if let Ok(json) = res_kline {
        assert!(json.contains("\"symbol\":\"sh600900\""));
        assert!(json.contains("\"candles\":["));
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        let count = val["count"].as_u64().unwrap();
        println!("test_kline_and_detail_live candles count: {count}");
        assert!(count >= 800, "Must contain at least 800 candles");
    }

    let req_detail =
        HttpRequest::for_internal("GET", "/api/market/detail?symbol=sh600900", Vec::new());
    let res_detail = super::handlers::api_market_detail(&req_detail);
    if let Ok(json) = res_detail {
        assert!(json.contains("\"marketCap\":"));
        assert!(json.contains("\"pe\":"));
        assert!(json.contains("\"pb\":"));
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        println!("test_detail marketCap: {}", val["marketCap"]);
        assert!(val["marketCap"].as_f64().is_some() || val["marketCap"].is_null());
    }
}

/// 详情契约（右侧 1:1 Dashboard 卡片流）：
/// - 股本位与市值位数学自洽：circShares×现价 ≈ circMarketCap、totalShares×现价 ≈
///   marketCap（此前 [72]/[73] 互换导致部分流通标的股本卡错位）；
/// - PE 动（[52]）与委比/委差键必须输出（详情指标卡「市盈率(PE动)」「委比%」
///   「委差(手)」此前分别显示 TTM 值与永远为 '-'）。
#[test]
fn detail_live_contract() {
    let req = HttpRequest::for_internal("GET", "/api/market/detail?symbol=sh601398", Vec::new());
    let json = super::handlers::api_market_detail(&req)
        .map_err(|(c, m)| format!("detail live 上游不可用({c}): {m}"))
        .unwrap();
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();

    let price = val["price"].as_f64().unwrap();
    let circ_shares = val["circShares"].as_f64().expect("circShares 数值");
    let total_shares = val["totalShares"].as_f64().expect("totalShares 数值");
    let circ_cap = val["circMarketCap"].as_f64().expect("circMarketCap 数值");
    let market_cap = val["marketCap"].as_f64().expect("marketCap 数值");
    assert!(
        total_shares > circ_shares,
        "工商银行 A 股流通必须小于总股本: circ={circ_shares} total={total_shares}"
    );
    let circ_expect = circ_shares / 1e8 * price;
    let total_expect = total_shares / 1e8 * price;
    assert!(
        (circ_expect - circ_cap).abs() / circ_cap < 0.02,
        "流通股本×现价 {circ_expect} 亿应 ≈ 流通市值 {circ_cap} 亿"
    );
    assert!(
        (total_expect - market_cap).abs() / market_cap < 0.02,
        "总股本×现价 {total_expect} 亿应 ≈ 总市值 {market_cap} 亿"
    );

    let pe_dyn = val["peDyn"].as_f64().expect("peDyn 键必须存在");
    assert!(pe_dyn > 0.0, "工商银行 PE 动应为正，实际 {pe_dyn}");
    let wei_bi = val["weiBi"].as_f64().expect("weiBi 键必须存在");
    let wei_cha = val["weiCha"].as_f64().expect("weiCha 键必须存在");
    assert!(
        (-100.0..=100.0).contains(&wei_bi),
        "委比% 应在 ±100 内：{wei_bi}"
    );
    println!("detail_live_contract: peDyn={pe_dyn} weiBi={wei_bi} weiCha={wei_cha} circ={circ_shares} total={total_shares}");
}

/// 分时契约（个股详情页）：响应必须携带上游权威的交易日 `date` 与昨收
/// `preClose`（详情页 y 轴基准/涨跌幅右轴锚定真实昨收，禁止回退开盘价），
/// 且 points 为当日分钟序列。
#[test]
fn minute_live_contract() {
    let req = HttpRequest::for_internal("GET", "/api/market/minute?symbol=sh600900", Vec::new());
    let res = super::handlers::api_market_minute(&req);
    let json = match res {
        Ok(json) => json,
        Err((code, msg)) => panic!("minute live 上游不可用({code}): {msg}"),
    };
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(val["symbol"], "sh600900");
    let date = val["date"].as_str().expect("date 必须为字符串");
    assert_eq!(date.len(), 8, "交易日格式应为 YYYYMMDD，实际 {date}");
    let pre_close = val["preClose"].as_f64().expect("preClose 必须为数值");
    assert!(pre_close > 0.0, "昨收必须为正数，实际 {pre_close}");
    let points = val["points"].as_array().expect("points 数组");
    assert!(!points.is_empty(), "分时点不应为空");
    let first = &points[0];
    assert!(first["time"].as_str().is_some(), "点必须含 time");
    assert!(first["price"].as_f64().is_some(), "点必须含 price");
    assert!(first["vwap"].as_f64().is_some(), "点必须含 vwap");
}
#[test]
fn cached_trend_uses_only_closed_bars_without_network() {
    let today = crate::assets::civil_from_days(19060);
    let candles=(0..=60).map(|i|serde_json::json!({"time":crate::assets::civil_from_days(19000+i),"open":10.0,"close":10.0,"high":11.0,"low":9.0,"volume":100.0})).collect::<Vec<_>>();
    super::cache::cached(
        "kline:sh600998:day:qfq",
        std::time::Duration::from_secs(30),
        || Ok(serde_json::json!({"candles":candles}).to_string()),
    )
    .unwrap();
    let trend = super::cached_closed_trend("SSE", "600998", &today).unwrap();
    assert_eq!(trend["barCount"], 60);
    assert_eq!(trend["asOf"], crate::assets::civil_from_days(19059));
    assert_eq!(trend["period"], "closed-day");
    assert!(super::cached_closed_trend("SZSE", "600998", &today).is_none());
}

#[test]
fn closed_analysis_window_accepts_old_qfq_negatives_but_rejects_bad_recent_data() {
    let today = crate::assets::civil_from_days(20252);
    let valid=(0..=252).map(|i|serde_json::json!({"time":crate::assets::civil_from_days(20000+i),"open":if i<2 {-10.0} else {10.0},"close":if i<2 {-10.0} else {10.0},"high":if i<2 {-9.0} else {11.0},"low":if i<2 {-11.0} else {9.0},"volume":100.0})).collect::<Vec<_>>();
    super::cache::cached(
        "kline:sh600996:day:qfq",
        std::time::Duration::from_secs(30),
        || Ok(serde_json::json!({"candles":valid}).to_string()),
    )
    .unwrap();
    let trend = super::cached_closed_trend("SSE", "600996", &today).unwrap();
    assert_eq!(trend["barCount"], 250);
    assert_eq!(trend["asOf"], crate::assets::civil_from_days(20251));
    let mut invalid = valid.clone();
    invalid[251]["high"] = serde_json::json!(8.0);
    super::cache::cached(
        "kline:sh600995:day:qfq",
        std::time::Duration::from_secs(30),
        || Ok(serde_json::json!({"candles":invalid}).to_string()),
    )
    .unwrap();
    assert!(super::cached_closed_trend("SSE", "600995", &today).is_none());
}

#[test]
fn refresh_analysis_cache_keeps_only_window_and_prefers_a_new_chart() {
    let today = crate::assets::civil_from_days(21600);
    let candles=(0..=600).map(|i|serde_json::json!({"time":crate::assets::civil_from_days(21000+i),"open":10.0,"close":10.0,"high":11.0,"low":9.0,"volume":100.0})).collect::<Vec<_>>();
    super::save_opportunity_history(
        "sh600993",
        serde_json::json!({"source":"isolated-analysis","candles":candles}).to_string(),
    )
    .unwrap();
    let raw = super::cache::read_success("opportunity:sh600993").unwrap();
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(v["candles"].as_array().unwrap().len(), 251);
    let trend = super::cached_closed_trend("SSE", "600993", &today).unwrap();
    assert_eq!(trend["barCount"], 250);
    assert_eq!(trend["source"], "isolated-analysis");
    super::cache::put(
        "kline:sh600993:day:qfq".into(),
        std::time::Duration::from_secs(30),
        Ok(serde_json::json!({"source":"isolated-new-chart","candles":candles}).to_string()),
    );
    assert_eq!(
        super::cached_closed_trend("SSE", "600993", &today).unwrap()["source"],
        "isolated-new-chart"
    );
    assert!(super::save_opportunity_history("sh600994", "not JSON".into()).is_err());
    assert!(super::cache::read_success("opportunity:sh600994").is_none());
}
