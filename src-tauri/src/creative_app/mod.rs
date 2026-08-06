//! Creative App domain — multi-source personal creations.
//!
//! Sources: internal workshop, external GitHub container, local project.
//! See ADR-0013 and the local_project upgrade plan.
//!
//! Catalog + lifecycle seam: [`adapters`] (three real adapters, not a plugin framework).

pub mod adapters;
pub mod browser;
pub mod docker;
pub mod downloads;
pub mod driver;
pub mod github;
pub mod grant_store;
pub mod install;
pub mod local;
pub mod model;
pub mod non_owned;
pub mod oauth;
pub mod operation;
pub mod paths;
pub mod port_lease;
pub mod probe;
pub mod process_driver;
pub mod profile_store;
pub mod proposal;
pub mod proposal_inbox;
pub mod runtime_store;
pub mod service;
pub mod service_store;
pub mod state_machine;
pub mod store;
pub mod surface_store;
pub mod window;

pub use adapters::{LifecycleCtx, ResolvedSource};
pub use model::*;
pub use service::CreativeAppService;
