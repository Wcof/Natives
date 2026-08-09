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
