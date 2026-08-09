//! Run authority domain modules (ARCH-002).
//!
//! The daemon-side sole Run Authority's implementation is split here by
//! lifecycle/state ownership. The public API is exposed through the
//! `crate::run_manager` facade (`RunManager`, `global_run_manager`,
//! `install_global_for_test`, `install_memory_global_for_test`,
//! `protocol_version`), so existing callers are unchanged.
//!
//! # Lineage Contract: Retry / Continue / Fork / Resume
//!
//! Every operation that creates a new run from a prior run's checkpoint
//! records a durable `resume_plan` row and inherits the source snapshot's
//! typed transcript (AgentMessage), not the legacy EngineMessage history.
//!
//! ## Retry
//! - Creates a new run in the same conversation with a distinct `run_id`.
//! - Re-executes the last turn: the new run starts from the checkpoint
//!   whose `turn_id` matches the retry source, and the user content is
//!   the original input that triggered that turn.
//! - `retry_of_turn_id` on the checkpoint row links the new run to the
//!   exact turn being retried.
//! - Blocked if the source run has `uncertain` side effects.
//!
//! ## Continue
//! - Creates a new run from a durable checkpoint (the caller selects which
//!   checkpoint via `checkpoint_id`).
//! - The new run share the conversation_id but has a distinct run_id.
//! - User content may be provided by the caller; if absent, the last known
//!   content is used.
//! - A `resume_plan` row with action `'continue'` and decision
//!   `'SafeToContinue'` is persisted before the run is returned.
//! - Blocked if the source run has `uncertain` side effects.
//!
//! ## Fork (not a separate API — equivalent to Continue with new content)
//! - Fork is logically the same as Continue: a new run from a checkpoint.
//! - The caller provides new user content to diverge from the source path.
//! - No separate `fork` action — `resume_plan.action = 'continue'` covers
//!   both continuation and divergence from a checkpoint.
//!
//! ## Resume
//! - Creates a new run from a durable checkpoint after a crash/interrupt.
//! - The resume decision is recorded in a `resume_plan` row with action
//!   `'retry'` or `'continue'` and a decision of `'SafeToContinue'`,
//!   `'ConfirmationRequired'`, or `'Blocked'`.
//! - `ConfirmationRequired` means the caller must explicitly confirm before
//!   the run can proceed (e.g., uncertain side effects exist).
//! - `Blocked` means the run cannot resume (e.g., unresolved side effects).
//!
//! ## Snapshot lineage
//! - Every checkpoint captures the typed transcript (AgentMessage) at the
//!   point of commit. The typed transcript is the single source of truth
//!   for the provider context — the legacy EngineMessage is never used
//!   for checkpoint-based lineage operations.
//! - A new run created from a checkpoint inherits the snapshot's typed
//!   transcript, not the legacy EngineMessage history.
//! - The `resume_plan` table is the authoritative lineage record: it links
//!   `source_run_id` → `new_run_id` with the action, checkpoint, and
//!   decision.

pub mod cancel;
pub mod create;
pub mod lifecycle;
pub mod manager;
pub mod project_binding;
pub mod recovery;
pub mod repository;
pub mod resume;
pub mod retry;
pub mod start;
pub mod start_helpers;

pub use manager::{global_run_manager, protocol_version, RunManager};
#[cfg(test)]
pub use manager::{install_global_for_test, install_memory_global_for_test};
