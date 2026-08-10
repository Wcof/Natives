//! Run terminal commit + workspace rewind handlers (W9 split from
//! rpc/handlers/run.rs). `run.finish` delegates terminal transitions to
//! `RunManager::commit_status` (the sole committer); rewind goes through the
//! bound CheckpointManager authority — never a process-global default DB.

use super::run::{required_param, run_manager};

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
