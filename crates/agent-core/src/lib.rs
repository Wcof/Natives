//! # agent-core
//!
//! Core agent runtime: run state machine, event log, context management,
//! checkpointing, and orchestration.
//!
//! This crate implements the single authority for Run state transitions,
//! ensuring that the UI and providers never duplicate transition rules.

pub mod run_state;
pub mod permissions;
pub mod subagents;
pub mod event_seq;
pub mod doom_loop;
pub mod engine;
pub mod hooks;
pub mod hook_handlers;
pub mod profile;
pub mod context;
pub mod mcp;
pub mod compaction;
pub mod session_harness;

pub use run_state::*;
pub use permissions::*;
pub use subagents::*;
pub use event_seq::*;
pub use doom_loop::*;
pub use engine::*;
pub use hooks::*;
pub use hook_handlers::*;
pub use profile::*;
pub use context::*;
pub use mcp::*;
// Prefer explicit imports for compaction to avoid clashing with context helpers.
pub use compaction::{compact_messages as compact_history_messages, repair_dangling_tool_calls, CompactResult};
pub use session_harness::{
    is_parallel_safe_tool, HarnessAction, PromptSource, QueueItem, SafePoint, SessionHarness,
    PARALLEL_SAFE_MAX_CONCURRENCY,
};