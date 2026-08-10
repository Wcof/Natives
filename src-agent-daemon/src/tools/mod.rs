//! Permission-gated tool runtime, split from `production_tools.rs` by
//! responsibility (ARCH-002).
//!
//! The original single-file `production_tools.rs` grew to ~5.8k lines. This
//! module splits it into the domains the `PermissionGatedTools` facade drives:
//! permission/grant decisions, plan mode, MCP execution, subagent/task
//! execution, progress batching, artifact attachment, invocation/context
//! construction, and allowlist/pattern policy. Behavior is unchanged from the
//! pre-split single-file version — this is a pure structural split.

pub mod artifact;
pub mod gated;
pub mod invocation;
pub mod mcp;
pub mod permission;
pub mod plan;
pub mod policy;
pub mod progress;
pub mod subagent;
// W9 split: subagent task execution / watcher / terminal / requeue live in
// sibling modules under `tools/`; subagent.rs re-exports the public paths.
mod subagent_execute;
mod subagent_requeue;
mod subagent_terminal;
mod subagent_watcher;

// Cross-module helpers referenced by the facade and its siblings.
pub use artifact::attach_tool_output_artifact;
pub use gated::PermissionGatedTools;
pub use invocation::{model_visible_tool_schemas, validate_tool_limit, MAX_MODEL_VISIBLE_TOOLS};
pub use mcp::pending_mcp_call_count;
pub use permission::{hook_permission_gate, HookPermissionGate, HOOK_AUTO_APPROVE_SCOPE};
pub use plan::{PLAN_APPROVAL_SCOPE, PLAN_APPROVAL_TIMEOUT_SECS};
pub use policy::{mcp_server_of_tool, tool_pattern};
pub use progress::DaemonToolProgressSink;
pub use subagent::{
    DEFAULT_CHILD_MAX_STEPS, MAX_CHILD_MAX_STEPS, MAX_CHILD_SYSTEM_PROMPT_BYTES,
    MAX_SUBAGENT_RETRIES,
};
