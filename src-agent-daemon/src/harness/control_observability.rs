//! Audit, trace, and source-drift observability: what ran, what changed, and
//! what on-disk Hook sources currently disagree with their published digests.

use super::control_profile::discovered_hooks;
use super::super::repository;
use super::super::{opt_i64, opt_str, req_str, HarnessError};
use serde_json::Value;

pub(super) fn audit_list(params: &Value) -> Result<Value, HarnessError> {
    let limit = opt_i64(params, &["limit"], 50, 500);
    repository::with_conn(|conn| {
        Ok(serde_json::json!({ "entries": repository::list_audit(conn, limit)? }))
    })
}

pub(super) fn audit_export(params: &Value) -> Result<Value, HarnessError> {
    let limit = opt_i64(params, &["limit"], 50, 500);
    repository::with_conn(|conn| {
        Ok(serde_json::json!({"entries": repository::list_audit(conn, limit)?, "redacted": true}))
    })
}

pub(super) fn trace_list(params: &Value) -> Result<Value, HarnessError> {
    let run_id = opt_str(params, &["run_id", "runId"]);
    let limit = opt_i64(params, &["limit"], 100, 200);
    let after = opt_i64(params, &["after_sequence", "afterSequence"], 0, i64::MAX);
    repository::with_conn(|conn| {
        Ok(
            serde_json::json!({"entries": repository::list_run_hook_trace(conn, run_id.as_deref(), after, limit)?, "page_size": limit, "after_sequence": after}),
        )
    })
}

pub(super) fn source_list(params: &Value) -> Result<Value, HarnessError> {
    let limit = opt_i64(params, &["limit"], 100, 500);
    let discovered = discovered_hooks(params);
    let sources = discovered.into_iter().map(|hook| serde_json::json!({
        "source_id": hook.id.as_str(),
        "scope": hook.source.scope,
        "origin": hook.source.origin,
        "digest": harness_core::sha256_hex(&serde_json::to_string(&hook.kind).unwrap_or_default()),
        "tracked": hook.source.scope == harness_core::HookScope::Project,
        "pinned": false,
    })).collect::<Vec<_>>();
    repository::with_conn(|conn| {
        let stored = repository::list_sources(conn, limit)?;
        let mut merged = std::collections::BTreeMap::new();
        for source in sources {
            if let Some(id) = source.get("source_id").and_then(Value::as_str) {
                merged.insert(id.to_string(), source);
            }
        }
        for stored_source in stored {
            let Some(id) = stored_source
                .get("source_id")
                .and_then(Value::as_str)
                .map(str::to_string)
            else {
                continue;
            };
            if let (Some(current), Some(update)) = (
                merged.get_mut(&id).and_then(Value::as_object_mut),
                stored_source.as_object(),
            ) {
                current.extend(update.clone());
            } else {
                merged.insert(id, stored_source);
            }
        }
        Ok(
            serde_json::json!({"sources": merged.into_values().take(limit as usize).collect::<Vec<_>>(), "page_size": limit}),
        )
    })
}

pub(super) fn source_acknowledge_drift(params: &Value) -> Result<Value, HarnessError> {
    let source = req_str(params, &["source_id", "sourceId"])?;
    let profile_id = req_str(params, &["profile_id", "profileId"])?;
    let observed_digest = req_str(params, &["observed_digest", "observedDigest"])?;
    let expected_revision = params
        .get("revision")
        .and_then(Value::as_i64)
        .ok_or_else(|| HarnessError::invalid("revision is required"))?;
    repository::with_conn(|conn| {
        let draft = repository::get_draft(conn, &profile_id)?
            .ok_or_else(|| HarnessError::not_found("no draft exists for drift acknowledgement"))?;
        if draft.revision != expected_revision {
            return Err(HarnessError::draft_conflict(
                "draft revision changed; reload before acknowledging drift",
            ));
        }
        let acknowledged = repository::acknowledge_source_drift(
            conn,
            &profile_id,
            &source,
            &observed_digest,
            expected_revision,
        )?;
        repository::append_audit(
            conn,
            "source_drift_ack",
            Some(&profile_id),
            None,
            None,
            None,
            &serde_json::json!({"source_id": source, "observed_digest": observed_digest, "revision": acknowledged.revision}),
        )?;
        Ok(
            serde_json::json!({"source_id": source, "profile_id": profile_id, "revision": acknowledged.revision}),
        )
    })
}
