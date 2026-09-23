//! 运行锁与运行槽（managed-app-contract v2 §6/§9）。
//!
//! 每 appId 一个 OS 排他 runtime 锁（跨 Chrome profile 兜底）；
//! 每 OS 用户命名空间四个全局运行槽，持锁代表占用。
//! 崩溃由 OS 释放；不自动终止 busy 应用。
//! 锁文件使用稳定 inode，不在释放时删除；release/Drop 不重复释放。

use std::fs::{File, OpenOptions};
use std::io;
#[cfg(unix)]
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

/// 一个已持有的排他文件锁；Drop/release 时释放锁，保留稳定 inode 文件。
pub struct FileLock {
    path: PathBuf,
    file: Option<File>,
}

impl FileLock {
    /// 尝试获取排他锁；不可得返回 None（调用方映射为 APP_RUNNING_ELSEWHERE / APP_RUNTIME_LIMIT / APP_BUSY）。
    pub fn try_acquire(path: PathBuf) -> io::Result<Option<Self>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        #[cfg(unix)]
        {
            if flock_exclusive(file.as_raw_fd()) {
                Ok(Some(Self {
                    path,
                    file: Some(file),
                }))
            } else {
                Ok(None)
            }
        }
        #[cfg(not(unix))]
        {
            match file.try_lock() {
                Ok(()) => Ok(Some(Self {
                    path,
                    file: Some(file),
                })),
                Err(std::fs::TryLockError::WouldBlock) => Ok(None),
                Err(std::fs::TryLockError::Error(e)) => Err(e),
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn release(mut self) {
        self.unlock();
    }

    fn unlock(&mut self) {
        if let Some(file) = self.file.take() {
            #[cfg(unix)]
            {
                let _ = flock_unlock(file.as_raw_fd());
            }
            drop(file);
        }
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        self.unlock();
    }
}

/// 统一剥除 `com.natives.app.` 前缀，与 Core `acquire_app_lock` 保持严格一致。
pub fn normalize_app_id(app_id: &str) -> &str {
    app_id.strip_prefix("com.natives.app.").unwrap_or(app_id)
}

/// appId 运行锁路径：apps/.locks/<appId>.runtime.lock
pub fn runtime_lock_path(apps_root: &Path, app_id: &str) -> PathBuf {
    let id = normalize_app_id(app_id);
    apps_root.join(".locks").join(format!("{id}.runtime.lock"))
}

/// appId 产品配置/管理锁路径：apps/.locks/<appId>.product.lock
pub fn product_lock_path(apps_root: &Path, app_id: &str) -> PathBuf {
    let id = normalize_app_id(app_id);
    apps_root.join(".locks").join(format!("{id}.product.lock"))
}

/// 第 slot 个全局运行槽路径：apps/.locks/runtime-slot-<n>.lock
pub fn slot_lock_path(apps_root: &Path, slot: usize) -> PathBuf {
    apps_root
        .join(".locks")
        .join(format!("runtime-slot-{slot}.lock"))
}

/// 在 apps_root 下寻找一个空闲运行槽并占用；全部占用返回 Ok(None)（映射 APP_RUNTIME_LIMIT）。
pub fn acquire_slot(apps_root: &Path, max_slots: usize) -> io::Result<Option<(usize, FileLock)>> {
    for slot in 0..max_slots {
        if let Some(lock) = FileLock::try_acquire(slot_lock_path(apps_root, slot))? {
            return Ok(Some((slot, lock)));
        }
    }
    Ok(None)
}

/// 一次真实运行持有的 app 锁与一个全局槽。仅在 `app:start` 成功时创建。
pub struct RuntimeLease {
    pub slot: usize,
    _runtime: FileLock,
    _slot: FileLock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeUnavailable {
    AlreadyRunning,
    Busy,
    Limit,
}

/// 获取应用运行时租约。
/// 严格遵守 product → runtime 锁获取顺序：
/// 1. 尝试获取 product 锁（若 Core 正进行配置/恢复，则返回 Busy 映射 APP_BUSY）；
/// 2. 在 product 锁保护下获取 runtime 锁（若已有实例运行，返回 AlreadyRunning）；
/// 3. 获取全局运行槽（若已满，返回 Limit）；
/// 4. 释放 product 锁，保留 runtime 锁与全局运行槽。
pub fn acquire_runtime(
    apps_root: &Path,
    app_id: &str,
) -> io::Result<Result<RuntimeLease, RuntimeUnavailable>> {
    let Some(product) = FileLock::try_acquire(product_lock_path(apps_root, app_id))? else {
        return Ok(Err(RuntimeUnavailable::Busy));
    };
    let Some(runtime) = FileLock::try_acquire(runtime_lock_path(apps_root, app_id))? else {
        return Ok(Err(RuntimeUnavailable::AlreadyRunning));
    };
    let Some((slot, slot_lock)) = acquire_slot(apps_root, crate::protocol::MAX_RUNTIME_SLOTS)?
    else {
        return Ok(Err(RuntimeUnavailable::Limit));
    };
    drop(product);
    Ok(Ok(RuntimeLease {
        slot,
        _runtime: runtime,
        _slot: slot_lock,
    }))
}

#[cfg(unix)]
fn flock_exclusive(fd: i32) -> bool {
    extern "C" {
        fn flock(fd: i32, operation: i32) -> i32;
    }
    // LOCK_EX=2 | LOCK_NB=4
    unsafe { flock(fd, 2 | 4) == 0 }
}

#[cfg(unix)]
fn flock_unlock(fd: i32) -> bool {
    extern "C" {
        fn flock(fd: i32, operation: i32) -> i32;
    }
    // LOCK_UN=8
    unsafe { flock(fd, 8) == 0 }
}

#[cfg(not(unix))]
fn flock_exclusive(_fd: i32) -> bool {
    false // 非 unix 平台在 A5 平台门禁单独实现与验证，不静默假装可用
}

#[cfg(not(unix))]
fn flock_unlock(_fd: i32) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_lock_is_exclusive() {
        let root = std::env::temp_dir().join(format!("natives-lock-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&root);
        let path = runtime_lock_path(&root, "sample");
        let first = FileLock::try_acquire(path.clone()).unwrap();
        assert!(first.is_some());
        let second = FileLock::try_acquire(path.clone()).unwrap();
        assert!(
            second.is_none(),
            "second holder must not acquire the same runtime lock"
        );
        drop(first);
        let third = FileLock::try_acquire(path).unwrap();
        assert!(third.is_some(), "lock must be re-acquirable after release");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn lock_file_inode_is_preserved_across_releases() {
        let root = std::env::temp_dir().join(format!("natives-inode-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&root);
        let path = runtime_lock_path(&root, "stable-inode");
        let first = FileLock::try_acquire(path.clone()).unwrap().unwrap();
        assert!(path.exists());
        first.release();
        // File must NOT be unlinked on release so its inode remains stable
        assert!(
            path.exists(),
            "lock file must remain on disk to preserve inode stability"
        );
        let second = FileLock::try_acquire(path).unwrap();
        assert!(
            second.is_some(),
            "stable inode lock is immediately re-acquirable"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn slots_are_bounded() {
        let root = std::env::temp_dir().join(format!("natives-slot-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&root);
        let mut held = Vec::new();
        for expected in 0..4 {
            let slot = acquire_slot(&root, 4).unwrap();
            assert_eq!(slot.as_ref().map(|(index, _)| *index), Some(expected));
            held.push(slot);
        }
        let exhausted = acquire_slot(&root, 4).unwrap();
        assert!(
            exhausted.is_none(),
            "fifth concurrent instance must not get a slot"
        );
        drop(held);
        let again = acquire_slot(&root, 4).unwrap();
        assert!(again.is_some());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn runtime_lease_holds_one_app_lock_and_one_slot() {
        let root =
            std::env::temp_dir().join(format!("natives-runtime-lease-test-{}", std::process::id()));
        let lease = acquire_runtime(&root, "sample").unwrap().unwrap();
        assert_eq!(lease.slot, 0);
        assert!(matches!(
            acquire_runtime(&root, "sample").unwrap(),
            Err(RuntimeUnavailable::AlreadyRunning)
        ));
        drop(lease);
        assert!(acquire_runtime(&root, "sample").unwrap().is_ok());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn acquire_runtime_rejects_when_product_lock_held() {
        let root = std::env::temp_dir().join(format!("natives-busy-test-{}", std::process::id()));
        let product_lock = FileLock::try_acquire(product_lock_path(&root, "busy-app"))
            .unwrap()
            .unwrap();
        let result = acquire_runtime(&root, "busy-app").unwrap();
        assert!(matches!(result, Err(RuntimeUnavailable::Busy)));
        drop(product_lock);
        let second = acquire_runtime(&root, "busy-app").unwrap();
        assert!(second.is_ok());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn app_id_normalization_matches_prefixed_and_unprefixed() {
        let root = std::env::temp_dir().join(format!("natives-norm-test-{}", std::process::id()));
        let p1 = runtime_lock_path(&root, "com.natives.app.fund");
        let p2 = runtime_lock_path(&root, "fund");
        assert_eq!(p1, p2);
        let i1 = product_lock_path(&root, "com.natives.app.fund");
        let i2 = product_lock_path(&root, "fund");
        assert_eq!(i1, i2);
        let _ = std::fs::remove_dir_all(root);
    }
}
