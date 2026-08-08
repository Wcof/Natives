//! # Natives Agent Daemon Library
//!
//! Library crate for the Agent Daemon, exposing storage, event log,
//! run authority, and RPC.

pub mod artifact_store;
pub mod authority;
pub mod capability;
pub mod capability_resolution;
pub mod checkpoint;
pub mod cli_runtime_bridge;
pub mod client;
pub mod codex_runtime_bridge;
pub mod conversation_projector;
pub mod conversation_store;
pub mod creative_ai;
pub mod event_log;
pub mod extension_store;
pub mod governor;
pub mod interaction_store;
pub mod loopback;
pub mod mcp_runtime;
pub mod memory_store;
pub mod metrics;
pub mod natives_db_broker;
pub mod prepared_session;
pub mod production;
pub mod production_credentials;
pub mod production_hooks;
pub mod production_tools;
pub mod project_identity;
pub mod prompt_queue_store;
pub mod proposal_fact;
pub mod request_rectifier;
pub mod routing;
pub mod rpc;
pub mod run_manager;
pub mod runtime;
pub mod session_harness;
pub mod side_effect_ledger;
pub mod skill_store;
pub mod storage;
pub mod stream_protocol;
pub mod subagent_store;
pub mod task_store;

use governor::ProviderRequestGovernor;
use std::sync::Arc;
use std::sync::OnceLock;

pub static GLOBAL_GOVERNOR: OnceLock<Arc<ProviderRequestGovernor>> = OnceLock::new();

pub fn global_governor() -> Option<Arc<ProviderRequestGovernor>> {
    GLOBAL_GOVERNOR.get().cloned()
}

pub use authority::{
    build_execution_authority, AuthorityError, EmbeddedAuthority, ExecutionAuthority, UdsAuthority,
};
pub use client::{
    client_protocol_version, mode_requires_uds, resolve_run_authority_mode, DaemonClient,
    DaemonClientError, RunAuthorityMode,
};
pub use mcp_runtime::{global_mcp, McpRuntime};
pub use natives_db_broker::{
    default_assistant_db_path, default_natives_db_path, try_install_natives_db_broker,
    NativesDbBroker,
};
pub use production::*;
pub use run_manager::*;
