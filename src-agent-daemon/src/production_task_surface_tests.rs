use super::*;
use std::time::Duration;


#[tokio::test]
async fn list_and_kill_task_roundtrip() {
    let rt = ProductionRuntime::new();
    rt.task_outputs.lock().await.insert(
        "task-1".into(),
        TaskRecord {
            run_id: "run-child-1".into(),
            status: "running".into(),
            output: None,
        },
    );
    rt.task_outputs.lock().await.insert(
        "task-2".into(),
        TaskRecord {
            run_id: "run-child-2".into(),
            status: "completed".into(),
            output: Some("done".into()),
        },
    );

    let listed = rt.list_tasks().await;
    assert_eq!(listed.len(), 2);
    assert!(listed
        .iter()
        .any(|(id, r)| id == "task-1" && r.status == "running"));
    assert!(listed
        .iter()
        .any(|(id, r)| id == "task-2" && r.output.as_deref() == Some("done")));

    // Unknown task → false; known task flips status even without a live engine.
    assert!(!rt.kill_task("missing").await);
    assert!(rt.kill_task("task-1").await);
    let rec = rt.task_output("task-1").await.expect("task-1 present");
    assert_eq!(rec.status, "cancelled");
}

#[tokio::test]
async fn wait_task_returns_completed() {
    let rt = Arc::new(ProductionRuntime::new());
    rt.task_outputs.lock().await.insert(
        "wait-done".into(),
        TaskRecord {
            run_id: "run-w1".into(),
            status: "running".into(),
            output: None,
        },
    );
    let rt_bg = rt.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(30)).await;
        if let Some(rec) = rt_bg.task_outputs.lock().await.get_mut("wait-done") {
            rec.status = "completed".into();
            rec.output = Some("ok".into());
        }
    });
    let rec = rt
        .wait_task("wait-done", 2_000)
        .await
        .expect("should complete");
    assert_eq!(rec.status, "completed");
    assert_eq!(rec.output.as_deref(), Some("ok"));
}

#[tokio::test]
async fn wait_task_times_out_while_running() {
    let rt = ProductionRuntime::new();
    rt.task_outputs.lock().await.insert(
        "wait-slow".into(),
        TaskRecord {
            run_id: "run-w2".into(),
            status: "running".into(),
            output: None,
        },
    );
    let err = rt.wait_task("wait-slow", 80).await.unwrap_err();
    assert_eq!(err, "timeout");
}

#[tokio::test]
async fn wait_task_unknown_id() {
    let rt = ProductionRuntime::new();
    let err = rt.wait_task("missing-task", 50).await.unwrap_err();
    assert!(err.contains("unknown task_id"), "{err}");
}
