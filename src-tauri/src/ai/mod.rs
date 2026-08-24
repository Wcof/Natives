//! AI Resources 模块（ADR-0020 §4 / plan3）。
//!
//! 提供 Provider / Connection / Credential / Model / Quota 的领域实体、持久化 Store、模型发现、OAuth 与数据迁移服务。

pub mod discovery;
pub mod facade;
pub mod migration;
pub mod model;
pub mod oauth;
pub mod store;

pub use discovery::{discover_models, DiscoveredModel};
pub use migration::{run_ai_resources_migration, MigrationReport};
pub use model::{
    AiResourcesSummary, Connection, ConnectionHealthStatus, Credential, CredentialKind,
    CredentialStatus, DeleteImpact, Model, ModelAvailability, ModelSource, Provider, QuotaSnapshot,
    QuotaStatus, QuotaWindow, UpstreamProtocol,
};
pub use oauth::{
    get_oauth_preset, OauthFlowType, OauthProviderPreset, OauthService, OauthSessionInfo,
    OauthSessionStatus, OAUTH_PRESETS,
};
