//! Source-drift tracking for the Harness SQLite store.
//!
//! Split out of `repository.rs` during the modular-architecture remediation.
//! A drift candidate is metadata-only: source bodies never cross this
//! boundary, and drift must be acknowledged before a draft can be published.
//! Callers keep using `harness::repository::*` via the parent module's
//! re-exports.

use super::repository_version::{current_version, get_draft, DraftRow};
use super::{now, sql};
use crate::rpc::harness::HarnessError;
use rusqlite::{params, Connection};
use serde_json::Value;

/// Create one drift candidate from the currently published document. The
/// candidate is metadata-only: source bodies never cross this boundary.
pub fn ensure_source_drift_candidate(
    conn: &Connection,
    profile_id: &str,
    mismatches: &Value,
) -> Result<Option<DraftRow>, HarnessError> {
    if mismatches.as_array().is_none_or(Vec::is_empty) {
        return Ok(None);
    }
    let candidate_json = mismatches.to_string();
    if let Some(existing) = get_draft(conn, profile_id)? {
        let same_candidate = existing
            .source_candidate_json
            .as_deref()
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
            .and_then(|value| value.as_array().cloned())
            .is_some_and(|items| {
                let mut existing_keys = items
                    .iter()
                    .filter_map(source_candidate_key)
                    .collect::<Vec<_>>();
                let mut observed_keys = mismatches
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(source_candidate_key)
                    .collect::<Vec<_>>();
                existing_keys.sort();
                observed_keys.sort();
                existing_keys == observed_keys
            });
        if same_candidate {
            return Ok(Some(existing));
        }
        return Err(HarnessError::draft_conflict(
            "an unpublished Draft already exists; reload before acknowledging source drift",
        ));
    }
    let current = current_version(conn, profile_id)?
        .ok_or_else(|| HarnessError::not_found("published Harness profile has no version"))?;
    conn.execute(
        "INSERT INTO harness_draft
           (profile_id, base_version_id, document_json, revision, updated_at, source_candidate_json)
         VALUES (?1, ?2, ?3, 0, ?4, ?5)",
        params![
            profile_id,
            current.id,
            current.document_json,
            now(),
            candidate_json
        ],
    )
    .map_err(sql)?;
    Ok(Some(get_draft(conn, profile_id)?.ok_or_else(|| {
        HarnessError::internal("drift candidate vanished")
    })?))
}

fn source_candidate_key(value: &Value) -> Option<(String, String)> {
    Some((
        value.get("source_id")?.as_str()?.to_string(),
        value.get("observed_digest")?.as_str()?.to_string(),
    ))
}

pub fn acknowledge_source_drift(
    conn: &Connection,
    profile_id: &str,
    source_id: &str,
    observed_digest: &str,
    expected_revision: i64,
) -> Result<DraftRow, HarnessError> {
    let tx_started = conn.execute_batch("BEGIN IMMEDIATE").is_ok();
    let result = acknowledge_source_drift_inner(
        conn,
        profile_id,
        source_id,
        observed_digest,
        expected_revision,
    );
    if tx_started {
        let _ = conn.execute_batch(if result.is_ok() { "COMMIT" } else { "ROLLBACK" });
    }
    result
}

fn acknowledge_source_drift_inner(
    conn: &Connection,
    profile_id: &str,
    source_id: &str,
    observed_digest: &str,
    expected_revision: i64,
) -> Result<DraftRow, HarnessError> {
    let draft = get_draft(conn, profile_id)?
        .ok_or_else(|| HarnessError::not_found("no drift candidate exists"))?;
    if draft.revision != expected_revision {
        return Err(HarnessError::draft_conflict(
            "draft revision changed; reload before acknowledging drift",
        ));
    }
    let candidate = draft
        .source_candidate_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .ok_or_else(|| HarnessError::invalid("draft has no source drift candidate"))?;
    let items = candidate
        .as_array()
        .ok_or_else(|| HarnessError::invalid("source drift candidate is not an array"))?;
    if !items.iter().any(|item| {
        item.get("source_id").and_then(Value::as_str) == Some(source_id)
            && item.get("observed_digest").and_then(Value::as_str) == Some(observed_digest)
            && item.get("acknowledged").and_then(Value::as_bool) != Some(true)
    }) {
        return Err(HarnessError::invalid(
            "observed digest does not match drift candidate",
        ));
    }
    let acknowledged = items
        .iter()
        .cloned()
        .map(|mut item| {
            if item.get("source_id").and_then(Value::as_str) == Some(source_id) {
                if let Some(object) = item.as_object_mut() {
                    object.insert("acknowledged".into(), Value::Bool(true));
                }
            }
            item
        })
        .collect::<Vec<_>>();
    let candidate_json = Value::Array(acknowledged).to_string();
    let changed = conn
        .execute(
            "UPDATE harness_draft
            SET revision = revision + 1, updated_at = ?2, source_candidate_json = ?4
          WHERE profile_id = ?1 AND revision = ?3",
            params![profile_id, now(), expected_revision, candidate_json],
        )
        .map_err(sql)?;
    if changed == 0 {
        return Err(HarnessError::draft_conflict(
            "draft changed while acknowledging source drift",
        ));
    }
    get_draft(conn, profile_id)?
        .ok_or_else(|| HarnessError::internal("draft vanished after acknowledgement"))
}

pub fn publish_source_drift_manifest(
    conn: &Connection,
    draft: &DraftRow,
) -> Result<(), HarnessError> {
    let Some(items) = draft
        .source_candidate_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .and_then(|value| value.as_array().cloned())
    else {
        return Ok(());
    };
    if items
        .iter()
        .any(|item| item.get("acknowledged").and_then(Value::as_bool) != Some(true))
    {
        return Err(HarnessError::validation_failed(
            "tracked source drift must be acknowledged before publishing",
        ));
    }
    for item in items {
        let source_id = item
            .get("source_id")
            .and_then(Value::as_str)
            .ok_or_else(|| HarnessError::invalid("drift candidate source_id is missing"))?;
        let digest = item
            .get("observed_digest")
            .and_then(Value::as_str)
            .ok_or_else(|| HarnessError::invalid("drift candidate digest is missing"))?;
        let changed = conn
            .execute(
                "UPDATE harness_source_manifest
                    SET digest = ?2, status = 'current', updated_at = ?3
                  WHERE source_id = ?1",
                params![source_id, digest, now()],
            )
            .map_err(sql)?;
        if changed == 0 {
            return Err(HarnessError::not_found(format!(
                "source manifest not found: {source_id}"
            )));
        }
    }
    Ok(())
}
