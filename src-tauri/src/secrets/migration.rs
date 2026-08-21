//! Secret 迁移状态机（ADR-0020 P0 Gate #6 · R-S12）。
//!
//! 从旧 AES-256-GCM / KEK-DEK SQLite 密文迁移到 OS Keychain：
//!
//! ```text
//! 读旧密文 → 写 Keychain → 回读验证 → 原子切换 secret_ref → 延后清理旧密文
//! ```
//!
//! 不变量：
//! - 幂等：重复启动 / 已迁移条目直接判定完成，不重复写入；
//! - 可恢复：任意阶段崩溃后重跑可继续；
//! - 可回滚：验证失败时删除 Keychain 写入并保留旧数据；
//! - Keychain locked/unavailable 显式报错，旧数据保持可恢复；
//! - 未验证通过前绝不删除旧密文；明文不进入日志 / Debug。

use super::store::{SecretRef, SecretStore, SecretStoreError};

/// 一次迁移的结果（供调用方决定是否切换 DB 中的 `secret_ref` 与清理旧密文）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationOutcome {
    /// 本次完成迁移：Secret 已写入 Keychain 且回读验证通过。
    /// 调用方现在可以原子切换 `secret_ref` 并延后清理旧密文。
    Migrated,
    /// 该 ref 已经存在于 Keychain（幂等重复执行）。
    AlreadyMigrated,
    /// 没有旧密文可迁移（新条目，无历史数据）。
    NothingToMigrate,
    /// 验证失败，已回滚 Keychain 写入；旧数据原样保留，可稍后重试。
    RolledBack,
    /// Keychain 当前 locked/unavailable，操作未执行；旧数据可恢复。
    Blocked(SecretStoreError),
}

/// 迁移单个 Secret。
///
/// - `legacy_reader`: 读取旧密文解密后的明文；返回 `None` 表示没有旧数据。
/// - `store`: 目标 SecretStore（OS Keychain 或测试用内存实现）。
/// - `reference`: 目标 opaque ref（迁移后写入 DB 的 `secret_ref`）。
pub fn migrate_secret(
    store: &dyn SecretStore,
    reference: &SecretRef,
    legacy_reader: impl FnOnce() -> Option<Vec<u8>>,
) -> MigrationOutcome {
    // 1. 幂等检查：已存在且可读 → AlreadyMigrated。
    match store.read(reference) {
        Ok(_) => return MigrationOutcome::AlreadyMigrated,
        Err(SecretStoreError::NotFound) => {}
        Err(SecretStoreError::Locked) => {
            return MigrationOutcome::Blocked(SecretStoreError::Locked)
        }
        Err(SecretStoreError::Unavailable) => {
            return MigrationOutcome::Blocked(SecretStoreError::Unavailable)
        }
        Err(SecretStoreError::Other(_)) => {}
    }

    // 2. 读旧密文；没有旧数据 → NothingToMigrate。
    let Some(plaintext) = legacy_reader() else {
        return MigrationOutcome::NothingToMigrate;
    };

    // 3. 写 Keychain；locked/unavailable 显式阻断且不触碰旧数据。
    if let Err(error) = store.write(reference, &plaintext) {
        return MigrationOutcome::Blocked(error);
    }

    // 4. 回读验证；失败则回滚（删除写入，保留旧数据）。
    match store.read(reference) {
        Ok(read_back) if read_back == plaintext => MigrationOutcome::Migrated,
        Ok(_) => {
            let _ = store.delete(reference);
            MigrationOutcome::RolledBack
        }
        Err(_) => {
            let _ = store.delete(reference);
            MigrationOutcome::RolledBack
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::store::{MemorySecretStore, SecretStoreResult};
    use super::*;

    fn reader(plaintext: Option<&'static str>) -> impl FnOnce() -> Option<Vec<u8>> {
        move || plaintext.map(|p| p.as_bytes().to_vec())
    }

    #[test]
    fn migrates_and_verifies() {
        let store = MemorySecretStore::new();
        let reference = SecretRef::new("cred:acct-1");
        let outcome = migrate_secret(&store, &reference, reader(Some("legacy-secret")));
        assert_eq!(outcome, MigrationOutcome::Migrated);
        assert_eq!(
            store.read(&reference).unwrap(),
            b"legacy-secret",
            "Secret 必须写入 Keychain"
        );
    }

    #[test]
    fn already_migrated_is_idempotent() {
        let store = MemorySecretStore::new();
        let reference = SecretRef::new("cred:acct-2");
        store.write(&reference, b"first").unwrap();
        // 第二次执行不再覆盖、不再依赖旧密文。
        let outcome = migrate_secret(&store, &reference, reader(Some("should-not-be-used")));
        assert_eq!(outcome, MigrationOutcome::AlreadyMigrated);
        assert_eq!(store.read(&reference).unwrap(), b"first");
    }

    #[test]
    fn nothing_to_migrate_for_new_secrets() {
        let store = MemorySecretStore::new();
        let reference = SecretRef::new("cred:brand-new");
        let outcome = migrate_secret(&store, &reference, reader(None));
        assert_eq!(outcome, MigrationOutcome::NothingToMigrate);
        assert!(!store.contains(&reference));
    }

    #[test]
    fn locked_blocks_and_keeps_legacy_data() {
        let store = MemorySecretStore::new();
        store.set_locked(true);
        let reference = SecretRef::new("cred:locked");
        let outcome = migrate_secret(&store, &reference, reader(Some("legacy")));
        assert_eq!(outcome, MigrationOutcome::Blocked(SecretStoreError::Locked));
        // 旧数据未被触碰（内存实现无旧数据时即为未写入）
        assert!(!store.contains(&reference));
    }

    #[test]
    fn unavailable_blocks_and_keeps_legacy_data() {
        let store = MemorySecretStore::new();
        store.set_unavailable(true);
        let reference = SecretRef::new("cred:unavailable");
        let outcome = migrate_secret(&store, &reference, reader(Some("legacy")));
        assert_eq!(
            outcome,
            MigrationOutcome::Blocked(SecretStoreError::Unavailable)
        );
        assert!(!store.contains(&reference));
    }

    #[test]
    fn rollback_when_store_returns_different_bytes() {
        // 用一个"损坏"store：write 后 read 返回不同内容 → 验证失败 → 回滚。
        let store = CorruptWriteStore(MemorySecretStore::new());
        let reference = SecretRef::new("cred:corrupt");
        let outcome = migrate_secret(&store, &reference, reader(Some("legacy")));
        assert_eq!(outcome, MigrationOutcome::RolledBack);
        // 回滚后不应残留 Keychain 写入
        assert!(!store.0.contains(&reference));
    }

    struct CorruptWriteStore(MemorySecretStore);

    impl SecretStore for CorruptWriteStore {
        fn write(&self, reference: &SecretRef, secret: &[u8]) -> SecretStoreResult<()> {
            // 写入时篡改一个字节，模拟 Keychain 返回损坏数据
            let mut corrupted = secret.to_vec();
            if let Some(byte) = corrupted.first_mut() {
                *byte ^= 0xFF;
            }
            self.0.write(reference, &corrupted)
        }
        fn read(&self, reference: &SecretRef) -> SecretStoreResult<Vec<u8>> {
            self.0.read(reference)
        }
        fn delete(&self, reference: &SecretRef) -> SecretStoreResult<()> {
            self.0.delete(reference)
        }
    }

    #[test]
    fn legacy_reader_error_path_never_writes_partial_state() {
        // legacy_reader 返回 None（无旧数据）时即使 store 可用也不写入。
        let store = MemorySecretStore::new();
        let reference = SecretRef::new("cred:no-legacy");
        let outcome = migrate_secret(&store, &reference, reader(None));
        assert_eq!(outcome, MigrationOutcome::NothingToMigrate);
        assert!(!store.contains(&reference));
    }
}
