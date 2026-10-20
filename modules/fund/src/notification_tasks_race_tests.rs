// Deterministic notification admission, cancellation and worker-reaping tests.
use super::*;

// ==== 复核更正回归（确定性调度，非重复碰运气） ====

/// 持有 Store 内部锁的辅助线程：worker 在渠道查询处确定性阻塞，
/// 直到测试放行。用 mpsc 作为显式调度屏障。
fn hold_store_lock(
    store: &Arc<Store>,
) -> (
    std::thread::JoinHandle<()>,
    mpsc::Receiver<()>,
    mpsc::Sender<()>,
) {
    let (locked_tx, locked_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let store = store.clone();
    let handle = std::thread::spawn(move || {
        let _: Result<(), ()> = store.with_read(|_conn| {
            locked_tx.send(()).unwrap();
            let _ = release_rx.recv(); // 显式屏障：测试决定何时放行
            Ok(())
        });
    });
    (handle, locked_rx, release_tx)
}

#[test]
fn cancel_is_bound_to_verified_generation_under_route_lock() {
    // 缺陷 A 确定性回归：验证＋取消在同一临界区完成并绑定代际。
    // 调度（显式）：A 入队（worker 被 store 锁确定性阻塞）→ 取消 A →
    // 放行 A 完成 → 接纳 B → 断言 B 未被 A 的取消波及。
    let owner = Arc::new(Tasks::new(None));
    let store = Arc::new(Store::open_in_memory().unwrap());
    let (_helper, locked, release) = hold_store_lock(&store);

    let request = build_request(
        "POST",
        "/api/notify/channels/test",
        json!({"id":1,"title":"t","body":"b","requestId":"a1"}),
    );
    let started: serde_json::Value = serde_json::from_str(
        &owner
            .handle(store.clone(), None, &request, "/api/notify/channels/test")
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    let task_a = started["taskId"].as_u64().unwrap();
    locked.recv().unwrap(); // 屏障：A 的 worker 已确定性进入渠道查询

    // 取消 A：验证＋cancel_generation 同临界区（缺陷 A 修复点）。
    let cancel = build_request("POST", "/api/notify/cancel", json!({"taskId":task_a}));
    let receipt: serde_json::Value = serde_json::from_str(
        &owner
            .handle(store.clone(), None, &cancel, "/api/notify/cancel")
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(receipt["taskId"], task_a);
    assert_eq!(receipt["deliveryMayHaveOccurred"], true);
    assert!(
        owner.registry.is_cancelled(task_a),
        "A 必须被其取消请求置位"
    );

    drop(release);
    _helper.join().unwrap();
    // 在锁外等待真实 worker，不能伪造 result 来允许替换仍运行的线程。
    let handle = owner
        .state
        .lock()
        .unwrap()
        .current
        .as_mut()
        .unwrap()
        .handle
        .take()
        .unwrap();
    handle.join().unwrap();
    let request_b = build_request(
        "POST",
        "/api/notify/channels/test",
        json!({"id":1,"title":"t","body":"b","requestId":"b1"}),
    );
    let started_b: serde_json::Value = serde_json::from_str(
        &owner
            .handle(store.clone(), None, &request_b, "/api/notify/channels/test")
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    let task_b = started_b["taskId"].as_u64().unwrap();
    assert_ne!(task_b, task_a);
    assert!(
        !owner.registry.is_cancelled(task_b),
        "A 的取消不得波及后继任务 B"
    );
    owner.close(Instant::now() + Duration::from_millis(2000));
}

#[test]
fn stale_cancel_after_supersession_returns_not_found_and_spares_new_task() {
    // 迟到取消（任务替换后）：A 已完成并被 B 取代——按 id 与按 requestId
    // 的旧取消都必须 404，且 B 完好。确定性：直接构造状态，无时序依赖。
    let owner = Arc::new(Tasks::new(None));
    let store = Arc::new(Store::open_in_memory().unwrap());
    let gen_a = owner.registry.start();
    {
        let mut state = owner.state.lock().unwrap();
        state.current = Some(Task {
            id: gen_a,
            input: json!({"requestId":"old"}),
            result: Some(json!({"status":"sent"})),
            handle: None,
        });
    }
    let gen_b = owner.registry.start();
    {
        let mut state = owner.state.lock().unwrap();
        state.current = Some(Task {
            id: gen_b,
            input: json!({"requestId":"new"}),
            result: None,
            handle: None,
        });
    }
    let by_id = build_request("POST", "/api/notify/cancel", json!({"taskId":gen_a}));
    assert_eq!(
        owner
            .handle(store.clone(), None, &by_id, "/api/notify/cancel")
            .unwrap()
            .unwrap_err()
            .0,
        404
    );
    let by_key = build_request("POST", "/api/notify/cancel", json!({"requestId":"old"}));
    assert_eq!(
        owner
            .handle(store.clone(), None, &by_key, "/api/notify/cancel")
            .unwrap()
            .unwrap_err()
            .0,
        404
    );
    assert!(
        !owner.registry.is_cancelled(gen_b),
        "过期取消不得置位新代际"
    );
    owner.close(Instant::now() + Duration::from_millis(1000));
}

#[test]
fn duplicate_cancel_is_idempotent_while_task_is_current() {
    // 重复取消：任务仍是 current 时两次取消都返回回执且幂等（代际只置位一次）。
    let owner = Arc::new(Tasks::new(None));
    let store = Arc::new(Store::open_in_memory().unwrap());
    let gen = owner.registry.start();
    {
        let mut state = owner.state.lock().unwrap();
        state.current = Some(Task {
            id: gen,
            input: json!({"requestId":"r"}),
            result: None,
            handle: None,
        });
    }
    for _ in 0..2 {
        let request = build_request("POST", "/api/notify/cancel", json!({"taskId":gen}));
        let receipt: serde_json::Value = serde_json::from_str(
            &owner
                .handle(store.clone(), None, &request, "/api/notify/cancel")
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(receipt["taskId"], gen);
    }
    assert!(owner.registry.is_cancelled(gen));
    owner.close(Instant::now() + Duration::from_millis(1000));
}

#[test]
fn handle_registration_is_atomic_with_admission_and_close_reports_honestly() {
    // 缺陷 B 确定性回归：接纳＋spawn＋句柄登记同一临界区；
    // close 对 joined/abandoned 如实区分；Store 引用以 Arc 强计数验证。
    let owner = Arc::new(Tasks::new(None));
    let store = Arc::new(Store::open_in_memory().unwrap());
    let (_helper, locked, release) = hold_store_lock(&store);

    let request = build_request(
        "POST",
        "/api/notify/channels/test",
        json!({"id":1,"title":"t","body":"b","requestId":"h1"}),
    );
    let started: serde_json::Value = serde_json::from_str(
        &owner
            .handle(store.clone(), None, &request, "/api/notify/channels/test")
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    let task_a = started["taskId"].as_u64().unwrap();
    locked.recv().unwrap(); // worker 确定性阻塞在渠道查询

    // 接纳返回时句柄必须已登记（缺陷 B 修复点：无 handle=None 窗口）。
    {
        let state = owner.state.lock().unwrap();
        let task = state.current.as_ref().unwrap();
        assert_eq!(task.id, task_a);
        assert!(task.handle.is_some(), "接纳返回前句柄必须登记完成");
    }
    // worker 持有 Store 引用：强计数 = 测试 + worker 闭包 + worker 线程
    // thread_local 的网络上下文（bind 持 Context→Store）。
    assert_eq!(Arc::strong_count(&store), 3, "worker 必须持有 Store 引用");

    // worker 仍被阻塞：close 在共同 deadline 内放弃等待并如实计入 abandoned。
    let start = Instant::now();
    let report = owner.close(Instant::now() + Duration::from_millis(80));
    assert!(
        start.elapsed() < Duration::from_millis(2000),
        "close 必须有界返回"
    );
    assert_eq!(
        report,
        CloseReport {
            joined: 0,
            pending: 1
        },
        "放弃等待必须如实表达，不得当作回收完成"
    );

    // 放行后 worker 退出：迟到结果被 stopping 守卫丢弃，状态不复活。
    drop(release);
    _helper.join().unwrap();
    {
        let state = owner.state.lock().unwrap();
        assert!(state.stopping);
        let task = state.current.as_ref().unwrap();
        assert!(task.handle.is_some(), "超时后句柄不能丢弃");
        assert!(task.result.is_none(), "迟到结果不能写入关闭状态");
    }
    assert_eq!(
        owner.close(Instant::now() + Duration::from_secs(2)),
        CloseReport {
            joined: 1,
            pending: 0
        }
    );
    assert_eq!(
        Arc::strong_count(&store),
        1,
        "再次关闭必须真正释放 worker 的 Store 引用"
    );

    // joined 路径：worker 无阻塞时 close join 成功，线程退出 → Store 引用释放。
    let owner2 = Arc::new(Tasks::new(None));
    let store2 = Arc::new(Store::open_in_memory().unwrap());
    let request2 = build_request(
        "POST",
        "/api/notify/channels/test",
        json!({"id":1,"title":"t","body":"b","requestId":"h2"}),
    );
    owner2
        .handle(store2.clone(), None, &request2, "/api/notify/channels/test")
        .unwrap()
        .unwrap();
    let report2 = owner2.close(Instant::now() + Duration::from_millis(2000));
    assert_eq!(
        report2,
        CloseReport {
            joined: 1,
            pending: 0
        },
        "无阻塞 worker 必须被 join"
    );
    assert_eq!(
        Arc::strong_count(&store2),
        1,
        "join 后 worker 的 Store 引用必须释放"
    );
}

#[test]
fn admission_after_close_is_rejected_and_close_is_idempotent() {
    let owner = Arc::new(Tasks::new(None));
    let store = Arc::new(Store::open_in_memory().unwrap());
    let first = owner.close(Instant::now() + Duration::from_millis(50));
    let second = owner.close(Instant::now() + Duration::from_millis(50));
    assert_eq!((first.joined, first.pending), (0, 0));
    assert_eq!((second.joined, second.pending), (0, 0));
    let request = build_request(
        "POST",
        "/api/notify/channels/test",
        json!({"id":1,"requestId":"r"}),
    );
    assert_eq!(
        owner
            .handle(store, None, &request, "/api/notify/channels/test")
            .unwrap()
            .unwrap_err(),
        (503, "APP_RUNTIME_STOPPING".into())
    );
}

#[test]
fn cancel_generation_registry_semantics() {
    // Registry 原语：活动代际可取消；过期代际 no-op 且不清除新代际状态。
    let registry = TaskRegistry::new();
    let a = registry.start();
    assert!(registry.cancel_generation(a));
    assert!(registry.is_cancelled(a));
    let b = registry.start(); // start 置位 prev（a）为已取消——既有语义
    assert!(registry.is_cancelled(a));
    assert!(!registry.is_cancelled(b));
    // 过期代际取消：no-op，且不得清除/破坏 b 的未取消状态。
    assert!(!registry.cancel_generation(a));
    assert!(!registry.is_cancelled(b));
    // 活动代际取消生效。
    assert!(registry.cancel_generation(b));
    assert!(registry.is_cancelled(b));
}

fn pause_at(owner: &Tasks, stage: &'static str) -> (mpsc::Receiver<()>, mpsc::Sender<()>) {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Mutex::new(release_rx);
    *owner.test_hook.lock().unwrap() = Some(Box::new(move |actual| {
        if actual == stage {
            entered_tx.send(()).unwrap();
            release_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
        }
    }));
    (entered_rx, release_tx)
}

#[test]
fn verified_cancel_keeps_admission_locked_until_generation_is_cancelled() {
    let owner = Arc::new(Tasks::new(None));
    let store = Arc::new(Store::open_in_memory().unwrap());
    let old = owner.registry.start();
    owner.state.lock().unwrap().current = Some(Task {
        id: old,
        input: json!({"requestId":"old"}),
        result: Some(json!({})),
        handle: None,
    });
    let (entered, release) = pause_at(&owner, "cancel");
    let cancelling = {
        let owner = owner.clone();
        let store = store.clone();
        std::thread::spawn(move || {
            owner
                .handle(
                    store,
                    None,
                    &build_request("POST", "/api/notify/cancel", json!({"taskId":old})),
                    "/api/notify/cancel",
                )
                .unwrap()
        })
    };
    entered.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(
        owner.state.try_lock().is_err(),
        "validated cancel must still own the admission lock"
    );
    release.send(()).unwrap();
    assert!(cancelling.join().unwrap().is_ok());
    let next: Value = serde_json::from_str(
        &owner
            .handle(
                store.clone(),
                None,
                &build_request(
                    "POST",
                    "/api/notify/channels/test",
                    json!({"id":1,"requestId":"next"}),
                ),
                "/api/notify/channels/test",
            )
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert!(!owner
        .registry
        .is_cancelled(next["taskId"].as_u64().unwrap()));
    owner.close(Instant::now() + Duration::from_secs(2));
    assert_eq!(Arc::strong_count(&store), 1);
}

#[test]
fn close_cannot_enter_between_admission_and_spawn_registration() {
    let owner = Arc::new(Tasks::new(None));
    let store = Arc::new(Store::open_in_memory().unwrap());
    let (entered, release) = pause_at(&owner, "spawn");
    let admitting = {
        let owner = owner.clone();
        let store = store.clone();
        std::thread::spawn(move || {
            owner
                .handle(
                    store,
                    None,
                    &build_request(
                        "POST",
                        "/api/notify/channels/test",
                        json!({"id":1,"requestId":"first"}),
                    ),
                    "/api/notify/channels/test",
                )
                .unwrap()
        })
    };
    entered.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(
        owner.state.try_lock().is_err(),
        "close must not observe an unregistered worker"
    );
    let closing = {
        let owner = owner.clone();
        std::thread::spawn(move || owner.close(Instant::now() + Duration::from_secs(2)))
    };
    release.send(()).unwrap();
    assert!(admitting.join().unwrap().is_ok());
    assert_eq!(
        closing.join().unwrap(),
        CloseReport {
            joined: 1,
            pending: 0
        }
    );
    assert_eq!(Arc::strong_count(&store), 1);
    assert!(owner.state.lock().unwrap().current.is_none());
}

#[test]
fn spawn_failure_releases_reservation_and_store_and_allows_retry() {
    let owner = Arc::new(Tasks::new(None));
    let store = Arc::new(Store::open_in_memory().unwrap());
    owner
        .fail_spawn
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let request = build_request(
        "POST",
        "/api/notify/channels/test",
        json!({"id":1,"requestId":"retry"}),
    );
    assert_eq!(
        owner
            .handle(store.clone(), None, &request, "/api/notify/channels/test")
            .unwrap()
            .unwrap_err(),
        (503, "NOTIFY_UNAVAILABLE".into())
    );
    assert!(owner.state.lock().unwrap().current.is_none());
    assert_eq!(Arc::strong_count(&store), 1);
    owner
        .fail_spawn
        .store(false, std::sync::atomic::Ordering::SeqCst);
    assert!(owner
        .handle(store.clone(), None, &request, "/api/notify/channels/test")
        .unwrap()
        .is_ok());
    assert_eq!(
        owner.close(Instant::now() + Duration::from_secs(2)).pending,
        0
    );
    assert_eq!(Arc::strong_count(&store), 1);
}
