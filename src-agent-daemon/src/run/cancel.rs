//! Cancel tree and cancellation.

use super::manager::RunManager;
use agent_core::TransitionMetadata;
use assistant_protocol::v2::{CancelRunRequest, RunStatusV2, RunV2};

impl RunManager {
    pub async fn cancel(&self, req: CancelRunRequest) -> Result<RunV2, String> {
        // Phase 1: journal Cancelling via commit_status (sole lifecycle authority).
        // Never mutate memory Run.status directly — DB + lifecycle event + memory + broadcast
        // must succeed as one commit_transition.
        {
            let current = self
                .get_run(&req.run_id)
                .ok_or_else(|| "run not found".to_string())?;
            if current.status.is_terminal() {
                return Ok(current);
            }
            if current.status != RunStatusV2::Cancelling {
                if let Err(e) = self.commit_status(
                    &req.run_id,
                    RunStatusV2::Cancelling,
                    TransitionMetadata::empty()
                        .with_reason("cancelling")
                        .with_lifecycle_hint("cancelling"),
                ) {
                    // Fail-safe: still signal cancel tree so work stops, but do not claim
                    // success or invent a pseudo-terminal status.
                    self.runtime.cancel_run(&req.run_id).await;
                    return Err(e);
                }
            }
        }

        // Phase 2: signal tree + grace + force cleanup (domain only; no status writes).
        self.runtime.cancel_run(&req.run_id).await;

        // Phase 3: Cancelled only after registry quiet (or Failed on cleanup fail).
        // quiet is scoped to this run's tree (not global active_count).
        let quiet = self.runtime.execution.tree_quiet(&req.run_id).await;
        let current = self
            .get_run(&req.run_id)
            .ok_or_else(|| "run not found".to_string())?;
        if current.status.is_terminal() {
            return Ok(current);
        }
        if quiet {
            self.commit_status(
                &req.run_id,
                RunStatusV2::Cancelled,
                TransitionMetadata::empty()
                    .with_reason("cancelled")
                    .with_lifecycle_hint("cancelled"),
            )
        } else {
            self.commit_status(
                &req.run_id,
                RunStatusV2::Failed,
                TransitionMetadata::empty()
                    .with_reason("cancel_cleanup_failed")
                    .with_lifecycle_hint("failed"),
            )
        }
    }
}
