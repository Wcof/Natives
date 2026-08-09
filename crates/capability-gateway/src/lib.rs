//! # Capability Gateway
//!
//! Tool schemas, policy enforcement, sandbox, and audit.
//! Every tool must be registered with a schema, side effect declaration,
//! permission class, path scope, timeout, output limit, and cancellation policy.

mod context;
mod contract;
mod gateway;

pub use context::{ToolCallContext, ToolProgressChunk, TrustedPath, TERMINAL_PROGRESS_CAPACITY};
pub use contract::{
    ExecutionMode, PathScope, PermissionClass, SideEffect, Tool, ToolCapability, ToolError,
    ToolHandler, ToolOutput,
};

pub mod manifest;
pub mod plan_mode;
pub mod platform_sandbox;
pub mod policy;
pub mod process_supervisor;
pub mod tools;

pub use gateway::CapabilityGateway;
pub use manifest::ToolManifest;
pub use plan_mode::{Plan, PlanDecision, PlanSession, PlanState, PlanStep, PLAN_PROFILE};
pub use platform_sandbox::{
    allow_autonomous_shell, wrap_command_macos, PlatformCapabilities, SandboxProfile,
};
pub use process_supervisor::{
    global_process_supervisor, FakeProcessSupervisor, LocalProcessSupervisor, ProcessSnapshot,
    ProcessSpec, ProcessState, ProcessSupervisor, DEFAULT_FOREGROUND_BUDGET_MS,
};

#[cfg(test)]
#[path = "gateway_tests.rs"]
mod tests;
