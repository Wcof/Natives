//! # Natives Agent Daemon Library
//!
//! Library crate for the Agent Daemon, exposing storage, event log,
//! run authority, and RPC.

pub mod storage;
pub mod event_log;
pub mod run_manager;
pub mod production;
pub mod rpc;
pub mod client;
pub mod natives_db_broker;
pub mod authority;
pub mod scheduler_store;
pub mod mcp_runtime;
pub mod extension_store;
pub mod skill_store;
pub mod memory_store;
pub mod artifact_store;

pub use run_manager::*;
pub use production::*;
pub use client::{
    client_protocol_version, mode_requires_uds, resolve_run_authority_mode, DaemonClient,
    DaemonClientError, RunAuthorityMode,
};
pub use natives_db_broker::{
    default_natives_db_path, try_install_natives_db_broker, NativesDbBroker,
};
pub use authority::{
    build_execution_authority, AuthorityError, EmbeddedAuthority, ExecutionAuthority, UdsAuthority,
};
pub use scheduler_store::{
    ensure_scheduler_runner, global_scheduler, CreateSchedulerJob, ScheduleKind, SchedulerJob,
    SchedulerRunner,
};
pub use mcp_runtime::{global_mcp, McpRuntime};