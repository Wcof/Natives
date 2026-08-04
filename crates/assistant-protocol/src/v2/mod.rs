//! Protocol v2 — single production wire protocol for the Agent Daemon.
//!
//! v2 replaces the dual execution chains in Tauri (`assistant_stream_proxy` +
//! `agent_loop`) with one daemon-owned run authority. Types are JSON-serialised
//! over authenticated UDS / Named Pipe.
//!
//! Formal envelopes live in [`envelope`]. Prefer them over legacy v1
//! `RpcRequest` / `RpcResponse` shells. Servers may still accept v1-shaped JSON
//! via [`envelope::parse_request_compat`].

pub mod capabilities;
pub mod creative;
pub mod credential;
pub mod envelope;
pub mod harness;
pub mod methods;
pub mod run;
pub mod run_event;

pub use capabilities::*;
pub use creative::*;
pub use credential::*;
pub use envelope::*;
pub use harness::*;
pub use methods::*;
pub use run::*;
pub use run_event::*;

/// Wire protocol major version for v2.
pub const PROTOCOL_V2: &str = "2.0.0";
