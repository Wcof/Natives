//! 标的搜索联想（smartbox）（拆分自 market.rs，仅移动，无逻辑变更）。

use super::quotes::agent;
use super::util::{json_string, query_param, urlencode, url_decode, upstream_err};
use app_runtime_core::http::HttpRequest;

/// 规范化股票/基金标的代码（补齐 sh/sz/bj 前缀）。
pub fn normalize_symbol(raw: &str) -> String {
    let s = raw.trim().to_lowercase();
    if s.starts_with("sh") || s.starts_with("sz") || s.starts_with("bj") || s.starts_with("us") {
        return s;
    }
    if s.len() == 6 && s.bytes().all(|b| b.is_ascii_digit()) {
        if s == "000001" {
            // 特殊处理 000001：默认为上证指数 sh000001
            return format!("sh{s}");
        }
        if s.starts_with("60")
            || s.starts_with("68")
            || s.starts_with("51")
            || s.starts_with("56")
            || s.starts_with("58")
        {
            return format!("sh{s}");
        }
        if s.starts_with("00")
            || s.starts_with("30")
            || s.starts_with("15")
            || s.starts_with("16")
            || s.starts_with("39")
        {
            return format!("sz{s}");
        }
        if s.starts_with('8') || s.starts_with('4') || s.starts_with("92") {
            return format!("bj{s}");
        }
    }
    s
}

/// GET /api/market/suggest?input=600519|贵州茅台|gzmt — 标的搜索联想。
/// 上游：腾讯 smartbox（代码/中文名/拼音均可命中）。响应为 GBK，但中文名
/// 以 `\uXXXX` 转义输出，ASCII 解析安全；字段 `类型~代码~名称~拼音~标签`，
/// 类型前缀即市场归属（sh/sz/bj/jj=场外基金 等）。不使用东财 suggest——
/// 东财域在 rustls 下不可用（不发送 TLS close_notify，见文件头注释）。
/// fail-closed：上游失败返回 APP_UPSTREAM，绝不编造联想结果。
pub fn api_market_suggest(request: &HttpRequest) -> Result<String, (u16, String)> {
    let raw = query_param(&request.path, "input").ok_or((
        400,
        "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"input is required\"}".into(),
    ))?;
    let input = url_decode(&raw).trim().to_string();
    if input.is_empty() || input.chars().count() > 40 {
        return Err((
            400,
            "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"invalid input\"}".into(),
        ));
    }
    let url = format!(
        "https://smartbox.gtimg.cn/s3/?v=2&q={}&t=all",
        urlencode(&input)
    );
    let text = agent()
        .get(&url)
        .call()
        .map_err(|e| upstream_err(format!("suggest: {e}")))?
        .into_string()
        .map_err(|e| upstream_err(format!("suggest read: {e}")))?;
    let items = parse_smartbox(&text);
    Ok(format!("{{\"suggestions\":[{}]}}", items.join(",")))
}

/// 解析 smartbox 响应：`v_hint="sh~600519~\u8d35...~gzmt~GP-A^sz000001~..."`，
/// 多条以 `^` 分隔，字段以 `~` 分隔；名称为 `\uXXXX` 转义。
pub(crate) fn parse_smartbox(text: &str) -> Vec<String> {
    let payload = match (text.find('"'), text.rfind('"')) {
        (Some(start), Some(end)) if end > start => &text[start + 1..end],
        _ => return Vec::new(),
    };
    let mut items = Vec::new();
    for entry in payload.split('^') {
        let fields: Vec<&str> = entry.split('~').collect();
        if fields.len() < 4 {
            continue;
        }
        let (kind, code, name) = (fields[0].trim(), fields[1].trim(), fields[2].trim());
        let name = decode_js_unicode(name);
        if code.is_empty() || name.is_empty() {
            continue;
        }
        let symbol = match kind {
            "sh" | "sz" | "bj" => format!("{kind}{code}"),
            "jj" => code.to_string(), // 场外公募基金：裸代码
            "us" => format!("us{code}"),
            _ => continue, // 期货/外汇/未知类型不进联想白名单
        };
        items.push(format!(
            "{{\"symbol\":{},\"code\":{},\"name\":{},\"market\":{}}}",
            json_string(&symbol),
            json_string(code),
            json_string(&name),
            json_string(kind),
        ));
    }
    items
}

/// 还原 `\uXXXX` JS 转义为 UTF-8（smartbox 中文名形式）。
pub(crate) fn decode_js_unicode(s: &str) -> String {
    if !s.contains("\\u") {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('u') => {
                    let hex: String = chars.by_ref().take(4).collect();
                    if let Some(cp) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                        out.push(cp);
                    }
                }
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod smartbox_tests {
    use super::super::quotes::agent;
    use super::super::util::urlencode;
    use super::{api_market_suggest, parse_smartbox};
    use app_runtime_core::http::HttpRequest;

    #[test]
    fn parse_smartbox_stock_and_fund() {
        let raw = "v_hint=\"sh~600519~\\u8d35\\u5dde\\u8305\\u53f0~gzmt~GP-A^jj~005827~\\u6613\\u65b9\\u8fbe\\u84dd\\u7b79\\u7cbe\\u9009\\u6df7\\u5408~yfdlcjxhh~KJ^fu~AU2506~\\u6caa\\u91d1\"^^";
        let items = parse_smartbox(raw);
        assert_eq!(items.len(), 2);
        assert!(items[0].contains("\"symbol\":\"sh600519\""));
        assert!(items[0].contains("\"name\":\"贵州茅台\""));
        assert!(items[1].contains("\"symbol\":\"005827\""));
        assert!(items[1].contains("\"name\":\"易方达蓝筹精选混合\""));
    }

    #[test]
    fn parse_smartbox_empty_or_malformed() {
        assert!(parse_smartbox("").is_empty());
        assert!(parse_smartbox("v_hint=\"\"").is_empty());
        assert!(parse_smartbox("v_hint=\"sh~\"").is_empty());
    }

    /// 端到端（需真实网络，默认忽略）：验证 ureq/rustls 对 smartbox 的 TLS
    /// 可达性与解析——"添加自选不可用"的最可疑残余点。
    #[test]
    #[ignore = "requires network"]
    fn smartbox_reachable_via_rustls() {
        let text = agent()
            .get("https://smartbox.gtimg.cn/s3/?v=2&q=600519&t=all")
            .call()
            .expect("smartbox TLS call failed")
            .into_string()
            .expect("smartbox body read failed");
        let items = parse_smartbox(&text);
        assert!(!items.is_empty(), "no suggestions parsed from: {text}");
        assert!(items[0].contains("sh600519"));
    }

    /// 端到端（需真实网络，默认忽略）：走 api_market_suggest 完整入口
    /// （query 解码 → 上游 → 解析），覆盖中文 URL 编码输入。
    #[test]
    #[ignore = "requires network"]
    fn suggest_api_end_to_end() {
        let request = HttpRequest::for_internal(
            "GET",
            &format!("/api/market/suggest?input={}", urlencode("贵州茅台")),
            Vec::new(),
        );
        let body = api_market_suggest(&request).expect("suggest api failed");
        assert!(body.contains("sh600519"), "unexpected body: {body}");
        assert!(body.contains("贵州茅台"), "unexpected body: {body}");
    }
}
