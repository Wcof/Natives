//! Subagent route policy + session registry (migration 010).
//!
//! Stores only provider/key/model **IDs** — never plaintext credentials.
//!
//! Aggregate module: route policy lives in [`subagent_route`], the durable
//! pending Child Directive in [`subagent_directive`], and slot/budget
//! reservations in [`subagent_reservation`]. All their public items are
//! re-exported here so `crate::subagent_store::*` keeps working unchanged.
//!
//! W2/P0-02: the split files live as siblings of this file (Rust's default
//! `mod x;` resolution looks in `subagent_store/x.rs`), so each declaration
//! carries an explicit `#[path]` — the same documented pattern used by
//! `run/manager_tests.rs` for its test splits.
#[path = "subagent_directive.rs"]
mod subagent_directive;
#[path = "subagent_reservation.rs"]
mod subagent_reservation;
#[path = "subagent_route.rs"]
mod subagent_route;
#[path = "subagent_session.rs"]
mod subagent_session;
#[path = "subagent_store_rpc.rs"]
mod subagent_store_rpc;

use crate::storage::DataStore;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
pub use subagent_directive::*;
pub use subagent_reservation::*;
pub use subagent_route::*;
pub use subagent_session::*;
pub use subagent_store_rpc::*;
use uuid::Uuid;

fn store() -> Result<DataStore, String> {
    // W2: single Daemon DataStore open path (assistant.db authority + test hook).
    crate::storage::open_daemon_store()
}
#[cfg(test)]
#[path = "subagent_store_tests.rs"]
mod tests;
