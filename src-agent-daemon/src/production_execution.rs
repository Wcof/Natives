//! Execution-domain `ProductionRuntime` methods (extracted from `production.rs`, task-01 structure).
//!
//! Cancel tree (registry token tree + waiters), execution tokens, and the task surface
//! (`task_output` / `list_tasks` / `kill_task` / `wait_task`). Public API unchanged — these are
//! the same methods on the same type.

use agent_core::SubAgentStatus;
use crate::runtime::TaskRecord;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

impl super::ProductionRuntime {
    /// Sole production cancel API: cancel `run_id` and every nested descendant
    /// child run via the ExecutionRegistry token tree (task-03).
    ///
    /// Order: signal tokens → wake waiters → grace → force process/join cleanup.
    /// Lifecycle status (`Cancelling`/`Cancelled`) is committed by RunManager,
    /// not here. Domain cleanup only.
    ///
    /// All cancel entry points (RPC, UI, kill_task, parent cancel) must call this.
    pub async fn cancel_run_tree(&self, run_id: &str) {
        // Prefer registry tree; fall back to subagent metadata for legacy paths.
        let mut run_ids = self.execution.list_tree(run_id).await;
        let descendants = self.subagents.list_descendants(run_id).await;
        for d in &descendants {
            if !run_ids.contains(&d.run_id) {
                run_ids.push(d.run_id.clone());
            }
        }
        if run_ids.is_empty() {
            run_ids.push(run_id.to_string());
        }

        // Signal cooperative cancel on registry tokens + engines.
        let _ = self.execution.signal_tree(run_id).await;
        {
            let engines = self.engines.lock().await;
            for rid in &run_ids {
                if let Some(engine) = engines.get(rid) {
                    engine.request_cancel();
                }
            }
        }

        // Wake permission / assignment waiters bound to this tree.
        self.cancel_waiters_for_runs(&run_ids).await;

        // Metadata + task_outputs for every descendant task.
        for d in &descendants {
            let _ = self
                .subagents
                .update_status(&d.id, SubAgentStatus::Cancelled)
                .await;
            if let Some(rec) = self.task_outputs.lock().await.get_mut(&d.id) {
                rec.status = "cancelled".into();
            }
        }
        let _ = self.subagents.cascade_cancel_metadata(run_id).await;

        // Grace + force via registry (process kill, join abort).
        let outcome = self.execution.cancel_tree(run_id).await;
        if !outcome.quiet {
            eprintln!(
                "[production] cancel_tree cleanup incomplete for {run_id}: {:?}",
                outcome.errors
            );
        }

        // Drop engine handles for this tree after force phase.
        {
            let mut engines = self.engines.lock().await;
            for rid in &outcome.run_ids {
                engines.remove(rid);
            }
        }

        // No terminal lifecycle events here — RunManager::cancel commits Cancelled/Failed
        // after cleanup quiet. Domain cleanup only.
    }

    /// Alias for tree cancel — never cancel a single node without descendants.
    pub async fn cancel_run(&self, run_id: &str) {
        self.cancel_run_tree(run_id).await;
    }

    /// Daemon shutdown entry (task-13 wiring): cancel every root and wait quiet.
    pub async fn cancel_all_execution_roots(&self) -> Vec<crate::runtime::CancelCleanupOutcome> {
        self.execution.cancel_all_execution_roots().await
    }

    async fn cancel_waiters_for_runs(&self, run_ids: &[String]) {
        let stale = self.interactions.cancel_runs(run_ids).await;
        for pid in stale {
            let _ = crate::interaction_store::mark_resolved(
                &pid,
                serde_json::json!({ "approved": false, "scope": "once", "reason": "cancelled" }),
            );
        }
    }

    /// Ensure a run is registered in the cancel tree; returns its token.
    pub async fn ensure_execution_token(
        &self,
        run_id: &str,
        parent_run_id: Option<&str>,
    ) -> Result<CancellationToken, String> {
        let reg = if let Some(parent) = parent_run_id {
            if self.execution.is_registered(parent).await {
                self.execution.register_child(run_id, parent).await?
            } else {
                // Legacy/test callers can name a parent that is not owned by
                // this process. It cannot participate in this cancel tree, so
                // the child must be a root here; the Run row still preserves
                // the parent relationship.
                self.execution.register_root(run_id).await?
            }
        } else {
            self.execution.register_root(run_id).await?
        };
        Ok(reg.token)
    }

    // spawn_child_task removed (ADR-0016): dead duplicate of the real task
    // path (PermissionGatedTools::execute_task -> RunManager). Zero callers.

    pub async fn task_output(&self, task_id: &str) -> Option<TaskRecord> {
        self.task_outputs.lock().await.get(task_id).cloned()
    }

    /// Snapshot of process-local background tasks (`task_id` → record).
    pub async fn list_tasks(&self) -> Vec<(String, TaskRecord)> {
        self.task_outputs
            .lock()
            .await
            .iter()
            .map(|(id, rec)| (id.clone(), rec.clone()))
            .collect()
    }

    pub async fn kill_task(&self, task_id: &str) -> bool {
        let child_run_id = self
            .task_outputs
            .lock()
            .await
            .get(task_id)
            .map(|r| r.run_id.clone());
        if let Some(rid) = child_run_id {
            // Tree cancel on the child run (and any nested grandchildren).
            self.cancel_run_tree(&rid).await;
            let _ = self
                .subagents
                .update_status(task_id, SubAgentStatus::Cancelled)
                .await;
            if let Some(rec) = self.task_outputs.lock().await.get_mut(task_id) {
                rec.status = "cancelled".into();
            }
            return true;
        }
        false
    }

    /// Poll `task_outputs` until the task reaches a terminal status or `timeout_ms` elapses.
    ///
    /// Terminal: completed / failed / cancelled / interrupted.
    /// Returns the task record on success; `"timeout"` or `"unknown task_id: …"` on error.
    pub async fn wait_task(&self, task_id: &str, timeout_ms: u64) -> Result<TaskRecord, String> {
        fn is_terminal(status: &str) -> bool {
            matches!(status, "completed" | "failed" | "cancelled" | "interrupted")
        }

        let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms.max(1));
        let mut saw_task = false;
        loop {
            if let Some(rec) = self.task_output(task_id).await {
                saw_task = true;
                if is_terminal(&rec.status) {
                    return Ok(rec);
                }
            } else if saw_task {
                // Task disappeared after we had seen it — treat as cancelled.
                return Err(format!("unknown task_id: {task_id}"));
            }
            if tokio::time::Instant::now() >= deadline {
                if !saw_task {
                    return Err(format!("unknown task_id: {task_id}"));
                }
                return Err("timeout".into());
            }
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let sleep = remaining.min(Duration::from_millis(50));
            tokio::time::sleep(sleep).await;
        }
    }
}
