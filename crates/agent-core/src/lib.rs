//! # agent-core
//!
//! Core agent runtime: run state machine, event log, context management,
//! checkpointing, and orchestration.
//!
//! This crate implements the single authority for Run state transitions,
//! ensuring that the UI and providers never duplicate transition rules.

pub mod capability;
pub mod compaction;
pub mod context;
pub mod context_snapshot;
pub mod doom_loop;
pub mod engine;
pub mod event_seq;
pub mod facade;
pub mod fs_util;
pub mod hook_handlers;
pub mod hooks;
pub mod input;
pub mod lineage;
pub mod live_event;
pub mod mcp;
pub mod message;
pub mod metrics;
pub mod permissions;
pub mod profile;
pub mod run_state;
pub mod session_coordinator;
/// Historical module path; re-exports [`session_coordinator`].
pub mod session_harness;
pub mod subagents;

pub use context::{
    assemble_context, compact_messages, discover_agents_md, AssembledContext, ContextBudget,
};
pub use context_snapshot::*;
pub use doom_loop::*;
pub use engine::*;
pub use event_seq::*;
pub use hook_handlers::*;
pub use hooks::*;
pub use input::*;
pub use lineage::*;
pub use live_event::{LiveEvent, LiveEventBus};
pub use mcp::*;
pub use message::*;
pub use permissions::*;
pub use profile::*;
pub use run_state::*;
pub use subagents::*;
// Prefer explicit imports for compaction to avoid clashing with context helpers.
pub use compaction::{
    compact_messages as compact_history_messages, repair_dangling_tool_calls, CompactResult,
};
pub use session_coordinator::{
    is_parallel_safe_tool, CoordinatorAction, HarnessAction, PromptSource, QueueItem,
    QueueItemStatus, SafePoint, SessionActorSnapshot, SessionCoordinator, SessionHarness,
    PARALLEL_SAFE_MAX_CONCURRENCY,
};
