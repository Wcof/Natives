//! sources_api：信息源与投资论点 REST 端点（F06）。

use crate::sources::{self, SourceError, SourceInput, ThesisInput};
use crate::storage::Store;
use serde_json::Value;

fn err(code: u16, e: &SourceError) -> (u16, String) {
    (
        if matches!(e, SourceError::Conflict) {
            409
        } else {
            code
        },
        format!(
            "{{\"error\":\"{}\",\"message\":\"{}\"}}",
            e.code(),
            e.message().replace('"', "'")
        ),
    )
}

fn body_json(body: &str) -> Result<Value, (u16, String)> {
    serde_json::from_str::<Value>(body).map_err(|e| {
        (
            400,
            format!(
                "{{\"error\":\"APP_PARAM_INVALID\",\"message\":\"JSON 无效: {}\"}}",
                e
            ),
        )
    })
}

fn source_input_from(value: &Value) -> Result<SourceInput, (u16, String)> {
    Ok(SourceInput {
        name: value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        kind: value
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                (
                    400,
                    "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"缺少 kind\"}".into(),
                )
            })?
            .to_string(),
        url: value
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        related_symbol: value
            .get("relatedSymbol")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        thesis_id: value.get("thesisId").and_then(Value::as_i64),
        publisher: value
            .get("publisher")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        published_at: value
            .get("publishedAt")
            .and_then(Value::as_str)
            .map(str::to_string),
        excerpt_text: value
            .get("excerptText")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    })
}

fn source_json(s: &sources::InfoSourceRow) -> String {
    format!(
        "{{\"id\":{id},\"name\":{name},\"url\":{url},\"kind\":{kind},\
         \"relatedSymbol\":{sym},\"thesisId\":{thesis},\"publisher\":{pub_},\
         \"publishedAt\":{pubat},\"excerptText\":{excerpt},\"excerptFetchedAt\":{fetched},\
         \"read\":{read},\"favorite\":{fav},\"archived\":{arch}}}",
        id = s.id,
        name = super::json_str(&s.name),
        url = super::json_str(&s.url),
        kind = super::json_str(&s.kind),
        sym = super::json_str(&s.related_symbol),
        thesis = s
            .thesis_id
            .map(|v| v.to_string())
            .unwrap_or_else(|| "null".into()),
        pub_ = super::json_str(&s.publisher),
        pubat = s
            .published_at
            .as_ref()
            .map(|v| super::json_str(v))
            .unwrap_or_else(|| "null".into()),
        excerpt = super::json_str(&s.excerpt_text),
        fetched = s
            .excerpt_fetched_at
            .as_ref()
            .map(|v| super::json_str(v))
            .unwrap_or_else(|| "null".into()),
        read = s.read_status,
        fav = s.favorite,
        arch = s.archived,
    )
}

pub fn api_sources_list(store: &Store, path: &str) -> Result<String, (u16, String)> {
    let include_archived = crate::market::query_param(path, "archived")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false);
    let rows = store
        .with_read(|conn| sources::list_sources(conn, include_archived))
        .map_err(|e| err(400, &e))?;
    let items = rows.iter().map(source_json).collect::<Vec<_>>().join(",");
    Ok(format!("{{\"sources\":[{items}]}}"))
}

pub fn api_source_create(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let input = source_input_from(&value)?;
    let id = store
        .with_write(|conn| sources::create_source(conn, &input))
        .map_err(|e| err(400, &e))?;
    Ok(format!("{{\"ok\":true,\"id\":{id}}}"))
}

pub fn api_source_update(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let id = value.get("id").and_then(Value::as_i64).ok_or_else(|| {
        (
            400,
            "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"缺少 id\"}".into(),
        )
    })?;
    let input = source_input_from(&value)?;
    store
        .with_write(|conn| sources::update_source(conn, id, &input))
        .map_err(|e| err(400, &e))?;
    Ok("{\"ok\":true}".into())
}

pub fn api_source_archive(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let id = value.get("id").and_then(Value::as_i64).ok_or_else(|| {
        (
            400,
            "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"缺少 id\"}".into(),
        )
    })?;
    store
        .with_write(|conn| sources::archive_source(conn, id))
        .map_err(|e| err(400, &e))?;
    Ok("{\"ok\":true}".into())
}

pub fn api_source_flags(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let id = value.get("id").and_then(Value::as_i64).ok_or_else(|| {
        (
            400,
            "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"缺少 id\"}".into(),
        )
    })?;
    let favorite = value.get("favorite").and_then(Value::as_bool);
    let read_status = value.get("read").and_then(Value::as_bool);
    store
        .with_write(|conn| sources::set_source_flags(conn, id, favorite, read_status))
        .map_err(|e| err(400, &e))?;
    Ok("{\"ok\":true}".into())
}

pub fn api_theses_list(store: &Store, path: &str) -> Result<String, (u16, String)> {
    let rows = store
        .with_read(|conn| {
            sources::list_theses_with_history(
                conn,
                crate::market::query_param(path, "archived").as_deref() == Some("1"),
            )
        })
        .map_err(|e| err(400, &e))?;
    let items = rows
        .iter()
        .map(|t| {
            format!(
                "{{\"id\":{id},\"name\":{name},\"holdingReason\":{reason},\"horizon\":{horizon},\
                 \"verifyMetrics\":{metrics},\"invalidCondition\":{invalid},\
                 \"relatedSymbols\":{symbols},\"version\":{version},\"archived\":{archived}}}",
                id = t.id,
                name = super::json_str(&t.name),
                reason = super::json_str(&t.holding_reason),
                horizon = super::json_str(&t.horizon),
                metrics = super::json_str(&t.verify_metrics),
                invalid = super::json_str(&t.invalid_condition),
                symbols = super::json_str(&t.related_symbols),
                version = t.version,
                archived = t.archived,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    Ok(format!("{{\"theses\":[{items}]}}"))
}

pub fn api_thesis_create(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    if value.get("relatedSymbols").is_some_and(|v| {
        v.as_array()
            .is_none_or(|a| a.len() > 30 || a.iter().any(|s| !s.is_string()))
    }) {
        return Err(super::api_error(
            400,
            "SOURCE_INVALID",
            "relatedSymbols 须为最多 30 项的字符串数组",
        ));
    }
    let symbols: Vec<String> = value
        .get("relatedSymbols")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let input = ThesisInput {
        name: value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        holding_reason: value
            .get("holdingReason")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        horizon: value
            .get("horizon")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        verify_metrics: value
            .get("verifyMetrics")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        invalid_condition: value
            .get("invalidCondition")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        related_symbols: symbols,
    };
    let id = store
        .with_write(|conn| {
            if let Some(id) = value["id"].as_i64() {
                let expected = value["expectedVersion"]
                    .as_i64()
                    .ok_or_else(|| SourceError::Invalid("缺少策略版本".into()))?;
                sources::require_thesis_version(conn, id, expected)?;
                sources::update_thesis(conn, id, &input)?;
                Ok(id)
            } else {
                sources::create_thesis(conn, &input)
            }
        })
        .map_err(|e| err(400, &e))?;
    Ok(format!("{{\"ok\":true,\"id\":{id}}}"))
}

pub fn api_thesis_archive(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let id = value.get("id").and_then(Value::as_i64).ok_or_else(|| {
        (
            400,
            "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"缺少 id\"}".into(),
        )
    })?;
    store
        .with_write(|conn| {
            sources::require_thesis_version(
                conn,
                id,
                value["expectedVersion"]
                    .as_i64()
                    .ok_or_else(|| SourceError::Invalid("缺少策略版本".into()))?,
            )?;
            sources::archive_thesis(conn, id)
        })
        .map_err(|e| err(400, &e))?;
    Ok("{\"ok\":true}".into())
}

/// 恢复归档策略（方案 05 §2：归档／删除及恢复）。
pub fn api_thesis_restore(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let id = value.get("id").and_then(Value::as_i64).ok_or_else(|| {
        (
            400,
            "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"缺少 id\"}".into(),
        )
    })?;
    store
        .with_write(|conn| {
            sources::require_thesis_version(
                conn,
                id,
                value["expectedVersion"]
                    .as_i64()
                    .ok_or_else(|| SourceError::Invalid("缺少策略版本".into()))?,
            )?;
            sources::restore_thesis(conn, id)
        })
        .map_err(|e| err(400, &e))?;
    Ok("{\"ok\":true}".into())
}
