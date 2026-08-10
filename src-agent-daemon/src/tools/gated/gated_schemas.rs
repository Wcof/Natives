//! Model-visible tool schema / capability surface for `PermissionGatedTools`.
//!
//! `list_tool_schemas` and `list_tool_capabilities` build the tool surface the
//! `AgentEngine` advertises: Gateway-registered tools filtered by the run's
//! allowlist, MCP schema selection and Plan Mode, plus each tool's execution
//! mode and side-effect class. Split out of `gated.rs` (ARCH-002).
//!
//! W3 note: the two `EngineToolRuntime` methods moved into the single impl
//! block in `gated_execute.rs` (Rust forbids multiple `impl Trait for Type`
//! blocks for the same trait). This file keeps the module split and the
//! documentation; the methods themselves live in `gated_execute`.
