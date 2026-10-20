//! Session-owned notification attempt. Cancelling never promises to recall an in-flight send.
//!
//! mode/T03 审计更正（A24）：任务 owner 登记 spawn 句柄并在关闭时有界等待；
//! stopping 与入队共用同一临界区拒绝新任务；迟到结果不写已关闭状态。
//! 复用 TaskRegistry（information 域既有机制），不建第二 registry。

use crate::notification_send::{NotifyCommand, SecretReveal};
use crate::{information::TaskRegistry, storage::Store};
use app_runtime_core::{http::HttpRequest, secrets::SecretStore, CancellationToken};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Task {
    id: u64,
    /// 传输载荷：requestId 重放与异载荷冲突判定（传输层身份，非业务事实）。
    input: Value,
    result: Option<Value>,
    handle: Option<std::thread::JoinHandle<()>>,
}

/// 同一临界区管理的运行状态：入队、stopping 拒绝、结果写入与清理互斥。
#[derive(Default)]
struct TaskState {
    stopping: bool,
    current: Option<Task>,
}

pub(crate) struct Tasks {
    registry: TaskRegistry,
    state: Mutex<TaskState>,
    #[cfg(test)]
    test_hook: Mutex<Option<Box<dyn Fn(&str) + Send + Sync>>>,
    #[cfg(test)]
    fail_spawn: std::sync::atomic::AtomicBool,
}

/// 超时后保留句柄，下次 close 可以继续回收；pending 不代表资源已释放。
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(crate) struct CloseReport {
    pub joined: usize,
    pub pending: usize,
}

impl Tasks {
    pub fn new(cancel: Option<Arc<CancellationToken>>) -> Self {
        Self {
            registry: TaskRegistry::with_cancellation(cancel),
            state: Mutex::new(TaskState::default()),
            #[cfg(test)]
            test_hook: Mutex::new(None),
            #[cfg(test)]
            fail_spawn: std::sync::atomic::AtomicBool::new(false),
        }
    }
    pub fn cancel(&self) {
        self.registry.cancel_current();
    }
    pub fn is_finished(&self) -> bool {
        self.state.lock().is_ok_and(|state| {
            state
                .current
                .as_ref()
                .is_none_or(|task| task.result.is_some())
        })
    }
    pub fn cancel_task(&self, task_id: u64) {
        self.registry.cancel_generation(task_id);
    }

    /// 接纳与关闭使用同一锁；等待不持锁，超时仍由本 owner 持有线程句柄。
    pub fn close(&self, deadline: Instant) -> CloseReport {
        {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.stopping = true;
            if let Some(task) = state.current.as_ref() {
                self.registry.cancel_generation(task.id);
            }
        }
        loop {
            let handle = {
                let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                match state.current.as_mut().and_then(|task| task.handle.as_mut()) {
                    None => {
                        state.current.take();
                        return CloseReport::default();
                    }
                    Some(handle) if handle.is_finished() => {
                        state.current.take().and_then(|task| task.handle)
                    }
                    Some(_) if Instant::now() >= deadline => {
                        return CloseReport {
                            joined: 0,
                            pending: 1,
                        };
                    }
                    Some(_) => None,
                }
            };
            if let Some(handle) = handle {
                let _ = handle.join();
                return CloseReport {
                    joined: 1,
                    pending: 0,
                };
            }
            std::thread::sleep(
                Duration::from_millis(10).min(deadline.saturating_duration_since(Instant::now())),
            );
        }
    }

    pub fn handle(
        self: &Arc<Self>,
        store: Arc<Store>,
        secrets: Option<Arc<SecretStore>>,
        request: &HttpRequest,
        route: &str,
    ) -> Option<Result<String, (u16, String)>> {
        if !matches!(
            route,
            "/api/notify/channels/test"
                | "/api/notify/send"
                | "/api/notify/status"
                | "/api/notify/cancel"
        ) {
            return None;
        }
        Some((|| {
            if request.method == "GET" && route == "/api/notify/status" {
                let id = crate::market::query_param(&request.path, "taskId")
                    .and_then(|v| v.parse::<u64>().ok())
                    .ok_or((400, "FIN_INVALID".into()))?;
                let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                let task = state
                    .current
                    .as_ref()
                    .filter(|t| t.id == id)
                    .ok_or((404, "FIN_NOT_FOUND".into()))?;
                let status = if self.registry.is_cancelled(id) {
                    "cancelled"
                } else if task.result.is_some() {
                    "complete"
                } else {
                    "running"
                };
                return Ok(json!({"taskId":id,"status":status,"result":task.result}).to_string());
            }
            if request.method != "POST" {
                return Err((405, "APP_METHOD_INVALID".into()));
            }
            let input: Value =
                serde_json::from_slice(&request.body).map_err(|_| (400, "FIN_INVALID".into()))?;
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if route == "/api/notify/cancel" {
                let id = input["taskId"].as_u64();
                let key = input["requestId"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 128);
                if !input.as_object().is_some_and(|o| o.len() == 1) || id.is_some() == key.is_some()
                {
                    return Err((400, "FIN_INVALID".into()));
                }
                let task = state
                    .current
                    .as_ref()
                    .filter(|t| {
                        id == Some(t.id) || key.is_some_and(|key| t.input["requestId"] == key)
                    })
                    .ok_or((404, "FIN_NOT_FOUND".into()))?;
                let task_id = task.id;
                #[cfg(test)]
                if let Some(hook) = self.test_hook.lock().unwrap().as_ref() {
                    hook("cancel");
                }
                // 验证与取消共用 admission 锁，取消只作用于已验证的代际。
                self.registry.cancel_generation(task_id);
                drop(state);
                return Ok(
                    json!({"taskId":task_id,"cancelled":true,"deliveryMayHaveOccurred":true})
                        .to_string(),
                );
            }
            // stopping 与入队共用临界区，关闭后拒绝新任务。
            if state.stopping {
                return Err((503, "APP_RUNTIME_STOPPING".into()));
            }
            let key = input["requestId"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 128)
                .ok_or((400, "FIN_INVALID: requestId".into()))?;
            if let Some(task) = state.current.as_ref() {
                if task.input["requestId"] == key {
                    if task.input != input {
                        return Err((409, "FIN_CONFLICT: requestId".into()));
                    }
                    return Ok(json!({"taskId":task.id}).to_string());
                }
                if task.result.is_none()
                    || task
                        .handle
                        .as_ref()
                        .is_some_and(|handle| !handle.is_finished())
                {
                    return Err((409, "NOTIFY_IN_PROGRESS".into()));
                }
            }
            // 已完成结果不等于线程已退出；只有真正结束的旧线程才可被替换。
            if let Some(handle) = state.current.as_mut().and_then(|task| task.handle.take()) {
                let _ = handle.join(); // 上面的 is_finished 已确认退出，无 worker 锁等待。
            }
            let id = self.registry.start();
            state.current = Some(Task {
                id,
                input: input.clone(),
                result: None,
                handle: None,
            });
            let owner = self.clone();
            let business = route == "/api/notify/send";
            let network_context = crate::network::current();
            #[cfg(test)]
            if let Some(hook) = self.test_hook.lock().unwrap().as_ref() {
                hook("spawn");
            }
            // spawn 与句柄登记共用 admission 锁，close 不会漏掉已接纳的 worker。
            let worker = move || {
                let _network_scope = crate::network::bind(network_context);
                let command = match NotifyCommand::from_raw(&input, business) {
                    Ok(command) => command,
                    Err(e) => {
                        owner.record_result(id, notify_failure(&e));
                        return;
                    }
                };
                let secrets_view = secrets.as_ref().map(|s| s.as_ref() as &dyn SecretReveal);
                let result =
                    match crate::notification_send::send(&store, secrets_view, &command, || {
                        owner.registry.is_cancelled(id)
                    }) {
                        Ok(value) => value,
                        Err(e) => notify_failure(&e),
                    };
                owner.record_result(id, result);
            };
            let builder = std::thread::Builder::new().name("fund-notify".into());
            #[cfg(not(test))]
            let spawned = builder.spawn(worker);
            #[cfg(test)]
            let spawned = if self.fail_spawn.load(std::sync::atomic::Ordering::SeqCst) {
                Err(std::io::Error::from(std::io::ErrorKind::WouldBlock))
            } else {
                builder.spawn(worker)
            };
            match spawned {
                Ok(handle) => {
                    if let Some(task) = state.current.as_mut().filter(|t| t.id == id) {
                        task.handle = Some(handle);
                    }
                }
                Err(_) => {
                    // spawn 失败：同一临界区清理占位并取消代际，不悬挂。
                    self.registry.cancel_generation(id);
                    state.current.take();
                    return Err((503, "NOTIFY_UNAVAILABLE".into()));
                }
            }
            drop(state);
            Ok(json!({"taskId":id}).to_string())
        })())
    }

    /// 迟到结果写入：stopping 后或代际不匹配一律丢弃，不写已关闭状态。
    fn record_result(&self, id: u64, result: Value) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.stopping {
            return;
        }
        if let Some(task) = state.current.as_mut().filter(|t| t.id == id) {
            task.result = Some(result);
        }
    }
}

/// 任务失败投影（wire 形态由任务适配层组装；语义错误来自业务核心）。
fn notify_failure(e: &crate::notification_send::NotifyError) -> Value {
    use crate::notification_send::NotifyError;
    let status = match e {
        NotifyError::NotFound => 404,
        NotifyError::Conflict { .. } => 409,
        _ => 400,
    };
    json!({"error":"NOTIFY_FAILED","httpStatus":status,"code":e.code(),"message":e.message()})
}

#[cfg(test)]
#[path = "notification_tasks_tests.rs"]
mod tests;
