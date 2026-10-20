//! Explicit PanWatch v1/v2 mapping. Credentials and portfolio values never survive.
use crate::finance::FinanceError;
use serde_json::{json, Value};
pub(crate) fn normalize(input: &Value) -> Result<Value, FinanceError> {
    if input.get("formatVersion").is_some() {
        return Ok(input.clone());
    }
    let invalid = || FinanceError::Invalid("供体模板版本或字段无效".into());
    if input.to_string().len() > 200 * 1024
        || !input.as_object().is_some_and(|o| {
            o.keys().all(|k| {
                [
                    "version",
                    "exported_at",
                    "modules",
                    "settings",
                    "ai_services",
                    "notify_channels",
                    "agents",
                    "stocks",
                    "accounts",
                ]
                .contains(&k.as_str())
            })
        })
        || !matches!(input["version"].as_u64(), Some(1 | 2))
    {
        return Err(invalid());
    }
    let rows = |key: &str| -> Result<Vec<Value>, FinanceError> {
        match input.get(key) {
            None => Ok(vec![]),
            Some(v) => v
                .as_array()
                .filter(|a| a.len() <= 1000)
                .cloned()
                .ok_or_else(invalid),
        }
    };
    let selected = match input.get("modules") {
        None => vec!["portfolio", "notifications", "watchlist"],
        Some(value) => {
            let list = value
                .as_array()
                .filter(|a| a.len() <= 6)
                .ok_or_else(invalid)?;
            let mut values = vec![];
            for m in list {
                let m = m.as_str().ok_or_else(invalid)?;
                if ![
                    "settings",
                    "ai",
                    "notifications",
                    "agents",
                    "watchlist",
                    "portfolio",
                ]
                .contains(&m)
                {
                    return Err(invalid());
                }
                values.push(m);
            }
            if values.is_empty() {
                vec!["portfolio", "notifications", "watchlist"]
            } else {
                values
            }
        }
    };
    let mut accounts = vec![];
    for (i, a) in (if selected.contains(&"portfolio") {
        rows("accounts")?
    } else {
        vec![]
    })
    .iter()
    .enumerate()
    {
        accounts.push(json!({"name":a["name"].as_str().ok_or_else(invalid)?,"sortOrder":i}));
    }
    let mut items = vec![];
    for row in if selected.contains(&"watchlist") {
        rows("stocks")?
    } else {
        vec![]
    } {
        let raw = row["symbol"].as_str().ok_or_else(invalid)?;
        let symbol = match row["market"].as_str() {
            Some("CN") => {
                if raw.len() == 6
                    && raw.bytes().all(|b| b.is_ascii_digit())
                    && raw.starts_with("00")
                {
                    format!("sz{raw}")
                } else {
                    crate::market::normalize_symbol(raw)
                }
            }
            Some("HK") => {
                if raw.starts_with("hk") {
                    raw.to_string()
                } else if raw.len() == 5 && raw.bytes().all(|b| b.is_ascii_digit()) {
                    format!("hk{raw}")
                } else {
                    return Err(invalid());
                }
            }
            Some("US") => {
                if raw.starts_with("us") {
                    raw.to_string()
                } else {
                    format!("us{raw}")
                }
            }
            _ => return Err(invalid()),
        };
        let kind = if ["sh51", "sh56", "sh58", "sz15", "sz16"]
            .iter()
            .any(|p| symbol.starts_with(p))
        {
            "etf"
        } else {
            "stock"
        };
        items.push(json!({"symbol":symbol,"name":row["name"].as_str().ok_or_else(invalid)?,"assetType":kind}));
    }
    let mut channels = vec![];
    for row in if selected.contains(&"notifications") {
        rows("notify_channels")?
    } else {
        vec![]
    } {
        let raw = row["type"].as_str().ok_or_else(invalid)?;
        let kind = if raw == "lark" { "feishu" } else { raw };
        crate::channels::validate_kind(kind)?;
        let original = row["config"].as_object().ok_or_else(invalid)?;
        let mut config = serde_json::Map::new();
        for key in crate::channels::config_fields(kind) {
            if let Some(v) = original.get(*key) {
                if !v.is_string() {
                    return Err(invalid());
                }
                config.insert(key.to_string(), v.clone());
            }
        }
        channels.push(
            json!({"name":row["name"].as_str().ok_or_else(invalid)?,"kind":kind,"config":config}),
        );
    }
    Ok(
        json!({"formatVersion":1,"generatedAt":"","redacted":true,"modules":["accounts","channels","watchlists"],"settings":null,"accounts":accounts,"channels":channels,"watchlists":if items.is_empty(){vec![]}else{vec![json!({"name":"PanWatch","items":items})]},"strategies":[],"rules":[],"sources":[]}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strips_secrets_and_cash_and_maps_stock_identity_not_index() {
        let donor = json!({"version":2,"ai_services":[{"api_key":"PRIVATE_SECRET"}],"agents":[{}],"settings":{"http_proxy":"http://user:pass@localhost"},"accounts":[{"name":"A","available_funds":999999,"positions":[{"quantity":12345}]}],"stocks":[{"symbol":"000001","name":"Bank","market":"CN"},{"symbol":"513100","name":"ETF","market":"CN"}],"notify_channels":[{"name":"F","type":"lark","config":{"webhook_token":"PRIVATE_SECRET"}}]});
        let value = normalize(&donor).unwrap();
        let text = value.to_string();
        assert!(!text.contains("PRIVATE_SECRET"));
        assert!(!text.contains("999999"));
        assert!(!text.contains("12345"));
        assert_eq!(value["watchlists"][0]["items"][0]["symbol"], "sz000001");
        assert_eq!(value["watchlists"][0]["items"][1]["assetType"], "etf");
        assert_eq!(value["channels"][0]["kind"], "feishu");
        assert!(value["settings"].is_null());
        let store = crate::storage::Store::open_in_memory().unwrap();
        assert!(crate::config_templates::preview(&store, &donor).is_ok());
    }
}
