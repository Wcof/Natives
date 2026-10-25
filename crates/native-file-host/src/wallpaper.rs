//! Reserved public wallpaper metadata relay for both surface entries.
//!
//! Chrome extension host permissions do not apply to the loopback Web entry,
//! and www.bing.com does not send CORS headers. This method never accepts a URL
//! or headers: the Host owns the fixed endpoint and only relays the two image
//! fields already declared by the Space background contract.

use serde_json::{json, Value};
use std::io::Read;
use std::time::Duration;

const MARKETS: [&str; 5] = ["zh-CN", "en-US", "ja-JP", "en-GB", "de-DE"];
const MAX_BODY_BYTES: u64 = 64 * 1024;

pub(crate) fn bing_wallpaper(params: &Value) -> Result<Value, String> {
    let market = params
        .get("mkt")
        .and_then(Value::as_str)
        .ok_or("mkt is required")?;
    let market = MARKETS
        .iter()
        .find(|candidate| **candidate == market)
        .ok_or("unsupported mkt")?;
    let index = match params.get("idx") {
        None => 0,
        Some(value) => value
            .as_u64()
            .filter(|index| *index <= 7)
            .ok_or("invalid idx")?,
    };

    let url =
        format!("https://www.bing.com/HPImageArchive.aspx?format=js&idx={index}&n=1&mkt={market}");
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .https_only(true)
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(8)))
            .timeout_connect(Some(Duration::from_secs(5)))
            .user_agent(concat!("Natives/", env!("CARGO_PKG_VERSION")))
            .build(),
    );
    let mut response = agent
        .get(&url)
        .call()
        .map_err(|_| "wallpaper source unavailable".to_string())?;
    if response.status().as_u16() != 200 {
        return Err("wallpaper source unavailable".into());
    }
    let mut body = String::new();
    response
        .body_mut()
        .as_reader()
        .take(MAX_BODY_BYTES + 1)
        .read_to_string(&mut body)
        .map_err(|_| "wallpaper source unreadable".to_string())?;
    if body.len() as u64 > MAX_BODY_BYTES {
        return Err("wallpaper source too large".into());
    }

    let archive: Value =
        serde_json::from_str(&body).map_err(|_| "wallpaper source malformed".to_string())?;
    parse_archive(&archive)
}

fn parse_archive(archive: &Value) -> Result<Value, String> {
    let image = archive
        .get("images")
        .and_then(Value::as_array)
        .and_then(|images| images.first())
        .ok_or("wallpaper source empty")?;
    let url = image
        .get("url")
        .and_then(Value::as_str)
        .and_then(absolute_image_url)
        .ok_or("wallpaper image missing")?;
    let urlbase = image
        .get("urlbase")
        .and_then(Value::as_str)
        .and_then(absolute_image_url);
    Ok(json!({"url": url, "urlbase": urlbase}))
}

fn absolute_image_url(path: &str) -> Option<String> {
    if !path.starts_with("/th?")
        || path.len() > 2048
        || path
            .chars()
            .any(|c| c.is_control() || matches!(c, '"' | '\'' | '\\'))
    {
        return None;
    }
    Some(format!("https://www.bing.com{path}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_markets_outside_the_public_archive_whitelist() {
        assert!(bing_wallpaper(&json!({"mkt": "random", "idx": 0})).is_err());
        assert!(bing_wallpaper(&json!({"mkt": "https://example.com", "idx": 0})).is_err());
        assert!(bing_wallpaper(&json!({"idx": 0})).is_err());
    }

    #[test]
    fn rejects_out_of_range_archive_indexes() {
        assert!(bing_wallpaper(&json!({"mkt": "zh-CN", "idx": 8})).is_err());
        assert!(bing_wallpaper(&json!({"mkt": "zh-CN", "idx": -1})).is_err());
    }

    #[test]
    fn only_accepts_bing_th_paths() {
        assert_eq!(
            absolute_image_url("/th?id=OHR.X&pid=hp").as_deref(),
            Some("https://www.bing.com/th?id=OHR.X&pid=hp")
        );
        assert_eq!(absolute_image_url("https://evil.example/th?id=x"), None);
        assert_eq!(absolute_image_url("/other?id=x"), None);
        assert_eq!(absolute_image_url("/th?id=x\nHost: evil.example"), None);
    }

    #[test]
    fn returns_only_the_registered_image_fields() {
        let archive = json!({
            "images": [{
                "url": "/th?id=OHR.Today_1920x1080.jpg&pid=hp",
                "urlbase": "/th?id=OHR.Today",
                "copyright": "ignored",
                "copyrightlink": "https://www.bing.com/search?q=ignored"
            }]
        });
        assert_eq!(
            parse_archive(&archive).unwrap(),
            json!({
                "url": "https://www.bing.com/th?id=OHR.Today_1920x1080.jpg&pid=hp",
                "urlbase": "https://www.bing.com/th?id=OHR.Today"
            })
        );
        assert!(parse_archive(&json!({"images": []})).is_err());
        assert!(parse_archive(&json!({"images": [{"url": "https://evil.example/x"}]})).is_err());
    }
}
