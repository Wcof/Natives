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
use harness_core::validation::{diff, validate};
use rusqlite::Connection;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Route one `harness.*` method.
pub fn request(method: &str, params: Value) -> Result<Value, HarnessError> {
    match method {
        "harness.overview" => overview(&params),
        "harness.topology" => topology(&params),
        "harness.hook.catalog" => hook_catalog(&params),
        "harness.profile.list" => profile_list(&params),
        "harness.profile.get" => profile_get(&params),
        "harness.profile.create" => profile_create(&params),
        "harness.profile.archive" => profile_archive(&params),
        "harness.draft.get" => draft_get(&params),
        "harness.draft.save" => draft_save(&params),
        "harness.draft.validate" => draft_validate(&params),
        "harness.draft.diff" => draft_diff(&params),
        "harness.draft.publish" => draft_publish(&params),
        "harness.version.list" => version_list(&params),
        "harness.version.rollback" => version_rollback(&params),
        "harness.binding.get" => binding_get(&params),
        "harness.binding.set" => binding_set(&params),
        "harness.run.getSnapshot" => run_get_snapshot(&params),
        "harness.audit.list" => audit_list(&params),
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
    let (documents, layers) =
        layer_refs(conn, project_id.as_deref(), conversation_id.as_deref())?;
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
    if !["global_template", "project_overlay", "session_overlay"].contains(&kind.as_str()) {
        return Err(HarnessError::invalid(format!("unknown profile kind: {kind}")));
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
        let report = validate(&document, &discovered);
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
        let report = validate(&document, &discovered);
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
        let version = repository::publish_version(conn, &profile_id, &document, &summary)?;
        repository::clear_draft(conn, &profile_id)?;
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
    repository::with_conn(|conn| {
        let source = repository::get_version(conn, &version_id)?
            .ok_or_else(|| HarnessError::not_found(format!("version not found: {version_id}")))?;
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
    let scope_type = opt_str(params, &["scope_type", "scopeType"]).unwrap_or_else(|| "global".into());
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
        return Err(HarnessError::invalid(format!("unknown binding mode: {mode}")));
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
    let discovered = crate::production_hooks::discover_production_hooks(project);
    repository::with_conn(|conn| {
        let (documents, refs) = layer_refs(conn, project_id, conversation_id)?;
        let resolution = resolve(&discovered, &documents);
        let snapshot = ResolvedHarnessSnapshot::new(
            run_id,
            conversation_id.map(str::to_string),
            project_id.map(str::to_string),
            refs,
            &resolution,
            chrono::Utc::now().to_rfc3339(),
        );
        repository::insert_run_snapshot(conn, &snapshot)?;
        Ok(RunHarnessPlan {
            snapshot,
            resolution,
        })
    })
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
}

impl RunHarnessPlan {
    /// Compile the Hooks this plan says should run, in resolved order.
    ///
    /// Built from the same [`Resolution`] the snapshot was derived from rather
    /// than from a second discovery pass — a second pass is exactly how a Run
    /// ends up doing something its own evidence does not describe.
    pub fn compile(&self, project: Option<&Path>) -> agent_core::HookRegistry {
        let definitions: Vec<HookDefinition> = self.resolution.enabled_definitions();
        crate::production_hooks::compile_production_hooks(&definitions, project)
    }
}
