use super::Controller;
use crate::{CONTROLLER_STATE, ControllerState};
use std::sync::atomic::Ordering;

impl Controller {
    pub(super) fn enter_stopping(&self) {
        self.cancellation.cancel();
        self.server_shutdown.store(true, Ordering::SeqCst);
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
        }
    }
}
