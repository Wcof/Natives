//! 协作式取消令牌（计划 §15）：所有长任务（网络请求、数据导入、
//! 历史同步、批量计算、文件解析）必须接入；Shutdown 首步即 cancel()。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Default)]
pub struct CancellationToken {
    cancelled: AtomicBool,
    lock: Mutex<()>,
    notify: Condvar,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.notify.notify_all();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// 阻塞等待取消；返回 true 表示已取消，false 表示超时（timeout=None 为无限等待）。
    pub fn wait_cancelled(&self, timeout: Option<Duration>) -> bool {
        if self.is_cancelled() {
            return true;
        }
        let guard = self.lock.lock().unwrap();
        match timeout {
            None => {
                let _unused = self
                    .notify
                    .wait_while(guard, |_| !self.cancelled.load(Ordering::SeqCst))
                    .unwrap();
                true
            }
            Some(limit) => {
                let deadline = Instant::now() + limit;
                let mut guard = guard;
                while !self.cancelled.load(Ordering::SeqCst) {
                    let now = Instant::now();
                    if now >= deadline {
                        return false;
                    }
                    let (next, _timed_out) =
                        self.notify.wait_timeout(guard, deadline - now).unwrap();
                    guard = next;
                }
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn cancel_is_visible_and_wait_returns_immediately() {
        let token = Arc::new(CancellationToken::new());
        assert!(!token.is_cancelled());
        token.cancel();
        assert!(token.is_cancelled());
        assert!(token.wait_cancelled(Some(Duration::from_millis(0))));
    }

    #[test]
    fn wait_timeout_returns_false_when_not_cancelled() {
        let token = CancellationToken::new();
        assert!(!token.wait_cancelled(Some(Duration::from_millis(30))));
        token.cancel();
        assert!(token.wait_cancelled(None));
    }

    #[test]
    fn waiter_wakes_on_cancel_from_another_thread() {
        let token = Arc::new(CancellationToken::new());
        let waiter = token.clone();
        let handle = std::thread::spawn(move || waiter.wait_cancelled(None));
        std::thread::sleep(Duration::from_millis(20));
        token.cancel();
        assert!(handle.join().unwrap());
    }
}
