//! MutationLock（APP-018 / 主 Agent 决策 #3）。
//!
//! 从 `creative_app::service` 下沉到 Apps 域：按 **application_id** 串行化
//! start/stop/restart/remove 等 mutation，不同 app 可并行；重安装/依赖安装走
//! 独立有界信号量（不阻塞无关 app 的 lifecycle）。
//!
//! `creative_app` 侧改为 `pub use` re-export（旧 `MutationLock` 路径在迁移期继续
//! 可用，同一类型 → 同一 Tauri managed state）。

use std::collections::HashMap;
use std::sync::{Arc, Weak};
use tokio::sync::Mutex as TokioMutex;

/// Application-keyed mutation lock registry。
///
/// - per-application async mutex（同一 app 独占，无关 app 并行）；
/// - 有界 install semaphore（重安装限流但不串行化 lifecycle）。
///
/// 条目以 `Weak` 持有：锁的最后 guard drop 后可被回收；check-or-create 在
/// `std::sync::Mutex` 下完成，两个并发 acquire 同一 key 必然升级同一个底层
/// mutex（独占性不会分裂成两把锁）。
pub struct MutationLockRegistry {
    inner: std::sync::Mutex<HashMap<String, Weak<TokioMutex<()>>>>,
    install: Arc<tokio::sync::Semaphore>,
}

/// 安装（GitHub 容器 / 依赖安装）共享一个有界信号量。
const INSTALL_SEMAPHORE_PERMITS: usize = 2;

impl MutationLockRegistry {
    pub fn new() -> Self {
        Self {
            inner: std::sync::Mutex::new(HashMap::new()),
            install: Arc::new(tokio::sync::Semaphore::new(INSTALL_SEMAPHORE_PERMITS)),
        }
    }

    /// 取 per-application 锁。只阻塞**同一 application**的其他 mutation ——
    /// 无关 app 并行推进（CR-202）。
    pub async fn acquire_app(&self, key: &str) -> tokio::sync::OwnedMutexGuard<()> {
        let arc = Self::upgrade_or_create(&self.inner, key);
        arc.lock_owned().await
    }

    /// 非阻塞变体（2s watchdog 用）：当前 app 正在 mutation 时跳过而不是卡住
    /// reconcile 循环。
    pub fn try_acquire_app(&self, key: &str) -> Option<tokio::sync::OwnedMutexGuard<()>> {
        let arc = Self::upgrade_or_create(&self.inner, key);
        arc.try_lock_owned().ok()
    }

    /// install/Docker 重 mutation 的有界 permit。
    pub async fn acquire_install(&self) -> tokio::sync::OwnedSemaphorePermit {
        // 信号量永不 close；acquire_owned 只在 registry 被拆除时失败，视为
        // 安装门关闭。
        self.install
            .clone()
            .acquire_owned()
            .await
            .expect("install semaphore is never closed")
    }

    fn upgrade_or_create(
        inner: &std::sync::Mutex<HashMap<String, Weak<TokioMutex<()>>>>,
        key: &str,
    ) -> Arc<TokioMutex<()>> {
        let mut map = inner.lock().unwrap_or_else(|e| e.into_inner());
        match map.get(key) {
            Some(weak) => match weak.upgrade() {
                Some(a) => a,
                None => {
                    let a = Arc::new(TokioMutex::new(()));
                    map.insert(key.to_string(), Arc::downgrade(&a));
                    a
                }
            },
            None => {
                let a = Arc::new(TokioMutex::new(()));
                map.insert(key.to_string(), Arc::downgrade(&a));
                a
            }
        }
    }
}

impl Default for MutationLockRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Tauri-managed 共享句柄（apps 与 creative_app 共用同一类型）。
pub type MutationLock = Arc<MutationLockRegistry>;

/// 构造 managed state。
pub fn new_mutation_lock() -> MutationLock {
    Arc::new(MutationLockRegistry::new())
}

#[cfg(test)]
mod lock_tests {
    //! APP-018 验收：并发测试 —— 同 App 冲突操作不交叉，不同 App 并行。

    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime")
    }

    #[test]
    fn same_app_is_exclusive() {
        let lock = new_mutation_lock();
        let _a = rt().block_on(lock.acquire_app("app-a"));
        let entered = Arc::new(AtomicBool::new(false));
        let lock2 = lock.clone();
        let entered2 = entered.clone();
        let handle = std::thread::spawn(move || {
            let _g = rt().block_on(lock2.acquire_app("app-a"));
            entered2.store(true, Ordering::SeqCst);
        });
        std::thread::sleep(Duration::from_millis(50));
        assert!(
            !entered.load(Ordering::SeqCst),
            "a second same-app mutation must wait for the first"
        );
        drop(_a);
        handle
            .join()
            .expect("second acquire completes after release");
        assert!(entered.load(Ordering::SeqCst));
    }

    #[test]
    fn different_apps_run_in_parallel() {
        let lock = new_mutation_lock();
        let _a = rt().block_on(lock.acquire_app("app-a"));
        let completed = Arc::new(AtomicBool::new(false));
        let lock2 = lock.clone();
        let c2 = completed.clone();
        let handle = std::thread::spawn(move || {
            let _g = rt().block_on(lock2.acquire_app("app-b"));
            c2.store(true, Ordering::SeqCst);
        });
        // B must acquire without waiting for A's long operation (CR-202 #04).
        std::thread::sleep(Duration::from_millis(50));
        assert!(
            completed.load(Ordering::SeqCst),
            "unrelated apps must run in parallel"
        );
        drop(_a);
        handle.join().expect("join");
    }
}
