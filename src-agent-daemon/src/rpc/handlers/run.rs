//! `run.*` RPC handlers and run-domain helpers.
//!
//! Split out of the former single-file `rpc.rs` (A2-03).

use assistant_protocol::v2::RunEventV2;

// {A2-03} MAX_WIRE_REPLAY_EVENTS const (moved verbatim from rpc.rs)
/// Cap on how many run events a single `run.getEvents` / `run.subscribe`
/// wire response carries (R-P4 / T11). A run's full history can exceed
/// `MAX_FRAME_BYTES` once serialized; the client pages forward through its
/// `last_sequence` cursor, and the Renderer keeps only a 2000-event window.
/// In-process callers (resume / subagent reconciliation) keep the full replay
/// through `replay_checked` — only the UDS boundary is bounded.
pub const MAX_WIRE_REPLAY_EVENTS: usize = 2000;

// {A2-03} cap_wire_replay (moved verbatim from rpc.rs)
/// Keep the oldest `MAX_WIRE_REPLAY_EVENTS` events of a replay batch
/// (ascending sequence order) so a bounded batch always advances the client
/// cursor monotonically.
pub(crate) fn cap_wire_replay(events: Vec<RunEventV2>) -> Vec<RunEventV2> {
    if events.len() <= MAX_WIRE_REPLAY_EVENTS {
        events
    } else {
        events.into_iter().take(MAX_WIRE_REPLAY_EVENTS).collect()
    }
}

// {A2-03} wire_replay_cap_tests mod (moved verbatim from rpc.rs)
#[cfg(test)]
mod wire_replay_cap_tests {
    use super::*;

    fn event(run_id: &str, seq: u64) -> RunEventV2 {
        RunEventV2 {
            event_id: format!("e-{seq}"),
            global_sequence: seq,
            run_id: run_id.to_string(),
            run_sequence: seq,
            sequence: seq,
            timestamp: chrono::Utc::now(),
            payload: assistant_protocol::v2::RunEventKind::TextDelta {
                text: format!("d{seq}"),
            },
        }
    }

    #[test]
    fn cap_keeps_batches_within_bounds() {
        let small = (1..=10).map(|s| event("r", s)).collect::<Vec<_>>();
        assert_eq!(cap_wire_replay(small).len(), 10, "small batch is untouched");
        let big = (1..=(MAX_WIRE_REPLAY_EVENTS + 500) as u64)
            .map(|s| event("r", s))
            .collect::<Vec<_>>();
        let capped = cap_wire_replay(big);
        assert_eq!(capped.len(), MAX_WIRE_REPLAY_EVENTS);
        assert_eq!(
            capped.first().map(|e| e.effective_run_sequence()),
            Some(1),
            "oldest events are kept so the client cursor advances monotonically"
        );
        assert_eq!(
            capped.last().map(|e| e.effective_run_sequence()),
            Some(MAX_WIRE_REPLAY_EVENTS as u64)
        );
    }
}

// {A2-03} run_manager (moved verbatim from rpc.rs)
/// Process-wide run manager (Phase 1 authority).
pub(crate) fn run_manager() -> &'static crate::run_manager::RunManager {
    // Must share the process-wide authority with Tauri host shims.
    crate::run_manager::global_run_manager()
}

// {A2-03} register_run_disabled_tools (moved verbatim from rpc.rs)
/// CONTRACT-001: register the typed subtract-only `disabled_tools` for a
/// created Run, keyed by `run.id`. `None`/empty = no subtraction (advertised
/// surface unchanged). Consumes the typed `CreateRunRequest` field only — the
/// raw `params.get("disabled_tools")` shadow read was deleted from the
/// `run.create` handler, so this typed field is the single read path.
pub(crate) async fn register_run_disabled_tools(
    disabled_tools: &Option<Vec<String>>,
    run_id: &str,
) {
    if let Some(disabled) = disabled_tools.as_ref().filter(|list| !list.is_empty()) {
        run_manager()
            .runtime
            .set_run_disabled_tools(run_id, disabled.clone())
            .await;
    }
}

// {A2-03} required_param (moved verbatim from rpc.rs)
/// Read a required non-empty string param, accepting snake_case and camelCase aliases.
pub(crate) fn required_param<'a>(
    params: &'a serde_json::Value,
    aliases: &[&str],
) -> Result<&'a str, String> {
    for key in aliases {
        if let Some(value) = params.get(*key).and_then(|v| v.as_str()) {
            let value = value.trim();
            if !value.is_empty() {
                return Ok(value);
            }
        }
    }
    Err(format!("{} is required", aliases[0]))
}

// {A2-03} handle_run_list_children (moved verbatim from rpc.rs)
/// `run.listChildren` — direct children of `parent_run_id`.
///
/// Source of truth is the RunManager projection (the sole owner of run identity), not
/// the live ExecutionRegistry: children of a finished parent must still be listable.
/// Depth-1 only; callers recurse if they want the whole tree.
pub(crate) fn handle_run_list_children(
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let parent = required_param(params, &["parent_run_id", "parentRunId", "run_id", "runId"])?;
    let mut children: Vec<assistant_protocol::v2::RunV2> = run_manager()
        .list_runs(None)
        .into_iter()
        .filter(|r| r.parent_run_id.as_deref() == Some(parent))
        .collect();
    children.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(serde_json::json!({
        "parent_run_id": parent,
        "children": children,
    }))
}

// {A2-03} handle_run_get_activity (moved verbatim from rpc.rs)
/// `run.getActivity` — point-in-time activity snapshot for one run.
///
/// Every field is projected from an existing source (RunManager run record, the run's
/// child projection, and the persisted interaction table). Nothing is synthesised.
pub(crate) fn handle_run_get_activity(
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let run_id = required_param(params, &["run_id", "runId"])?;
    let run = run_manager()
        .get_run(run_id)
        .ok_or_else(|| format!("run not found: {run_id}"))?;
    let child_run_ids: Vec<String> = run_manager()
        .list_runs(None)
        .into_iter()
        .filter(|r| r.parent_run_id.as_deref() == Some(run_id))
        .map(|r| r.id)
        .collect();
    // Best-effort: a missing/locked interaction table must not fail the snapshot.
    let pending = crate::interaction_store::list_pending(serde_json::json!({ "run_id": run_id }))
        .ok()
        .and_then(|v| v.get("interactions").cloned())
        .unwrap_or_else(|| serde_json::Value::Array(Vec::new()));
    let pending_count = pending.as_array().map(|a| a.len()).unwrap_or(0);
    Ok(serde_json::json!({
        "run_id": run.id,
        "conversation_id": run.conversation_id,
        "status": run.status,
        "runtime_id": run.runtime_id,
        "provider_id": run.provider_id,
        "model_id": run.model_id,
        "agent_profile_id": run.agent_profile_id,
        "step_count": run.step_count,
        "max_steps": run.max_steps,
        "retry_count": run.retry_count,
        "last_event_sequence": run.last_event_sequence,
        "created_at": run.created_at,
        "started_at": run.started_at,
        "finished_at": run.finished_at,
        "error_code": run.error_code,
        "child_run_ids": child_run_ids,
        "pending_interaction_count": pending_count,
        "pending_interactions": pending,
    }))
}

// {A2-03} handle_run_finish (moved verbatim from rpc.rs)
/// `run.finish` — externally driven terminal commit.
///
/// Delegates to `RunManager::commit_status`, which is the sole committer of run
/// lifecycle transitions (see docs/architecture/NATIVE-DAEMON-CAPABILITY-MAP.md).
/// This handler never writes run state itself, and terminal races stay idempotent
/// because `commit_status` returns the existing run when it is already terminal.
pub(crate) fn handle_run_finish(params: &serde_json::Value) -> Result<serde_json::Value, String> {
    use assistant_protocol::v2::RunStatusV2;
    let run_id = required_param(params, &["run_id", "runId"])?;
    let requested = params
        .get("status")
        .or_else(|| params.get("outcome"))
        .and_then(|v| v.as_str())
        .unwrap_or("completed")
        .trim()
        .to_ascii_lowercase();
    let target = match requested.as_str() {
        "completed" | "complete" | "success" | "succeeded" => RunStatusV2::Completed,
        "failed" | "failure" | "error" => RunStatusV2::Failed,
        "cancelled" | "canceled" => RunStatusV2::Cancelled,
        "interrupted" => RunStatusV2::Interrupted,
        other => {
            return Err(format!(
                "status must be a terminal state (completed|failed|cancelled|interrupted), got: {other}"
            ))
        }
    };
    let mut metadata = agent_core::TransitionMetadata::empty().with_lifecycle_hint(match target {
        RunStatusV2::Completed => "completed",
        RunStatusV2::Failed => "failed",
        RunStatusV2::Cancelled => "cancelled",
        _ => "interrupted",
    });
    if let Some(reason) = params.get("reason").and_then(|v| v.as_str()) {
        if !reason.trim().is_empty() {
            metadata = metadata.with_reason(reason.trim());
        }
    }
    let run = run_manager().commit_status(run_id, target, metadata)?;
    serde_json::to_value(run).map_err(|e| e.to_string())
}

// {A2-03} handle_rewind_rpc (moved verbatim from rpc.rs)
pub(crate) async fn handle_rewind_rpc(
    method: &str,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let run_id = params
        .get("run_id")
        .and_then(|v| v.as_str())
        .ok_or("run_id is required")?;
    // Project path is taken from the bound run identity — callers cannot inject
    // an arbitrary path to restore files into another project (task-07/10).
    let run = crate::run_manager::global_run_manager()
        .get_run(run_id)
        .ok_or_else(|| format!("run not found: {run_id}"))?;
    // Legacy unbound ProjectIdentity: refuse restore entirely (no caller path injection).
    if run.project_id.is_none() {
        return Err(
            "workspace.restore refused: run has no verified ProjectIdentity; restored=0".into(),
        );
    }
    let bound_path = run.project_path.map(std::path::PathBuf::from);
    let project_path = if let Some(bound) = bound_path {
        if let Some(caller) = params
            .get("project_path")
            .or_else(|| params.get("project_root"))
            .and_then(|v| v.as_str())
        {
            let caller_p = std::path::PathBuf::from(caller);
            let b = bound.canonicalize().unwrap_or_else(|_| bound.clone());
            let c = caller_p.canonicalize().unwrap_or(caller_p);
            if b != c {
                return Err(
                    "workspace.restore refused: caller project_path does not match run identity; restored=0"
                        .into(),
                );
            }
        }
        bound
    } else {
        return Err(
            "workspace.restore refused: run has project_id but no bound project_path; restored=0"
                .into(),
        );
    };
    let paths: Option<Vec<String>> = params.get("paths").and_then(|v| {
        v.as_array().map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
    });
    // Rewind must use the same CheckpointManager/SQLite authority that owns
    // this Run's events and ledger, not a process-global default database.
    let mgr = crate::run_manager::global_run_manager()
        .runtime
        .checkpoint_manager();
    match method {
        "run.rewindPreview" | "run.rewind" => {
            // Deprecated: ambiguous "whole run rewind". Prefer workspace.restore*.
            let replacement = if method.contains("Preview") {
                "workspace.restorePreview"
            } else {
                "workspace.restore"
            };
            Ok(serde_json::json!({
                "deprecated": true,
                "method": method,
                "scope": "workspace_file_only",
                "message": "run.rewind/run.rewindPreview are deprecated. Use workspace.restorePreview / workspace.restore for checkpoint-covered files only. Conversation rewind and execution replay are separate APIs. External side-effects are not rolled back.",
                "replacement": replacement,
            }))
        }
        "workspace.restorePreview" => {
            let preview = mgr
                .rewind_preview_async(run_id, &project_path, paths.as_deref())
                .await?;
            let mut value = serde_json::to_value(preview)
                .map_err(|e| format!("serialize restore preview: {e}"))?;
            if let Some(obj) = value.as_object_mut() {
                let coverage = crate::side_effect_ledger::coverage_for_run(run_id)?;
                obj.insert("coverage".into(), serde_json::json!(coverage));
                obj.insert("scope".into(), serde_json::json!("workspace_file_only"));
            }
            Ok(value)
        }
        "workspace.restore" => {
            let checkpoint_id = params
                .get("checkpoint_id")
                .and_then(|v| v.as_str())
                .ok_or("checkpoint_id is required")?;
            let policy = params
                .get("conflict_policy")
                .and_then(|v| v.as_str())
                .unwrap_or("fail");
            let restored = mgr
                .workspace_restore_async(
                    run_id,
                    checkpoint_id,
                    &project_path,
                    paths.as_deref(),
                    policy,
                )
                .await?;
            // Restore audit event — does not alter old Run terminal status.
            crate::run_manager::global_run_manager().events().append(
                run_id,
                assistant_protocol::v2::RunEventKind::CheckpointRewound {
                    checkpoint_id: checkpoint_id.to_string(),
                    paths: restored.clone(),
                    conflict_policy: Some(policy.to_string()),
                },
            );
            Ok(serde_json::json!({
                "ok": true,
                "scope": "workspace_file_only",
                "checkpoint_id": checkpoint_id,
                "restored_paths": restored,
            }))
        }
        other => Err(format!("unsupported restore method: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // {A2-03} run_create_registers_typed_disabled_tools_only test (moved verbatim from rpc.rs)
    /// CONTRACT-001: disabled_tools must flow through the typed CreateRunRequest
    /// field, not a raw params shadow read. `Some(list)` registers per run.id;
    /// `None`/empty (the raw-shadow-equivalent "absent from typed") is ignored —
    /// the typed field is the single read path after the raw
    /// `params.get("disabled_tools")` shadow read was deleted.
    #[tokio::test]
    async fn run_create_registers_typed_disabled_tools_only() {
        let _env_guard = crate::storage::DataStore::env_test_lock();
        let mgr = crate::run_manager::install_memory_global_for_test();

        // Positive: typed Some(list) registers per run.id (run.start applies the
        // final subtract-only step).
        register_run_disabled_tools(&Some(vec!["read_file".to_string()]), "run-disabled-pos").await;
        assert_eq!(
            mgr.runtime
                .take_run_disabled_tools("run-disabled-pos")
                .await,
            Some(vec!["read_file".to_string()]),
            "typed disabled_tools must be registered for the created run"
        );

        // Negative: typed None is ignored (a raw-only shadow value would NOT be
        // read anymore — the typed field is the single source).
        register_run_disabled_tools(&None, "run-disabled-none").await;
        assert_eq!(
            mgr.runtime
                .take_run_disabled_tools("run-disabled-none")
                .await,
            None,
            "typed None must be ignored (no subtraction registered)"
        );

        // Negative: empty list is treated as no subtraction.
        register_run_disabled_tools(&Some(vec![]), "run-disabled-empty").await;
        assert_eq!(
            mgr.runtime
                .take_run_disabled_tools("run-disabled-empty")
                .await,
            None,
            "empty typed list must be ignored (no subtraction registered)"
        );
    }
}

// {A2-03} dispatch_run: routes the `run.*` method family. Arms moved verbatim
// from handle_rpc's match in the former single-file rpc.rs; the trailing
// `_ =>` keeps any method that reaches this handler but has no arm here on the
// honest unsupported path (R-B1).
pub(crate) async fn dispatch_run(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    request: &assistant_protocol::v1::daemon::RpcRequest,
) {
    use crate::rpc::{send_error, send_success, write_stream_frame};
    use assistant_protocol::error::{error_codes, DaemonError, ErrorCategory};
    use assistant_protocol::v2::methods::names;
    use assistant_protocol::v2::{
        CancelRunRequest, ContinueRunRequest, CreateRunRequest, ReplayRunRequest, ResumeRunRequest,
        RetryRunRequest, StartRunRequest,
    };
    match request.method.as_str() {
        names::RUN_CREATE => {
            match serde_json::from_value::<CreateRunRequest>(request.params.clone()) {
                Ok(req) => {
                    // CONTRACT-001: disabled_tools arrives in the typed
                    // CreateRunRequest (single source in assistant-protocol).
                    // The old raw `params.get("disabled_tools")` shadow read is
                    // deleted — the typed field is the only read path (a raw
                    // value absent from the typed struct is ignored).
                    let run_disabled_tools = req.disabled_tools.clone();
                    match run_manager().create_run(req) {
                        Ok(run) => {
                            register_run_disabled_tools(&run_disabled_tools, &run.id).await;
                            send_success(
                                writer,
                                &request.request_id,
                                &request.client_id,
                                &request.session_token,
                                serde_json::to_value(run).unwrap_or_default(),
                            )
                            .await
                        }
                        Err(e) => {
                            send_error(
                                writer,
                                &DaemonError::new(
                                    "run_create_failed",
                                    ErrorCategory::Internal,
                                    false,
                                    e,
                                ),
                            )
                            .await
                        }
                    }
                }
                Err(e) => {
                    send_error(
                        writer,
                        &DaemonError::new(
                            error_codes::INVALID_INPUT,
                            ErrorCategory::Validation,
                            false,
                            e.to_string(),
                        ),
                    )
                    .await
                }
            }
        }

        names::RUN_START => {
            match serde_json::from_value::<StartRunRequest>(request.params.clone()) {
                Ok(req) => {
                    // Non-blocking: engine runs in background so this connection can
                    // still accept run.cancel / permission.respond / run.getEvents.
                    match crate::run_manager::RunManager::start_detached_global(req) {
                        Ok(run) => {
                            send_success(
                                writer,
                                &request.request_id,
                                &request.client_id,
                                &request.session_token,
                                serde_json::to_value(run).unwrap_or_default(),
                            )
                            .await
                        }
                        Err(e) => {
                            send_error(
                                writer,
                                &DaemonError::new(
                                    "run_start_failed",
                                    ErrorCategory::Internal,
                                    true,
                                    e,
                                ),
                            )
                            .await
                        }
                    }
                }
                Err(e) => {
                    send_error(
                        writer,
                        &DaemonError::new(
                            error_codes::INVALID_INPUT,
                            ErrorCategory::Validation,
                            false,
                            e.to_string(),
                        ),
                    )
                    .await
                }
            }
        }

        names::RUN_CANCEL => {
            match serde_json::from_value::<CancelRunRequest>(request.params.clone()) {
                Ok(req) => match run_manager().cancel(req).await {
                    Ok(run) => {
                        send_success(
                            writer,
                            &request.request_id,
                            &request.client_id,
                            &request.session_token,
                            serde_json::to_value(run).unwrap_or_default(),
                        )
                        .await
                    }
                    Err(e) => {
                        send_error(
                            writer,
                            &DaemonError::new(
                                "run_cancel_failed",
                                ErrorCategory::NotFound,
                                false,
                                e,
                            ),
                        )
                        .await
                    }
                },
                Err(e) => {
                    send_error(
                        writer,
                        &DaemonError::new(
                            error_codes::INVALID_INPUT,
                            ErrorCategory::Validation,
                            false,
                            e.to_string(),
                        ),
                    )
                    .await
                }
            }
        }

        names::RUN_RETRY => {
            match serde_json::from_value::<RetryRunRequest>(request.params.clone()) {
                Ok(req) => {
                    // M1: create new run and start it detached (do not leave Queued).
                    match run_manager().retry(req) {
                        Ok(new_run) => {
                            let start_req = StartRunRequest {
                                agent_profile_id: None,
                                capability_selection: None,
                                run_id: Some(new_run.id.clone()),
                                conversation_id: Some(new_run.conversation_id.clone()),
                                provider_id: Some(new_run.provider_id.clone()),
                                model_id: Some(new_run.model_id.clone()),
                                key_id: new_run.key_id.clone(),
                                content: None,
                                attachments: None,
                                trigger_message_id: None,
                                permission_profile: Some(new_run.permission_profile.clone()),
                                max_steps: Some(new_run.max_steps),
                                project_path: new_run.project_path.clone(),
                                idempotency_key: None,
                                effort: None,
                                runtime_id: None,
                            };
                            match crate::run_manager::RunManager::start_detached_global(start_req) {
                                Ok(run) => {
                                    send_success(
                                        writer,
                                        &request.request_id,
                                        &request.client_id,
                                        &request.session_token,
                                        serde_json::to_value(run).unwrap_or_default(),
                                    )
                                    .await
                                }
                                Err(e) => {
                                    send_error(
                                        writer,
                                        &DaemonError::new(
                                            "run_retry_start_failed",
                                            ErrorCategory::Internal,
                                            true,
                                            e,
                                        ),
                                    )
                                    .await
                                }
                            }
                        }
                        Err(e) => {
                            send_error(
                                writer,
                                &DaemonError::new(
                                    "run_retry_failed",
                                    ErrorCategory::NotFound,
                                    false,
                                    e,
                                ),
                            )
                            .await
                        }
                    }
                }
                Err(e) => {
                    send_error(
                        writer,
                        &DaemonError::new(
                            error_codes::INVALID_INPUT,
                            ErrorCategory::Validation,
                            false,
                            e.to_string(),
                        ),
                    )
                    .await
                }
            }
        }

        names::RUN_CONTINUE => {
            match serde_json::from_value::<ContinueRunRequest>(request.params.clone()) {
                Ok(req) => match run_manager().continue_run(req) {
                    Ok(new_run) => {
                        let start_req = StartRunRequest {
                            agent_profile_id: new_run.agent_profile_id.clone(),
                            capability_selection: None,
                            run_id: Some(new_run.id.clone()),
                            conversation_id: Some(new_run.conversation_id.clone()),
                            provider_id: Some(new_run.provider_id.clone()),
                            model_id: Some(new_run.model_id.clone()),
                            key_id: new_run.key_id.clone(),
                            content: None,
                            attachments: None,
                            trigger_message_id: None,
                            permission_profile: Some(new_run.permission_profile.clone()),
                            max_steps: Some(new_run.max_steps),
                            project_path: new_run.project_path.clone(),
                            idempotency_key: None,
                            effort: new_run.effort.clone(),
                            runtime_id: new_run.runtime_id.clone(),
                        };
                        match crate::run_manager::RunManager::start_detached_global(start_req) {
                            Ok(run) => {
                                send_success(
                                    writer,
                                    &request.request_id,
                                    &request.client_id,
                                    &request.session_token,
                                    serde_json::to_value(run).unwrap_or_default(),
                                )
                                .await
                            }
                            Err(e) => {
                                send_error(
                                    writer,
                                    &DaemonError::new(
                                        "run_continue_start_failed",
                                        ErrorCategory::Internal,
                                        true,
                                        e,
                                    ),
                                )
                                .await
                            }
                        }
                    }
                    Err(e) => {
                        send_error(
                            writer,
                            &DaemonError::new(
                                "run_continue_failed",
                                ErrorCategory::Conflict,
                                false,
                                e,
                            ),
                        )
                        .await
                    }
                },
                Err(e) => {
                    send_error(
                        writer,
                        &DaemonError::new(
                            error_codes::INVALID_INPUT,
                            ErrorCategory::Validation,
                            false,
                            e.to_string(),
                        ),
                    )
                    .await
                }
            }
        }

        names::RUN_RESUME => {
            match serde_json::from_value::<ResumeRunRequest>(request.params.clone()) {
                Ok(req) => match run_manager().resume_run(req) {
                    Ok(response) => {
                        let new_run_id = response.new_run_id.clone();
                        if let Some(new_run_id) = new_run_id {
                            if let Some(new_run) = run_manager().get_run(&new_run_id) {
                                let start_req = StartRunRequest {
                                    agent_profile_id: None,
                                    capability_selection: None,
                                    run_id: Some(new_run.id.clone()),
                                    conversation_id: Some(new_run.conversation_id.clone()),
                                    provider_id: Some(new_run.provider_id.clone()),
                                    model_id: Some(new_run.model_id.clone()),
                                    key_id: new_run.key_id.clone(),
                                    content: None,
                                    attachments: None,
                                    trigger_message_id: None,
                                    permission_profile: Some(new_run.permission_profile.clone()),
                                    max_steps: Some(new_run.max_steps),
                                    project_path: new_run.project_path.clone(),
                                    idempotency_key: None,
                                    effort: new_run.effort.clone(),
                                    runtime_id: new_run.runtime_id.clone(),
                                };
                                match crate::run_manager::RunManager::start_detached_global(
                                    start_req,
                                ) {
                                    Ok(_) => {}
                                    Err(e) => {
                                        send_error(
                                            writer,
                                            &DaemonError::new(
                                                "run_resume_start_failed",
                                                ErrorCategory::Internal,
                                                true,
                                                e,
                                            ),
                                        )
                                        .await;
                                        return;
                                    }
                                }
                            }
                        }
                        send_success(
                            writer,
                            &request.request_id,
                            &request.client_id,
                            &request.session_token,
                            serde_json::to_value(response).unwrap_or_default(),
                        )
                        .await
                    }
                    Err(e) => {
                        send_error(
                            writer,
                            &DaemonError::new(
                                "run_resume_failed",
                                ErrorCategory::Conflict,
                                false,
                                e,
                            ),
                        )
                        .await
                    }
                },
                Err(e) => {
                    send_error(
                        writer,
                        &DaemonError::new(
                            error_codes::INVALID_INPUT,
                            ErrorCategory::Validation,
                            false,
                            e.to_string(),
                        ),
                    )
                    .await
                }
            }
        }

        names::RUN_REPLAY | names::RUN_GET_EVENTS => {
            // Non-blocking event batch (array payload for UI/Tauri poll compatibility).
            let after = request
                .params
                .get("after_sequence")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let run_id = request
                .params
                .get("run_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            #[allow(clippy::needless_return)] // return exits the outer method dispatch
            match run_manager().replay_checked(ReplayRunRequest {
                run_id: run_id.to_string(),
                after_sequence: after,
            }) {
                Ok(events) => {
                    // R-P4 / T11: bound the wire batch so a huge run never
                    // produces an oversized UDS frame. In-process callers keep
                    // the full replay; only the client-facing replay is capped.
                    let payload = match serde_json::to_value(cap_wire_replay(events)) {
                        Ok(payload) => payload,
                        Err(error) => {
                            send_error(
                                writer,
                                &DaemonError::new(
                                    error_codes::INTERNAL_ERROR,
                                    ErrorCategory::Internal,
                                    false,
                                    format!("serialize replay response failed: {error}"),
                                ),
                            )
                            .await;
                            return;
                        }
                    };
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        payload,
                    )
                    .await;
                    return;
                }
                Err(error) => {
                    send_error(
                        writer,
                        &DaemonError::new(
                            error_codes::INTERNAL_ERROR,
                            ErrorCategory::Internal,
                            false,
                            format!("authoritative event replay failed: {error}"),
                        ),
                    )
                    .await;
                    return;
                }
            }
        }

        names::RUN_SUBSCRIBE => {
            // Hybrid subscribe:
            // 1) Always return replay after_sequence (sequence gap fill).
            // 2) Optional wait_ms / mode=push: block up to wait_ms for *new*
            //    broadcast events (real-time push over long-poll style).
            // Continuous multi-line push on a dedicated connection is future work;
            // this unblocks UI without blocking cancel on a second connection.
            let after = request
                .params
                .get("after_sequence")
                .or_else(|| request.params.get("last_sequence"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let run_id = request
                .params
                .get("run_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let wait_ms = request
                .params
                .get("wait_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let want_push = request
                .params
                .get("mode")
                .and_then(|v| v.as_str())
                .map(|m| m == "push" || m == "long_poll")
                .unwrap_or(false)
                || wait_ms > 0;

            let mut events = match run_manager().replay_checked(ReplayRunRequest {
                run_id: run_id.clone(),
                after_sequence: after,
            }) {
                Ok(events) => events,
                Err(error) => {
                    send_error(
                        writer,
                        &DaemonError::new(
                            error_codes::INTERNAL_ERROR,
                            ErrorCategory::Internal,
                            false,
                            format!("authoritative event replay failed: {error}"),
                        ),
                    )
                    .await;
                    return;
                }
            };
            let mut mode = "subscribe_poll";
            if want_push && events.is_empty() {
                let timeout = std::time::Duration::from_millis(wait_ms.clamp(1, 30_000));
                let mut rx = run_manager().events().subscribe(&run_id);
                mode = "subscribe_push_wait";
                let deadline = tokio::time::Instant::now() + timeout;
                loop {
                    let left = deadline.saturating_duration_since(tokio::time::Instant::now());
                    if left.is_zero() {
                        break;
                    }
                    match tokio::time::timeout(left, rx.recv()).await {
                        Ok(Ok(ev)) if ev.effective_run_sequence() > after => {
                            events.push(ev);
                            // Drain a small batch without extra waits.
                            while let Ok(more) = rx.try_recv() {
                                if more.effective_run_sequence() > after {
                                    events.push(more);
                                }
                            }
                            break;
                        }
                        Ok(Ok(_)) => continue,
                        Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => {
                            events = match run_manager().replay_checked(ReplayRunRequest {
                                run_id: run_id.clone(),
                                after_sequence: after,
                            }) {
                                Ok(events) => events,
                                Err(error) => {
                                    send_error(
                                        writer,
                                        &DaemonError::new(
                                            error_codes::INTERNAL_ERROR,
                                            ErrorCategory::Internal,
                                            false,
                                            format!("authoritative event replay failed: {error}"),
                                        ),
                                    )
                                    .await;
                                    return;
                                }
                            };
                            break;
                        }
                        Ok(Err(_)) | Err(_) => break,
                    }
                }
            }
            // R-P4 / T11: bound the wire batch (same cap as run.getEvents) so a
            // run that accumulated a long history while the client was away
            // never produces an oversized frame. The client pages forward.
            let events = cap_wire_replay(events);
            let terminal = run_manager()
                .get_run(&run_id)
                .map(|r| r.status.is_terminal())
                .unwrap_or(false);
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({
                    "run_id": run_id,
                    "events": events,
                    "terminal": terminal,
                    "mode": mode,
                }),
            )
            .await;
        }

        names::RUN_WATCH => {
            // Persistent event stream (RunWatchStreamV2, STREAM-CONTRACT-V2):
            // 1) validate the run exists,
            // 2) establish the durable receiver AND the live receiver
            //    (subscribe_after with bounded live replay) BEFORE the ACK so no
            //    replay→subscribe window is lost,
            // 3) return a normal RPC ACK `{stream:"run.watch", streamVersion:2}`,
            // 4) replay durable events > after_durable_sequence,
            // 5) then tokio::select! multiplex durable / live / heartbeat.
            //
            // - durable receiver lag  → gap-fill by replaying from the cursor
            //   (durable facts are replayable).
            // - live receiver lag     → ResyncRequired(live); never fabricate a
            //   durable fact from the ephemeral lane.
            // - long idle             → Heartbeat so the 30s client frame
            //   timeout never fires on a healthy stream.
            // - terminal durable event → clean close.
            // - The two lanes never share a sequence namespace.
            let after_durable = request
                .params
                .get("after_durable_sequence")
                .or_else(|| request.params.get("after_sequence"))
                .or_else(|| request.params.get("last_sequence"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let after_live = request
                .params
                .get("after_live_sequence")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let run_id = request
                .params
                .get("run_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if run_id.is_empty() {
                send_error(
                    writer,
                    &DaemonError::new(
                        error_codes::INVALID_INPUT,
                        ErrorCategory::Validation,
                        false,
                        "run.watch requires run_id".to_string(),
                    ),
                )
                .await;
                return;
            }

            // 1) Run must exist before we establish any receiver or ACK.
            if run_manager().get_run(&run_id).is_none() {
                send_error(
                    writer,
                    &DaemonError::new(
                        error_codes::NOT_FOUND,
                        ErrorCategory::NotFound,
                        false,
                        format!("run not found: {run_id}"),
                    ),
                )
                .await;
                return;
            }

            // 2) Establish durable + live receivers BEFORE the ACK.
            let mut durable_rx = run_manager().events().subscribe(&run_id);

            // S1 frozen API: runtime.live_events() -> LiveEventBus. The bounded
            // ring replay arrives atomically with the live receiver.
            let live_bus = run_manager().runtime.live_events();
            let live_sub = live_bus.subscribe_after(&run_id, after_live);
            let mut live_rx = live_sub.receiver;
            let mut live_cursor = live_sub.last_sequence;
            let live_gap = live_sub.gap;
            let live_replay = live_sub.buffered;

            // 3) ACK — a normal RPC Response, then frames.
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                crate::stream_protocol::stream_ack(),
            )
            .await;

            // 4) Replay durable events > after_durable_sequence.
            let replayed = match run_manager().replay_checked(ReplayRunRequest {
                run_id: run_id.clone(),
                after_sequence: after_durable,
            }) {
                Ok(events) => events,
                Err(error) => {
                    // After the ACK the client is in frame mode; a clean close
                    // (reconnect by cursor) is safer than a malformed error frame.
                    let _ = error;
                    return;
                }
            };
            let mut durable_seq = after_durable;
            for event in cap_wire_replay(replayed) {
                let seq = event.effective_run_sequence();
                durable_seq = durable_seq.max(seq);
                if write_stream_frame(
                    writer,
                    &crate::stream_protocol::RunStreamFrameV2::durable_event(&run_id, &event),
                )
                .await
                .is_err()
                {
                    return; // client dropped — clean exit, no orphan task
                }
            }

            // 5a) Replay the bounded live prefix (> after_live_sequence) after
            //     the durable replay. Never touches the durable cursor.
            for ev in live_replay {
                if ev.live_sequence <= after_live {
                    continue;
                }
                live_cursor = live_cursor.max(ev.live_sequence);
                if write_stream_frame(
                    writer,
                    &crate::stream_protocol::RunStreamFrameV2::live_event(&run_id, &ev),
                )
                .await
                .is_err()
                {
                    return;
                }
            }
            // If the requested live cursor fell before the bounded ring start,
            // some ephemeral deltas are unrecoverable → ResyncRequired(live).
            // This is never a durable-fact gap.
            if live_gap
                && write_stream_frame(
                    writer,
                    &crate::stream_protocol::RunStreamFrameV2::resync_required(
                        &run_id,
                        crate::stream_protocol::RunStreamLane::Live,
                        "live_buffer_gap",
                    ),
                )
                .await
                .is_err()
            {
                return;
            }

            // 5b) If the run already reached terminal, clean close — never hang
            //     waiting for a broadcast that will not come.
            if run_manager()
                .get_run(&run_id)
                .map(|r| r.status.is_terminal())
                .unwrap_or(false)
            {
                return;
            }

            // 5c) Live push phase: multiplex durable / live / heartbeat.
            use crate::stream_protocol::{RunStreamFrameV2, RunStreamLane};
            let heartbeat_interval = std::time::Duration::from_secs(15);
            let mut heartbeat = tokio::time::interval(heartbeat_interval);
            heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            heartbeat.tick().await; // consume the immediate first tick
            let mut last_frame_at = std::time::Instant::now();

            loop {
                tokio::select! {
                    result = durable_rx.recv() => {
                        match result {
                            Ok(event) => {
                                let seq = event.effective_run_sequence();
                                if seq <= durable_seq {
                                    continue; // already forwarded
                                }
                                durable_seq = seq;
                                if write_stream_frame(writer, &RunStreamFrameV2::durable_event(&run_id, &event)).await.is_err() {
                                    return; // client dropped
                                }
                                last_frame_at = std::time::Instant::now();
                                if event.payload.is_terminal() {
                                    return; // terminal durable fact — clean close
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                                // Durable facts are replayable: gap-fill by cursor.
                                let replay = match run_manager().replay_checked(ReplayRunRequest {
                                    run_id: run_id.clone(),
                                    after_sequence: durable_seq,
                                }) {
                                    Ok(events) => events,
                                    Err(_) => return,
                                };
                                for event in cap_wire_replay(replay) {
                                    let seq = event.effective_run_sequence();
                                    if seq <= durable_seq {
                                        continue;
                                    }
                                    durable_seq = seq;
                                    if write_stream_frame(writer, &RunStreamFrameV2::durable_event(&run_id, &event)).await.is_err() {
                                        return;
                                    }
                                    last_frame_at = std::time::Instant::now();
                                    if event.payload.is_terminal() {
                                        return;
                                    }
                                }
                            }
                            Err(_) => return, // broadcast closed — clean close
                        }
                    }
                    result = live_rx.recv() => {
                        match result {
                            Ok(ev) => {
                                if ev.live_sequence <= live_cursor {
                                    continue;
                                }
                                live_cursor = ev.live_sequence;
                                if write_stream_frame(writer, &RunStreamFrameV2::live_event(&run_id, &ev)).await.is_err() {
                                    return;
                                }
                                last_frame_at = std::time::Instant::now();
                            }
                            Err(_) => {
                                // Live bus lag → ResyncRequired(live). Never
                                // fabricate a durable fact from the live lane.
                                if write_stream_frame(
                                    writer,
                                    &RunStreamFrameV2::resync_required(
                                        &run_id,
                                        RunStreamLane::Live,
                                        "live_buffer_gap",
                                    ),
                                )
                                .await
                                .is_err()
                                {
                                    return;
                                }
                                last_frame_at = std::time::Instant::now();
                            }
                        }
                    }
                    _ = heartbeat.tick() => {
                        if last_frame_at.elapsed() >= heartbeat_interval {
                            if write_stream_frame(
                                writer,
                                &RunStreamFrameV2::heartbeat(&run_id, durable_seq, live_cursor),
                            )
                            .await
                            .is_err()
                            {
                                return;
                            }
                            last_frame_at = std::time::Instant::now();
                        }
                    }
                }
            }
        }

        names::RUN_LIST => {
            let conversation_id = request
                .params
                .get("conversation_id")
                .and_then(|v| v.as_str());
            let runs = run_manager().list_runs(conversation_id);
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({ "runs": runs }),
            )
            .await;
        }

        names::RUN_LIST_CHILDREN => match handle_run_list_children(&request.params) {
            Ok(value) => {
                send_success(
                    writer,
                    &request.request_id,
                    &request.client_id,
                    &request.session_token,
                    value,
                )
                .await
            }
            Err(e) => {
                send_error(
                    writer,
                    &DaemonError::new(
                        error_codes::INVALID_INPUT,
                        ErrorCategory::Validation,
                        false,
                        e,
                    ),
                )
                .await
            }
        },

        names::RUN_GET_ACTIVITY => match handle_run_get_activity(&request.params) {
            Ok(value) => {
                send_success(
                    writer,
                    &request.request_id,
                    &request.client_id,
                    &request.session_token,
                    value,
                )
                .await
            }
            Err(e) => {
                let not_found = e.contains("not found");
                send_error(
                    writer,
                    &DaemonError::new(
                        if not_found {
                            error_codes::NOT_FOUND
                        } else {
                            error_codes::INVALID_INPUT
                        },
                        if not_found {
                            ErrorCategory::NotFound
                        } else {
                            ErrorCategory::Validation
                        },
                        false,
                        e,
                    ),
                )
                .await
            }
        },

        names::RUN_FINISH => match handle_run_finish(&request.params) {
            Ok(value) => {
                send_success(
                    writer,
                    &request.request_id,
                    &request.client_id,
                    &request.session_token,
                    value,
                )
                .await
            }
            Err(e) => {
                let not_found = e.contains("not found");
                send_error(
                    writer,
                    &DaemonError::new(
                        if not_found {
                            error_codes::NOT_FOUND
                        } else {
                            error_codes::INVALID_INPUT
                        },
                        if not_found {
                            ErrorCategory::NotFound
                        } else {
                            ErrorCategory::Validation
                        },
                        false,
                        e,
                    ),
                )
                .await
            }
        },

        // Unimplemented catalogue methods: fail closed (not empty success).
        // Method disposition: known→unsupported, unknown→unsupported (invalid only for bad shape).
        // promptQueue.* is handled above via prompt_queue_store (daemon DB + harness).
        "run.rewindPreview" | "run.rewind" | "workspace.restorePreview" | "workspace.restore" => {
            match handle_rewind_rpc(&request.method, &request.params).await {
                Ok(value) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        value,
                    )
                    .await
                }
                Err(e) => {
                    send_error(
                        writer,
                        &DaemonError::new(
                            error_codes::INVALID_INPUT,
                            ErrorCategory::Validation,
                            false,
                            e,
                        ),
                    )
                    .await
                }
            }
        }

        _ => {
            let status = assistant_protocol::v2::method_status(&request.method);
            let code = match status {
                assistant_protocol::v2::MethodStatus::Unsupported => "unsupported",
                assistant_protocol::v2::MethodStatus::InvalidRequest => "invalid_request",
                assistant_protocol::v2::MethodStatus::Implemented => "internal_error",
            };
            let err = DaemonError::new(
                code,
                ErrorCategory::Unsupported,
                false,
                format!(
                    "method not implemented: {} (status={status:?}; see daemon.getCapabilities)",
                    request.method
                ),
            );
            send_error(writer, &err).await;
        }
    }
}
