//! RPC server — authenticated Unix Domain Socket listener with dispatch.
//!
//! ## Protocol
//!
//! 1. Client connects and sends `HandshakeRequest` (bootstrap token, client version).
//! 2. Server validates bootstrap token, generates session token, responds `HandshakeResponse`.
//! 3. Client sends `V2Request` with session token, method, and params.
//! 4. Server authenticates, dispatches to handler, returns `V2Response`.
//! 5. Client can subscribe to run events via `SubscribeRequest` (stream).
//!
//! Messages are newline-delimited JSON (one JSON object per line, terminated by `\n`).

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Harness control plane — the `harness.*` method family.
///
/// The files live at `src-agent-daemon/src/harness/`, where the design
/// (第 7.2 节) puts them; only the `mod` declaration is here, via `#[path]`.
/// The reason is boundary hygiene rather than architecture: declaring it in
/// `lib.rs` alongside the other stores is the right home and is a one-line
/// move, but `lib.rs` is owned by a parallel workstream this round. Promoting
/// it later is: delete these two lines, add `pub mod harness;` to `lib.rs`, and
/// rename `crate::rpc::harness` to `crate::harness` at its handful of call
/// sites.
#[path = "../harness/mod.rs"]
pub mod harness;

/// Active session: maps session_token -> client_id.
type SessionMap = Arc<Mutex<HashMap<String, String>>>;

/// Dispatch an RPC request to the appropriate handler.
///
/// Public so the dispatch-coverage contract test (`tests/rpc_dispatch_contract.rs`) can
/// drive every advertised method through the *real* match instead of re-deriving the
/// arm list from source text. Callers must supply a connected write half; use
/// `tokio::net::UnixStream::pair()` in tests.
pub use dispatch::handle_rpc;

pub(crate) mod dispatch;
pub(crate) mod framing;
pub(crate) mod server;

pub(crate) use framing::FRAME_READ_TIMEOUT;
pub use framing::MAX_FRAME_BYTES;
pub use server::RpcServer;
pub(crate) use server::{read_frame, FrameError};

pub(crate) use framing::{
    send_error, send_rpc_failure, send_success, write_json_line, write_stream_frame,
};

pub(crate) mod handlers {
    pub(crate) mod artifact;
    pub(crate) mod capability;
    pub(crate) mod conversation;
    pub(crate) mod daemon;
    pub(crate) mod discovery;
    pub(crate) mod gateway;
    pub(crate) mod harness;
    pub(crate) mod mcp;
    pub(crate) mod permission;
    pub(crate) mod provider;
    pub(crate) mod run;
    pub(crate) mod run_finish;
    pub(crate) mod run_query;
    pub(crate) mod run_wire;
    pub(crate) mod task;
}

pub(crate) use handlers::provider::resolve_provider_adapter;
pub use handlers::run::MAX_WIRE_REPLAY_EVENTS;
#[allow(unused_imports)]
// A2-03 split contract: register_run_disabled_tools stays reachable as crate::rpc::register_run_disabled_tools
pub(crate) use handlers::run::{register_run_disabled_tools, required_param, run_manager};
