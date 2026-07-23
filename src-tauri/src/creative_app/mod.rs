//! Creative App domain — multi-source personal creations.
//!
//! Sources: internal workshop, external GitHub container, local project.
//! See ADR-0013 and the local_project upgrade plan.

pub mod browser;
pub mod docker;
pub mod github;
pub mod install;
pub mod local;
pub mod model;
pub mod paths;
pub mod probe;
pub mod service;
pub mod state_machine;
pub mod store;

pub use model::*;
pub use service::CreativeAppService;
