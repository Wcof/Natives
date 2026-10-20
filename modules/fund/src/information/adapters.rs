//! 东财公开来源采集适配器（PanWatch M01/M02/M03 迁移）。
//!
//! 供体来源：
//! - M01 `packages/marketdata/src/marketdata/vendors/news.py`
//!   `EastmoneyStockNewsVendor`（search-api-web JSONP）与 `_parse_ann_item` 的公告列表。
//! - M02 `vendors/events.py` 的公告列表请求参数与分类词表（词表在 normalize.rs）。
//! - M03 `src/platform/marketdata/collectors/events_collector.py::fetch_announcement_fulltext`
//!   （np-cnotice-stock 正文 API）。
//!
//! 必须改写的供体缺陷（方案 04 §2/§8、08）：
//! - `verify=False` 不复刻：ureq 默认 rustls 证书与主机名校验，失败显式错误。
//! - 失败不返回空数组冒充成功：每来源区分 ok_with_items / ok_empty / 错误类别。
//! - `trust_env`/环境代理与无限重定向不复刻：固定 HTTPS 域、无代理、不跟随重定向。
//! - 响应有界读取（≤2 MiB），正文存储截断在 store 层（≤64 KiB → truncated）。
//! - 只发送公开证券代码/名称关键词，无 cookie/凭据。

use super::normalize::{self, TimePrecision};
use serde_json::Value;
use std::io::Read;
use std::time::{Duration, Instant};

/// 固定来源 ID（方案 04 §6：初期固定适配器，白名单）。
pub const SRC_FINANCE: &str = "em_finance";
pub const SRC_STOCK_NEWS: &str = "em_stock_news";
pub const SRC_STOCK_ANN: &str = "em_stock_ann";
pub const SRC_FUND_ANN: &str = "em_fund_ann";

/// 单响应解压后上限（方案 04 §7）。
const MAX_RESPONSE_BYTES: u64 = 2 * 1024 * 1024;
/// 每请求超时（方案 04 §7：≤8 秒）。
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

fn agent() -> &'static crate::network::Client {
    crate::network::client()
}

/// 网络错误类别（方案 04 §6 状态分类的来源侧入口）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// 网络失败（连接/超时/TLS/DNS）。
    Network(String),
    /// 限流（HTTP 429 或明确 Retry-After）。
    RateLimited,
    /// 被拦截（403/挑战页）。
    Blocked,
    /// 响应结构不符合已核验契约（字段变化）。
    Parse(String),
}

impl FetchError {
    pub fn code(&self) -> &'static str {
        match self {
            FetchError::Network(_) => "network_error",
            FetchError::RateLimited => "rate_limited",
            FetchError::Blocked => "blocked",
            FetchError::Parse(_) => "parse_error",
        }
    }

    pub fn detail(&self) -> String {
        match self {
            FetchError::Network(m) | FetchError::Parse(m) => m.clone(),
            FetchError::RateLimited => "rate limited".into(),
            FetchError::Blocked => "blocked by source".into(),
        }
    }
}

/// GET 有界读取（不跟随重定向；非 2xx 显式分类）。
fn get_text(url: &str, referer: Option<&str>) -> Result<String, FetchError> {
    get_text_until(url, referer, Instant::now() + REQUEST_TIMEOUT)
}
pub(super) fn get_text_until(
    url: &str,
    referer: Option<&str>,
    deadline: Instant,
) -> Result<String, FetchError> {
    let timeout = deadline
        .saturating_duration_since(Instant::now())
        .min(REQUEST_TIMEOUT);
    if timeout.is_zero() {
        return Err(FetchError::Network("deadline".into()));
    }
    let mut req = agent().get(url).timeout(timeout);
    if let Some(r) = referer {
        req = req.set("Referer", r);
    }
    let resp = req.call().map_err(|e| match e {
        crate::network::Error::Status(code, _) if code == 429 => FetchError::RateLimited,
        crate::network::Error::Status(403, _) => FetchError::Blocked,
        crate::network::Error::Status(code, _) => FetchError::Network(format!("HTTP {code}")),
        other => FetchError::Network(format!("{other}")),
    })?;
    let mut buf = Vec::new();
    resp.into_reader()
        .take(MAX_RESPONSE_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|e| FetchError::Network(format!("read: {e}")))?;
    if buf.len() as u64 > MAX_RESPONSE_BYTES {
        return Err(FetchError::Parse("response_too_large".into()));
    }
    String::from_utf8(buf).map_err(|e| FetchError::Parse(format!("utf8: {e}")))
}

fn parse_json(text: &str) -> Result<Value, FetchError> {
    serde_json::from_str(text).map_err(|e| FetchError::Parse(format!("json: {e}")))
}

/// 规范化后的单条上游条目；关联在调度层按身份建立。
#[derive(Debug, Clone)]
pub struct RawItem {
    pub source_id: &'static str,
    pub external_id: String,
    pub title: String,
    pub url: String,
    pub summary: String,
    /// 主关联证券代码（上游明确给出的代码；新闻为搜索关键词来源）。
    pub codes: Vec<String>,
    pub published_at: Option<String>,
    pub precision: TimePrecision,
    pub importance: i32,
    pub event_types: Vec<&'static str>,
    pub event_date: Option<String>,
}

/// A 股个股新闻（M01 EastmoneyStockNewsVendor：search-api-web JSONP）。
///
/// 供体经验：按名称搜索效果远好于代码；缺名时回退代码。结果是「内容提到该关键词」
/// 的文章，关联为 candidate 级（方案 04 §3：关键词匹配不作 direct）。
pub fn fetch_stock_news(keyword: &str) -> Result<Vec<RawItem>, FetchError> {
    fetch_stock_news_until(keyword, Instant::now() + REQUEST_TIMEOUT)
}
pub fn fetch_stock_news_until(
    keyword: &str,
    deadline: Instant,
) -> Result<Vec<RawItem>, FetchError> {
    let keyword = keyword.trim();
    if keyword.is_empty() {
        return Ok(vec![]);
    }
    let param = serde_json::json!({
        "uid": "", "keyword": keyword, "type": ["cmsArticleWebOld"], "client": "web",
        "clientType": "web", "clientVersion": "curr",
        "param": {"cmsArticleWebOld": {"searchScope": "default", "sort": "default",
                    "pageIndex": 1, "pageSize": 15, "preTag": "", "postTag": ""}}
    })
    .to_string();
    let url = format!(
        "https://search-api-web.eastmoney.com/search/jsonp?cb=jQuery&param={}",
        urlencode(&param)
    );
    let text = get_text_until(&url, Some("https://so.eastmoney.com/"), deadline)?;
    let inner = normalize::parse_jsonp(&text)
        .ok_or_else(|| FetchError::Parse("jsonp wrapper mismatch".into()))?;
    let data = parse_json(inner)?;
    if data["code"].as_i64() != Some(0) {
        return Err(FetchError::Parse(format!("code={}", data["code"])));
    }
    let items = data["result"]["cmsArticleWebOld"]
        .as_array()
        .ok_or_else(|| FetchError::Parse("missing cmsArticleWebOld".into()))?;
    let mut out = Vec::new();
    for item in items {
        let external_id = str_field(item, "code");
        let title = normalize::strip_html(&str_field(item, "title"));
        if external_id.is_empty() || title.is_empty() {
            continue;
        }
        let (published_at, precision) = normalize::parse_publish_time(&str_field(item, "date"));
        let article_url = {
            let u = str_field(item, "url");
            if u.is_empty() {
                format!(
                    "https://finance.eastmoney.com/a/{}.html",
                    item["code"].as_str().unwrap_or("")
                )
            } else {
                u
            }
        };
        let importance = normalize::news_importance(&title);
        out.push(RawItem {
            source_id: SRC_STOCK_NEWS,
            external_id,
            title,
            url: article_url,
            summary: normalize::strip_html(&str_field(item, "content")),
            codes: vec![keyword.to_string()],
            published_at,
            precision,
            importance,
            event_types: vec![],
            event_date: None,
        });
    }
    Ok(out)
}

/// A 股公告列表（M01/M02 ann API：`np-anotice-stock`）。
///
/// `codes[].stock_code` 是上游明确给出的证券身份 → direct 关联依据。
pub fn fetch_stock_announcements(stock_list: &[String]) -> Result<Vec<RawItem>, FetchError> {
    fetch_stock_announcements_until(stock_list, Instant::now() + REQUEST_TIMEOUT)
}
pub fn fetch_stock_announcements_until(
    stock_list: &[String],
    deadline: Instant,
) -> Result<Vec<RawItem>, FetchError> {
    let codes: Vec<&str> = stock_list
        .iter()
        .map(|s| s.trim())
        .filter(|c| c.len() == 6 && c.chars().all(|d| d.is_ascii_digit()))
        .collect();
    if codes.is_empty() {
        return Ok(vec![]);
    }
    let url = format!(
        "https://np-anotice-stock.eastmoney.com/api/security/ann?sr=-1&page_size=50&page_index=1&ann_type=A&stock_list={}&f_node=0&s_node=0",
        urlencode(&codes.join(","))
    );
    let text = get_text_until(&url, None, deadline)?;
    let data = parse_json(&text)?;
    if data["success"].as_i64() != Some(1) {
        return Err(FetchError::Parse(format!("success={}", data["success"])));
    }
    let items = data["data"]["list"]
        .as_array()
        .cloned()
        .ok_or_else(|| FetchError::Parse("missing announcement list".into()))?;
    let mut out = Vec::new();
    for item in &items {
        let external_id = str_field(item, "art_code");
        let title = str_field(item, "title").trim().to_string();
        if external_id.is_empty() || title.is_empty() {
            continue;
        }
        // 上游明确列出的证券（direct 关联身份）。
        let item_codes: Vec<String> = item["codes"]
            .as_array()
            .map(|cs| {
                cs.iter()
                    .filter_map(|c| c["stock_code"].as_str())
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        if item_codes.is_empty() {
            continue;
        }
        let columns: Vec<String> = item["columns"]
            .as_array()
            .map(|cs| {
                cs.iter()
                    .filter_map(|c| c["column_name"].as_str())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let (published_at, precision) =
            normalize::parse_publish_time(&str_field(item, "notice_date"));
        let detail_url = format!(
            "https://data.eastmoney.com/notices/detail/{}/{external_id}.html",
            item_codes[0]
        );
        let importance = normalize::announcement_importance(&title, &columns);
        let event_types = normalize::classify_event(&title, &columns);
        out.push(RawItem {
            source_id: SRC_STOCK_ANN,
            external_id,
            title,
            url: detail_url,
            summary: String::new(),
            codes: item_codes,
            published_at,
            precision,
            importance,
            event_types,
            // 上游结构化字段：display_time/eiTime 与 notice_date 均为发布侧信息；
            // 事件日期（股权登记/除息等）上游列表未给出，保持 None 不推算。
            event_date: None,
        });
    }
    Ok(out)
}

/// 基金/ETF 公告（覆盖矩阵实测：F10 JJGG 端点，需要对应网页 Referer）。
///
/// 股票公告端点对基金代码不覆盖（`.tmp/investment-panwatch-migration/samples/`
/// 探针证据），基金必须走本适配器；不能套用 `stock_list` 六位代码路径。
pub fn fetch_fund_announcements(fund_code: &str) -> Result<Vec<RawItem>, FetchError> {
    fetch_fund_announcements_until(fund_code, Instant::now() + REQUEST_TIMEOUT)
}
pub fn fetch_fund_announcements_until(
    fund_code: &str,
    deadline: Instant,
) -> Result<Vec<RawItem>, FetchError> {
    let code = fund_code.trim();
    if code.is_empty() {
        return Ok(vec![]);
    }
    let url = format!(
        "https://api.fund.eastmoney.com/f10/JJGG?fundcode={}&pageIndex=1&pageSize=20&type=3",
        urlencode(code)
    );
    let referer = format!("https://fundf10.eastmoney.com/jjgg_{code}.html");
    let text = get_text_until(&url, Some(&referer), deadline)?;
    let data = parse_json(&text)?;
    if data["ErrCode"].as_i64() != Some(0) {
        return Err(FetchError::Parse(format!("ErrCode={}", data["ErrCode"])));
    }
    let items = data["Data"]
        .as_array()
        .cloned()
        .ok_or_else(|| FetchError::Parse("missing fund announcement list".into()))?;
    let mut out = Vec::new();
    for item in &items {
        let title = str_field(item, "TITLE").trim().to_string();
        if title.is_empty() {
            continue;
        }
        let external_id = match item["ID"].as_i64() {
            Some(id) => id.to_string(),
            None => match item["ID"].as_str() {
                Some(s) if !s.is_empty() => s.to_string(),
                _ => continue,
            },
        };
        let (published_at, precision) =
            normalize::parse_publish_time(&str_field(item, "PUBLISHDATE"));
        let fund_url = format!("https://fundf10.eastmoney.com/jjgg_{code}.html");
        let importance = normalize::announcement_importance(&title, &[]);
        let event_types = normalize::classify_event(&title, &[]);
        out.push(RawItem {
            source_id: SRC_FUND_ANN,
            external_id,
            title,
            url: fund_url,
            summary: String::new(),
            codes: vec![code.to_string()],
            published_at,
            precision,
            importance,
            event_types,
            event_date: None,
        });
    }
    Ok(out)
}

/// 公告正文（M03：np-cnotice-stock content API）。
///
/// 供体 fail-soft 返回空串不复刻：失败显式错误类别；调用方不得用标题冒充全文。
/// 返回正文与完整度：完整正文直接 `full`；超过存储上限截断（`truncated`）。
pub fn fetch_announcement_content(art_code: &str) -> Result<(String, &'static str), FetchError> {
    if art_code.trim().is_empty() {
        return Err(FetchError::Parse("empty art_code".into()));
    }
    let url = format!(
        "https://np-cnotice-stock.eastmoney.com/api/content/ann?art_code={}&client_source=web&page_index=1",
        urlencode(art_code.trim())
    );
    let text = get_text(&url, None)?;
    let data = parse_json(&text)?;
    if data["success"].as_i64() != Some(1) || !data["data"].is_object() {
        return Err(FetchError::Parse("invalid content response".into()));
    }
    let content = data["data"]["notice_content"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();
    if content.is_empty() {
        // 上游确认无正文（如图表型公告）：显式 unavailable，不是错误也不伪装标题为全文。
        return Ok((String::new(), "unavailable"));
    }
    Ok((content, "full"))
}

fn str_field(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or("").to_string()
}

/// 受控 percent-encoding（复用 market::util 同族规则；只编码必要字符）。
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
