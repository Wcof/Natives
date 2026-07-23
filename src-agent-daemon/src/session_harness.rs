//! Compatibility shim — SessionCoordinator lives in `agent_core`.
//!
//! The daemon no longer maintains a second SessionHarness. All coordination
//! goes through [`agent_core::SessionCoordinator`] (via `prompt_queue_store::global_harness`).

pub use agent_core::{
    is_parallel_safe_tool, CoordinatorAction, HarnessAction, PromptSource, QueueItem,
    QueueItemStatus, SafePoint, SessionActorSnapshot, SessionCoordinator, SessionHarness,
    PARALLEL_SAFE_MAX_CONCURRENCY,
};

/// Historical constant name used by older daemon code.
pub const MAX_PARALLEL_READONLY_TOOLS: usize = PARALLEL_SAFE_MAX_CONCURRENCY;

/// Process-wide coordinator handle (same instance as prompt_queue_store).
pub fn global_session_harness() -> std::sync::Arc<SessionCoordinator> {
    crate::prompt_queue_store::global_harness()
}
