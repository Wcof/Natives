//! Runtime execution seams owned by the Agent Daemon.
//!
//! Agent C will relocate these into the thin ProductionRuntime facade (task-01).
//! Agent B implements correct behavior here without splitting ProductionRuntime.

pub mod execution_registry;
pub mod mcp_invocation;
pub mod tool_policy;

pub use execution_registry::{
    CancelCleanupOutcome, CancelPhase, ExecutionRegistration, ExecutionRegistry,
    ManagedResource, ProcessCancelHook, DEFAULT_CANCEL_GRACE_MS,
};
pub use tool_policy::{
    invocation_from_gate, permission_class_for_tool, GrantDecision, StructuredToolGrant,
    ToolInvocation, ToolPolicyState, TOOL_GRANT_POLICY_VERSION,
};
