//! OAuth 模块（ADR-0020 §4 / plan3 OAU-001..008）。

pub mod authenticator;
pub mod catalog;
pub mod pkce;
pub mod service;
pub mod session;

pub use catalog::{get_oauth_preset, OauthFlowType, OauthProviderPreset, OAUTH_PRESETS};
pub use service::OauthService;
pub use session::{OauthSessionInfo, OauthSessionStatus, OauthTokenBundle};
