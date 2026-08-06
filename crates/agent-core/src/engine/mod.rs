//! Agent engine — single authority for Turn execution and provider/tool orchestration.
//!
//! This module is the extracted directory structure for the original `engine.rs`.
//! The public API is re-exported from `engine_core.rs`; tests live in `tests.rs`.
//!
//! # Module structure
//!
//! - `engine_core.rs` — AgentEngine, EngineProvider, EngineMessage, EngineError,
//!   conversion functions, and all public types.
//! - `tests.rs` — unit tests (previously inline in engine.rs).
//!
//! Future splits should extract provider/tool types and conversion functions to
//! their own files (`provider.rs`, `tool_runtime.rs`, `conversion.rs`).

pub mod engine_core;
pub use engine_core::*;
