//! Non-secret financial display settings owned by Fund, with versioned commits.
use crate::{finance::FinanceError, storage::Store};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

pub(crate) fn read(conn: &Connection) -> Result<Value, FinanceError> {
    let text: Option<String> = conn
        .query_row(
            "SELECT value FROM app_meta WHERE key='financial_settings'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    match text {
        Some(text) => {
            let mut value: Value = serde_json::from_str(&text)
                .map_err(|_| FinanceError::Invalid("财务设置损坏，请使用配置恢复".into()))?;
            if value.get("sourcePriority").is_none() {
                value["sourcePriority"] = json!("em_stock_ann");
            }
            if value.get("benchmarkSymbol").is_none() {
                value["benchmarkSymbol"] = json!("sh000300");
            }
            if value.get("quoteRefreshSeconds").is_none() {
                value["quoteRefreshSeconds"] = json!(3);
            }
            if value.get("aiMaxTokens").is_none() {
                value["aiMaxTokens"] = json!(4096);
            }
            if value["version"].as_i64().is_none_or(|v| v < 0) {
                return Err(FinanceError::Invalid("财务设置版本损坏".into()));
            }
            let mut fields = value.clone();
            fields
                .as_object_mut()
                .ok_or_else(|| FinanceError::Invalid("财务设置损坏".into()))?
                .remove("version");
            validate(&fields)?;
            Ok(value)
        }
        None => Ok(
            json!({"version":0,"colorMode":"auto","linkPlatform":"eastmoney","sourcePriority":"em_stock_ann","benchmarkSymbol":"sh000300","quoteRefreshSeconds":3,"aiMaxTokens":4096}),
        ),
    }
}
pub fn get(store: &Store) -> Result<Value, FinanceError> {
    store.with_read(read)
}

pub fn validate(value: &Value) -> Result<(), FinanceError> {
    if !value.as_object().is_some_and(|o| {
        o.keys().all(|k| {
            [
                "colorMode",
                "linkPlatform",
                "sourcePriority",
                "benchmarkSymbol",
                "quoteRefreshSeconds",
                "aiMaxTokens",
            ]
            .contains(&k.as_str())
        })
    }) || !matches!(value["colorMode"].as_str(), Some("auto" | "cn" | "intl"))
        || !matches!(value["linkPlatform"].as_str(), Some("eastmoney" | "xueqiu"))
        || value.get("sourcePriority").is_some_and(|v| {
            !matches!(
                v.as_str(),
                Some("em_stock_ann" | "em_stock_news" | "em_fund_ann")
            )
        })
        || value.get("benchmarkSymbol").is_some_and(|v| {
            !v.as_str()
                .is_some_and(|s| crate::portfolio_benchmark::SYMBOLS.contains(&s))
        })
        || value
            .get("quoteRefreshSeconds")
            .is_some_and(|v| !matches!(v.as_u64(), Some(3 | 10 | 30 | 60)))
        || value
            .get("aiMaxTokens")
            .is_some_and(|v| !v.as_u64().is_some_and(|n| (512..=8192).contains(&n)))
    {
        return Err(FinanceError::Invalid("财务设置字段或值无效".into()));
    }
    Ok(())
}
pub fn save(store: &Store, value: &Value) -> Result<Value, FinanceError> {
    if !value.as_object().is_some_and(|o| {
        o.keys()
            .all(|k| ["settings", "expectedVersion", "requestId"].contains(&k.as_str()))
    }) {
        return Err(FinanceError::Invalid("财务设置字段无效".into()));
    }
    validate(&value["settings"])?;
    let request = value["requestId"]
        .as_str()
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 128
                && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
        .ok_or_else(|| FinanceError::Invalid("缺少请求身份".into()))?;
    let key = format!("financial_settings_op:{request}");
    store.with_write(|conn| {
        let receipt: Option<String> = conn.query_row("SELECT value FROM app_meta WHERE key=?1", [&key], |r| r.get(0)).optional()?;
        if let Some(receipt) = receipt {
            let receipt: Value = serde_json::from_str(&receipt).map_err(|_| FinanceError::Invalid("配置回执损坏".into()))?;
            if receipt["input"] != *value { return Err(FinanceError::Conflict("请求身份已用于不同配置".into())); }
            return Ok(receipt["result"].clone());
        }
        let current = read(conn)?;
        if value["expectedVersion"].as_i64() != current["version"].as_i64() {
            return Err(FinanceError::Conflict("财务设置已变化，请刷新后重试".into()));
        }
        let version = current["version"].as_i64().and_then(|v| v.checked_add(1)).ok_or_else(|| FinanceError::Invalid("版本超界".into()))?;
        let mut result = value["settings"].clone(); result["version"] = json!(version);
        if result.get("sourcePriority").is_none(){result["sourcePriority"]=current["sourcePriority"].clone();}
        if result.get("aiMaxTokens").is_none(){result["aiMaxTokens"]=current["aiMaxTokens"].clone();}
        conn.execute("INSERT INTO app_meta(key,value) VALUES('financial_settings',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [result.to_string()])?;
        conn.execute("INSERT INTO app_meta(key,value) VALUES(?1,?2)", params![key,json!({"input":value,"result":result}).to_string()])?;
        conn.execute("DELETE FROM app_meta WHERE key IN (SELECT key FROM app_meta WHERE key LIKE 'financial_settings_op:%' ORDER BY rowid DESC LIMIT -1 OFFSET 100)", [])?;
        Ok(result)
    })
}
pub fn health(store: &Store) -> Result<Value, FinanceError> {
    store.with_read(|conn| {
        let check: String = conn.query_row("PRAGMA quick_check(1)", [], |r| r.get(0))?;
        let foreign_keys: bool = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?;
        let foreign_key_error = conn.prepare("PRAGMA foreign_key_check")?.query([])?.next()?.is_some();
        Ok(json!({"schemaVersion":conn.query_row("PRAGMA user_version", [], |r|r.get::<_,i64>(0))?,
            "dataRevision":Store::data_revision(conn)?,"databaseOk":check=="ok" && foreign_keys && !foreign_key_error}))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_are_versioned_idempotent_and_reject_unknown_fields() {
        let store = Store::open_in_memory().unwrap();
        let input = json!({"settings":{"colorMode":"intl","linkPlatform":"xueqiu"},"expectedVersion":0,"requestId":"settings-one"});
        assert_eq!(save(&store, &input).unwrap()["version"], 1);
        assert_eq!(save(&store, &input).unwrap()["version"], 1);
        let mut stale = input.clone();
        stale["requestId"] = json!("settings-two");
        assert!(save(&store, &stale).is_err());
        assert!(validate(
            &json!({"colorMode":"cn","linkPlatform":"eastmoney","token":"forbidden"})
        )
        .is_err());
        assert_eq!(get(&store).unwrap()["colorMode"], "intl");
        assert_eq!(health(&store).unwrap()["databaseOk"], true);
    }

    #[test]
    fn inference_budget_defaults_validates_and_survives_other_settings_edits() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(get(&store).unwrap()["aiMaxTokens"], 4096);
        for budget in [
            json!(511),
            json!(8193),
            json!(4096.5),
            json!("4096"),
            Value::Null,
        ] {
            assert!(validate(
                &json!({"colorMode":"auto","linkPlatform":"eastmoney","aiMaxTokens":budget})
            )
            .is_err());
        }
        let input = json!({"settings":{"colorMode":"auto","linkPlatform":"eastmoney","aiMaxTokens":8192},"expectedVersion":0,"requestId":"budget-one"});
        assert_eq!(save(&store, &input).unwrap()["aiMaxTokens"], 8192);
        assert_eq!(save(&store, &input).unwrap()["version"], 1);
        let edit = json!({"settings":{"colorMode":"cn","linkPlatform":"xueqiu"},"expectedVersion":1,"requestId":"budget-two"});
        assert_eq!(save(&store, &edit).unwrap()["aiMaxTokens"], 8192);
        assert_eq!(get(&store).unwrap()["aiMaxTokens"], 8192);
    }
}
