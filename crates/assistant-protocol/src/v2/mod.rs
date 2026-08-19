//! Protocol v2 — single production wire protocol for the Agent Daemon.
//!
//! v2 replaces the dual execution chains in Tauri (`assistant_stream_proxy` +
//! `agent_loop`) with one daemon-owned run authority. Types are JSON-serialised
//! over authenticated UDS / Named Pipe.
//!
//! Formal envelopes live in [`envelope`]. The v2 typed envelopes are the only
//! wire shape; legacy v1 `RpcRequest` / `RpcResponse` shell normalization is
//! retired (MIG-004).

pub mod capabilities;
pub mod creative;
pub mod credential;
pub mod daemon;
pub mod envelope;
pub mod gateway;
pub mod harness;
pub mod methods;
pub mod run;
pub mod run_event;

pub use capabilities::*;
pub use creative::*;
pub use credential::*;
pub use daemon::*;
pub use envelope::*;
pub use gateway::*;
pub use harness::*;
pub use methods::*;
pub use run::*;
pub use run_event::*;

/// Wire protocol major version for v2.
pub const PROTOCOL_V2: &str = "2.0.0";
