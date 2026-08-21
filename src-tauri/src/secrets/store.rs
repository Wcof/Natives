//! SecretStore 抽象：持久 Secret 的唯一 seam（R-S12）。

use std::fmt;
use std::sync::Mutex;

/// 指向 OS Keychain 中 Secret 的 opaque 引用。
///
/// SQLite 只允许保存 `SecretRef`；任何对象都不得从 `SecretRef` 反推 Secret。
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SecretRef(pub String);

impl SecretRef {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SecretRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SecretRef(…{:4})",
            self.0.chars().rev().take(4).collect::<String>()
        )
    }
}

/// SecretStore 错误：locked/unavailable 必须与普通 IO 错误区分（R-S12）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretStoreError {
    /// Keychain 被锁定或用户交互不可用，操作可稍后重试，旧数据保持可恢复。
    Locked,
    /// 平台不支持 / Keychain 不可用（如 headless、非 macOS）。
    Unavailable,
    /// 引用的 Secret 不存在。
    NotFound,
    /// 其它底层错误（不携带 Secret 内容）。
    Other(String),
}

impl fmt::Display for SecretStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Locked => write!(f, "secret store locked"),
            Self::Unavailable => write!(f, "secret store unavailable"),
            Self::NotFound => write!(f, "secret not found"),
            Self::Other(msg) => write!(f, "secret store error: {msg}"),
        }
    }
}

impl std::error::Error for SecretStoreError {}

pub type SecretStoreResult<T> = Result<T, SecretStoreError>;

/// OS 级 Secret 存储 seam（R-S12）。
///
/// 实现必须：拒绝 Debug 输出明文；locked/unavailable 用独立错误类型表达；
/// write 必须可重复执行（幂等 upsert）。
pub trait SecretStore: Send + Sync {
    /// 写入（upsert）Secret。已存在同一 ref 时覆盖。
    fn write(&self, reference: &SecretRef, secret: &[u8]) -> SecretStoreResult<()>;

    /// 读取 Secret；不存在返回 `NotFound`。
    fn read(&self, reference: &SecretRef) -> SecretStoreResult<Vec<u8>>;

    /// 删除 Secret；不存在视为成功（幂等）。
    fn delete(&self, reference: &SecretRef) -> SecretStoreResult<()>;
}

/// 内存实现：仅用于单元测试与 Spike 验证 locked/unavailable 语义。
/// 生产路径不得使用（明文驻留进程内存是测试环境可接受边界）。
#[derive(Debug)]
pub struct MemorySecretStore {
    inner: Mutex<MemoryInner>,
}

#[derive(Debug, Default)]
struct MemoryInner {
    entries: std::collections::HashMap<String, Vec<u8>>,
    locked: bool,
    unavailable: bool,
}

impl Default for MemorySecretStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MemorySecretStore {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(MemoryInner::default()),
        }
    }

    /// 模拟 Keychain 锁定（locked 语义）。
    pub fn set_locked(&self, locked: bool) {
        self.inner.lock().unwrap().locked = locked;
    }

    /// 模拟 Keychain 不可用（unavailable 语义）。
    pub fn set_unavailable(&self, unavailable: bool) {
        self.inner.lock().unwrap().unavailable = unavailable;
    }

    /// 测试断言：指定 ref 是否已存在。
    pub fn contains(&self, reference: &SecretRef) -> bool {
        self.inner
            .lock()
            .unwrap()
            .entries
            .contains_key(reference.as_str())
    }

    /// 清空全部条目（测试隔离用；生产路径不使用）。
    pub fn clear(&self) {
        self.inner.lock().unwrap().entries.clear();
    }
}

impl SecretStore for MemorySecretStore {
    fn write(&self, reference: &SecretRef, secret: &[u8]) -> SecretStoreResult<()> {
        let mut inner = self.inner.lock().unwrap();
        if inner.unavailable {
            return Err(SecretStoreError::Unavailable);
        }
        if inner.locked {
            return Err(SecretStoreError::Locked);
        }
        inner
            .entries
            .insert(reference.as_str().to_string(), secret.to_vec());
        Ok(())
    }

    fn read(&self, reference: &SecretRef) -> SecretStoreResult<Vec<u8>> {
        let inner = self.inner.lock().unwrap();
        if inner.unavailable {
            return Err(SecretStoreError::Unavailable);
        }
        if inner.locked {
            return Err(SecretStoreError::Locked);
        }
        inner
            .entries
            .get(reference.as_str())
            .cloned()
            .ok_or(SecretStoreError::NotFound)
    }

    fn delete(&self, reference: &SecretRef) -> SecretStoreResult<()> {
        let mut inner = self.inner.lock().unwrap();
        if inner.unavailable {
            return Err(SecretStoreError::Unavailable);
        }
        if inner.locked {
            return Err(SecretStoreError::Locked);
        }
        inner.entries.remove(reference.as_str());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> MemorySecretStore {
        MemorySecretStore::new()
    }

    #[test]
    fn write_read_roundtrip() {
        let s = store();
        let r = SecretRef::new("cred:antigravity:acct-1");
        s.write(&r, b"opaque-secret").unwrap();
        assert_eq!(s.read(&r).unwrap(), b"opaque-secret");
        assert!(s.contains(&r));
    }

    #[test]
    fn write_is_upsert() {
        let s = store();
        let r = SecretRef::new("cred:key");
        s.write(&r, b"first").unwrap();
        s.write(&r, b"second").unwrap();
        assert_eq!(s.read(&r).unwrap(), b"second");
    }

    #[test]
    fn read_missing_is_not_found() {
        let s = store();
        assert_eq!(
            s.read(&SecretRef::new("missing")),
            Err(SecretStoreError::NotFound)
        );
    }

    #[test]
    fn delete_is_idempotent() {
        let s = store();
        let r = SecretRef::new("cred:tmp");
        s.write(&r, b"x").unwrap();
        s.delete(&r).unwrap();
        s.delete(&r).unwrap(); // 第二次仍成功
        assert!(!s.contains(&r));
    }

    #[test]
    fn locked_blocks_write_read_delete() {
        let s = store();
        s.set_locked(true);
        let r = SecretRef::new("cred:locked");
        assert_eq!(s.write(&r, b"x"), Err(SecretStoreError::Locked));
        assert_eq!(s.read(&r), Err(SecretStoreError::Locked));
        assert_eq!(s.delete(&r), Err(SecretStoreError::Locked));
    }

    #[test]
    fn unavailable_blocks_all_operations() {
        let s = store();
        s.set_unavailable(true);
        let r = SecretRef::new("cred:unavailable");
        assert_eq!(s.write(&r, b"x"), Err(SecretStoreError::Unavailable));
        assert_eq!(s.read(&r), Err(SecretStoreError::Unavailable));
        assert_eq!(s.delete(&r), Err(SecretStoreError::Unavailable));
    }

    #[test]
    fn unlock_recovers_operations() {
        let s = store();
        s.set_locked(true);
        s.set_locked(false);
        let r = SecretRef::new("cred:recover");
        s.write(&r, b"x").unwrap();
        assert_eq!(s.read(&r).unwrap(), b"x");
    }

    #[test]
    fn secret_ref_display_never_leaks_full_value() {
        let r = SecretRef::new("cred:very-long-secret-value");
        let shown = format!("{r}");
        assert!(!shown.contains("very-long-secret-value"));
        assert!(shown.starts_with("SecretRef(…"));
    }
}
