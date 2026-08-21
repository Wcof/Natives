//! SQLite settings 中的长期 Secret 迁移（R-S12 完成态）。
//!
//! 迁移对象（原 SQLite `settings` 中的明文主密钥）：
//! - `provider_kek`（provider_key_manager，32B hex）
//! - `env_encryption_key`（env_manager，32B hex）
//!
//! 流程（ADR-0020 / R-S12）：
//! ```text
//! 读旧密文 → 写 Keychain → 回读验证 → 原子切换（删除 SQLite 明文）→ 旧路径失效
//! ```
//!
//! 不变量：
//! - 幂等：Keychain 已有该 ref 且可读 → AlreadyMigrated；
//! - 可恢复：Keychain locked/unavailable → Blocked，SQLite 明文保留；
//! - 可回滚：写入后回读不一致 → 删除 Keychain 条目并保留 SQLite 明文；
//! - 只有验证通过才删除 SQLite 明文（旧路径失效）；
//! - 明文绝不进入日志 / Debug / Renderer。

use rusqlite::Connection;

use super::keychain::KeychainSecretStore;
use super::migration::{migrate_secret, MigrationOutcome};
use super::store::SecretRef;
use crate::db;

/// Keychain 中 provider KEK 的 opaque ref。
pub const PROVIDER_KEK_REF: &str = "kek:provider";
/// Keychain 中 env 加密密钥的 opaque ref。
pub const ENV_ENCRYPTION_KEY_REF: &str = "kek:env";
/// SQLite settings 旧键名（迁移源，迁移完成后删除）。
pub const PROVIDER_KEK_SETTING: &str = "provider_kek";
/// SQLite settings 旧键名（迁移源，迁移完成后删除）。
pub const ENV_ENCRYPTION_KEY_SETTING: &str = "env_encryption_key";

/// 一次迁移的详细结果（供命令层与状态查询使用）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsKeyMigrationResult {
    pub provider_kek: String,
    pub env_encryption_key: String,
}

fn outcome_label(outcome: &MigrationOutcome) -> &'static str {
    match outcome {
        MigrationOutcome::Migrated => "migrated",
        MigrationOutcome::AlreadyMigrated => "already_migrated",
        MigrationOutcome::NothingToMigrate => "nothing_to_migrate",
        MigrationOutcome::RolledBack => "rolled_back",
        MigrationOutcome::Blocked(_) => "blocked",
    }
}

/// 迁移单个 settings 密钥：读 SQLite 明文 → Keychain → 回读验证 → 删 SQLite 明文。
///
/// 返回 `(outcome_label, maybe_ref)`；`Some(ref)` 表示 SQLite 明文已被移除，
/// 后续读取必须经 Keychain（旧路径失效）。
fn migrate_one(
    conn: &Connection,
    store: &dyn super::store::SecretStore,
    setting_key: &str,
    reference: &SecretRef,
) -> String {
    // 幂等：Keychain 已有 → 无需读取 SQLite。
    if let MigrationOutcome::AlreadyMigrated = migrate_secret(store, reference, || None) {
        // AlreadyMigrated 且 SQLite 明文仍在 → 补一次清理（旧路径失效）。
        let _ = db::delete_setting(conn, setting_key);
        return "already_migrated".to_string();
    }

    let outcome = migrate_secret(store, reference, || {
        db::get_setting(conn, setting_key)
            .ok()
            .flatten()
            .map(|value| value.into_bytes())
    });

    if outcome == MigrationOutcome::Migrated {
        // 验证通过后才移除 SQLite 明文（旧路径失效）。
        let _ = db::delete_setting(conn, setting_key);
    }
    outcome_label(&outcome).to_string()
}

/// 迁移 provider KEK 与 env 加密密钥（幂等；locked/unavailable 显式阻断）。
pub fn migrate_settings_keys(conn: &Connection) -> SettingsKeyMigrationResult {
    let store = KeychainSecretStore::default();
    SettingsKeyMigrationResult {
        provider_kek: migrate_one(
            conn,
            &store,
            PROVIDER_KEK_SETTING,
            &SecretRef::new(PROVIDER_KEK_REF),
        ),
        env_encryption_key: migrate_one(
            conn,
            &store,
            ENV_ENCRYPTION_KEY_SETTING,
            &SecretRef::new(ENV_ENCRYPTION_KEY_REF),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::store::{MemorySecretStore, SecretStore};
    use rusqlite::Connection;

    fn setup(provider_kek: Option<&str>, env_key: Option<&str>) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at TEXT NOT NULL
            );",
        )
        .unwrap();
        if let Some(value) = provider_kek {
            db::set_setting(&conn, PROVIDER_KEK_SETTING, value).unwrap();
        }
        if let Some(value) = env_key {
            db::set_setting(&conn, ENV_ENCRYPTION_KEY_SETTING, value).unwrap();
        }
        conn
    }

    #[test]
    fn migrates_both_keys_and_removes_sqlite_plaintext() {
        let conn = setup(
            Some("ab".repeat(32).as_str()),
            Some("cd".repeat(32).as_str()),
        );
        let store = MemorySecretStore::new();

        let provider_outcome = migrate_one(
            &conn,
            &store,
            PROVIDER_KEK_SETTING,
            &SecretRef::new(PROVIDER_KEK_REF),
        );
        let env_outcome = migrate_one(
            &conn,
            &store,
            ENV_ENCRYPTION_KEY_SETTING,
            &SecretRef::new(ENV_ENCRYPTION_KEY_REF),
        );

        assert_eq!(provider_outcome, "migrated");
        assert_eq!(env_outcome, "migrated");
        // 迁移后 SQLite 明文必须删除（旧路径失效）。
        assert!(db::get_setting(&conn, PROVIDER_KEK_SETTING)
            .unwrap()
            .is_none());
        assert!(db::get_setting(&conn, ENV_ENCRYPTION_KEY_SETTING)
            .unwrap()
            .is_none());
        // Keychain 持有真实值。
        assert_eq!(
            store.read(&SecretRef::new(PROVIDER_KEK_REF)).unwrap(),
            "ab".repeat(32).as_bytes()
        );
        assert_eq!(
            store.read(&SecretRef::new(ENV_ENCRYPTION_KEY_REF)).unwrap(),
            "cd".repeat(32).as_bytes()
        );
    }

    #[test]
    fn already_migrated_is_idempotent_and_cleans_leftover_sqlite() {
        let conn = setup(Some("ab".repeat(32).as_str()), None);
        let store = MemorySecretStore::new();
        // 首次迁移
        migrate_one(
            &conn,
            &store,
            PROVIDER_KEK_SETTING,
            &SecretRef::new(PROVIDER_KEK_REF),
        );
        // 模拟 SQLite 明文残留（异常中断场景）
        db::set_setting(&conn, PROVIDER_KEK_SETTING, "ab".repeat(32).as_str()).unwrap();
        // 再次迁移：AlreadyMigrated，同时清理残留明文
        let outcome = migrate_one(
            &conn,
            &store,
            PROVIDER_KEK_SETTING,
            &SecretRef::new(PROVIDER_KEK_REF),
        );
        assert_eq!(outcome, "already_migrated");
        assert!(db::get_setting(&conn, PROVIDER_KEK_SETTING)
            .unwrap()
            .is_none());
    }

    #[test]
    fn nothing_to_migrate_when_no_sqlite_key() {
        let conn = setup(None, None);
        let store = MemorySecretStore::new();
        let outcome = migrate_one(
            &conn,
            &store,
            PROVIDER_KEK_SETTING,
            &SecretRef::new(PROVIDER_KEK_REF),
        );
        assert_eq!(outcome, "nothing_to_migrate");
    }

    #[test]
    fn locked_blocks_and_keeps_sqlite_plaintext() {
        let conn = setup(Some("ab".repeat(32).as_str()), None);
        let store = MemorySecretStore::new();
        store.set_locked(true);
        let outcome = migrate_one(
            &conn,
            &store,
            PROVIDER_KEK_SETTING,
            &SecretRef::new(PROVIDER_KEK_REF),
        );
        assert_eq!(outcome, "blocked");
        // 旧明文保留（可恢复）。
        assert!(db::get_setting(&conn, PROVIDER_KEK_SETTING)
            .unwrap()
            .is_some());
    }

    #[test]
    fn rollback_when_store_returns_different_bytes() {
        let conn = setup(Some("ab".repeat(32).as_str()), None);
        let store = CorruptWriteStore(MemorySecretStore::new());
        let outcome = migrate_one(
            &conn,
            &store,
            PROVIDER_KEK_SETTING,
            &SecretRef::new(PROVIDER_KEK_REF),
        );
        assert_eq!(outcome, "rolled_back");
        // 回滚后 SQLite 明文保留。
        assert!(db::get_setting(&conn, PROVIDER_KEK_SETTING)
            .unwrap()
            .is_some());
    }

    struct CorruptWriteStore(MemorySecretStore);

    impl super::super::store::SecretStore for CorruptWriteStore {
        fn write(
            &self,
            reference: &SecretRef,
            secret: &[u8],
        ) -> super::super::store::SecretStoreResult<()> {
            let mut corrupted = secret.to_vec();
            if let Some(byte) = corrupted.first_mut() {
                *byte ^= 0xFF;
            }
            self.0.write(reference, &corrupted)
        }
        fn read(&self, reference: &SecretRef) -> super::super::store::SecretStoreResult<Vec<u8>> {
            self.0.read(reference)
        }
        fn delete(&self, reference: &SecretRef) -> super::super::store::SecretStoreResult<()> {
            self.0.delete(reference)
        }
    }
}
