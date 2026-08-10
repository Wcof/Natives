//! Agent engine — single authority for Turn execution and provider/tool orchestration.
//!
//! This module is the extracted directory structure for the original `engine.rs`.
//! The public API is re-exported from the per-responsibility modules; tests
//! live in `tests.rs`.
//!
//! # Module structure
//!
//! - `engine_core.rs` — `AgentEngine` state machine and the run loop (only
//!   orchestration; provider/tool contracts and conversion live below).
//! - `provider.rs` — `EngineProvider` stream seam and provider-side types
//!   (`EngineMessage`, `EngineProviderEvent`, `ProviderStopReason`, ...).
//! - `tool_runtime.rs` — `EngineToolRuntime` seam and tool-side types
//!   (`ToolSchema`, `ToolExecutionResult`, `ToolProgressUpdate`, ...).
//! - `conversion.rs` — pure message serialisation/shaping between engine
//!   domain types, provider JSON, hooks and agent messages.
//! - `error.rs` — `EngineError` shared by the loop and both seams.
//! - `tests.rs` — unit tests.

mod conversion;
mod error;
pub mod provider;
pub mod tool_runtime;

pub mod engine_core;
pub use engine_core::*;
// W9: impl AgentEngine 按行为域拆分（events/safe_point/tools/compaction/input）。
mod engine_compaction;
mod engine_events;
mod engine_input;
mod engine_run;
mod engine_safe_point;
mod engine_tools;

pub use conversion::{
    agent_messages_from_json, agent_messages_to_engine_messages, engine_messages_to_agent_messages,
    try_agent_messages_from_json,
};
pub use error::EngineError;
pub use provider::*;
pub use tool_runtime::*;
