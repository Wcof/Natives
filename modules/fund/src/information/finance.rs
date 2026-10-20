//! Public editorial finance columns; no portfolio identities are sent upstream.
use super::{
    adapters::{get_text_until, FetchError, RawItem, SRC_FINANCE},
    normalize,
};
use std::{collections::HashSet, time::Instant};

pub(super) fn fetch(deadline: Instant) -> Result<Vec<RawItem>, FetchError> {
    // Columns verified against the publisher's domestic/global/market/industry pages.
    let url = "https://np-listapi.eastmoney.com/comm/web/getNewsByColumns?client=web&biz=web_news_col&column=350,351,353,348&order=1&needInteractData=0&page_index=1&page_size=30&req_trace=natives&fields=code,showTime,title,mediaName,summary,url&types=1,20";
    parse(&get_text_until(
        url,
        Some("https://finance.eastmoney.com/"),
        deadline,
    )?)
}

fn parse(text: &str) -> Result<Vec<RawItem>, FetchError> {
    let data: serde_json::Value =
        serde_json::from_str(text).map_err(|_| FetchError::Parse("finance_json".into()))?;
    if data["code"] != "1" && data["code"] != 1 {
        return Err(FetchError::Parse("finance_status".into()));
    }
    let rows = data["data"]["list"]
        .as_array()
        .ok_or_else(|| FetchError::Parse("finance_list".into()))?;
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    for row in rows.iter().take(100) {
        let field = |key| row[key].as_str().unwrap_or("");
        let id = field("code");
        let title = normalize::strip_html(field("title"));
        if id.is_empty()
            || id.len() > 32
            || !id.bytes().all(|b| b.is_ascii_digit())
            || title.is_empty()
            || !seen.insert(id.to_owned())
        {
            continue;
        }
        let (published_at, precision) = normalize::parse_publish_time(field("showTime"));
        // Unknown dates cannot stand in for current public news.
        if published_at.is_none() {
            continue;
        }
        items.push(RawItem {
            source_id: SRC_FINANCE,
            external_id: id.to_owned(),
            importance: importance(&title),
            title: title.chars().take(256).collect(),
            url: format!("https://finance.eastmoney.com/a/{id}.html"),
            summary: normalize::strip_html(field("summary"))
                .chars()
                .take(1200)
                .collect(),
            codes: vec![],
            published_at,
            precision,
            event_types: vec![],
            event_date: None,
        });
        if items.len() == 30 {
            break;
        }
    }
    Ok(items)
}

// Editorial topic priority, not a prediction or a personal investment recommendation.
fn importance(title: &str) -> i32 {
    if [
        "央行",
        "美联储",
        "利率",
        "降息",
        "加息",
        "降准",
        "通胀",
        "CPI",
        "PPI",
        "GDP",
        "PMI",
        "财政",
        "证监会",
        "货币政策",
        "经济数据",
        "经济展望",
        "就业数据",
        "关税",
    ]
    .iter()
    .any(|word| title.contains(word))
    {
        2
    } else if [
        "全球市场",
        "A股",
        "美股",
        "港股",
        "债券",
        "国债",
        "汇率",
        "人民币",
        "黄金",
        "原油",
        "欧佩克",
        "ETF",
        "财报",
        "半导体",
    ]
    .iter()
    .any(|word| title.contains(word))
    {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finance_feed_is_public_bounded_deduplicated_and_dated() {
        let row = serde_json::json!({"code":"202610141234567890","title":"<b>央行利率政策</b>","showTime":"2026-10-14 08:00:00","summary":"<p>公开新闻摘要</p>","url":"javascript:bad"});
        let data = serde_json::json!({"code":"1","data":{"list":[row.clone(),row,{"code":"bad","title":"bad"}]}});
        let items = parse(&data.to_string()).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "央行利率政策");
        assert_eq!(
            items[0].published_at.as_deref(),
            Some("2026-10-14T00:00:00Z")
        );
        assert!(items[0].codes.is_empty());
        assert_eq!(items[0].importance, 2);
        assert_eq!(importance("全球市场：美股收盘"), 1);
        assert_eq!(importance("重磅独家！地方园区活动"), 0);
        assert!(items[0].url.starts_with("https://finance.eastmoney.com/a/"));
        assert!(parse(r#"{"code":0,"data":null}"#).is_err());
        assert!(parse(r#"{"code":"1","data":{}}"#).is_err());
        assert!(parse(r#"{"code":"1","data":{"list":[]}}"#)
            .unwrap()
            .is_empty());
    }
}
