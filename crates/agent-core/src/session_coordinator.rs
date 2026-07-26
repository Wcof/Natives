//! Compatibility re-export — the coordinator lives in
//! [`harness_core::session_actor`].
//!
//! The Session Actor is Harness domain behaviour, not engine execution, so it
//! moved to `harness-core` (task T2). This shim keeps every existing import
//! path valid; prefer importing from `harness_core` in new code.

pub use harness_core::session_actor::*;
