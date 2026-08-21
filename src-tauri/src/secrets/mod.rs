//! Secret Ownership Spike（P0-A · ADR-0020 P0 Gate #6 · R-S12）
//!
//! 验证 OS Keychain 作为持久 Secret 唯一权威的 write/read/verify/rollback/
//! locked 语义。本模块是 Host-private `SecretStore` seam：
//!
//! - SQLite 只保存非敏感 metadata 与 opaque `secret_ref`，绝不保存 Secret；
//! - 持久 Secret（API Key / OAuth token / client secret）写入 OS Keychain；
//! - 迁移流程幂等、可恢复、可回滚：读旧密文 → 写 Keychain → 回读验证 →
//!   原子切换 `secret_ref` → 延后清理旧密文；
//! - Keychain locked/unavailable 必须显式报错并保持旧数据可恢复；
//! - 任何阶段不得把明文写入日志、事件、Renderer 或临时文件。

pub mod keychain;
pub mod migration;
pub mod settings_keys;
pub mod store;

pub use keychain::KeychainSecretStore;
pub use migration::{migrate_secret, MigrationOutcome};
pub use settings_keys::{migrate_settings_keys, SettingsKeyMigrationResult};
pub use store::{MemorySecretStore, SecretRef, SecretStore, SecretStoreError};
