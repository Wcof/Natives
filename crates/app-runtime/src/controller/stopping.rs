use super::Controller;
use crate::{ControllerState, CONTROLLER_STATE};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// 看门狗去重旗标：同一进程生命周期内只允许创建一次（重复 shutdown 不重复
/// spawn 看门狗线程，L15）。
static WATCHDOG_STARTED: AtomicBool = AtomicBool::new(false);

/// 看门狗触发时报告的最近阶段：正常完成后为 stopped；看门狗日志出现任何
/// 非 stopped 阶段即代表该阶段未在预算内完成（L15 证据口径：兜底触发不记作
/// 优雅释放成功）。
static SHUTDOWN_PHASE: std::sync::Mutex<Option<&'static str>> = std::sync::Mutex::new(None);

fn shutdown_phase_snapshot() -> &'static str {
    match SHUTDOWN_PHASE.lock() {
        Ok(guard) => guard.unwrap_or("entry"),
        Err(_) => "entry",
    }
}

fn shutdown_phase_record(phase: &'static str) {
    if let Ok(mut guard) = SHUTDOWN_PHASE.lock() {
        *guard = Some(phase);
    }
}

impl Controller {
    pub(super) fn enter_stopping(&self) {
        self.cancellation.cancel();
        self.server_shutdown.store(true, Ordering::SeqCst);

        // 计划 §18 / R-A7 / 架构 Wave 4 硬契约：自关闭起算，2000ms 内进程彻底退出。
        // 看门狗只创建一次；若任何系统调用或第三方 I/O 阻塞超时，由 OS 回收进程
        // 并记录卡住阶段——强制退出不记作优雅清理完成。
        if !WATCHDOG_STARTED.swap(true, Ordering::SeqCst) {
            let phase = shutdown_phase_snapshot();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(2000));
                eprintln!(
                    "natives-app-runtime: event=watchdog_timeout phase={phase} graceful_shutdown_deadline_exceeded"
                );
                std::process::exit(0);
            });
        }

        // 主动解除 accept 阻塞：向自身回环端口发起一次唤醒连接，accept 循环
        // 观察到 shutdown 旗标后退出（F6：不能只设标记继续阻塞在 accept）。
        let port = crate::LOOPBACK_PORT.load(Ordering::SeqCst);
        if port != 0 {
            std::thread::spawn(move || {
                let _ = TcpStream::connect(("127.0.0.1", port));
            });
        }

        // 计划 §18 关闭顺序：cancel → stop accept →（进入 STOPPING 拒绝新请求）
        // → revoke bearer → shutdown module → 释放 runtime lease → STOPPED。
        let taken = {
            let mut state = CONTROLLER_STATE.lock().unwrap();
            match *state {
                ControllerState::Booting | ControllerState::Stopping | ControllerState::Stopped => {
                    None
                }
                _ => Some(std::mem::replace(&mut *state, ControllerState::Stopping)),
            }
        };
        shutdown_phase_record("module_shutdown");
        match taken {
            Some(ControllerState::Ready(ready)) => {
                eprintln!(
                    "natives-app-runtime: event=shutdown app_id={} phase=ready",
                    ready.bound.app_id
                );
                if let Ok(mut sessions) = ready.sessions.lock() {
                    sessions.revoke();
                }
                if let Ok(mut module) = ready.bound.module.lock() {
                    let _ = module.shutdown();
                }
            }
            Some(ControllerState::Bound(bound)) => {
                eprintln!(
                    "natives-app-runtime: event=shutdown app_id={} phase=bound",
                    bound.app_id
                );
                if let Ok(mut module) = bound.module.lock() {
                    let _ = module.shutdown();
                }
            }
            _ => {}
        }
        // 资源释放完成 → 最终 STOPPED（此后 handshake/start 均拒绝）。
        let mut state = CONTROLLER_STATE.lock().unwrap();
        if matches!(*state, ControllerState::Stopping) {
            *state = ControllerState::Stopped;
            shutdown_phase_record("stopped");
        }
    }
}
