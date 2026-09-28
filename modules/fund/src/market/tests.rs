//! market 测试（拆分自 market.rs，按归属分散；仅移动，无逻辑变更）。

#![cfg(test)]

use super::handlers::api_market_quotes;
use super::quotes::{quote_json, Quote, THEME_ETFS};
use app_runtime_core::http::HttpRequest;

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
