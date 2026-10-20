//! Public fund themes; recommendations are never treated as fund membership.
use super::NavError;
use serde_json::{json, Value};
use std::time::Duration;

fn valid_code(code: &str) -> bool {
    code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit())
}

fn parse_page(html: &str, code: &str) -> Result<Vec<Value>, NavError> {
    let title = html
        .split_once("<title>")
        .and_then(|(_, s)| s.split_once("</title>"))
        .map(|(s, _)| s);
    if !title.is_some_and(|s| s.contains(&format!("({code})"))) {
        return Err(NavError::BadPayload("基金主题页面身份不匹配".into()));
    }
    let Some((_, section)) = html.split_once("class=\"fund_item relatedThemeFund\"") else {
        return Ok(vec![]);
    };
    let Some((header, body)) = section.split_once("class=\"themeFund buyFundItemMain popTab\"")
    else {
        return Err(NavError::BadPayload("基金主题页面结构变化".into()));
    };
    if !header.contains("相关主题基金") {
        return Ok(vec![]);
    }
    let tabs = body
        .split_once("</ul>")
        .map(|(s, _)| s)
        .ok_or_else(|| NavError::BadPayload("基金主题列表缺失".into()))?;
    let mut result = vec![];
    for item in tabs.split("<li").skip(1).take(12) {
        let Some((_, id)) = item.split_once("data-id=") else {
            continue;
        };
        let id = id
            .trim_start_matches(['\'', '"'])
            .split(|c: char| c == '\'' || c == '"' || c == '>' || c.is_whitespace())
            .next()
            .unwrap_or("");
        let Some(name) = item
            .split_once("<span>")
            .and_then(|(_, s)| s.split_once("</span>"))
            .map(|(s, _)| s.trim())
        else {
            continue;
        };
        if !id.starts_with("BK")
            || id.len() > 16
            || !id[2..].bytes().all(|b| b.is_ascii_digit())
            || name.is_empty()
            || name.chars().count() > 40
            || name
                .chars()
                .any(|c| c.is_control() || c == '<' || c == '>' || c == '&')
        {
            return Err(NavError::BadPayload("基金主题字段无效".into()));
        }
        if !result.iter().any(|value: &Value| value["id"] == id) {
            result.push(json!({"id":id,"name":name}));
        }
    }
    Ok(result)
}

fn get(url: &str) -> Result<String, NavError> {
    crate::network::client()
        .get(url)
        .timeout(Duration::from_secs(5))
        .set("User-Agent", "Mozilla/5.0 natives-fund/1.0")
        .set("Referer", "https://fund.eastmoney.com/")
        .call()
        .map_err(|e| NavError::Source(e.to_string()))?
        .into_string()
        .map_err(|e| NavError::Source(e.to_string()))
}

pub(crate) fn fetch(code: &str) -> Result<String, NavError> {
    if !valid_code(code) {
        return Err(NavError::BadPayload("基金代码须为 6 位数字".into()));
    }
    crate::market::cached_public_response(format!("fund-classification:{code}"), Duration::from_secs(86400), || {
        let mut url = format!("https://fund.eastmoney.com/{code}.html");
        let themes = parse_page(&get(&url).map_err(|e| (502,e.message()))?, code).map_err(|e| (502,e.message()))?;
        let mut index_name = None;
        if themes.is_empty() {
            url = format!("https://fundmobapi.eastmoney.com/FundMNewApi/FundMNDetailInformation?FCODE={code}&deviceid=natives-fund&plat=Iphone&product=EFund&version=6.2.8");
            let profile = get(&url).map_err(|e|(502,e.message()))?;
            let profile: Value = serde_json::from_str(&profile).map_err(|_|(502,"基金资料响应无效".into()))?;
            if profile["ErrCode"] != 0 || profile["Datas"]["FCODE"] != code {
                return Err((502,"基金资料身份不匹配".into()));
            }
            index_name = profile["Datas"]["INDEXNAME"].as_str().filter(|s| !s.is_empty() && *s != "--" && s.chars().count() <= 80).map(str::to_string);
        }
        let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
        Ok(json!({"code":code,"status":if themes.is_empty()&&index_name.is_none(){"unclassified"}else{"ready"},"themes":themes,"indexName":index_name,"source":"eastmoney","sourceUrl":url,"fetchedAt":at}).to_string())
    }).map_err(|(_, message)| NavError::Source(message))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classification_requires_explicit_membership_and_matching_identity() {
        let page = "<title>基金(512480)</title><div class=\"fund_item relatedThemeFund\"><h3>相关主题基金</h3><div class=\"themeFund buyFundItemMain popTab\"><ul><li data-id=BK000054><span>半导体</span></li><li data-id=\"BK000388\"><span>TMT</span></li></ul><span>推荐广告</span>";
        assert_eq!(
            parse_page(page, "512480").unwrap(),
            vec![
                json!({"id":"BK000054","name":"半导体"}),
                json!({"id":"BK000388","name":"TMT"})
            ]
        );
        assert!(
            parse_page(&page.replace("相关主题基金", "热门主题基金"), "512480")
                .unwrap()
                .is_empty()
        );
        assert!(parse_page(page, "110022").is_err());
        assert!(parse_page(&page.replace("半导体", "<img src=x>"), "512480").is_err());
        assert!(fetch("../bad").is_err());
    }
}
