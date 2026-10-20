//! T01/T09 实网公开来源探针（方案 04 §2）。
//!
//! 这些测试用真实公开来源验证 rustls（ureq）兼容性与字段结构，
//! `#[ignore]` 标注：默认 `cargo test` 不跑，验收时显式
//! `cargo test -p fund-module --test information_upstream_probe -- --ignored --nocapture`。
//! 只发送公开证券代码/名称，不带凭据；断言宽松（来源内容随时间变化）。

use std::io::Read;
use std::time::Duration;

const UA: &str = "natives-fund/1.0 (information probe)";

fn agent() -> &'static ureq::Agent {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .proxy(None)
            .max_redirects(0)
            .timeout_global(Some(Duration::from_secs(10)))
            .user_agent(UA)
            .build()
            .into()
    })
}

#[test]
#[ignore = "real network probe; run explicitly during acceptance"]
fn eastmoney_news_jsonp_over_rustls() {
    let param = serde_json::json!({
        "uid": "", "keyword": "贵州茅台", "type": ["cmsArticleWebOld"], "client": "web",
        "clientType": "web", "clientVersion": "curr",
        "param": {"cmsArticleWebOld": {"searchScope": "default", "sort": "default",
                    "pageIndex": 1, "pageSize": 15, "preTag": "", "postTag": ""}}
    })
    .to_string();
    let url = format!(
        "https://search-api-web.eastmoney.com/search/jsonp?cb=jQuery&param={}",
        urlencode(&param)
    );
    let text = agent()
        .get(&url)
        .call()
        .expect("news endpoint must be reachable over rustls")
        .into_body()
        .read_to_string()
        .expect("news body");
    assert!(text.starts_with("jQuery("), "JSONP wrapper expected");
    let payload = text
        .trim_start_matches("jQuery(")
        .trim_end_matches(')')
        .to_string();
    let data: serde_json::Value = serde_json::from_str(&payload).expect("JSONP payload is JSON");
    assert_eq!(data["code"], 0);
    let items = data["result"]["cmsArticleWebOld"]
        .as_array()
        .expect("cmsArticleWebOld array")
        .clone();
    assert!(!items.is_empty(), "keyword search should return articles");
    let first = &items[0];
    assert!(first["code"].as_str().is_some(), "external id (code)");
    assert!(first["title"].as_str().is_some(), "title");
    assert!(first["date"].as_str().is_some(), "publish date string");
    assert!(first["url"].as_str().is_some(), "article url");
}

#[test]
#[ignore = "real network probe; run explicitly during acceptance"]
fn eastmoney_announcement_list_over_rustls() {
    let url = "https://np-anotice-stock.eastmoney.com/api/security/ann?sr=-1&page_size=10&page_index=1&ann_type=A&stock_list=600519&f_node=0&s_node=0";
    let body = agent()
        .get(url)
        .call()
        .expect("announcement endpoint must be reachable over rustls")
        .into_body()
        .read_to_string()
        .expect("announcement body");
    let data: serde_json::Value = serde_json::from_str(&body).expect("JSON body");
    assert_eq!(data["success"], 1);
    let items = data["data"]["list"].as_array().expect("list").clone();
    assert!(!items.is_empty(), "600519 should have announcements");
    let first = &items[0];
    assert!(first["art_code"].as_str().is_some(), "art_code");
    assert!(first["title"].as_str().is_some(), "title");
    assert!(first["notice_date"].as_str().is_some(), "notice_date");
}

#[test]
#[ignore = "real network probe; run explicitly during acceptance"]
fn eastmoney_announcement_content_over_rustls() {
    // 先取列表拿有效 art_code（正文只接受已存在条目派生的 art_code）。
    let list_url = "https://np-anotice-stock.eastmoney.com/api/security/ann?sr=-1&page_size=5&page_index=1&ann_type=A&stock_list=600519&f_node=0&s_node=0";
    let body = agent()
        .get(list_url)
        .call()
        .expect("list endpoint")
        .into_body()
        .read_to_string()
        .expect("list body");
    let data: serde_json::Value = serde_json::from_str(&body).expect("JSON body");
    let art_code = data["data"]["list"][0]["art_code"]
        .as_str()
        .expect("art_code")
        .to_string();
    let content_url = format!(
        "https://np-cnotice-stock.eastmoney.com/api/content/ann?art_code={}&client_source=web&page_index=1",
        urlencode(&art_code)
    );
    let body = agent()
        .get(&content_url)
        .call()
        .expect("content endpoint must be reachable over rustls")
        .into_body()
        .read_to_string()
        .expect("content body");
    let data: serde_json::Value = serde_json::from_str(&body).expect("JSON body");
    let content = data["data"]["notice_content"]
        .as_str()
        .expect("notice_content");
    assert!(!content.trim().is_empty(), "content must be non-empty");
}

#[test]
#[ignore = "real network probe; run explicitly during acceptance"]
fn eastmoney_fund_announcements_over_rustls() {
    // 基金/ETF 公告走 F10 JJGG；需要网页 Referer（公开页面引用头，非凭据）。
    let url =
        "https://api.fund.eastmoney.com/f10/JJGG?fundcode=510300&pageIndex=1&pageSize=5&type=3";
    let body = agent()
        .get(url)
        .header("Referer", "https://fundf10.eastmoney.com/jjgg_510300.html")
        .call()
        .expect("fund announcement endpoint must be reachable over rustls")
        .into_body()
        .read_to_string()
        .expect("fund body");
    let data: serde_json::Value = serde_json::from_str(&body).expect("JSON body");
    assert_eq!(data["ErrCode"], 0);
    let items = data["Data"].as_array().expect("Data array").clone();
    assert!(!items.is_empty(), "510300 should have fund announcements");
    let first = &items[0];
    assert!(first["TITLE"].as_str().is_some(), "title");
    assert!(first["PUBLISHDATE"].as_str().is_some(), "publish date");
}

#[test]
#[ignore = "real network probe; run explicitly during acceptance"]
fn invalid_code_is_success_with_empty_list() {
    // 非法/未知代码不是错误：success=1 + 空列表（方案 04 §6 ok_empty）。
    let url = "https://np-anotice-stock.eastmoney.com/api/security/ann?sr=-1&page_size=5&page_index=1&ann_type=A&stock_list=000000&f_node=0&s_node=0";
    let body = agent()
        .get(url)
        .call()
        .expect("endpoint")
        .into_body()
        .read_to_string()
        .expect("body");
    let data: serde_json::Value = serde_json::from_str(&body).expect("JSON body");
    assert_eq!(data["success"], 1);
    assert!(data["data"]["list"].as_array().expect("list").is_empty());
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[allow(dead_code)]
fn _drain<R: Read>(_r: R) {}
