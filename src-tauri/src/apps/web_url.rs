//! Web 应用 URL 策略共享层（APPV2-T01）。
//!
//! 应用中心 Web 注册 / 编辑 / child WebView 导航过滤的**唯一** URL 策略来源
//! （此前注册校验与导航规则不一致：注册允许 http/https，导航只放行 http 前缀，
//! 导致已注册的 https 公网应用打开必失败）。
//!
//! 策略（FR-02）：
//! - [`normalize_web_url`]：trim；无 scheme 输入补 `https://`；补后必须通过校验。
//! - [`validate_web_url`]：仅 http/https；**公网必须 https**；显式 http 仅允许
//!   loopback（127.0.0.1 / localhost / ::1）；host 必填（公网 host 必须含点，
//!   拒绝 `notaurl` 这类明显非 URL 输入）。
//! - [`derive_origin`]：URL → host（默认 approved origin 推导，忽略 scheme/port）。
//! - [`navigation_within_origins`]：child WebView 导航过滤 —— http/https 均放行，
//!   host 必须落在 approved origins 内（origin 条目允许裸 host 或带 scheme 写法）。
//!
//! 安全边界不变：approved origins 是 remote trust domain 的唯一边界；本模块只修复
//! 规则不一致，不做任何放宽。child label 永不获得 main capability 的防线在
//! `creative_app::browser`，不在此处。

use crate::{Error, Result};

/// loopback host（与既有 `navigation_allowed` 的 loopback 口径一致，不扩展）。
pub fn is_loopback_host(host: &str) -> bool {
    let h = host.trim().to_ascii_lowercase();
    h == "127.0.0.1" || h == "localhost" || h == "::1"
}

/// 从 URL/origin 提取 host（忽略 scheme、userinfo、port、path；IPv6 取括号内）。
fn host_of(input: &str) -> Option<String> {
    let rest = input
        .trim()
        .split_once("://")
        .map(|(_, r)| r)
        .unwrap_or_else(|| input.trim());
    let authority = rest.split(['/', '?', '#']).next()?.trim();
    if authority.is_empty() {
        return None;
    }
    let authority = authority
        .rsplit_once('@')
        .map(|(_, h)| h)
        .unwrap_or(authority);
    let host = if let Some(bracketed) = authority.strip_prefix('[') {
        bracketed.split(']').next().unwrap_or(bracketed)
    } else {
        authority
            .rsplit_once(':')
            .map(|(h, _)| h)
            .unwrap_or(authority)
    };
    let host = host.trim().to_ascii_lowercase();
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

/// 规范化用户输入为可持久化 URL（无 scheme 补 https），并做策略校验。
pub fn normalize_web_url(input: &str) -> Result<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(Error::InvalidInput("web url must not be empty".into()));
    }
    let candidate = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };
    validate_web_url(&candidate)?;
    Ok(candidate)
}

/// 策略校验：http/https only；公网必须 https；显式 http 仅 loopback；host 必填。
pub fn validate_web_url(url: &str) -> Result<()> {
    let u = url.trim();
    let scheme = u
        .split_once("://")
        .map(|(s, _)| s)
        .ok_or_else(|| Error::InvalidInput(format!("web url must be http/https: {u}")))?;
    if scheme != "http" && scheme != "https" {
        return Err(Error::InvalidInput(format!(
            "web url must be http/https: {u}"
        )));
    }
    let host =
        host_of(u).ok_or_else(|| Error::InvalidInput(format!("web url missing host: {u}")))?;
    if !is_loopback_host(&host)
        && (!host.contains('.')
            || host.starts_with('.')
            || host.ends_with('.')
            || host.contains(".."))
    {
        return Err(Error::InvalidInput(format!(
            "web url host looks invalid: {u}"
        )));
    }
    if scheme == "http" && !is_loopback_host(&host) {
        return Err(Error::InvalidInput(format!(
            "public web url must use https: {u}"
        )));
    }
    Ok(())
}

/// 从 URL 推导默认 approved origin（host；approved origin 条目按 host 匹配）。
pub fn derive_origin(url: &str) -> Option<String> {
    host_of(url)
}

/// child WebView 导航过滤：http/https 且 host 在 approved origins 内。
///
/// approved origin 条目允许裸 host（`github.com` / `127.0.0.1`）或带 scheme 的
/// origin（`https://github.com`）。空 approved 集合 = 全部拒绝（fail closed）。
pub fn navigation_within_origins(url: &str, approved_origins: &[String]) -> bool {
    let u = url.trim();
    let scheme = u.split_once("://").map(|(s, _)| s);
    if !matches!(scheme, Some("http") | Some("https")) {
        return false;
    }
    let Some(host) = host_of(u) else {
        return false;
    };
    approved_origins.iter().any(|origin| {
        let o = origin.trim();
        if o.is_empty() {
            return false;
        }
        let approved = host_of(o).unwrap_or_else(|| o.to_ascii_lowercase());
        host == approved
            || host.ends_with(&format!(".{approved}"))
            || approved
                .strip_prefix("www.")
                .is_some_and(|root| host == root)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_schemeless_to_https() {
        assert_eq!(
            normalize_web_url("chatgpt.com").unwrap(),
            "https://chatgpt.com"
        );
        assert_eq!(
            normalize_web_url(" alidocs.dingtalk.com/i/spaces/demo ").unwrap(),
            "https://alidocs.dingtalk.com/i/spaces/demo"
        );
        // 带 scheme 的输入原样保留（trim 后）。
        assert_eq!(
            normalize_web_url("https://github.com").unwrap(),
            "https://github.com"
        );
    }

    #[test]
    fn validate_scheme_policy() {
        assert!(validate_web_url("https://example.com").is_ok());
        assert!(validate_web_url("http://127.0.0.1:8080").is_ok());
        assert!(validate_web_url("http://localhost:5173").is_ok());
        assert!(validate_web_url("http://[::1]:9000").is_ok());
        assert!(
            validate_web_url("http://example.com").is_err(),
            "public http rejected"
        );
        assert!(validate_web_url("ftp://x.com").is_err());
        assert!(
            validate_web_url("notaurl").is_err(),
            "schemeless garbage stays garbage"
        );
        assert!(
            validate_web_url("https://").is_err(),
            "missing host rejected"
        );
        assert!(normalize_web_url("").is_err());
        assert!(normalize_web_url("   ").is_err());
        assert!(
            normalize_web_url("notaurl").is_err(),
            "host without dot rejected"
        );
        assert!(normalize_web_url("baidu.").is_err());
    }

    #[test]
    fn origin_derivation_ignores_scheme_port_and_userinfo() {
        assert_eq!(
            derive_origin("https://user:pass@GitHub.com:8443/x?q=1"),
            Some("github.com".into())
        );
        assert_eq!(
            derive_origin("http://127.0.0.1:8080/"),
            Some("127.0.0.1".into())
        );
        assert_eq!(derive_origin("https://[::1]:9000/"), Some("::1".into()));
        assert_eq!(derive_origin("https://"), None);
    }

    #[test]
    fn navigation_within_approved_origins_is_the_only_gate() {
        let approved = vec!["chatgpt.com".to_string()];
        assert!(navigation_within_origins("https://chatgpt.com/", &approved));
        assert!(navigation_within_origins(
            "https://chatgpt.com/c/abc",
            &approved
        ));
        assert!(!navigation_within_origins("https://evil.com/x", &approved));
        assert!(
            !navigation_within_origins("http://chatgpt.com/", &[]),
            "empty set fails closed"
        );
        assert!(!navigation_within_origins("ftp://chatgpt.com/", &approved));
        // 带 scheme 的 origin 写法等价于裸 host。
        assert!(navigation_within_origins(
            "https://github.com/repo",
            &["https://github.com".to_string()]
        ));
        assert!(navigation_within_origins(
            "https://www.baidu.com/",
            &["baidu.com".to_string()]
        ));
        assert!(!navigation_within_origins(
            "https://evilbaidu.com/",
            &["baidu.com".to_string()]
        ));
        // loopback http 在 approved 内放行（本地开发站点）。
        assert!(navigation_within_origins(
            "http://127.0.0.1:8080/login",
            &["127.0.0.1".to_string()]
        ));
    }
}
