//! rules_api：规则、事件、行动候选与模拟 REST 端点（G4）。

use crate::rules::{self, RuleError, RuleInput};
use crate::storage::Store;
use serde_json::Value;

fn err(code: u16, e: &RuleError) -> (u16, String) {
    (
        code,
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

fn rule_input_from(value: &Value) -> Result<RuleInput, (u16, String)> {
    Ok(RuleInput {
        name: value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("未命名规则")
            .to_string(),
        rule_type: value
            .get("ruleType")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                (
                    400,
                    "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"缺少 ruleType\"}".into(),
                )
            })?
            .to_string(),
        object_key: value
            .get("objectKey")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        params: value
            .get("params")
            .cloned()
            .unwrap_or(serde_json::json!({})),
        period: value
            .get("period")
            .and_then(Value::as_str)
            .unwrap_or("quote")
            .to_string(),
        enabled: value
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

pub fn api_rules_list(store: &Store, path: &str) -> Result<String, (u16, String)> {
    let include_archived = crate::market::query_param(path, "archived")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false);
    let items = store
        .with_read(|conn| rules::list_rules(conn, include_archived))
        .map_err(|e| err(400, &e))?;
    let rows = items
        .iter()
        .map(|r| {
            format!(
                "{{\"id\":{id},\"name\":{name},\"ruleType\":{rt},\"objectKey\":{ok},\
                 \"params\":{params},\"period\":{period},\"enabled\":{en},\"version\":{v},\"archived\":{arch}}}",
                id = r.id,
                name = super::json_str(&r.name),
                rt = super::json_str(&r.rule_type),
                ok = super::json_str(&r.object_key),
                params = super::json_str(&r.params_json),
                period = super::json_str(&r.period),
                en = r.enabled,
                v = r.version,
                arch = r.archived,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    Ok(format!("{{\"rules\":[{rows}]}}"))
}

pub fn api_rule_create(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let input = rule_input_from(&value)?;
    let id = store
        .with_write(|conn| rules::create_rule(conn, &input))
        .map_err(|e| err(400, &e))?;
    Ok(format!("{{\"ok\":true,\"id\":{id}}}"))
}

pub fn api_rule_update(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let id = value.get("id").and_then(Value::as_i64).ok_or_else(|| {
        (
            400,
            "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"缺少 id\"}".into(),
        )
    })?;
    let input = rule_input_from(&value)?;
    let version = store
        .with_write(|conn| {
            check_version(conn, &value, id)?;
            rules::update_rule(conn, id, &input)
        })
        .map_err(|e| err(400, &e))?;
    Ok(format!("{{\"ok\":true,\"version\":{version}}}"))
}

pub fn api_rule_set_enabled(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let id = value.get("id").and_then(Value::as_i64).ok_or_else(|| {
        (
            400,
            "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"缺少 id\"}".into(),
        )
    })?;
    let enabled = value
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    store
        .with_write(|conn| {
            check_version(conn, &value, id)?;
            rules::set_rule_enabled(conn, id, enabled)
        })
        .map_err(|e| err(400, &e))?;
    Ok("{\"ok\":true}".into())
}

pub fn api_rule_archive(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let id = value.get("id").and_then(Value::as_i64).ok_or_else(|| {
        (
            400,
            "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"缺少 id\"}".into(),
        )
    })?;
    store
        .with_write(|conn| {
            check_version(conn, &value, id)?;
            rules::archive_rule(conn, id)
        })
        .map_err(|e| err(400, &e))?;
    Ok("{\"ok\":true}".into())
}

/// 运行一次检查（开页/手动刷新）。返回事件数与行动候选。
pub fn api_check_run(store: &Store) -> Result<String, (u16, String)> {
    let (count, actions) = rules::run_check(store).map_err(|e| err(400, &e))?;
    let items =
        actions
            .iter()
            .map(|a| {
                format!(
                "{{\"resultType\":{rt},\"objectKey\":{ok},\"objectName\":{on},\"reason\":{reason},\
                 \"ruleType\":{rtp},\"eventId\":{eid},\"amountCapMinor\":{cap},\
                 \"missing\":[{missing}]}}",
                rt = super::json_str(a.result_type),
                ok = super::json_str(&a.object_key),
                on = super::json_str(&a.object_name),
                reason = super::json_str(&a.reason),
                rtp = super::json_str(&a.rule_type),
                eid = a.event_id.map(|v| v.to_string()).unwrap_or_else(|| "null".into()),
                cap = a
                    .amount_cap_minor
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "null".into()),
                missing = a
                    .missing
                    .iter()
                    .map(|m| super::json_str(m))
                    .collect::<Vec<_>>()
                    .join(","),
            )
            })
            .collect::<Vec<_>>()
            .join(",");
    Ok(format!("{{\"eventCount\":{count},\"actions\":[{items}]}}"))
}

pub fn api_events_list(store: &Store, path: &str) -> Result<String, (u16, String)> {
    let status = crate::market::query_param(path, "status");
    let items = store
        .with_read(|conn| rules::list_events(conn, status.as_deref()))
        .map_err(|e| err(400, &e))?;
    let rows = items
        .iter()
        .map(|e| {
            format!(
                "{{\"id\":{id},\"ruleId\":{rid},\"ruleVersion\":{rv},\"ruleType\":{rt},\
                 \"objectKey\":{ok},\"firstTriggeredAt\":{ft},\"lastCheckedAt\":{lc},\
                 \"triggerValue\":{tv},\"thresholdValue\":{th},\"status\":{st},\
                 \"readyState\":{rs},\"snoozedUntil\":{su}}}",
                id = e.id,
                rid = e.rule_id,
                rv = e.rule_version,
                rt = super::json_str(&e.rule_type),
                ok = super::json_str(&e.object_key),
                ft = super::json_str(&e.first_triggered_at),
                lc = super::json_str(&e.last_checked_at),
                tv = super::json_str(&e.trigger_value),
                th = super::json_str(&e.threshold_value),
                st = super::json_str(&e.status),
                rs = super::json_str(&e.ready_state),
                su = e
                    .snoozed_until
                    .as_ref()
                    .map(|s| super::json_str(s))
                    .unwrap_or_else(|| "null".into()),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    Ok(format!("{{\"events\":[{rows}]}}"))
}

pub fn api_event_resolve(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let id = value.get("id").and_then(Value::as_i64).ok_or_else(|| {
        (
            400,
            "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"缺少 id\"}".into(),
        )
    })?;
    let note = value.get("note").and_then(Value::as_str).unwrap_or("");
    store
        .with_write(|conn| rules::resolve_event(conn, id, note))
        .map_err(|e| err(400, &e))?;
    Ok("{\"ok\":true}".into())
}

pub fn api_event_ack(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let id = value.get("id").and_then(Value::as_i64).ok_or_else(|| {
        (
            400,
            "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"缺少 id\"}".into(),
        )
    })?;
    store
        .with_write(|conn| rules::acknowledge_event(conn, id))
        .map_err(|e| err(409, &e))?;
    Ok("{\"ok\":true}".into())
}

pub fn api_event_snooze(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let id = value.get("id").and_then(Value::as_i64).ok_or_else(|| {
        (
            400,
            "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"缺少 id\"}".into(),
        )
    })?;
    let until = value.get("until").and_then(Value::as_str).ok_or_else(|| {
        (
            400,
            "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"缺少 until\"}".into(),
        )
    })?;
    store
        .with_write(|conn| rules::snooze_event(conn, id, until))
        .map_err(|e| err(400, &e))?;
    Ok("{\"ok\":true}".into())
}

/// 买入金额模拟（独立测算上界；不改账本）。
pub fn api_simulate_buy(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let account_id = value["accountId"]
        .as_i64()
        .ok_or_else(|| super::api_error(400, "APP_PARAM_INVALID", "请指定付款账户"))?;
    let revision = value["dataRevision"]
        .as_str()
        .ok_or_else(|| super::api_error(400, "APP_PARAM_INVALID", "缺少业务版本"))?;
    let values = value["candidates"]
        .as_array()
        .cloned()
        .unwrap_or_else(|| vec![value.clone()]);
    let inputs: Vec<_> = values
        .iter()
        .map(|v| crate::simulate::SimulateBuyInput {
            risk_class: v["riskClass"].as_str().unwrap_or("").into(),
            symbol: v["symbol"].as_str().unwrap_or("").into(),
            single_limit_text: v["singleLimit"].as_str().map(str::to_string),
            fee_text: v["fee"].as_str().map(str::to_string),
        })
        .collect();
    let results = crate::simulate::simulate_basket(store, account_id, Some(revision), &inputs)
        .map_err(|e| {
            err(
                if matches!(e, RuleError::Conflict(_)) {
                    409
                } else {
                    400
                },
                &e,
            )
        })?;
    Ok(serde_json::json!({"results":results}).to_string())
}

/// 决策记录（观察/暂缓/已处理）。
pub fn api_decision_create(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let input = crate::decisions::DecisionInput {
        event_id: value
            .get("eventId")
            .and_then(Value::as_i64)
            .ok_or_else(|| {
                (
                    400,
                    "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"缺少 eventId\"}".into(),
                )
            })?,
        action: value
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                (
                    400,
                    "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"缺少 action\"}".into(),
                )
            })?
            .to_string(),
        note: value
            .get("note")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        snapshot_id: value
            .get("snapshotId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    };
    let id = store
        .with_write(|conn| crate::decisions::record_decision(conn, &input))
        .map_err(|e| err(400, &e))?;
    Ok(format!("{{\"ok\":true,\"id\":{id}}}"))
}

fn check_version(c: &rusqlite::Connection, v: &Value, id: i64) -> Result<(), RuleError> {
    let expected = v["expectedVersion"]
        .as_i64()
        .ok_or_else(|| RuleError::Invalid("缺少规则版本".into()))?;
    let current: i64 = c.query_row("SELECT version FROM watch_rules WHERE id=?1", [id], |r| {
        r.get(0)
    })?;
    if expected != current {
        return Err(RuleError::Conflict("规则已变更，请重新打开编辑".into()));
    }
    Ok(())
}
pub fn api_decisions_list(store: &Store) -> Result<String, (u16, String)> {
    let items=store.with_read(|c| -> rusqlite::Result<Vec<Value>> {
        let mut q=c.prepare("SELECT id,event_id,action,note,snapshot_id,created_at,evidence_json FROM decision_records ORDER BY id DESC LIMIT 100")?;
        let rows=q.query_map([],|r| Ok(serde_json::json!({"id":r.get::<_,i64>(0)?,"eventId":r.get::<_,i64>(1)?,"action":r.get::<_,String>(2)?,"note":r.get::<_,String>(3)?,"snapshotId":r.get::<_,String>(4)?,"createdAt":r.get::<_,String>(5)?,"evidence":r.get::<_,String>(6)?})))?.collect::<Result<Vec<_>,_>>()?;
        Ok(rows)
    }).map_err(|_|super::api_error(500,"FIN_DB","历史读取失败"))?;
    Ok(serde_json::json!({"records":items}).to_string())
}
