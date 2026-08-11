//! Profile lifecycle and the shared resolution helpers every `harness.*`
//! inspection and Run-start path leans on.
//!
//! Owns the configuration hierarchy's raw material: the stable project
//! identity check (`validate_project_scope`), layer resolution
//! (`layer_refs` / `resolved_context`), and the profile CRUD behind
//! `harness.profile.*`.

use super::super::repository;
use super::super::{opt_str, req_str, HarnessError};
use harness_core::blueprint::HarnessBlueprint;
use harness_core::hooks::HookDefinition;
use harness_core::resolver::{resolve, ProfileLayer, Resolution};
use harness_core::snapshot::LayerRef;
use rusqlite::Connection;
use serde_json::Value;
use std::path::{Path, PathBuf};

// ── shared resolution ───────────────────────────────────────────────────────

/// Where the caller says the project is, for Hook discovery.
///
/// Discovery reads files, so it needs a path. Binding lookup needs the stable
/// `project_id`. They are separate parameters on purpose: an inspection call
/// must never mint a project identity as a side effect.
pub(super) fn project_path(params: &Value) -> Option<PathBuf> {
    opt_str(params, &["project_path", "projectPath"]).map(PathBuf::from)
}

pub(super) fn discovered_hooks(params: &Value) -> Vec<HookDefinition> {
    crate::production_hooks::discover_production_hooks(project_path(params).as_deref())
}

pub(super) struct ResolvedContext {
    pub(super) resolution: Resolution,
    pub(super) layers: Vec<LayerRef>,
}

pub(super) fn validate_project_scope(
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
    if requested.as_path() != std::path::Path::new(&identity.canonical_path) {
        return Err(HarnessError::scope_mismatch(format!(
            "project_id {project_id} resolves to {}, not {}",
            identity.canonical_path,
            requested.display()
        )));
    }
    Ok(())
}

#[allow(clippy::type_complexity)] // pre-existing: factored type alias deferred
pub(super) fn layer_refs(
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

pub(super) fn resolved_context(
    conn: &Connection,
    params: &Value,
) -> Result<ResolvedContext, HarnessError> {
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

pub(super) fn layers_json(layers: &[LayerRef]) -> Value {
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

// ── profiles ────────────────────────────────────────────────────────────────

pub(super) fn profile_list(params: &Value) -> Result<Value, HarnessError> {
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

pub(super) fn profile_get(params: &Value) -> Result<Value, HarnessError> {
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

pub(super) fn profile_create(params: &Value) -> Result<Value, HarnessError> {
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

pub(super) fn profile_archive(params: &Value) -> Result<Value, HarnessError> {
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
