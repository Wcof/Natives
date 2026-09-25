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
