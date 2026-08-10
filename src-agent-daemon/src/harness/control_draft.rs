//! Draft lifecycle: edit, validate, diff, review, simulate, and publish, plus
//! the builtin prompt surface integrity check that gates a publish.

use super::super::repository;
use super::super::{opt_str, req_str, HarnessError};
use super::control_inspect::prompt_preview;
use super::control_profile::{discovered_hooks, resolved_context};
use harness_core::blueprint::HarnessBlueprint;
use harness_core::validation::{diff, validate, Severity, ValidationFinding, ValidationReport};
use serde_json::Value;

// ── drafts ──────────────────────────────────────────────────────────────────

pub(super) fn draft_document(params: &Value) -> Result<HarnessBlueprint, HarnessError> {
    let raw = params
        .get("document")
        .ok_or_else(|| HarnessError::invalid("document is required"))?;
    HarnessBlueprint::parse(raw).map_err(HarnessError::validation_failed)
}

pub(super) fn draft_get(params: &Value) -> Result<Value, HarnessError> {
    let profile_id = req_str(params, &["profile_id", "profileId"])?;
    repository::with_conn(|conn| {
        let draft = repository::get_or_create_draft(conn, &profile_id)?;
        Ok(serde_json::json!({
            "profile_id": draft.profile_id,
            "base_version_id": draft.base_version_id,
            "revision": draft.revision,
            "updated_at": draft.updated_at,
            "document": serde_json::from_str::<Value>(&draft.document_json)
                .unwrap_or(Value::Null),
            "source_candidate": draft.source_candidate_json.as_deref().and_then(|v| serde_json::from_str::<Value>(v).ok()),
        }))
    })
}

pub(super) fn draft_save(params: &Value) -> Result<Value, HarnessError> {
    let profile_id = req_str(params, &["profile_id", "profileId"])?;
    let document = draft_document(params)?;
    let revision = params
        .get("revision")
        .and_then(Value::as_i64)
        .ok_or_else(|| {
            HarnessError::invalid(
                "revision is required; saving without one would overwrite another editor",
            )
        })?;
    repository::with_conn(|conn| {
        let saved = repository::save_draft(conn, &profile_id, &document, revision)?;
        Ok(serde_json::json!({
            "profile_id": saved.profile_id,
            "revision": saved.revision,
            "updated_at": saved.updated_at,
        }))
    })
}

pub(super) fn draft_validate(params: &Value) -> Result<Value, HarnessError> {
    let profile_id = req_str(params, &["profile_id", "profileId"])?;
    let discovered = discovered_hooks(params);
    repository::with_conn(|conn| {
        let document = match params.get("document") {
            Some(_) => draft_document(params)?,
            None => repository::get_or_create_draft(conn, &profile_id)?.document()?,
        };
        let mut report = validate(&document, &discovered);
        validate_builtin_prompt_replacements(&document, &mut report);
        Ok(serde_json::json!({
            "profile_id": profile_id,
            "publishable": report.is_publishable(),
            "findings": report.findings,
        }))
    })
}

pub(super) fn draft_diff(params: &Value) -> Result<Value, HarnessError> {
    let profile_id = req_str(params, &["profile_id", "profileId"])?;
    repository::with_conn(|conn| {
        let draft = repository::get_or_create_draft(conn, &profile_id)?;
        let after = draft.document()?;
        let before = repository::current_version(conn, &profile_id)?
            .map(|v| v.document())
            .transpose()?
            .unwrap_or_default();
        Ok(serde_json::json!({
            "profile_id": profile_id,
            "base_version_id": draft.base_version_id,
            "changes": diff(&before, &after),
        }))
    })
}

pub(super) fn draft_publish(params: &Value) -> Result<Value, HarnessError> {
    let profile_id = req_str(params, &["profile_id", "profileId"])?;
    let revision = params.get("revision").and_then(Value::as_i64);
    let discovered = discovered_hooks(params);

    repository::with_conn(|conn| {
        let draft = repository::get_or_create_draft(conn, &profile_id)?;
        // Publishing is a write against a document the caller believes it has
        // seen. Checking the revision here closes the same window `draft.save`
        // closes: otherwise "validate then publish" could publish someone
        // else's edits.
        if let Some(expected) = revision {
            if draft.revision != expected {
                return Err(HarnessError::draft_conflict(format!(
                    "draft revision is {}, not {expected}; reload before publishing",
                    draft.revision
                )));
            }
        }
        let document = draft.document()?;
        let mut report = validate(&document, &discovered);
        validate_builtin_prompt_replacements(&document, &mut report);
        if !report.is_publishable() {
            return Err(HarnessError::validation_failed(format!(
                "draft has {} blocking finding(s): {}",
                report.errors().count(),
                report
                    .errors()
                    .map(|f| f.code.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
        let before = repository::current_version(conn, &profile_id)?
            .map(|v| v.document())
            .transpose()?
            .unwrap_or_default();
        let changes = diff(&before, &document);
        let summary = serde_json::json!({ "findings": report.findings });
        let version =
            repository::publish_draft_version(conn, &profile_id, &document, &summary, &draft)?;
        repository::append_audit(
            conn,
            "publish",
            Some(&profile_id),
            Some(&version.id),
            None,
            None,
            &serde_json::json!({ "changes": changes }),
        )?;
        Ok(serde_json::json!({
            "version": version.to_summary_json(),
            "changes": changes,
            "findings": report.findings,
        }))
    })
}

pub(super) fn validate_builtin_prompt_replacements(
    document: &HarnessBlueprint,
    report: &mut ValidationReport,
) {
    for replacement in &document.builtin_prompt_replacements {
        let Some(default) =
            crate::production::builtin_prompt_surface_default(&replacement.surface_id)
        else {
            report.findings.push(ValidationFinding {
                severity: Severity::Error,
                code: "harness_unknown_builtin_prompt_surface".into(),
                hook_id: None,
                message: format!(
                    "Unknown Native builtin prompt surface: {}",
                    replacement.surface_id
                ),
            });
            continue;
        };
        if replacement.base_default_digest != harness_core::sha256_hex(default) {
            report.findings.push(ValidationFinding {
                severity: Severity::Error,
                code: "harness_prompt_source_changed".into(),
                hook_id: None,
                message: format!(
                    "Native builtin prompt source changed: {}",
                    replacement.surface_id
                ),
            });
        }
    }
}

pub(super) fn draft_review(params: &Value) -> Result<Value, HarnessError> {
    repository::with_conn(|conn| {
        let profile_id = req_str(params, &["profile_id", "profileId"])?;
        let draft = repository::get_draft(conn, &profile_id)?
            .ok_or_else(|| HarnessError::not_found(format!("no draft for profile {profile_id}")))?;

        let doc = draft.document()?;
        let discovered = discovered_hooks(params);
        let val_report = validate(&doc, &discovered);

        let current_published = repository::current_version(conn, &profile_id)?
            .map(|v| v.document())
            .transpose()?;
        let empty_bp = HarnessBlueprint::default();
        let before_doc = current_published.as_ref().unwrap_or(&empty_bp);
        let blueprint_diff = diff(before_doc, &doc);
        let preview = prompt_preview(params)?;

        Ok(serde_json::json!({
            "profile_id": profile_id,
            "revision": draft.revision,
            "validation": val_report,
            "diff": blueprint_diff,
            "prompt_preview": preview,
        }))
    })
}

pub(super) fn draft_simulate(params: &Value) -> Result<Value, HarnessError> {
    repository::with_conn(|conn| {
        let event_str = req_str(params, &["event"])?;
        let event = harness_core::hooks::HookEvent::parse(&event_str)
            .ok_or_else(|| HarnessError::invalid(format!("unknown event: {event_str}")))?;

        let tool_name = opt_str(params, &["tool_name", "toolName"]);
        let input_val = params.get("input").cloned().unwrap_or(Value::Null);

        let context = resolved_context(conn, params)?;
        let mut steps = Vec::new();

        for hook in context.resolution.enabled_definitions() {
            if hook.event != event {
                continue;
            }
            let matches_tool = harness_core::hooks::tool_pattern_matches(
                hook.matcher.as_deref(),
                tool_name.as_deref(),
            );
            let conditions_match = hook.conditions.iter().all(|c| c.matches(&input_val));

            steps.push(serde_json::json!({
                "hook_id": hook.id.as_str(),
                "matcher": hook.matcher,
                "matches_tool": matches_tool,
                "conditions_count": hook.conditions.len(),
                "conditions_match": conditions_match,
                "would_execute": matches_tool && conditions_match,
            }));
        }

        Ok(serde_json::json!({
            "event": event_str,
            "tool_name": tool_name,
            "steps": steps,
        }))
    })
}
