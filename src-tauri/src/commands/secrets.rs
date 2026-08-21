//! Secret 迁移命令（R-S12 / ADR-0020 P0 Gate #6）。
//!
//! 显式触发 `provider_kek` 与 `env_encryption_key` 从 SQLite `settings`
//! 迁到 OS Keychain；返回每项迁移结果（migrated / already_migrated /
//! nothing_to_migrate / rolled_back / blocked）。幂等、可回滚；
//! Keychain locked/unavailable 显式报错并保留 SQLite 旧数据。

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::AppState;
use crate::Result;

/// 一次迁移的逐项结果。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretMigrationReport {
    pub provider_kek: String,
    pub env_encryption_key: String,
}

/// 显式触发 KEK/长期 Secret 迁移（旧 SQLite 明文 → OS Keychain）。
#[tauri::command]
pub fn secrets_migrate_to_keychain(state: State<'_, AppState>) -> Result<SecretMigrationReport> {
    let pool_conn = state
        .db
        .get()
        .map_err(|e| crate::Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &pool_conn;
    let result = crate::secrets::settings_keys::migrate_settings_keys(conn);
    Ok(SecretMigrationReport {
        provider_kek: result.provider_kek,
        env_encryption_key: result.env_encryption_key,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_serde_camel_case() {
        let report = SecretMigrationReport {
            provider_kek: "migrated".into(),
            env_encryption_key: "already_migrated".into(),
        };
        let value = serde_json::to_value(&report).unwrap();
        assert_eq!(value["providerKek"], "migrated");
        assert_eq!(value["envEncryptionKey"], "already_migrated");
    }
}
