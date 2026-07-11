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

pub use run_state::*;
pub use permissions::*;
pub use subagents::*;