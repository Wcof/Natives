//! The `HarnessControlPlane` operations behind the `harness.*` methods.
//!
//! The interface the design names (第 8 节) — `validate`, `publish`,
//! `resolve_run`, `inspect` — appears here as four groups of free functions
//! over one repository. Persistence, discovery, and projection stay internal
//! seams: a caller asks for an answer, never for the steps.
//!
//! ## The configuration hierarchy, concretely
//!
//! Decision B (design 第 21 节) is global template → project overlay → session
//! selection. That maps onto storage as:
//!
//! | Layer | Profile row | Binding row |
//! |---|---|---|
//! | Global | `kind='global_template'`, seeded as `harness.global.default` | `scope_type='global'`, `scope_id='global'` |
//! | Project | `kind='project_overlay'`, `project_id=<id>` | `scope_type='project'`, `scope_id=<project id>` |
//! | Session | any published profile — a session never owns one | `scope_type='session'`, `scope_id=<conversation id>` |
//!
//! A session *selects* a published profile; it never forks a private copy
//! (第 9.1 节), which is why there is no session-scoped draft.

use super::projection;
use super::repository::{self, GLOBAL_SCOPE_ID};
use super::{opt_i64, opt_str, req_str, HarnessError};
use harness_core::blueprint::HarnessBlueprint;
use harness_core::hooks::HookDefinition;
use harness_core::resolver::{resolve, ProfileLayer, Resolution};
use harness_core::snapshot::{LayerRef, ResolvedHarnessSnapshot};
use harness_core::validation::{diff, validate, Severity, ValidationFinding, ValidationReport};
use rusqlite::Connection;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Route one `harness.*` method.
pub fn request(method: &str, params: Value) -> Result<Value, HarnessError> {
    match method {
        "harness.overview" => overview(&params),
        "harness.topology" => topology(&params),
        "harness.workspace.get" => {
            let request = serde_json::from_value::<
                assistant_protocol::v2::HarnessWorkspaceGetRequest,
            >(params)
            .map_err(|error| {
                HarnessError::invalid(format!("invalid workspace request: {error}"))
            })?;
            workspace_get(&request)
        }
        "harness.template.list" => template_list(&params),
        "harness.hook.catalog" => hook_catalog(&params),
        "harness.profile.list" => profile_list(&params),
        "harness.profile.get" => profile_get(&params),
        "harness.profile.create" => profile_create(&params),
        "harness.profile.archive" => profile_archive(&params),
        "harness.draft.get" => draft_get(&params),
        "harness.draft.save" => draft_save(&params),
        "harness.draft.validate" => draft_validate(&params),
        "harness.draft.diff" => draft_diff(&params),
        "harness.draft.review" => draft_review(&params),
        "harness.draft.simulate" => draft_simulate(&params),
        "harness.draft.publish" => draft_publish(&params),
        "harness.version.list" => version_list(&params),
        "harness.version.rollback" => version_rollback(&params),
        "harness.binding.get" => binding_get(&params),
        "harness.binding.set" => binding_set(&params),
        "harness.run.getSnapshot" => run_get_snapshot(&params),
        "harness.audit.list" => audit_list(&params),
        "harness.prompt.preview" => prompt_preview(&params),
        "harness.source.list" => source_list(&params),
        "harness.source.acknowledgeDrift" => source_acknowledge_drift(&params),
        "harness.external.inspect" => external_inspect(&params),
        // Async long-poll is handled by `harness::request` before this blocking
        // dispatcher. Keeping it out of SQLite's blocking pool lets the Daemon
        // continue serving cancel and other RPCs while a subscriber waits.
        "harness.trace.list" => trace_list(&params),
        "harness.audit.export" => audit_export(&params),
        "project.identity.register" => project_identity_register(&params),
        "project.identity.list" => project_identity_list(&params),
        other => Err(HarnessError::invalid(format!(
            "unsupported harness method: {other}"
        ))),
    }
}

// ── shared resolution ───────────────────────────────────────────────────────

/// Where the caller says the project is, for Hook discovery.
///
/// Discovery reads files, so it needs a path. Binding lookup needs the stable
/// `project_id`. They are separate parameters on purpose: an inspection call
/// must never mint a project identity as a side effect.
fn project_path(params: &Value) -> Option<PathBuf> {
    opt_str(params, &["project_path", "projectPath"]).map(PathBuf::from)
}

fn discovered_hooks(params: &Value) -> Vec<HookDefinition> {
    crate::production_hooks::discover_production_hooks(project_path(params).as_deref())
}

struct ResolvedContext {
    resolution: Resolution,
    layers: Vec<LayerRef>,
}

fn validate_project_scope(
    conn: &Connection,
    project_id: Option<&str>,
    project_path: Option<&Path>,
) -> Result<(), HarnessError> {
    let (Some(project_id), Some(project_path)) = (project_id, project_path) else {
        return Ok(());
    };
    let identity = crate::project_identity::store::verify_for_invocation(conn, project_id)
        .map_err(HarnessError::scope_mismatch)?;
    let requested = project_path
        .canonicalize()
        .map_err(|e| HarnessError::scope_mismatch(e.to_string()))?;
    if requested != PathBuf::from(&identity.canonical_path) {
        return Err(HarnessError::scope_mismatch(format!(
            "project_id {project_id} resolves to {}, not {}",
            identity.canonical_path,
            requested.display()
        )));
    }
    Ok(())
}

fn layer_refs(
    conn: &Connection,
    project_id: Option<&str>,
    conversation_id: Option<&str>,
) -> Result<(Vec<(ProfileLayer, HarnessBlueprint)>, Vec<LayerRef>), HarnessError> {
    let rows = repository::resolve_layers(conn, project_id, conversation_id)?;
    let mut documents = Vec::new();
    let mut refs = Vec::new();
    for (layer, profile, version) in rows {
        documents.push((layer, version.document()?));
        refs.push(LayerRef {
            layer,
            profile_id: profile.id,
            profile_name: profile.name,
            version_id: version.id,
            version_number: version.version_number,
            canonical_hash: version.canonical_hash,
        });
    }
    Ok((documents, refs))
}

fn resolved_context(conn: &Connection, params: &Value) -> Result<ResolvedContext, HarnessError> {
    let project_id = opt_str(params, &["project_id", "projectId"]);
    let conversation_id = opt_str(params, &["conversation_id", "conversationId"]);
    let path = project_path(params);
    validate_project_scope(conn, project_id.as_deref(), path.as_deref())?;
    let (documents, layers) = layer_refs(conn, project_id.as_deref(), conversation_id.as_deref())?;
    let discovered = discovered_hooks(params);
    Ok(ResolvedContext {
        resolution: resolve(&discovered, &documents),
        layers,
    })
}

fn layers_json(layers: &[LayerRef]) -> Value {
    Value::Array(
        layers
            .iter()
            .map(|l| {
                serde_json::json!({
                    "layer": l.layer.as_str(),
                    "profile_id": l.profile_id,
                    "profile_name": l.profile_name,
                    "version_id": l.version_id,
                    "version_number": l.version_number,
                    "canonical_hash": l.canonical_hash,
                })
            })
            .collect(),
    )
}

// ── inspection ──────────────────────────────────────────────────────────────

fn overview(params: &Value) -> Result<Value, HarnessError> {
    repository::with_conn(|conn| {
        let context = resolved_context(conn, params)?;
        let profiles = repository::list_profiles(conn, false)?;
        Ok(serde_json::json!({
            "topology_version": harness_core::topology::TOPOLOGY_VERSION,
            "hook_semantics_version": context.resolution.semantics.as_str(),
            "blueprint_schema_version": harness_core::blueprint::BLUEPRINT_SCHEMA_VERSION,
            "layers": layers_json(&context.layers),
            "hook_counts": projection::counts(&context.resolution),
            "issues": context.resolution.issues,
            "profiles": profiles.iter().map(|p| p.to_json()).collect::<Vec<_>>(),
            // Phase 3 owns per-invocation telemetry. Saying so beats an empty
            // array a caller would read as "nothing ever ran".
            "telemetry": { "available": false, "reason": "phase_3" },
        }))
    })
}

fn topology(params: &Value) -> Result<Value, HarnessError> {
    repository::with_conn(|conn| {
        let context = resolved_context(conn, params)?;
        let mut value = projection::topology(&context.resolution);
        if let Some(object) = value.as_object_mut() {
            object.insert("layers".into(), layers_json(&context.layers));
        }
        Ok(value)
    })
}

fn hook_catalog(params: &Value) -> Result<Value, HarnessError> {
    repository::with_conn(|conn| {
        let context = resolved_context(conn, params)?;
        let mut value = projection::hook_catalog(&context.resolution);
        if let Some(object) = value.as_object_mut() {
            object.insert("layers".into(), layers_json(&context.layers));
        }
        Ok(value)
    })
}

// ── profiles ────────────────────────────────────────────────────────────────

fn profile_list(params: &Value) -> Result<Value, HarnessError> {
    let include_archived = params
        .get("include_archived")
        .or_else(|| params.get("includeArchived"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    repository::with_conn(|conn| {
        let profiles = repository::list_profiles(conn, include_archived)?;
        Ok(serde_json::json!({
            "profiles": profiles.iter().map(|p| p.to_json()).collect::<Vec<_>>(),
        }))
    })
}

fn profile_get(params: &Value) -> Result<Value, HarnessError> {
    let id = req_str(params, &["profile_id", "profileId", "id"])?;
    repository::with_conn(|conn| {
        let profile = repository::get_profile(conn, &id)?
            .ok_or_else(|| HarnessError::not_found(format!("profile not found: {id}")))?;
        let current = repository::current_version(conn, &id)?;
        Ok(serde_json::json!({
            "profile": profile.to_json(),
            "current_version": current.as_ref().map(|v| v.to_summary_json()),
            "document": current
                .as_ref()
                .map(|v| serde_json::from_str::<Value>(&v.document_json).unwrap_or(Value::Null)),
        }))
    })
}

fn profile_create(params: &Value) -> Result<Value, HarnessError> {
    let name = req_str(params, &["name"])?;
    let kind = req_str(params, &["kind"])?;
    if !["global_template", "project_overlay"].contains(&kind.as_str()) {
        return Err(HarnessError::invalid(format!(
            "unknown profile kind: {kind}"
        )));
    }
    let project_id = opt_str(params, &["project_id", "projectId"]);
    if kind == "project_overlay" && project_id.is_none() {
        return Err(HarnessError::invalid(
            "project_overlay requires a project_id; an overlay bound to nothing is unreachable",
        ));
    }
    let description = opt_str(params, &["description"]).unwrap_or_default();
    let id = format!("hp-{}", uuid::Uuid::new_v4());

    repository::with_conn(|conn| {
        if let Some(project_id) = project_id.as_deref() {
            crate::project_identity::store::get(conn, project_id)
                .map_err(HarnessError::invalid)?
                .ok_or_else(|| {
                    HarnessError::scope_mismatch(format!(
                        "unknown stable project identity: {project_id}"
                    ))
                })?;
        }
        let profile = repository::insert_profile(
            conn,
            &id,
            &name,
            &description,
            &kind,
            project_id.as_deref(),
        )?;
        repository::append_audit(
            conn,
            "profile_create",
            Some(&id),
            None,
            None,
            None,
            &serde_json::json!({ "name": name, "kind": kind }),
        )?;
        Ok(serde_json::json!({ "profile": profile.to_json() }))
    })
}

fn profile_archive(params: &Value) -> Result<Value, HarnessError> {
    let id = req_str(params, &["profile_id", "profileId", "id"])?;
    repository::with_conn(|conn| {
        repository::archive_profile(conn, &id)?;
        repository::append_audit(
            conn,
            "profile_archive",
            Some(&id),
            None,
            None,
            None,
            &serde_json::json!({}),
        )?;
        Ok(serde_json::json!({ "ok": true, "profile_id": id }))
    })
}

// ── drafts ──────────────────────────────────────────────────────────────────

fn draft_document(params: &Value) -> Result<HarnessBlueprint, HarnessError> {
    let raw = params
        .get("document")
        .ok_or_else(|| HarnessError::invalid("document is required"))?;
    HarnessBlueprint::parse(raw).map_err(HarnessError::validation_failed)
}

fn draft_get(params: &Value) -> Result<Value, HarnessError> {
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

fn draft_save(params: &Value) -> Result<Value, HarnessError> {
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

fn draft_validate(params: &Value) -> Result<Value, HarnessError> {
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

fn draft_diff(params: &Value) -> Result<Value, HarnessError> {
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

fn draft_publish(params: &Value) -> Result<Value, HarnessError> {
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

fn validate_builtin_prompt_replacements(
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

// ── versions ────────────────────────────────────────────────────────────────

fn version_list(params: &Value) -> Result<Value, HarnessError> {
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
fn version_rollback(params: &Value) -> Result<Value, HarnessError> {
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

fn binding_scope(params: &Value) -> Result<(String, String), HarnessError> {
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

fn binding_get(params: &Value) -> Result<Value, HarnessError> {
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

fn binding_set(params: &Value) -> Result<Value, HarnessError> {
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

// ── run snapshots and audit ─────────────────────────────────────────────────

fn run_get_snapshot(params: &Value) -> Result<Value, HarnessError> {
    let run_id = req_str(params, &["run_id", "runId"])?;
    repository::with_conn(|conn| match repository::get_run_snapshot(conn, &run_id)? {
        Some(snapshot) => Ok(serde_json::json!({
            "run_id": run_id,
            "resolved": true,
            "canonical_hash": snapshot.canonical_hash(),
            "snapshot": snapshot,
        })),
        // Honest absence, not an error: a Run started before the resolve seam
        // was wired, or one that never reached start, genuinely has no
        // snapshot. The caller can tell that apart from a failure.
        None => Ok(serde_json::json!({
            "run_id": run_id,
            "resolved": false,
            "snapshot": Value::Null,
        })),
    })
}

fn audit_list(params: &Value) -> Result<Value, HarnessError> {
    let limit = opt_i64(params, &["limit"], 50, 500);
    repository::with_conn(|conn| {
        Ok(serde_json::json!({ "entries": repository::list_audit(conn, limit)? }))
    })
}

fn prompt_preview(params: &Value) -> Result<Value, HarnessError> {
    repository::with_conn(|conn| {
        let context = resolved_context(conn, params)?;
        let mut prompt_blocks = Vec::new();
        let mut replacements = std::collections::BTreeMap::new();
        for layer in context.layers.iter() {
            if let Some(version) = repository::get_version(conn, &layer.version_id)? {
                let doc = version.document()?;
                for replacement in doc.builtin_prompt_replacements {
                    replacements.insert(replacement.surface_id.clone(), replacement);
                }
                for block in doc.prompt_blocks.iter().filter(|b| b.enabled) {
                    prompt_blocks.push(block.clone());
                }
            }
        }
        let mut blocks = prompt_blocks
            .iter()
            .map(|block| {
                serde_json::json!({
                    "id": block.id, "name": block.name, "order": block.order, "placement": block.placement,
                    "source_digest": harness_core::sha256_hex(&block.markdown),
                    "token_estimate": block.markdown.chars().count().div_ceil(4),
                })
            })
            .collect::<Vec<_>>();
        blocks.sort_by_key(|b| b.get("order").and_then(Value::as_i64).unwrap_or_default());
        let surface_id = crate::production::CREATIVE_DRAFT_PROMPT_SURFACE_ID;
        let default = crate::production::builtin_prompt_surface_default(surface_id)
            .expect("registered Native prompt surface");
        let replaced = replacements.contains_key(surface_id);
        let effective_digest = harness_core::sha256_hex(
            replacements
                .get(surface_id)
                .map(|item| item.markdown.as_str())
                .unwrap_or(default),
        );
        let replacements = replacements.into_values().collect::<Vec<_>>();
        let project_path = opt_str(params, &["project_path", "projectPath"]).map(PathBuf::from);
        let compiled = crate::production::compile_effective_prompt(
            Some("creative_draft"),
            None,
            None,
            project_path.as_deref(),
            None,
            &prompt_blocks,
            &replacements,
            None,
        );
        let prompt_summary = harness_core::PromptPlanSummary::from(&compiled);
        Ok(serde_json::json!({
            "blocks": blocks,
            "layers": prompt_summary.layers,
            "effective_prompt_hash": prompt_summary.effective_prompt_hash,
            "token_estimate": prompt_summary.token_estimate,
            "builtin_surfaces": [{
                "surface_id": surface_id,
                "default_markdown": default,
                "default_digest": harness_core::sha256_hex(default),
                "effective_digest": effective_digest,
                "replaced": replaced,
            }],
            "raw_persisted": false
        }))
    })
}

fn source_list(params: &Value) -> Result<Value, HarnessError> {
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

fn source_acknowledge_drift(params: &Value) -> Result<Value, HarnessError> {
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

fn trace_list(params: &Value) -> Result<Value, HarnessError> {
    let run_id = opt_str(params, &["run_id", "runId"]);
    let limit = opt_i64(params, &["limit"], 100, 200);
    let after = opt_i64(params, &["after_sequence", "afterSequence"], 0, i64::MAX);
    repository::with_conn(|conn| {
        Ok(
            serde_json::json!({"entries": repository::list_run_hook_trace(conn, run_id.as_deref(), after, limit)?, "page_size": limit, "after_sequence": after}),
        )
    })
}

fn audit_export(params: &Value) -> Result<Value, HarnessError> {
    let limit = opt_i64(params, &["limit"], 50, 500);
    repository::with_conn(|conn| {
        Ok(serde_json::json!({"entries": repository::list_audit(conn, limit)?, "redacted": true}))
    })
}

// ── the Run start seam ──────────────────────────────────────────────────────

/// Resolve and persist the Harness a Run will use, then hand it back.
///
/// **This is the single seam the Run start path needs.** Design 第 5.3 节 puts
/// it after `run.start` validation and before `ProductionRuntime` builds
/// `AgentEngine`:
///
/// ```text
/// Assistant run.start
///   → existing RunManager validation
///   → harness::control_plane::resolve_run   ← here
///   → existing Provider routing and credential lease
///   → existing ProductionRuntime / AgentEngine path
/// ```
///
/// It is deliberately callable and fully tested without that call site
/// existing, because `run_manager.rs` is owned by another workstream in this
/// round. Wiring it is one statement; until then `harness.run.getSnapshot`
/// reports `resolved: false` rather than inventing evidence.
///
/// An error here must fail the Run (第 16 节): a Run whose Harness cannot be
/// proved is worse than a Run that did not start.
pub fn resolve_run(
    run_id: &str,
    conversation_id: Option<&str>,
    project_id: Option<&str>,
    project: Option<&Path>,
) -> Result<RunHarnessPlan, HarnessError> {
    resolve_run_with_tool_plan(run_id, conversation_id, project_id, project, &[])
}

pub fn resolve_run_with_tool_plan(
    run_id: &str,
    conversation_id: Option<&str>,
    project_id: Option<&str>,
    project: Option<&Path>,
    tool_schemas: &[agent_core::ToolSchema],
) -> Result<RunHarnessPlan, HarnessError> {
    let mut plan =
        prepare_run_with_tool_plan(run_id, conversation_id, project_id, project, tool_schemas)?;
    persist_prepared_run_plan(&mut plan, None)?;
    Ok(plan)
}

/// Resolve live Harness inputs and freeze Tool evidence without writing a
/// partial Run snapshot. Production uses this seam so the exact effective
/// prompt can be compiled before the immutable evidence row is inserted.
pub fn prepare_run_with_tool_plan(
    run_id: &str,
    conversation_id: Option<&str>,
    project_id: Option<&str>,
    project: Option<&Path>,
    tool_schemas: &[agent_core::ToolSchema],
) -> Result<RunHarnessPlan, HarnessError> {
    let discovered = crate::production_hooks::discover_production_hooks(project);
    repository::with_conn(|conn| {
        validate_project_scope(conn, project_id, project)?;
        let (documents, refs) = layer_refs(conn, project_id, conversation_id)?;
        let drift_profile = refs.first().map(|r| r.profile_id.clone());
        let base_version_id = refs.first().map(|r| r.version_id.clone());
        let mut mismatches = Vec::new();
        for hook in &discovered {
            let digest =
                harness_core::sha256_hex(&serde_json::to_string(&hook.kind).unwrap_or_default());
            if let Some((published, mode)) = repository::source_digest(conn, hook.id.as_str())? {
                if published != digest && mode == "tracked" {
                    mismatches.push(serde_json::json!({
                        "source_id": hook.id.as_str(),
                        "published_digest": published,
                        "observed_digest": digest,
                        "policy": "tracked",
                        "observed_at": chrono::Utc::now().to_rfc3339(),
                        "manifest": {"scope": hook.source.scope, "origin": hook.source.origin}
                    }));
                }
            }
            let _ = repository::sync_source(conn, hook.id.as_str(), &digest, "tracked")?;
        }
        if let (Some(profile_id), Some(base_version_id)) = (drift_profile, base_version_id) {
            let candidate = serde_json::Value::Array(mismatches);
            if candidate.as_array().is_some_and(|items| !items.is_empty()) {
                let cursor = repository::append_notice(
                    conn,
                    "source_drift",
                    Some(&profile_id),
                    Some(run_id),
                )?;
                super::publish_notice(cursor);
                let had_draft = repository::get_draft(conn, &profile_id)?.is_some();
                let _ = repository::ensure_source_drift_candidate(conn, &profile_id, &candidate)?;
                if !had_draft {
                    repository::append_audit(
                        conn,
                        "source_drift_ack",
                        Some(&profile_id),
                        Some(&base_version_id),
                        None,
                        None,
                        &serde_json::json!({"candidate": true, "source_count": candidate.as_array().map_or(0, Vec::len)}),
                    )?;
                }
            }
        }
        let resolution = resolve(&discovered, &documents);
        let mut snapshot = ResolvedHarnessSnapshot::new(
            run_id,
            conversation_id.map(str::to_string),
            project_id.map(str::to_string),
            refs,
            &resolution,
            chrono::Utc::now().to_rfc3339(),
        );
        let mut prompt_plan = harness_core::PromptPlanSummary::default();
        let mut prompt_blocks = Vec::new();
        let mut native_hooks = Vec::new();
        let mut builtin_prompt_replacements = std::collections::BTreeMap::new();
        for (_, document) in &documents {
            native_hooks.extend(document.native_hooks.iter().cloned());
            for replacement in &document.builtin_prompt_replacements {
                let Some(default) =
                    crate::production::builtin_prompt_surface_default(&replacement.surface_id)
                else {
                    return Err(HarnessError::validation_failed(format!(
                        "unknown Native builtin prompt surface: {}",
                        replacement.surface_id
                    )));
                };
                let current_digest = harness_core::sha256_hex(default);
                if replacement.base_default_digest != current_digest {
                    return Err(HarnessError::validation_failed(format!(
                        "harness_prompt_source_changed: {}",
                        replacement.surface_id
                    )));
                }
                builtin_prompt_replacements
                    .insert(replacement.surface_id.clone(), replacement.clone());
                prompt_plan
                    .source_digests
                    .push(harness_core::sha256_hex(&replacement.markdown));
                prompt_plan.token_estimate += replacement.markdown.chars().count().div_ceil(4);
            }
            for block in document.prompt_blocks.iter().filter(|b| b.enabled) {
                prompt_blocks.push(block.clone());
                prompt_plan
                    .source_digests
                    .push(harness_core::sha256_hex(&block.markdown));
                prompt_plan.token_estimate += block.markdown.chars().count().div_ceil(4);
            }
        }
        snapshot.prompt_plan = prompt_plan;
        let tools = tool_schemas
            .iter()
            .map(|schema| harness_core::ToolPlanEntry {
                name: schema.name.clone(),
                source: schema
                    .name
                    .strip_prefix("mcp__")
                    .and_then(|rest| rest.split_once("__").map(|(server, _)| server))
                    .map(|server| format!("mcp:{server}"))
                    .unwrap_or_else(|| "builtin".into()),
                schema_digest: harness_core::sha256_hex(&harness_core::canonical_json(
                    &schema.input_schema,
                )),
            })
            .collect::<Vec<_>>();
        snapshot.tool_plan = harness_core::ToolPlanSummary {
            canonical_hash: harness_core::sha256_hex(
                &serde_json::to_string(&tools).unwrap_or_default(),
            ),
            tools,
        };
        Ok(RunHarnessPlan {
            snapshot,
            resolution,
            prompt_blocks,
            native_hooks,
            builtin_prompt_replacements: builtin_prompt_replacements.into_values().collect(),
        })
    })
}

/// Bind the compiled Provider prompt to a prepared Run and atomically persist
/// the immutable Harness evidence. Raw prompt text is intentionally discarded.
pub fn persist_run_plan(
    plan: &mut RunHarnessPlan,
    compiled_prompt: &harness_core::CompiledPromptPlan,
) -> Result<(), HarnessError> {
    persist_prepared_run_plan(plan, Some(compiled_prompt))
}

fn persist_prepared_run_plan(
    plan: &mut RunHarnessPlan,
    compiled_prompt: Option<&harness_core::CompiledPromptPlan>,
) -> Result<(), HarnessError> {
    if let Some(compiled_prompt) = compiled_prompt {
        plan.snapshot.prompt_plan = compiled_prompt.into();
    }
    repository::with_conn(|conn| repository::insert_run_snapshot(conn, &plan.snapshot))
}

/// What one Run start produced: the evidence, and the thing to execute.
///
/// The two differ and must not be conflated. `snapshot` is **redacted** — a
/// Hook URL's query string is stripped before it is written down — so
/// compiling from it would fire a different HTTP request than the user
/// configured. `resolution` keeps the live values and is the only thing that
/// may be compiled.
pub struct RunHarnessPlan {
    /// Redacted, persisted, safe to show. Never compile from this.
    pub snapshot: ResolvedHarnessSnapshot,
    /// Live values, never persisted, never sent to the Renderer.
    pub resolution: Resolution,
    pub prompt_blocks: Vec<harness_core::blueprint::PromptBlock>,
    pub native_hooks: Vec<harness_core::blueprint::NativeHookSpecV3>,
    pub builtin_prompt_replacements: Vec<harness_core::blueprint::BuiltinPromptReplacementSpecV4>,
}

impl RunHarnessPlan {
    /// Compile the Hooks this plan says should run, in resolved order.
    ///
    /// Built from the same [`Resolution`] the snapshot was derived from rather
    /// than from a second discovery pass — a second pass is exactly how a Run
    /// ends up doing something its own evidence does not describe.
    pub fn compile(&self, project: Option<&Path>) -> agent_core::HookRegistry {
        let definitions: Vec<HookDefinition> = self.resolution.enabled_definitions();
        crate::production_hooks::compile_production_hooks_with_native(
            &definitions,
            &self.native_hooks,
            project,
        )
    }
}

fn workspace_get(
    request: &assistant_protocol::v2::HarnessWorkspaceGetRequest,
) -> Result<Value, HarnessError> {
    let params = serde_json::to_value(request)
        .map_err(|error| HarnessError::internal(format!("serialize workspace request: {error}")))?;
    let prompt_plan = prompt_preview(&params)?;
    repository::with_conn(|conn| {
        let overview_val = overview(&params)?;
        let topology_val = topology(&params)?;
        let catalog_val = hook_catalog(&params)?;

        let profile_id = request.profile_id.clone();
        let draft_val = if let Some(pid) = &profile_id {
            repository::get_draft(conn, pid).ok().flatten().map(|d| {
                serde_json::json!({
                    "profile_id": d.profile_id,
                    "base_version_id": d.base_version_id,
                    "revision": d.revision,
                    "updated_at": d.updated_at,
                    "document": serde_json::from_str::<Value>(&d.document_json)
                        .unwrap_or(Value::Null),
                    "source_candidate": d.source_candidate_json
                        .as_deref()
                        .and_then(|value| serde_json::from_str::<Value>(value).ok()),
                })
            })
        } else {
            None
        };

        Ok(serde_json::json!({
            "overview": overview_val,
            "topology": topology_val,
            "catalog": catalog_val,
            "prompt_plan": prompt_plan,
            "draft": draft_val,
        }))
    })
}

fn template_list(_params: &Value) -> Result<Value, HarnessError> {
    Ok(serde_json::json!({
        "items": [
            {
                "id": "tpl-project-prompt",
                "name": "项目提示词策略",
                "description": "规范 AI 项目级全局架构约定与安全防守基线",
                "category": "prompt",
                "template": {
                    "schema_version": 3,
                    "prompt_blocks": [{
                        "id": "11111111-1111-1111-1111-111111111111",
                        "name": "项目架构准则",
                        "markdown": "## 项目架构与规范\n- 必须使用 RTK 执行 Shell 命令\n- 严禁假数据与空 fallback",
                        "enabled": true,
                        "order": 1,
                        "placement": "after_project_instructions"
                    }]
                }
            },
            {
                "id": "tpl-cmd-quality-gate",
                "name": "Command 质量门禁",
                "description": "在提交代码与删除敏感目录前触发指令质量审计",
                "category": "command",
                "template": {
                    "schema_version": 3,
                    "native_hooks": [{
                        "id": "22222222-2222-2222-2222-222222222222",
                        "name": "Git Command Gate",
                        "enabled": true,
                        "event": "PreToolUse",
                        "order": 10,
                        "matcher": "run_command|Bash",
                        "conditions": [{
                            "field": "command",
                            "operator": "regex_match",
                            "pattern": "git\\s+(commit|push)"
                        }],
                        "timeout_ms": 10000,
                        "failure_policy": "fail",
                        "adapter": {
                            "type": "command",
                            "program": "cargo",
                            "args": ["test", "--workspace"],
                            "trusted": false
                        }
                    }]
                }
            },
            {
                "id": "tpl-http-audit-notice",
                "name": "HTTP 审计通知",
                "description": "工具调用后向审计 Webhook 发送结构化通知",
                "category": "http",
                "template": {
                    "schema_version": 3,
                    "native_hooks": [{
                        "id": "33333333-3333-3333-3333-333333333333",
                        "name": "HTTP Audit Webhook",
                        "enabled": true,
                        "event": "PostToolUse",
                        "order": 20,
                        "matcher": "*",
                        "conditions": [],
                        "timeout_ms": 5000,
                        "failure_policy": "skip",
                        "adapter": {
                            "type": "http",
                            "url": "http://127.0.0.1:8080/audit",
                            "allow_hosts": ["127.0.0.1", "localhost"]
                        }
                    }]
                }
            },
            {
                "id": "tpl-mcp-security-scan",
                "name": "MCP 安全扫描",
                "description": "调用高危 MCP 工具前执行二次安全检查",
                "category": "mcp",
                "template": {
                    "schema_version": 3,
                    "native_hooks": [{
                        "id": "44444444-4444-4444-4444-444444444444",
                        "name": "MCP Tool Gate",
                        "enabled": true,
                        "event": "PreToolUse",
                        "order": 5,
                        "matcher": "mcp:*",
                        "conditions": [],
                        "timeout_ms": 10000,
                        "failure_policy": "fail",
                        "adapter": {
                            "type": "mcp_tool",
                            "server_id": "security_scanner",
                            "tool_name": "scan_input",
                            "input_template": "{\"input\": \"${input}\"}"
                        }
                    }]
                }
            },
            {
                "id": "tpl-prompt-risk-assessment",
                "name": "Prompt 风险判断",
                "description": "使用同 Provider 模型执行 Tool 使用前的风险判断",
                "category": "prompt_eval",
                "template": {
                    "schema_version": 3,
                    "native_hooks": [{
                        "id": "55555555-5555-5555-5555-555555555555",
                        "name": "Safety Risk Evaluation",
                        "enabled": true,
                        "event": "PreToolUse",
                        "order": 2,
                        "matcher": "run_command",
                        "conditions": [],
                        "timeout_ms": 8000,
                        "failure_policy": "fail",
                        "adapter": {
                            "type": "prompt",
                            "template": "Evaluate if the tool input executes irreversible systemic damage. Reply allow or deny."
                        }
                    }]
                }
            },
            {
                "id": "tpl-agent-completion-check",
                "name": "Agent 完成度检查",
                "description": "Run 结束前启动 Agent 检查任务完成质量",
                "category": "agent",
                "template": {
                    "schema_version": 3,
                    "native_hooks": [{
                        "id": "66666666-6666-6666-6666-666666666666",
                        "name": "Agent Quality Assancer",
                        "enabled": true,
                        "event": "Stop",
                        "order": 1,
                        "matcher": "*",
                        "conditions": [],
                        "timeout_ms": 30000,
                        "failure_policy": "skip",
                        "adapter": {
                            "type": "agent",
                            "prompt": "Verify if all user requirement criteria are met in project repository.",
                            "max_steps": 3,
                            "readonly_tools": ["view_file", "list_dir", "grep_search"]
                        }
                    }]
                }
            }
        ]
    }))
}

fn draft_review(params: &Value) -> Result<Value, HarnessError> {
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

fn draft_simulate(params: &Value) -> Result<Value, HarnessError> {
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

fn external_inspect(params: &Value) -> Result<Value, HarnessError> {
    let p_path = project_path(params);
    let report = super::external_inspector::inspect_external_runtimes(p_path.as_deref());
    serde_json::to_value(report).map_err(|e| HarnessError::internal(e.to_string()))
}

fn project_identity_register(params: &Value) -> Result<Value, HarnessError> {
    repository::with_conn(|conn| {
        let path = req_str(params, &["canonical_path", "canonicalPath", "path"])?;
        let name = opt_str(params, &["name"]).unwrap_or_else(|| {
            Path::new(&path)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "project".into())
        });
        repository::register_project_identity(conn, &path, &name)
    })
}

fn project_identity_list(_params: &Value) -> Result<Value, HarnessError> {
    repository::with_conn(|conn| {
        let items = repository::list_project_identities(conn)?;
        Ok(serde_json::json!({ "items": items }))
    })
}
