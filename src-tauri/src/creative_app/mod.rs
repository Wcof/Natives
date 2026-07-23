//! Creative App domain — dual-source (internal workshop + external GitHub container).
//!
//! See ADR-0013 and docs/architecture/creative-app-github-container-install.md.

pub mod browser;
pub mod docker;
pub mod github;
pub mod install;
pub mod model;
pub mod paths;
pub mod probe;
pub mod service;
pub mod state_machine;
pub mod store;

pub use model::*;
pub use service::CreativeAppService;
