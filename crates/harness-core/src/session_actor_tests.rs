//! SessionCoordinator unit tests (extracted from `session_actor.rs`).

use super::*;

use super::*;

#[test]
fn enqueue_preserves_fifo_order() {
    let h = SessionCoordinator::new();
    let a = h.enqueue("c1", "first", PromptSource::User, None, Some("i1".into()));
    let b = h.enqueue("c1", "second", PromptSource::User, None, Some("i2".into()));
    let c = h.enqueue("c1", "third", PromptSource::User, None, Some("i3".into()));
    assert_eq!(a.position, 0);
    assert_eq!(b.position, 1);
    assert_eq!(c.position, 2);
    let list = h.list("c1");
    assert_eq!(
        list.iter().map(|i| i.content.as_str()).collect::<Vec<_>>(),
        vec!["first", "second", "third"]
    );
}

#[test]
fn interject_injected_at_safe_point() {
    let h = SessionCoordinator::new();
    h.mark_running("c1", "run-1", "hello");
    h.interject("c1", "stop and do X instead");
    assert!(h.pending_interjection("c1").is_some());

    h.enqueue("c1", "queued", PromptSource::User, None, None);
    let action = h.on_safe_point("c1", SafePoint::AfterTool);
    match action {
        CoordinatorAction::InjectInterjection { content } => {
            assert_eq!(content, "stop and do X instead");
        }
        other => panic!("expected InjectInterjection, got {other:?}"),
    }
    assert!(h.pending_interjection("c1").is_none());
    assert_eq!(
        h.on_safe_point("c1", SafePoint::ProviderBatchBoundary),
        CoordinatorAction::None
    );
    assert_eq!(h.queue_len("c1"), 1);
}

#[test]
fn cancel_and_send_waits_for_terminal_then_starts() {
    let h = SessionCoordinator::new();
    h.mark_running("c1", "run-old", "old prompt");
    let item = h.enqueue(
        "c1",
        "urgent",
        PromptSource::User,
        None,
        Some("q-urgent".into()),
    );

    let action = h.cancel_and_send("c1", &item.id).expect("cancel_and_send");
    match action {
        CoordinatorAction::CancelThenStart { item: i } => {
            assert_eq!(i.content, "urgent");
            assert_eq!(i.id, "q-urgent");
        }
        other => panic!("expected CancelThenStart, got {other:?}"),
    }
    assert!(h.cancel_requested("c1"));
    assert!(h.is_running("c1"));

    let next = h.mark_finished("c1");
    match next {
        CoordinatorAction::StartPrompt { item: i } => {
            assert_eq!(i.id, "q-urgent");
            assert_eq!(i.content, "urgent");
        }
        other => panic!("expected StartPrompt after terminal, got {other:?}"),
    }
    assert!(!h.cancel_requested("c1"));
}

#[test]
fn send_now_when_idle_starts_immediately() {
    let h = SessionCoordinator::new();
    let item = h.enqueue("c1", "go", PromptSource::User, None, Some("q1".into()));
    let action = h.send_now("c1", &item.id).unwrap();
    assert!(matches!(action, CoordinatorAction::StartPrompt { .. }));
    assert_eq!(h.queue_len("c1"), 0);
}

#[test]
fn drain_on_finish_starts_next_queue_item() {
    let h = SessionCoordinator::new();
    h.mark_running("c1", "r1", "p1");
    h.enqueue("c1", "next-1", PromptSource::User, None, Some("n1".into()));
    h.enqueue("c1", "next-2", PromptSource::User, None, Some("n2".into()));
    let action = h.mark_finished("c1");
    match action {
        CoordinatorAction::StartPrompt { item } => {
            assert_eq!(item.id, "n1");
        }
        other => panic!("expected drain StartPrompt, got {other:?}"),
    }
    assert_eq!(h.queue_len("c1"), 1);
}

#[test]
fn reorder_and_remove() {
    let h = SessionCoordinator::new();
    h.enqueue("c1", "a", PromptSource::User, None, Some("a".into()));
    h.enqueue("c1", "b", PromptSource::User, None, Some("b".into()));
    h.enqueue("c1", "c", PromptSource::User, None, Some("c".into()));
    h.reorder("c1", &["c".into(), "a".into(), "b".into()])
        .unwrap();
    let ids: Vec<_> = h.list("c1").into_iter().map(|i| i.id).collect();
    assert_eq!(ids, vec!["c", "a", "b"]);
    h.remove("c1", "a").unwrap();
    assert_eq!(h.queue_len("c1"), 2);
}

#[test]
fn parallel_safe_tool_names() {
    assert!(is_parallel_safe_tool("read_file"));
    assert!(is_parallel_safe_tool("list_dir"));
    assert!(is_parallel_safe_tool("grep"));
    assert!(is_parallel_safe_tool("search_files"));
    assert!(is_parallel_safe_tool("memory_search"));
    assert!(!is_parallel_safe_tool("write_file"));
    assert!(!is_parallel_safe_tool("run_terminal"));
    assert!(!is_parallel_safe_tool("mcp_call"));
    assert_eq!(PARALLEL_SAFE_MAX_CONCURRENCY, 4);
}

#[test]
fn all_safe_point_variants_accept_interjection() {
    let h = SessionCoordinator::new();
    for point in [
        SafePoint::ProviderBatchBoundary,
        SafePoint::BeforeTool,
        SafePoint::AfterTool,
        SafePoint::AfterPermissionResolved,
    ] {
        h.interject("c1", format!("inj-{point:?}"));
        let action = h.on_safe_point("c1", point);
        assert!(matches!(
            action,
            CoordinatorAction::InjectInterjection { .. }
        ));
    }
}

#[test]
fn reload_queue_and_snapshot_restore() {
    let h = SessionCoordinator::new();
    h.reload_queue(
        "c1",
        vec![
            QueueItem {
                id: "q1".into(),
                conversation_id: "c1".into(),
                content: "a".into(),
                source: PromptSource::User,
                position: 0,
                client_temp_id: None,
                created_at: "t".into(),
                status: QueueItemStatus::Queued,
            },
            QueueItem {
                id: "q2".into(),
                conversation_id: "c1".into(),
                content: "b".into(),
                source: PromptSource::User,
                position: 1,
                client_temp_id: None,
                created_at: "t".into(),
                status: QueueItemStatus::Running,
            },
        ],
    );
    assert_eq!(h.queue_len("c1"), 2);

    h.interject("c1", "keep me");
    h.set_pending_interaction("c1", Some("perm-1".into()));
    let snap = h.snapshot("c1");
    assert_eq!(snap.pending_interjection.as_deref(), Some("keep me"));
    assert_eq!(snap.pending_interaction_id.as_deref(), Some("perm-1"));

    let h2 = SessionCoordinator::new();
    h2.reload_queue("c1", h.list("c1"));
    h2.restore_snapshot(snap);
    assert_eq!(h2.pending_interjection("c1").as_deref(), Some("keep me"));
    assert_eq!(h2.pending_interaction("c1").as_deref(), Some("perm-1"));
    assert!(!h2.is_running("c1"));
}

#[test]
fn failed_terminal_marks_item_failed_and_drains_next() {
    let h = SessionCoordinator::new();
    let first = h.enqueue("c1", "p1", PromptSource::User, None, Some("q1".into()));
    h.enqueue("c1", "p2", PromptSource::User, None, Some("q2".into()));
    h.mark_running_item("c1", "r1", Some(&first.id), "p1");
    let action = h.mark_finished_with_outcome("c1", false);
    match action {
        CoordinatorAction::StartPrompt { item } => assert_eq!(item.id, "q2"),
        other => panic!("expected next after fail, got {other:?}"),
    }
}

#[test]
fn finish_run_rejects_stale_run_id() {
    let h = SessionCoordinator::new();
    h.mark_running("c1", "run-live", "prompt");
    assert!(h.finish_run("c1", "run-stale", true).is_none());
    assert!(h.is_running("c1"));
    let action = h.finish_run("c1", "run-live", true);
    assert!(matches!(action, Some(CoordinatorAction::None)));
    assert!(!h.is_running("c1"));
}

#[test]
fn finish_run_and_send_now_race_starts_exactly_one() {
    // Model: send_now claims CancelThenStart + pending; only one finish_run
    // may advance. Second terminal with same or different id is stale.
    let h = SessionCoordinator::new();
    h.mark_running("c1", "run-old", "old");
    let item = h.enqueue("c1", "urgent", PromptSource::User, None, Some("q-u".into()));
    let action = h.send_now("c1", &item.id).unwrap();
    assert!(matches!(action, CoordinatorAction::CancelThenStart { .. }));

    // First terminal for the cancelled run wins and yields StartPrompt.
    let first = h.finish_run("c1", "run-old", false);
    match first {
        Some(CoordinatorAction::StartPrompt { item: i }) => assert_eq!(i.id, "q-u"),
        other => panic!("expected StartPrompt, got {other:?}"),
    }
    // Second terminal (duplicate) must be stale even if run id matches a ghost.
    assert!(h.finish_run("c1", "run-old", false).is_none());
}

#[test]
fn requeue_restores_claimed_item_at_front() {
    let h = SessionCoordinator::new();
    let first = h.enqueue("c1", "first", PromptSource::User, None, Some("q1".into()));
    let second = h.enqueue("c1", "second", PromptSource::User, None, Some("q2".into()));
    h.mark_running_item("c1", "run-1", Some(&first.id), &first.content);

    h.requeue("c1", first);

    let items = h.list("c1");
    assert_eq!(
        items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["q1", "q2"]
    );
    assert_eq!(items[0].status, QueueItemStatus::Queued);
    assert_eq!(items[0].position, 0);
    assert_eq!(items[1].position, 1);
    assert!(!h.is_running("c1"));
    assert!(h.list("c1").iter().any(|item| item.id == second.id));
}
