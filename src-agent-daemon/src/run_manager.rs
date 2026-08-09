//! RunManager — Daemon-side sole Run Authority (production path).
//!
//! Facade only (ARCH-002): the implementation is split by lifecycle/state
//! ownership into the `run` domain modules (`crate::run`). This module keeps
//! the historical public surface — `crate::run_manager::RunManager`,
//! `global_run_manager()`, `install_global_for_test`,
//! `install_memory_global_for_test`, `protocol_version` — and the crate-root
//! re-export (`pub use run_manager::*`) working unchanged.

pub use crate::run::{global_run_manager, protocol_version, RunManager};
#[cfg(test)]
pub use crate::run::{install_global_for_test, install_memory_global_for_test};
