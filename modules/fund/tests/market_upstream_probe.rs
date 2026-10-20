//! R1/T04 · A23 港美行情能力探针（方案 11 R1.6）。
//!
//! 用真实公开上游验证港／美股 K 线端点的可达性与字段结构，为持仓能力矩阵
//! 提供证据。`#[ignore]` 标注：默认 `cargo test` 不跑，验收时显式
//! `cargo test -p fund-module --test market_upstream_probe -- --ignored --nocapture`。
//! 只发送公开证券代码，不带凭据；断言宽松（来源内容随时间变化）。
//!
//! 结论（2026-10-03 实测）：腾讯 ifzqgtimg 上游对 `hk`/`us` 前缀返回有效日线；
//! FX／汇率无公开端点，能力矩阵据实标注「不可用、不折算」。

use std::time::Duration;

fn agent() -> &'static ureq::Agent {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .proxy(None)
            .max_redirects(0)
            .timeout_global(Some(Duration::from_secs(10)))
            .user_agent("natives-fund/1.0 (market capability probe)")
            .build()
            .into()
    })
}

fn kline_bars(symbol: &str) -> Vec<(String, f64)> {
    let url = format!(
        "https://proxy.finance.qq.com/ifzqgtimg/appstock/app/newfqkline/get?param={symbol},day,,,20,qfq"
    );
    let mut response = agent()
        .get(&url)
        .call()
        .unwrap_or_else(|e| panic!("{symbol} kline endpoint must be reachable: {e}"));
    let text = response.body_mut().read_to_string().expect("kline body");
    let data: serde_json::Value = serde_json::from_str(&text).expect("kline payload is JSON");
    assert_eq!(data["code"], 0, "{symbol} business code must be 0");
    let rows = data["data"][symbol]["day"]
        .as_array()
        .unwrap_or_else(|| panic!("{symbol} day array expected"));
    let mut bars = Vec::new();
    for row in rows {
        let date = row[0].as_str().expect("date string").to_string();
        let close: f64 = row[2]
            .as_str()
            .or_else(|| row[2].as_f64().map(|_| ""))
            .map(|_| row[2].to_string().trim_matches('"').to_string())
            .and_then(|s| s.parse().ok())
            .or_else(|| row[2].as_f64())
            .expect("close price");
        assert!(close > 0.0, "{symbol} close must be positive");
        assert_eq!(date.len(), 10, "{symbol} date must be YYYY-MM-DD");
        bars.push((date, close));
    }
    bars
}

#[test]
#[ignore = "real network probe; run explicitly during acceptance"]
fn hk_kline_upstream_returns_valid_daily_bars() {
    let bars = kline_bars("hk00700");
    assert!(bars.len() >= 5, "hk00700 should return several daily bars");
    // 日期升序：HK 上游按交易日升序返回。
    for w in bars.windows(2) {
        assert!(w[0].0 < w[1].0, "hk00700 bars must be date ascending");
    }
}

#[test]
#[ignore = "real network probe; run explicitly during acceptance"]
fn us_kline_upstream_returns_valid_daily_bars() {
    let bars = kline_bars("usAAPL");
    assert!(bars.len() >= 2, "usAAPL should return daily bars");
    // 注意：US 上游返回顺序/窗口与 HK 不同（实测首行可能是历史补点），
    // 能力矩阵因此只声称「行情可达」，不声称升序与复权语义已验证。
    for (date, close) in &bars {
        assert_eq!(date.len(), 10);
        assert!(*close > 0.0);
    }
}
