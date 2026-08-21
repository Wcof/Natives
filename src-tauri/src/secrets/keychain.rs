//! macOS OS Keychain 实现（R-S12 SecretStore seam）。
//!
//! 使用 `security-framework` 的 Generic Password 条目：`service` 标识应用域，
//! `account` 使用 opaque `SecretRef`，`data` 为 Secret 明文（Keychain 保护）。
//! 该实现只作为 Host 内的一个 adapter；业务代码通过 `SecretStore` trait 使用。

use super::store::{SecretRef, SecretStore, SecretStoreError, SecretStoreResult};

/// Keychain service 名（应用域，避免与系统其它条目冲突）。
const SERVICE: &str = "ai.natives.secrets";

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use core_foundation::data::CFData;
    use security_framework::item::{
        ItemAddOptions, ItemAddValue, ItemClass, ItemSearchOptions, SearchResult,
    };

    pub struct KeychainSecretStore {
        service: String,
    }

    impl KeychainSecretStore {
        pub fn new(service: impl Into<String>) -> Self {
            Self {
                service: service.into(),
            }
        }
    }

    impl Default for KeychainSecretStore {
        fn default() -> Self {
            Self::new(SERVICE)
        }
    }

    fn map_error(error: security_framework::base::Error) -> SecretStoreError {
        // errSecItemNotFound = -25300 → NotFound
        if error.code() == -25300 {
            return SecretStoreError::NotFound;
        }
        // errSecInteractionNotAllowed = -25308、errSecAuthFailed = -25293 → Locked
        if error.code() == -25308 || error.code() == -25293 {
            return SecretStoreError::Locked;
        }
        SecretStoreError::Other(format!("keychain error {}", error.code()))
    }

    impl SecretStore for KeychainSecretStore {
        fn write(&self, reference: &SecretRef, secret: &[u8]) -> SecretStoreResult<()> {
            // 幂等 upsert：先删除已存在的条目再写入。
            let _ = self.delete(reference);
            let mut options = ItemAddOptions::new(ItemAddValue::Data {
                class: ItemClass::generic_password(),
                data: CFData::from_buffer(secret),
            });
            options
                .set_service(&self.service)
                .set_account_name(reference.as_str());
            options.add().map_err(map_error)
        }

        fn read(&self, reference: &SecretRef) -> SecretStoreResult<Vec<u8>> {
            let mut options = ItemSearchOptions::new();
            options
                .class(ItemClass::generic_password())
                .service(&self.service)
                .account(reference.as_str())
                .load_data(true);
            let results = options.search().map_err(map_error)?;
            match results.into_iter().next() {
                Some(SearchResult::Data(data)) => Ok(data),
                Some(_) => Err(SecretStoreError::Other(
                    "keychain returned non-data result".into(),
                )),
                None => Err(SecretStoreError::NotFound),
            }
        }

        fn delete(&self, reference: &SecretRef) -> SecretStoreResult<()> {
            let mut options = ItemSearchOptions::new();
            options
                .class(ItemClass::generic_password())
                .service(&self.service)
                .account(reference.as_str());
            // 幂等：条目不存在（NotFound）视为成功。
            match options.delete() {
                Ok(()) => Ok(()),
                Err(error) if error.code() == -25300 => Ok(()),
                Err(error) => Err(map_error(error)),
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// 真实 Keychain 集成测试：只在 macOS 且未显式禁用时运行。
        /// 使用临时 service 名，测试结束清理，避免污染用户 Keychain。
        #[test]
        fn real_keychain_roundtrip_and_delete() {
            let store = KeychainSecretStore::new("ai.natives.spike-tests");
            let reference = SecretRef::new(format!("spike-test-{}", std::process::id()));
            let secret = b"spike-secret-bytes";

            // write → read 回读验证
            store.write(&reference, secret).unwrap();
            let read_back = store.read(&reference).unwrap();
            assert_eq!(read_back, secret);

            // delete 后 NotFound（幂等删除）
            store.delete(&reference).unwrap();
            assert_eq!(store.read(&reference), Err(SecretStoreError::NotFound));
            store.delete(&reference).unwrap();
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::*;

    /// 非 macOS 平台：显式 Unavailable（R-S12 要求 locked/unavailable 显式报错）。
    pub struct KeychainSecretStore;

    impl SecretStore for KeychainSecretStore {
        fn write(&self, _: &SecretRef, _: &[u8]) -> SecretStoreResult<()> {
            Err(SecretStoreError::Unavailable)
        }
        fn read(&self, _: &SecretRef) -> SecretStoreResult<Vec<u8>> {
            Err(SecretStoreError::Unavailable)
        }
        fn delete(&self, _: &SecretRef) -> SecretStoreResult<()> {
            Err(SecretStoreError::Unavailable)
        }
    }
}

pub use imp::KeychainSecretStore;
