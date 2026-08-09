//! Permission-gated tool runtime (extracted from `production.rs`, task-01 structure).
//!
//! `PermissionGatedTools` is the `EngineToolRuntime` the AgentEngine drives: it
//! enforces the allowlist, ProjectIdentity verification, permission Ask/Allow/Deny,
//! subagent budgets, checkpoints, and the task/mcp orchestration tools. Behavior is
//! unchanged from the pre-split single-file version.

// Facade: the split implementation lives in `crate::tools` (ARCH-002). The
// glob re-export keeps the public paths of the original single-file module
// working (`crate::production_tools::PermissionGatedTools` and the
// `EngineToolRuntime` / progress-sink impls).
pub use crate::tools::*;

/// NE-P0-08 / 19.3-⑤: field-level redaction of the Parent Tool Input's
/// `system_prompt` for exports/logs.
///
/// The `task` tool accepts a parent-authored `system_prompt` (the dynamic Child
/// Directive). It is prompt text only — never consulted for permissions or the
/// tool surface — but it still must not appear verbatim in event payloads, hook
/// inputs, exports, or daemon logs. This helper replaces the text with a
/// `[REDACTED system_prompt sha256:<digest>]` marker so the field keeps
/// digest-level visibility (same persona ⇒ same digest) without exposing the
/// body. Every other field passes through untouched.
pub fn redact_task_system_prompt(input: &serde_json::Value) -> serde_json::Value {
    crate::tools::subagent::redact_task_input_system_prompt(input)
}
