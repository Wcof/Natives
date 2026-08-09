//! Sub-agent execution — bounded multi-agent orchestration.
//!
//! Manages independent Run identity, permission non-inheritance,
//! concurrency/depth/token limits, and parent recovery after child failure.
//!
//! Responsibilities are split:
//! - `permission` — pure allowlist/permission resolution helpers.
//! - `types` — domain types (config, status, failure policy).
//! - `manager` — [`SubAgentManager`] lifecycle and budget reservations.
//! - `subagents_tests.rs` — unit tests.

mod manager;
mod permission;
mod types;

pub use manager::SubAgentManager;
pub use permission::{
    cap_child_permission, default_subagent_tool_allowlist, resolve_child_permission,
    resolve_child_tool_allowlist, tool_list_allows,
};
pub use types::{ChildFailureEffect, FailurePolicy, SubAgent, SubAgentConfig, SubAgentStatus};

#[cfg(test)]
#[path = "subagents_tests.rs"]
mod tests;
