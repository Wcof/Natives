//! 运行锁与运行槽（managed-app-contract v1 §1/§5.1/§6）。
//!
//! 每 appId 一个 OS 排他 runtime 锁（跨 Chrome profile 兜底）；
//! 每 OS 用户命名空间四个全局运行槽，持锁代表占用。
//! 崩溃由 OS 释放；不自动终止 busy 应用。

use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

/// 一个已持有的排他文件锁；Drop 时释放并尽力移除锁文件。
pub struct FileLock {
    path: PathBuf,
    file: File,
}

impl FileLock {
    /// 尝试获取排他锁；不可得返回 None（调用方映射为 APP_RUNNING_ELSEWHERE / APP_RUNTIME_LIMIT）。
    pub fn try_acquire(path: PathBuf) -> io::Result<Option<Self>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&path)?;
        if flock_exclusive(file.as_raw_fd()) {
            Ok(Some(Self { path, file }))
        } else {
            Ok(None)
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn release(mut self) {
        self.unlock();
    }

    fn unlock(&mut self) {
        let _ = flock_unlock(self.file.as_raw_fd());
        let _ = std::fs::remove_file(&self.path);
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        self.unlock();
    }
}

/// appId 运行锁路径：apps/.locks/<appId>.runtime.lock
pub fn runtime_lock_path(apps_root: &Path, app_id: &str) -> PathBuf {
    apps_root
        .join(".locks")
        .join(format!("{app_id}.runtime.lock"))
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
}
