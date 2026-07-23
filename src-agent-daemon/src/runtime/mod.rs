//! Runtime execution seams owned by the Agent Daemon.
//!
//! Agent C will relocate these into the thin ProductionRuntime facade (task-01).
//! Agent B implements correct behavior here without splitting ProductionRuntime.

pub mod execution_registry;

pub use execution_registry::{
    CancelCleanupOutcome, CancelPhase, ExecutionRegistration, ExecutionRegistry,
    ManagedResource, ProcessCancelHook, DEFAULT_CANCEL_GRACE_MS,
};
