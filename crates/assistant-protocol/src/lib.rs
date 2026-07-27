//! # assistant-protocol
//!
//! Shared versioned wire and domain types for the Natives Agent Daemon.
//!
//! This crate defines the protocol between the Tauri client and the Agent Daemon,
//! covering conversation, run, message, content block, event, artifact, provider,
//! model, permission, extension, and daemon RPC types.
//!
//! ## Versioning
//!
//! Protocol compatibility is checked by major version number. The current version
//! is `0.1.0`. Breaking changes increment the major version.
//!
//! ## Wire format
//!
//! All types serialise to JSON. The `ProtocolEnvelope` wraps every RPC message
//! with the protocol version, request ID, client ID, and session token.

pub mod error;
pub mod v1;
pub mod v2;
pub mod version;

pub use error::*;
pub use v1::*;
pub use version::*;
// v2 is explicit — call sites import `assistant_protocol::v2::...` to avoid
// colliding with v1 type names during the migration.
