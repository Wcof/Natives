//! Runtime execution seams owned by the Agent Daemon.
//!
//! Deep modules behind the ProductionRuntime facade (task-01).

pub mod execution_registry;
pub mod interaction_hub;
pub mod mcp_invocation;
pub mod task_supervisor;
pub mod tool_policy;

pub use execution_registry::{
    CancelCleanupOutcome, CancelPhase, ExecutionRegistration, ExecutionRegistry, ManagedResource,
    ProcessCancelHook, DEFAULT_CANCEL_GRACE_MS,
};
pub use interaction_hub::InteractionHub;
pub use task_supervisor::{TaskRecord, TaskSupervisor};
pub use tool_policy::{
    invocation_from_gate, permission_class_for_tool, GrantDecision, StructuredToolGrant,
    ToolInvocation, ToolPolicyState, TOOL_GRANT_POLICY_VERSION,
};
