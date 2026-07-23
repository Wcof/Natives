//! Compatibility shim — the coordinator lives in [`crate::session_coordinator`].
//!
//! Historical name `SessionHarness` is an alias of [`SessionCoordinator`].
//! Prefer importing from `session_coordinator` for new code.

pub use crate::session_coordinator::*;
