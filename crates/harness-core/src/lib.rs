//! Natives Harness Core — the pure Harness domain model.
//!
//! This crate owns what a Harness *is*, never how it is executed, persisted, or
//! rendered. Execution adapters live in `agent-core`; persistence and RPC live
//! in the Daemon.
//!
//! Design: `docs/superpowers/specs/2026-07-26-native-harness-control-plane-design.md`

pub mod hooks;
pub mod session_actor;

pub use session_actor::{
    CoordinatorAction, PromptSource, QueueItem, QueueItemStatus, SafePoint, SessionActorSnapshot,
    SessionCoordinator, is_parallel_safe_tool, PARALLEL_SAFE_MAX_CONCURRENCY,
};

pub use hooks::{
    Condition, ConditionOperator, HookDefinition, HookEvent, HookFailurePolicy, HookId, HookKind,
    HookScope, HookSource, tool_pattern_matches,
};
