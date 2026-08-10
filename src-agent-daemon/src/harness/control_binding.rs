//! Which published document applies where: version listing and rollback, and
//! the scope → profile bindings that make a resolution concrete.

use super::super::repository::{self, GLOBAL_SCOPE_ID};
use super::super::{opt_i64, opt_str, req_str, HarnessError};
use serde_json::Value;

// ── versions ────────────────────────────────────────────────────────────────

pub(super) fn version_list(params: &Value) -> Result<Value, HarnessError> {
    let profile_id = req_str(params, &["profile_id", "profileId"])?;
    let limit = opt_i64(params, &["limit"], 50, 500);
    repository::with_conn(|conn| {
        let versions = repository::list_versions(conn, &profile_id, limit)?;
        Ok(serde_json::json!({
            "profile_id": profile_id,
            "versions": versions.iter().map(|v| v.to_summary_json()).collect::<Vec<_>>(),
        }))
    })
}

/// Republish an old document as a **new** version.
///
/// Design 第 9.2 节: rollback must not move a mutable pointer backwards. A Run
/// snapshot references `version_id`, so rewinding the pointer would make an
/// old Run's evidence describe a document it never used.
pub(super) fn version_rollback(params: &Value) -> Result<Value, HarnessError> {
    let version_id = req_str(params, &["version_id", "versionId"])?;
    let expected_current = req_str(
        params,
        &["expected_current_version_id", "expectedCurrentVersionId"],
    )?;
    repository::with_conn(|conn| {
        let source = repository::get_version(conn, &version_id)?
            .ok_or_else(|| HarnessError::not_found(format!("version not found: {version_id}")))?;
        let current = repository::current_version(conn, &source.profile_id)?
            .ok_or_else(|| HarnessError::not_found("profile has no current published version"))?;
        if current.id != expected_current {
            return Err(HarnessError::draft_conflict(format!(
                "published version changed from {expected_current} to {}; reload before rollback",
                current.id
            )));
        }
        let document = source.document()?;
        let summary = serde_json::json!({
            "rolled_back_from": source.id,
            "rolled_back_to_version_number": source.version_number,
        });
        let version = repository::publish_version(conn, &source.profile_id, &document, &summary)?;
        repository::append_audit(
            conn,
            "rollback",
            Some(&source.profile_id),
            Some(&version.id),
            None,
            None,
            &summary,
        )?;
        Ok(serde_json::json!({
            "version": version.to_summary_json(),
            "restored_from": source.to_summary_json(),
        }))
    })
}

// ── bindings ────────────────────────────────────────────────────────────────

pub(super) fn binding_scope(params: &Value) -> Result<(String, String), HarnessError> {
    let scope_type =
        opt_str(params, &["scope_type", "scopeType"]).unwrap_or_else(|| "global".into());
    let scope_id = match scope_type.as_str() {
        "global" => GLOBAL_SCOPE_ID.to_string(),
        "project" => req_str(params, &["scope_id", "scopeId", "project_id", "projectId"])?,
        "session" => req_str(
            params,
            &["scope_id", "scopeId", "conversation_id", "conversationId"],
        )?,
        other => {
            return Err(HarnessError::invalid(format!(
                "unknown binding scope: {other}"
            )))
        }
    };
    Ok((scope_type, scope_id))
}

pub(super) fn binding_get(params: &Value) -> Result<Value, HarnessError> {
    let (scope_type, scope_id) = binding_scope(params)?;
    repository::with_conn(|conn| {
        let binding = repository::get_binding(conn, &scope_type, &scope_id)?;
        let version = match &binding {
            Some(binding) => repository::version_for_binding(conn, binding)?,
            None => None,
        };
        Ok(serde_json::json!({
            "scope_type": scope_type,
            "scope_id": scope_id,
            "binding": binding.as_ref().map(|b| b.to_json()),
            "effective_version": version.as_ref().map(|v| v.to_summary_json()),
        }))
    })
}

pub(super) fn binding_set(params: &Value) -> Result<Value, HarnessError> {
    let (scope_type, scope_id) = binding_scope(params)?;
    let profile_id = req_str(params, &["profile_id", "profileId"])?;
    let mode = opt_str(params, &["mode"]).unwrap_or_else(|| "follow_published".into());
    if !["follow_published", "pinned"].contains(&mode.as_str()) {
        return Err(HarnessError::invalid(format!(
            "unknown binding mode: {mode}"
        )));
    }
    let version_id = opt_str(params, &["version_id", "versionId"]);

    repository::with_conn(|conn| {
        let binding = repository::set_binding(
            conn,
            &scope_type,
            &scope_id,
            &profile_id,
            version_id.as_deref(),
            &mode,
        )?;
        repository::append_audit(
            conn,
            "binding_change",
            Some(&profile_id),
            version_id.as_deref(),
            Some(&scope_type),
            Some(&scope_id),
            &serde_json::json!({ "mode": mode }),
        )?;
        Ok(serde_json::json!({ "binding": binding.to_json() }))
    })
}
