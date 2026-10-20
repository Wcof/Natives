//! AI 分析报告 API（方案 11 §7）：非交易分析记录；用户主动保存；不伪造 event_id。
//! 从 home_api.rs 拆出（架构规模检查 >= 1000 硬限制）；路由语义不变。

use rusqlite::OptionalExtension;
use serde_json::Value;

use crate::storage::Store;

use super::home_api::{body_json, get_i64, get_opt_str, get_str};

pub(crate) fn feedback_request(
    store: &Store,
    request: &app_runtime_core::http::HttpRequest,
    route: &str,
) -> Option<Result<String, (u16, String)>> {
    let result = match (request.method.as_str(), route) {
        ("GET", "/api/analysis/reports/stats") => crate::report_feedback::stats(store),
        ("POST", "/api/analysis/reports/feedback") => serde_json::from_slice(&request.body)
            .map_err(|_| crate::finance::FinanceError::Invalid("JSON 无效".into()))
            .and_then(|v| crate::report_feedback::save(store, &v)),
        ("GET", "/api/analysis/reports/feedback/history") => {
            let param = |key| {
                crate::market::query_param(&request.path, key).and_then(|s| s.parse::<i64>().ok())
            };
            crate::report_feedback::history(
                store,
                param("id").unwrap_or(0),
                param("page").unwrap_or(1),
            )
        }
        _ => return None,
    };
    Some(result.map(|v| v.to_string()).map_err(|e| {
        super::api_error(
            if matches!(e, crate::finance::FinanceError::Conflict(_)) {
                409
            } else if matches!(e, crate::finance::FinanceError::Db(_)) {
                500
            } else {
                400
            },
            e.code(),
            &e.message(),
        )
    }))
}

// ---------------------------------------------------------------------------
// AI 分析报告（方案 11 §7）：非交易分析记录；用户主动保存；不伪造 event_id。
// ---------------------------------------------------------------------------

pub fn api_analysis_reports(store: &Store, path: &str) -> Result<String, (u16, String)> {
    let selected_id = crate::market::query_param(path, "id")
        .map(|value| {
            value
                .parse::<i64>()
                .ok()
                .filter(|id| *id > 0)
                .ok_or_else(|| super::api_error(400, "FIN_INVALID", "报告身份无效"))
        })
        .transpose()?;
    let page = crate::market::query_param(path, "page")
        .and_then(|p| p.parse::<i64>().ok())
        .unwrap_or(1)
        .clamp(1, 1_000_000);
    let offset = if selected_id.is_some() {
        0
    } else {
        (page - 1) * 50
    };
    let include_deleted =
        crate::market::query_param(path, "includeDeleted").as_deref() == Some("1");
    let rows = store
        .with_read(|c| -> rusqlite::Result<Vec<Value>> {
            let mut stmt = c.prepare(
                "SELECT id, task, scope_json, snapshot_id, data_revision, provider_id, model_id, model_name, result_text, generated_at, saved_at,version,deleted_at,validation_json,evidence_json
                 FROM analysis_reports WHERE (deleted_at IS NULL OR ?2) AND (?3 IS NULL OR id=?3) ORDER BY saved_at DESC, id DESC LIMIT 50 OFFSET ?1",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![offset,include_deleted,selected_id], |r| {
                    let mut report=serde_json::json!({
                        "id": r.get::<_, i64>(0)?,
                        "task": r.get::<_, String>(1)?,
                        "scope": serde_json::from_str::<Value>(&r.get::<_, String>(2)?).unwrap_or(Value::Null),
                        "snapshotId": r.get::<_, String>(3)?,
                        "dataRevision": r.get::<_, String>(4)?,
                        "providerId": r.get::<_, String>(5)?,
                        "modelId": r.get::<_, String>(6)?,
                        "modelName": r.get::<_, String>(7)?,
                        "resultText": r.get::<_, String>(8)?,
                        "generatedAt": r.get::<_, String>(9)?,
                        "savedAt": r.get::<_, String>(10)?,
                        "version":r.get::<_,i64>(11)?,
                        "deleted":r.get::<_,Option<String>>(12)?.is_some(),
                        "validation":serde_json::from_str::<Value>(&r.get::<_,String>(13)?).unwrap_or(serde_json::json!({"status":"unverified"})),
                        "feedback":crate::report_feedback::latest(c,r.get(0)?)?,
                    });
                    if selected_id.is_some() {
                        report["evidence"]=serde_json::from_str(&r.get::<_,String>(14)?).map_err(|error|rusqlite::Error::FromSqlConversionFailure(14,rusqlite::types::Type::Text,Box::new(error)))?;
                    }
                    Ok(report)
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .map_err(|_| super::api_error(500, "FIN_DB", "分析报告读取失败"))?;
    Ok(serde_json::json!({"reports": rows, "page": page, "pageSize": 50}).to_string())
}

/// 报告变化回顾（M07 迁移语义；方案 05 §7）：同任务同范围的前次保存报告，
/// 确定性字段比较——数据版本、资讯条目内容版本（新增/更新）、规则事件数。
/// 不同范围不强比；没有前次报告明确返回 first:true。
pub fn api_analysis_report_compare(store: &Store, path: &str) -> Result<String, (u16, String)> {
    let current_id = crate::market::query_param(path, "id")
        .and_then(|p| p.parse::<i64>().ok())
        .ok_or_else(|| super::api_error(400, "FIN_INVALID", "缺少报告 id"))?;
    store
        .with_read(|c| -> Result<Value, (u16, String)> {
            let (task, scope_json, data_revision, evidence_json, saved_at): (String, String, String, String, String) = c
                .query_row(
                    "SELECT task, scope_json, data_revision, evidence_json, saved_at FROM analysis_reports WHERE id=?1",
                    [current_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                )
                .optional()
                .map_err(|_| super::api_error(500, "FIN_DB", "报告读取失败"))?
                .ok_or_else(|| super::api_error(404, "FIN_NOT_FOUND", "报告不存在"))?;
            // 前次同报告：同任务 + 同范围（方案 05 §7：不同范围不强比；审计五-4）。
            // scope_json 规范化字符串比较——{"asset":"manual:1"} 与 {"asset":"manual:2"} 不比。
            let prev: Option<(i64, String, String, String)> = c
                .query_row(
                    "SELECT id, data_revision, evidence_json, saved_at FROM analysis_reports \
                     WHERE task=?1 AND scope_json=?2 AND deleted_at IS NULL AND (saved_at < ?3 OR (saved_at=?3 AND id<?4)) ORDER BY saved_at DESC, id DESC LIMIT 1",
                    rusqlite::params![task, scope_json, saved_at, current_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .optional()
                .map_err(|_| super::api_error(500, "FIN_DB", "前次报告读取失败"))?;
            let Some((prev_id, prev_revision, prev_evidence_json, prev_saved_at)) = prev else {
                return Ok(serde_json::json!({"first": true, "reportId": current_id, "task": task}));
            };
            // 资讯内容版本对比（确定性：I 编号 → contentVersion）。
            fn info_versions(evidence_json: &str) -> std::collections::BTreeMap<String, i64> {
                let mut out = std::collections::BTreeMap::new();
                if let Ok(v) = serde_json::from_str::<Value>(evidence_json) {
                    if let Some(items) = v["evidence"].as_array() {
                        for e in items {
                            if e["kind"] == "information" {
                                if let (Some(id), Some(ver)) = (e["id"].as_str(), e["contentVersion"].as_i64()) {
                                    out.insert(id.to_string(), ver);
                                }
                            }
                        }
                    }
                }
                out
            }
            let current_infos = info_versions(&evidence_json);
            let prev_infos = info_versions(&prev_evidence_json);
            let added: Vec<&String> = current_infos.keys().filter(|k| !prev_infos.contains_key(*k)).collect();
            let updated: Vec<&String> = current_infos
                .iter()
                .filter(|(k, v)| prev_infos.get(*k).is_some_and(|pv| pv != *v))
                .map(|(k, _)| k)
                .collect();
            let removed: Vec<&String> = prev_infos.keys().filter(|k| !current_infos.contains_key(*k)).collect();
            let current_pack:Value=serde_json::from_str(&evidence_json).map_err(|_|super::api_error(500,"FIN_DB","报告证据损坏"))?;
            let previous_pack:Value=serde_json::from_str(&prev_evidence_json).map_err(|_|super::api_error(500,"FIN_DB","前次证据损坏"))?;
            let piece=|pack:&Value,id:&str|pack["evidence"].as_array().and_then(|a|a.iter().find(|p|p["id"]==id)).cloned().unwrap_or(Value::Null);
            let changed=|id:&str|{let a=piece(&current_pack,id);let b=piece(&previous_pack,id);if a.is_null()||b.is_null(){Value::Null}else{Value::Bool(a!=b)}};
            Ok(serde_json::json!({
                "first": false,
                "reportId": current_id,
                "previous": {"id": prev_id, "savedAt": prev_saved_at, "dataRevision": prev_revision},
                "dataRevisionChanged": data_revision != prev_revision,
                "informationAdded": added,
                "informationUpdated": updated,
                "informationRemoved": removed,
                "localFactsChanged":changed("E1"),"financeChanged":changed("E2"),"rulesChanged":changed("E3"),"strategiesChanged":changed("E4"),"quotesChanged":changed("E5"),
                // 简报概览（E6）仅简报任务有此证据块；其他任务为 null，不渲染该行。
                "briefingOverviewChanged": if task == "briefing" { changed("E6") } else { Value::Null },
            }))
        })
        .map(|v| v.to_string())
}

pub fn api_analysis_report_save(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let task = get_str(&value, "task")?.to_string();
    if !matches!(
        task.as_str(),
        "diagnosis" | "conditions" | "news_impact" | "briefing"
    ) {
        return Err(super::api_error(400, "FIN_INVALID", "未知的分析任务"));
    }
    let result_text = get_str(&value, "resultText")?.to_string();
    if result_text.trim().is_empty() || result_text.chars().count() > 20000 {
        return Err(super::api_error(
            400,
            "FIN_INVALID",
            "报告正文须为 1~20000 字符",
        ));
    }
    // 保存时核对证据版本：旧结果可保存但前端按旧快照标注（方案 11 §7）。
    let snapshot_id = get_opt_str(&value, "snapshotId").unwrap_or("").to_string();
    let data_revision = get_opt_str(&value, "dataRevision")
        .unwrap_or("")
        .to_string();
    if snapshot_id.is_empty() || data_revision.is_empty() {
        return Err(super::api_error(400, "FIN_INVALID", "缺少快照或数据版本"));
    }
    let current = store
        .with_read(Store::data_revision)
        .map_err(|_| super::api_error(500, "FIN_DB", "版本读取失败"))?;
    let stale = current != data_revision;
    let evidence = value
        .get("evidence")
        .filter(|v| v.is_object())
        .ok_or_else(|| super::api_error(400, "FIN_INVALID", "缺少证据快照"))?;
    if evidence["task"] != value["task"]
        || evidence["snapshotId"] != value["snapshotId"]
        || evidence["dataRevision"] != value["dataRevision"]
        || evidence.to_string().chars().count() > 16000
    {
        return Err(super::api_error(
            400,
            "FIN_INVALID",
            "报告与证据快照不一致或过长",
        ));
    }
    let scope = if let Some(asset) = evidence["scope"]["asset"].as_str() {
        serde_json::json!({"asset":asset})
    } else if let Some(symbol) = evidence["scope"]["research"].as_str() {
        serde_json::json!({"research":symbol})
    } else if evidence["scope"] == "portfolio" {
        serde_json::json!({"kind":"portfolio"})
    } else if evidence["task"] == "briefing"
        && evidence["scope"]["market"].as_str().is_some()
        && evidence["scope"]["session"].as_str().is_some()
        && evidence["scope"]["date"].as_str().is_some()
    {
        serde_json::json!({
            "kind":"briefing",
            "market":evidence["scope"]["market"],
            "session":evidence["scope"]["session"],
            "date":evidence["scope"]["date"],
        })
    } else {
        return Err(super::api_error(400, "FIN_INVALID", "报告范围无效"));
    };
    let scope_json = scope.to_string();
    let evidence_json = evidence.to_string();
    let validation = crate::analysis_output::validate(&result_text, evidence);
    let provider_id = get_opt_str(&value, "providerId").unwrap_or("").to_string();
    let model_id = get_opt_str(&value, "modelId").unwrap_or("").to_string();
    let model_name = get_opt_str(&value, "modelName").unwrap_or("").to_string();
    if provider_id.len() > 128 || model_id.len() > 256 || model_name.len() > 256 {
        return Err(super::api_error(400, "FIN_INVALID", "模型身份过长"));
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default();
    let id = store
        .with_write(|c| -> rusqlite::Result<i64> {
            c.execute(
                "INSERT INTO analysis_reports(task, scope_json, evidence_json, snapshot_id, data_revision, provider_id, model_id, model_name, result_text, generated_at, saved_at,validation_json)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                rusqlite::params![task, scope_json, evidence_json, snapshot_id, data_revision, provider_id, model_id, model_name, result_text, value.get("generatedAt").and_then(Value::as_str).unwrap_or(&now), now,validation.to_string()],
            )?;
            Ok(c.last_insert_rowid())
        })
        .map_err(|e| {
            eprintln!("analysis_report_save db error: {e}");
            super::api_error(500, "FIN_DB", "报告保存失败")
        })?;
    Ok(
        serde_json::json!({"ok": true, "id": id, "staleSnapshot": stale,"validation":validation})
            .to_string(),
    )
}

pub fn api_analysis_report_delete(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let id = get_i64(&value, "id")?;
    let expected = get_i64(&value, "expectedVersion")?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default();
    // 软删除（方案 05 §7）：带时点标记退出有效列表；证据不可变，恢复不重放推理。
    let deleted = store
        .with_write(|c| {
            c.execute(
                "UPDATE analysis_reports SET deleted_at=?2,version=version+1 WHERE id=?1 AND deleted_at IS NULL AND version=?3",
                rusqlite::params![id, now,expected],
            )
        })
        .map_err(|_| super::api_error(500, "FIN_DB", "报告删除失败"))?;
    if deleted == 0 {
        return Err(super::api_error(409, "FIN_CONFLICT", "报告已变化，请刷新"));
    }
    Ok("{\"ok\":true}".into())
}

pub fn api_analysis_report_restore(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let id = get_i64(&value, "id")?;
    let expected = get_i64(&value, "expectedVersion")?;
    let restored = store
        .with_write(|c| {
            c.execute(
                "UPDATE analysis_reports SET deleted_at=NULL,version=version+1 WHERE id=?1 AND deleted_at IS NOT NULL AND version=?2",
                rusqlite::params![id,expected],
            )
        })
        .map_err(|_| super::api_error(500, "FIN_DB", "报告恢复失败"))?;
    if restored == 0 {
        return Err(super::api_error(409, "FIN_CONFLICT", "报告已变化，请刷新"));
    }
    Ok("{\"ok\":true}".into())
}

pub fn api_analysis_validate(body: &str) -> Result<String, (u16, String)> {
    let v = body_json(body)?;
    let text = get_str(&v, "resultText")?;
    if text.chars().count() > 20000 || v["evidence"].to_string().chars().count() > 16000 {
        return Err(super::api_error(400, "FIN_INVALID", "分析结果或证据过长"));
    }
    Ok(crate::analysis_output::validate(text, &v["evidence"]).to_string())
}
