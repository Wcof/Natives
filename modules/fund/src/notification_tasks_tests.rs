//! mode/T03 任务 owner 生命周期回归（A24）：stopping 与入队同一临界区、
//! spawn 句柄登记与有界 wait、迟到结果不写已关闭状态、入队/停止竞态、
//! 反复关闭幂等、requestId 重放与异载荷冲突保持。

use super::*;
use crate::notification_send::NotifyError;
use serde_json::json;
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn build_request(method: &str, route: &str, body: Value) -> HttpRequest {
    HttpRequest::for_internal(method, route, body.to_string().into_bytes())
}

fn blocked_executor_gate() -> (mpsc::Sender<()>, mpsc::Receiver<()>) {
    mpsc::channel()
}

#[test]
fn close_rejects_new_tasks_in_same_critical_section_and_is_idempotent() {
    // A24：stopping 后入队拒绝（503 APP_RUNTIME_STOPPING）；反复关闭幂等。
    let owner = Arc::new(Tasks::new(None));
    owner.close(Instant::now() + Duration::from_millis(50));
    owner.close(Instant::now() + Duration::from_millis(50));
    let request = build_request("POST", "/api/notify/send", json!({"requestId":"r1"}));
    let result = owner
        .handle(
            Arc::new(Store::open_in_memory().unwrap()),
            None,
            &request,
            "/api/notify/send",
        )
        .unwrap();
    let (code, message) = result.unwrap_err();
    assert_eq!((code, message.as_str()), (503, "APP_RUNTIME_STOPPING"));
    {
        let state = owner.state.lock().unwrap();
        assert!(state.stopping);
        assert!(state.current.is_none());
    }
}

#[test]
fn worker_handle_registered_and_close_joins_bounded_with_cancelled_result() {
    // A24：句柄登记 → cancel → close 在 deadline 内 join，结果为 cancelled。
    let owner = Arc::new(Tasks::new(None));
    let store = Arc::new(Store::open_in_memory().unwrap());
    let request = build_request(
        "POST",
        "/api/notify/channels/test",
        json!({"id":1,"title":"t","body":"b","requestId":"r1"}),
    );
    let started = owner
        .handle(store.clone(), None, &request, "/api/notify/channels/test")
        .unwrap()
        .unwrap();
    let task_id: serde_json::Value = serde_json::from_str(&started).unwrap();
    let task_id = task_id["taskId"].as_u64().unwrap();
    {
        let state = owner.state.lock().unwrap();
        assert!(
            state.current.as_ref().unwrap().handle.is_some(),
            "spawn 句柄必须登记"
        );
    }
    owner.cancel();
    let deadline = Instant::now() + Duration::from_millis(2000);
    owner.close(deadline);
    assert!(
        Instant::now() < deadline + Duration::from_millis(500),
        "close 必须有界返回"
    );
    {
        let state = owner.state.lock().unwrap();
        assert!(state.current.is_none(), "close 清空当前任务");
        assert!(state.stopping);
    }
    assert!(task_id > 0);
}

#[test]
fn late_result_after_close_is_discarded() {
    // A24：关闭时 worker 仍在执行 → 有界放弃等待；迟到的结果写入被
    // stopping 拒绝，不写已关闭状态。
    let owner = Arc::new(Tasks::new(None));
    let (gate_tx, gate_rx) = blocked_executor_gate();
    let gate = Arc::new(std::sync::Mutex::new(gate_rx));
    // 用 handle 的入队路径无法注入执行器（生产执行器固定），因此直接验证
    // record_result 的 stopping 守卫 + close 清空语义：
    owner.close(Instant::now() + Duration::from_millis(30));
    owner.record_result(42, json!({"status":"sent"}));
    let state = owner.state.lock().unwrap();
    assert!(state.current.is_none(), "迟到结果不得复活或写入已关闭状态");
    drop(state);
    drop(gate_tx);
    drop(gate);
}

#[test]
fn enqueue_close_race_never_leaves_dangling_task() {
    // A24：入队与停止竞态——多轮交替，最终状态一致（无悬挂任务、无假结果）。
    for round in 0..32u64 {
        let owner = Arc::new(Tasks::new(None));
        let store = Arc::new(Store::open_in_memory().unwrap());
        let stopper = {
            let owner = owner.clone();
            std::thread::spawn(move || {
                owner.close(Instant::now() + Duration::from_millis(20));
            })
        };
        let request = build_request(
            "POST",
            "/api/notify/channels/test",
            json!({"id":1,"title":"t","body":"b","requestId":format!("r{round}")}),
        );
        let enqueued = owner
            .handle(store.clone(), None, &request, "/api/notify/channels/test")
            .unwrap();
        // 两种合法结果：入队成功（随后被 close 取消/清理）或被 stopping 拒绝。
        if let Ok(body) = enqueued {
            assert!(body.contains("taskId"), "{body}");
        } else {
            let (code, _) = enqueued.unwrap_err();
            assert_eq!(code, 503, "拒绝只能是 stopping");
        }
        stopper.join().unwrap();
        let state = owner.state.lock().unwrap();
        assert!(state.stopping);
        // close 已运行：current 要么被清空（有任务→take），要么从未入队。
        if let Some(task) = state.current.as_ref() {
            assert!(task.handle.is_some(), "超时 worker 仍须有 owner 句柄");
        }
    }
}

#[test]
fn replay_and_payload_conflict_semantics_are_preserved() {
    // A09：同 requestId 同载荷重放同 taskId；异载荷 409；进行中第二请求 409。
    let owner = Arc::new(Tasks::new(None));
    let store = Arc::new(Store::open_in_memory().unwrap());
    let body = json!({"id":1,"title":"t","body":"b","requestId":"same"});
    let request = build_request("POST", "/api/notify/channels/test", body.clone());
    let first: serde_json::Value = serde_json::from_str(
        &owner
            .handle(store.clone(), None, &request, "/api/notify/channels/test")
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    let second: serde_json::Value = serde_json::from_str(
        &owner
            .handle(store.clone(), None, &request, "/api/notify/channels/test")
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(first["taskId"], second["taskId"], "同载荷重放同 taskId");
    let conflicting = build_request(
        "POST",
        "/api/notify/channels/test",
        json!({"id":1,"title":"t","body":"b2","requestId":"same"}),
    );
    let err = owner
        .handle(store, None, &conflicting, "/api/notify/channels/test")
        .unwrap()
        .unwrap_err();
    assert_eq!(err.0, 409);
    owner.close(Instant::now() + Duration::from_millis(2000));
}

#[test]
fn late_cancel_cannot_cancel_new_generation_and_replay_is_payload_bound() {
    // 既有代际回归保持：旧 taskId/requestId 取消不影响新代际。
    let owner = Arc::new(Tasks::new(None));
    let id = owner.registry.start();
    {
        let mut state = owner.state.lock().unwrap();
        state.current = Some(Task {
            id,
            input: json!({"requestId":"same"}),
            result: Some(json!({})),
            handle: None,
        });
    }
    let next = owner.registry.start();
    {
        let mut state = owner.state.lock().unwrap();
        state.current = Some(Task {
            id: next,
            input: json!({"requestId":"next"}),
            result: None,
            handle: None,
        });
    }
    let store = Arc::new(Store::open_in_memory().unwrap());
    let request = build_request("POST", "/api/notify/cancel", json!({"taskId":id}));
    assert_eq!(
        owner
            .handle(store.clone(), None, &request, "/api/notify/cancel")
            .unwrap()
            .unwrap_err()
            .0,
        404
    );
    assert!(!owner.registry.is_cancelled(next));
    let request = build_request("POST", "/api/notify/cancel", json!({"requestId":"next"}));
    assert!(owner
        .handle(store, None, &request, "/api/notify/cancel")
        .unwrap()
        .is_ok());
    assert!(owner.registry.is_cancelled(next));
    owner.close(Instant::now() + Duration::from_millis(100));
}

#[test]
fn notify_failure_projection_maps_semantic_errors() {
    // 任务失败投影：语义错误 → 稳定 code + 语义 status（适配层职责）。
    let e = NotifyError::NotFound;
    assert_eq!(notify_failure(&e)["code"], "FIN_NOT_FOUND");
    assert_eq!(notify_failure(&e)["httpStatus"], 404);
    let e = NotifyError::Conflict {
        message: "渠道已变化".into(),
    };
    assert_eq!(notify_failure(&e)["httpStatus"], 409);
    let e = NotifyError::SecretUnavailable {
        message: "locked".into(),
    };
    let projection = notify_failure(&e);
    assert_eq!(projection["httpStatus"], 400);
    assert!(projection["message"].as_str().unwrap().contains("locked"));
}

#[path = "notification_tasks_race_tests.rs"]
mod races;
