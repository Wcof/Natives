//! The Run start seam: resolve live Harness inputs, freeze Tool evidence, and
//! persist the immutable snapshot a Run will be judged against.

use super::super::repository;
use super::super::{publish_notice, HarnessError};
use super::control_profile::{layer_refs, validate_project_scope};
use harness_core::hooks::HookDefinition;
use harness_core::resolver::{resolve, Resolution};
use harness_core::snapshot::ResolvedHarnessSnapshot;
use std::path::Path;

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
                publish_notice(cursor);
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
