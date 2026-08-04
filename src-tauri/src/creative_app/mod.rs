//! Creative App domain — multi-source personal creations.
//!
//! Sources: internal workshop, external GitHub container, local project.
//! See ADR-0013 and the local_project upgrade plan.
//!
//! Catalog + lifecycle seam: [`adapters`] (three real adapters, not a plugin framework).

pub mod adapters;
pub mod browser;
pub mod docker;
pub mod github;
pub mod install;
pub mod local;
pub mod model;
pub mod operation;
pub mod paths;
pub mod probe;
pub mod runtime_store;
pub mod service;
pub mod state_machine;
pub mod store;

pub use adapters::{LifecycleCtx, ResolvedSource};
pub use model::*;
pub use service::CreativeAppService;
