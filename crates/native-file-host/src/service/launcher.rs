//! Short-lived launcher handoff to the authenticated local Web surface.
//!
//! URL preparation and browser handoff are separate steps so the DSH plugin
//! entry (ADR-0048) can reuse the exact same bootstrap chain without opening a
//! browser: `prepare_url` owns `ensure_service` → `native:bootstrap` → `#token=`
//! and `open_web` keeps the original open-a-window behaviour unchanged.
use std::io;
use std::process::Command;

/// Whitelisted product routes reachable from the launcher handoff.
fn route(target: &str) -> io::Result<&'static str> {
    match target {
        "space" => Ok("space.html"),
        "files" => Ok("files.html"),
        "tokenusage" => Ok("app.html?app=tokenusage"),
        "fund" => Ok("app.html?app=fund"),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unknown launcher target",
        )),
    }
}

/// One-time bootstrap tokens are 43 URL-safe base64 characters.
fn valid_token(token: &str) -> bool {
    token.len() == 43
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Path, port and one-time token for one whitelisted route.
fn bootstrap(target: &str) -> io::Result<(String, u16, String)> {
    let path = route(target)?;
    let port = super::ensure_service(None)?;
    let body = serde_json::to_vec(
        &serde_json::json!({"id":"launcher-bootstrap","method":"native:bootstrap","params":{}}),
    )?;
    let response = super::relay::send_relay_http(port, &body)?;
    let response: serde_json::Value = serde_json::from_slice(&response)?;
    let token = response["result"]["bootstrapToken"]
        .as_str()
        .filter(|token| valid_token(token))
        .map(str::to_string)
        .ok_or_else(|| io::Error::other("Web session preparation failed"))?;
    Ok((path.to_string(), port, token))
}

fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Attaches to (or starts) the Core and returns the authenticated page URL.
///
/// The token stays in the URL fragment: it never reaches a server log and is
/// not written to the state directory.
pub fn prepare_url(target: &str) -> io::Result<String> {
    let (path, port, token) = bootstrap(target)?;
    Ok(format!("http://127.0.0.1:{port}/{path}#token={token}"))
}

/// Joins the embed params onto the route path. Routes may already carry a
/// query (`app.html?app=fund`), so the grant must join with `&`, never a
/// second `?`: `embed::frame_policy` and the pages parse the result as
/// `application/x-www-form-urlencoded`.
fn merge_embed_query(path: &str, grant: &str, ancestors: &str) -> String {
    let separator = if path.contains('?') { '&' } else { '?' };
    format!(
        "{path}{separator}embed={}&anc={}&standalone=1",
        percent_encode(grant),
        percent_encode(ancestors)
    )
}

/// Same URL plus a short-lived embed grant (ADR-0048 D5.2). `ancestors` is a
/// space-separated list already limited to `dsh:` and loopback origins.
/// Embedded documents always run in standalone mode: the pages hide the
/// product-level aggregation navigation and show only the module's own UI.
pub fn prepare_url_embedded(target: &str, ancestors: &str) -> io::Result<String> {
    let ancestors = super::embed::normalize_ancestors(ancestors)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid embed ancestors"))?;
    let (path, port, token) = bootstrap(target)?;
    let secret = super::read_relay_secret()?;
    let grant = super::embed::mint(&super::embed::normalize_target(&path), &ancestors, &secret);
    Ok(format!(
        "http://127.0.0.1:{port}/{}#token={token}",
        merge_embed_query(&path, &grant, &ancestors)
    ))
}

fn open_in_browser(url: &str) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    let status = Command::new("/usr/bin/open")
        .args(["-a", "Google Chrome", url])
        .status()?;
    #[cfg(target_os = "windows")]
    let status = Command::new("rundll32.exe")
        .args(["url.dll,FileProtocolHandler", url])
        .status()?;
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let status = Command::new("xdg-open").arg(url).status()?;
    if !status.success() {
        return Err(io::Error::other("browser handoff failed"));
    }
    Ok(())
}

/// Opens the requested product route in the system browser.
pub fn open_web(target: &str) -> io::Result<()> {
    open_web_with(target, prepare_url, open_in_browser)
}

/// Same handoff with injectable preparation and opener, so tests never start a
/// service or a browser but still exercise the open branch.
fn open_web_with<P, O>(target: &str, prepare: P, open: O) -> io::Result<()>
where
    P: Fn(&str) -> io::Result<String>,
    O: Fn(&str) -> io::Result<()>,
{
    let url = prepare(target)?;
    open(&url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn launcher_only_opens_registered_product_routes() {
        assert_eq!(route("tokenusage").unwrap(), "app.html?app=tokenusage");
        assert_eq!(route("fund").unwrap(), "app.html?app=fund");
        assert_eq!(route("files").unwrap(), "files.html");
        assert_eq!(route("space").unwrap(), "space.html");
        for invalid in [
            "https://example.com",
            "../files.html",
            "space#token=x",
            "",
            "apps",
        ] {
            assert!(route(invalid).is_err(), "{invalid} must be rejected");
        }
    }

    #[test]
    fn bootstrap_tokens_are_bounded_to_the_expected_shape() {
        assert!(valid_token(&"a".repeat(43)));
        assert!(valid_token(&"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopq"));
        assert!(!valid_token(&"a".repeat(42)));
        assert!(!valid_token(&"a".repeat(44)));
        assert!(!valid_token(&format!("{}=", "a".repeat(42))));
        assert!(!valid_token(""));
    }

    #[test]
    fn open_web_still_opens_the_prepared_url() {
        let opened: RefCell<Vec<String>> = RefCell::new(Vec::new());
        let result = open_web_with(
            "fund",
            |target| {
                assert_eq!(target, "fund");
                Ok("http://127.0.0.1:8770/app.html?app=fund#token=xyz".to_string())
            },
            |url| {
                opened.borrow_mut().push(url.to_string());
                Ok(())
            },
        );
        assert!(result.is_ok());
        assert_eq!(
            opened.into_inner(),
            vec!["http://127.0.0.1:8770/app.html?app=fund#token=xyz".to_string()]
        );
    }

    #[test]
    fn open_web_does_not_open_when_preparation_fails() {
        let opened: RefCell<Vec<String>> = RefCell::new(Vec::new());
        let result = open_web_with(
            "bogus",
            |_| Err(io::Error::new(io::ErrorKind::InvalidInput, "unknown")),
            |url| {
                opened.borrow_mut().push(url.to_string());
                Ok(())
            },
        );
        assert!(result.is_err());
        assert!(opened.into_inner().is_empty());
    }

    #[test]
    fn embed_query_joins_routes_that_already_carry_a_query() {
        assert_eq!(
            merge_embed_query("space.html", "G", "dsh:"),
            "space.html?embed=G&anc=dsh%3A&standalone=1"
        );
        assert_eq!(
            merge_embed_query("app.html?app=fund", "G", "dsh: http://127.0.0.1:59001"),
            "app.html?app=fund&embed=G&anc=dsh%3A%20http%3A%2F%2F127.0.0.1%3A59001&standalone=1"
        );
    }

    #[test]
    fn embedded_app_routes_keep_a_verifiable_grant_and_the_standalone_flag() {
        let secret = "relay-secret";
        let ancestors = "dsh: http://127.0.0.1:59001";
        for path in ["space.html", "app.html?app=fund", "app.html?app=tokenusage"] {
            let grant = super::super::embed::mint(
                &super::super::embed::normalize_target(path),
                ancestors,
                secret,
            );
            let full_path = format!("/{}", merge_embed_query(path, &grant, ancestors));
            assert!(full_path.contains("standalone=1"), "{full_path}");
            let policy = super::super::embed::frame_policy(&full_path, secret)
                .unwrap_or_else(|| panic!("grant must verify for {full_path}"));
            assert_eq!(policy, format!("frame-ancestors {ancestors}"));
        }
        // A tampered grant on a merged route must not gain framing.
        let forged = merge_embed_query(
            "app.html?app=fund",
            &super::super::embed::mint("space.html", ancestors, secret),
            ancestors,
        );
        assert_eq!(
            super::super::embed::frame_policy(&format!("/{forged}"), secret),
            None
        );
    }
}
