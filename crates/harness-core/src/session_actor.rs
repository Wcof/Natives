//! SessionCoordinator — single per-conversation execution actor.
//!
//! Replaces the dual SessionHarness split (agent-core skeleton + daemon-local
//! copy). One logical actor state per `conversation_id` owns:
//! - Prompt queue (FIFO + reorder)
//! - Interjection (latest wins, inject at safe points)
//! - Pending interaction (permission / ask id)
//! - Cancel-and-send / cancel-requested
//! - Drain-on-finish → next prompt
//!
//! Methods are synchronous and thread-safe via interior mutex so daemon RPC
//! and engine safe-point hooks share the same map. Persistence is the
//! daemon's responsibility (`prompt_queue` + `session_actor` tables); this
//! module is the in-process coordination authority the engine calls.
//!
//! ## Safe points (only these may inject interjection or drain queue)
//! - Provider batch boundary
//! - Before / after tool execution
//! - After permission resolution

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// Where the engine may inject pending interjection or drain the prompt queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafePoint {
    /// Between provider stream batches / turns.
    ProviderBatchBoundary,
    /// Immediately before a tool call starts.
    BeforeTool,
    /// Immediately after a tool call completes.
    AfterTool,
    /// After a permission request is resolved (allow/deny).
    AfterPermissionResolved,
}

/// Source of a queued prompt item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptSource {
    User,
    Scheduler,
    BackgroundWake,
    System,
    Interjection,
}

impl PromptSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Scheduler => "scheduler",
            Self::BackgroundWake => "background_wake",
            Self::System => "system",
            Self::Interjection => "interjection",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "scheduler" => Self::Scheduler,
            "background_wake" => Self::BackgroundWake,
            "system" => Self::System,
            "interjection" => Self::Interjection,
            _ => Self::User,
        }
    }
}

/// Status of a queue item (mirrors daemon `prompt_queue.status` when present).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueItemStatus {
    Queued,
    Running,
    Sent,
    Cancelled,
    Failed,
}

impl QueueItemStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Sent => "sent",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "running" => Self::Running,
            "sent" => Self::Sent,
            "cancelled" => Self::Cancelled,
            "failed" => Self::Failed,
            _ => Self::Queued,
        }
    }
}

/// One item in the in-memory prompt queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueItem {
    pub id: String,
    pub conversation_id: String,
    pub content: String,
    pub source: PromptSource,
    pub position: i64,
    pub client_temp_id: Option<String>,
    pub created_at: String,
    pub status: QueueItemStatus,
}

/// Action returned by safe-point / send_now / cancel_and_send / mark_finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoordinatorAction {
    /// No pending work at this safe point.
    None,
    /// Inject interjection content into the running turn (highest priority).
    InjectInterjection { content: String },
    /// Start the next queued prompt (after current run finishes or was cancelled).
    StartPrompt { item: QueueItem },
    /// Cancel was requested; wait for terminal then start this item.
    CancelThenStart { item: QueueItem },
}

/// Snapshot of actor state for persistence / diagnostics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionActorSnapshot {
    pub conversation_id: String,
    pub active_run_id: Option<String>,
    pub running_prompt_id: Option<String>,
    pub pending_interjection: Option<String>,
    pub pending_interaction_id: Option<String>,
    pub cancel_and_send_id: Option<String>,
    pub cancel_requested: bool,
    pub drain_on_finish: bool,
    pub version: u64,
}

/// Actor state for a single conversation.
#[derive(Debug, Default)]
struct ConversationActor {
    running_prompt: Option<String>,
    running_run_id: Option<String>,
    running_prompt_id: Option<String>,
    prompt_queue: VecDeque<QueueItem>,
    pending_interjection: Option<String>,
    pending_interaction: Option<String>,
    cancel_requested: bool,
    drain_on_finish: bool,
    pending_after_cancel: Option<QueueItem>,
    version: u64,
}

/// Multi-conversation coordinator registry — single write-side actor map.
#[derive(Debug, Default)]
pub struct SessionCoordinator {
    inner: Mutex<HashMap<String, ConversationActor>>,
}

impl SessionCoordinator {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    pub fn shared() -> Arc<Self> {
        Arc::new(Self::new())
    }

    fn with_actor<R>(&self, conversation_id: &str, f: impl FnOnce(&mut ConversationActor) -> R) -> R {
        let mut map = self.inner.lock().expect("session coordinator lock");
        let actor = map.entry(conversation_id.to_string()).or_default();
        f(actor)
    }

    pub fn enqueue(
        &self,
        conversation_id: &str,
        content: impl Into<String>,
        source: PromptSource,
        client_temp_id: Option<String>,
        id: Option<String>,
    ) -> QueueItem {
        let content = content.into();
        self.with_actor(conversation_id, |actor| {
            let position = actor
                .prompt_queue
                .back()
                .map(|i| i.position + 1)
                .unwrap_or(0);
            actor.version = actor.version.saturating_add(1);
            let item = QueueItem {
                id: id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                conversation_id: conversation_id.to_string(),
                content,
                source,
                position,
                client_temp_id,
                created_at: now_rfc3339(),
                status: QueueItemStatus::Queued,
            };
            actor.prompt_queue.push_back(item.clone());
            actor.drain_on_finish = true;
            item
        })
    }

    /// Replace in-memory queue from durable rows (e.g. after restart / list).
    pub fn reload_queue(&self, conversation_id: &str, items: Vec<QueueItem>) {
        self.with_actor(conversation_id, |actor| {
            actor.prompt_queue.clear();
            let mut ordered = items;
            ordered.sort_by_key(|i| i.position);
            for mut item in ordered {
                if item.status == QueueItemStatus::Running {
                    // Mid-flight queue rows after crash become queued again.
                    item.status = QueueItemStatus::Queued;
                }
                if item.status == QueueItemStatus::Queued {
                    actor.prompt_queue.push_back(item);
                }
            }
            for (i, q) in actor.prompt_queue.iter_mut().enumerate() {
                q.position = i as i64;
            }
            actor.drain_on_finish = !actor.prompt_queue.is_empty();
            actor.version = actor.version.saturating_add(1);
        });
    }

    /// Restore durable actor fields. Clears active run (no silent re-exec).
    pub fn restore_snapshot(&self, snap: SessionActorSnapshot) {
        let cid = snap.conversation_id.clone();
        self.with_actor(&cid, |actor| {
            actor.pending_interjection = snap.pending_interjection;
            actor.pending_interaction = snap.pending_interaction_id;
            actor.drain_on_finish = snap.drain_on_finish;
            actor.version = snap.version.max(actor.version);
            if let Some(id) = snap.cancel_and_send_id {
                if let Some(item) = actor.prompt_queue.iter().find(|q| q.id == id).cloned() {
                    actor.pending_after_cancel = Some(item);
                    actor.cancel_requested = true;
                }
            }
            // Crash recovery: never pretend a dead run is still live.
            actor.running_run_id = None;
            actor.running_prompt = None;
            actor.running_prompt_id = None;
            if actor.pending_after_cancel.is_none() {
                actor.cancel_requested = false;
            }
        });
    }

    pub fn snapshot(&self, conversation_id: &str) -> SessionActorSnapshot {
        self.with_actor(conversation_id, |actor| SessionActorSnapshot {
            conversation_id: conversation_id.to_string(),
            active_run_id: actor.running_run_id.clone(),
            running_prompt_id: actor.running_prompt_id.clone(),
            pending_interjection: actor.pending_interjection.clone(),
            pending_interaction_id: actor.pending_interaction.clone(),
            cancel_and_send_id: actor.pending_after_cancel.as_ref().map(|i| i.id.clone()),
            cancel_requested: actor.cancel_requested,
            drain_on_finish: actor.drain_on_finish,
            version: actor.version,
        })
    }

    pub fn list(&self, conversation_id: &str) -> Vec<QueueItem> {
        self.with_actor(conversation_id, |actor| {
            actor.prompt_queue.iter().cloned().collect()
        })
    }

    pub fn update(&self, conversation_id: &str, id: &str, content: &str) -> Result<QueueItem, String> {
        self.with_actor(conversation_id, |actor| {
            for item in actor.prompt_queue.iter_mut() {
                if item.id == id {
                    if item.status != QueueItemStatus::Queued {
                        return Err("only queued items can be edited".into());
                    }
                    item.content = content.to_string();
                    actor.version = actor.version.saturating_add(1);
                    return Ok(item.clone());
                }
            }
            Err(format!("queue item not found: {id}"))
        })
    }

    pub fn remove(&self, conversation_id: &str, id: &str) -> Result<(), String> {
        self.with_actor(conversation_id, |actor| {
            let before = actor.prompt_queue.len();
            actor.prompt_queue.retain(|i| i.id != id);
            if actor.prompt_queue.len() == before {
                return Err(format!("queue item not found: {id}"));
            }
            if let Some(pending) = &actor.pending_after_cancel {
                if pending.id == id {
                    actor.pending_after_cancel = None;
                }
            }
            actor.version = actor.version.saturating_add(1);
            Ok(())
        })
    }

    pub fn reorder(&self, conversation_id: &str, ids: &[String]) -> Result<Vec<QueueItem>, String> {
        self.with_actor(conversation_id, |actor| {
            let mut by_id: HashMap<String, QueueItem> = actor
                .prompt_queue
                .drain(..)
                .map(|i| (i.id.clone(), i))
                .collect();
            let mut ordered = VecDeque::new();
            for (pos, id) in ids.iter().enumerate() {
                if let Some(mut item) = by_id.remove(id) {
                    item.position = pos as i64;
                    ordered.push_back(item);
                }
            }
            let mut leftovers: Vec<QueueItem> = by_id.into_values().collect();
            leftovers.sort_by_key(|i| i.position);
            let mut pos = ordered.len() as i64;
            for mut item in leftovers {
                item.position = pos;
                pos += 1;
                ordered.push_back(item);
            }
            actor.prompt_queue = ordered;
            actor.version = actor.version.saturating_add(1);
            Ok(actor.prompt_queue.iter().cloned().collect())
        })
    }

    pub fn mark_running(&self, conversation_id: &str, run_id: &str, prompt: &str) {
        self.with_actor(conversation_id, |actor| {
            actor.running_prompt = Some(prompt.to_string());
            actor.running_run_id = Some(run_id.to_string());
            actor.cancel_requested = false;
            if let Some(item) = actor
                .prompt_queue
                .iter_mut()
                .find(|q| q.content == prompt && q.status == QueueItemStatus::Queued)
            {
                item.status = QueueItemStatus::Running;
                actor.running_prompt_id = Some(item.id.clone());
            }
        });
    }

    pub fn mark_running_item(
        &self,
        conversation_id: &str,
        run_id: &str,
        queue_item_id: Option<&str>,
        prompt: &str,
    ) {
        self.with_actor(conversation_id, |actor| {
            actor.running_prompt = Some(prompt.to_string());
            actor.running_run_id = Some(run_id.to_string());
            actor.cancel_requested = false;
            actor.running_prompt_id = queue_item_id.map(str::to_string);
            if let Some(pid) = queue_item_id {
                if let Some(item) = actor.prompt_queue.iter_mut().find(|q| q.id == pid) {
                    item.status = QueueItemStatus::Running;
                }
            }
        });
    }

    pub fn mark_finished(&self, conversation_id: &str) -> CoordinatorAction {
        self.mark_finished_with_outcome(conversation_id, true)
    }

    /// Atomically claim the terminal transition for `expected_run_id`.
    ///
    /// Returns `None` (stale) when:
    /// - no active run is recorded, or
    /// - the active run id does not match `expected_run_id`.
    ///
    /// Only the caller that receives `Some(action)` may start the next prompt.
    pub fn finish_run(
        &self,
        conversation_id: &str,
        expected_run_id: &str,
        success: bool,
    ) -> Option<CoordinatorAction> {
        self.with_actor(conversation_id, |actor| {
            match actor.running_run_id.as_deref() {
                Some(active) if active == expected_run_id => {
                    Some(Self::finish_actor(actor, success))
                }
                _ => None,
            }
        })
    }

    pub fn mark_finished_with_outcome(
        &self,
        conversation_id: &str,
        success: bool,
    ) -> CoordinatorAction {
        self.with_actor(conversation_id, |actor| Self::finish_actor(actor, success))
    }

    fn finish_actor(actor: &mut ConversationActor, success: bool) -> CoordinatorAction {
        if let Some(pid) = actor.running_prompt_id.take() {
            if let Some(q) = actor.prompt_queue.iter_mut().find(|q| q.id == pid) {
                q.status = if success {
                    QueueItemStatus::Sent
                } else {
                    QueueItemStatus::Failed
                };
            }
            actor
                .prompt_queue
                .retain(|q| q.status == QueueItemStatus::Queued);
        }
        actor.running_prompt = None;
        actor.running_run_id = None;
        actor.cancel_requested = false;
        actor.pending_interaction = None;

        if let Some(item) = actor.pending_after_cancel.take() {
            actor.running_prompt = Some(item.content.clone());
            actor.version = actor.version.saturating_add(1);
            return CoordinatorAction::StartPrompt { item };
        }

        if actor.drain_on_finish {
            if let Some(item) = actor
                .prompt_queue
                .iter()
                .position(|q| q.status == QueueItemStatus::Queued)
                .map(|idx| actor.prompt_queue.remove(idx).expect("index valid"))
            {
                for (i, q) in actor.prompt_queue.iter_mut().enumerate() {
                    q.position = i as i64;
                }
                actor.running_prompt = Some(item.content.clone());
                actor.version = actor.version.saturating_add(1);
                return CoordinatorAction::StartPrompt { item };
            }
        }
        CoordinatorAction::None
    }

    pub fn send_now(
        &self,
        conversation_id: &str,
        queue_item_id: &str,
    ) -> Result<CoordinatorAction, String> {
        self.with_actor(conversation_id, |actor| {
            let idx = actor
                .prompt_queue
                .iter()
                .position(|i| i.id == queue_item_id)
                .ok_or_else(|| format!("queue item not found: {queue_item_id}"))?;
            let item = actor.prompt_queue.remove(idx).expect("index valid");
            for (i, q) in actor.prompt_queue.iter_mut().enumerate() {
                q.position = i as i64;
            }
            actor.version = actor.version.saturating_add(1);

            if actor.running_run_id.is_some() {
                actor.cancel_requested = true;
                actor.pending_after_cancel = Some(item.clone());
                actor.drain_on_finish = false;
                Ok(CoordinatorAction::CancelThenStart { item })
            } else {
                actor.running_prompt = Some(item.content.clone());
                Ok(CoordinatorAction::StartPrompt { item })
            }
        })
    }

    pub fn interject(&self, conversation_id: &str, content: impl Into<String>) {
        let content = content.into();
        self.with_actor(conversation_id, |actor| {
            actor.pending_interjection = Some(content);
            actor.version = actor.version.saturating_add(1);
        });
    }

    pub fn set_pending_interaction(&self, conversation_id: &str, interaction_id: Option<String>) {
        self.with_actor(conversation_id, |actor| {
            actor.pending_interaction = interaction_id;
            actor.version = actor.version.saturating_add(1);
        });
    }

    pub fn pending_interaction(&self, conversation_id: &str) -> Option<String> {
        self.with_actor(conversation_id, |a| a.pending_interaction.clone())
    }

    pub fn cancel_and_send(
        &self,
        conversation_id: &str,
        queue_item_id: &str,
    ) -> Result<CoordinatorAction, String> {
        self.send_now(conversation_id, queue_item_id)
    }

    pub fn on_safe_point(&self, conversation_id: &str, point: SafePoint) -> CoordinatorAction {
        let _ = point;
        self.with_actor(conversation_id, |actor| {
            if let Some(content) = actor.pending_interjection.take() {
                actor.version = actor.version.saturating_add(1);
                return CoordinatorAction::InjectInterjection { content };
            }

            if actor.running_run_id.is_none() && actor.drain_on_finish {
                if let Some(item) = actor
                    .prompt_queue
                    .iter()
                    .position(|q| q.status == QueueItemStatus::Queued)
                    .map(|idx| actor.prompt_queue.remove(idx).expect("index valid"))
                {
                    for (i, q) in actor.prompt_queue.iter_mut().enumerate() {
                        q.position = i as i64;
                    }
                    actor.running_prompt = Some(item.content.clone());
                    actor.version = actor.version.saturating_add(1);
                    return CoordinatorAction::StartPrompt { item };
                }
            }
            CoordinatorAction::None
        })
    }

    pub fn is_running(&self, conversation_id: &str) -> bool {
        self.with_actor(conversation_id, |a| a.running_run_id.is_some())
    }

    pub fn cancel_requested(&self, conversation_id: &str) -> bool {
        self.with_actor(conversation_id, |a| a.cancel_requested)
    }

    pub fn pending_interjection(&self, conversation_id: &str) -> Option<String> {
        self.with_actor(conversation_id, |a| a.pending_interjection.clone())
    }

    pub fn queue_len(&self, conversation_id: &str) -> usize {
        self.with_actor(conversation_id, |a| a.prompt_queue.len())
    }

    pub fn version(&self, conversation_id: &str) -> u64 {
        self.with_actor(conversation_id, |a| a.version)
    }

    pub fn clear_conversation(&self, conversation_id: &str) {
        let mut map = self.inner.lock().expect("session coordinator lock");
        map.remove(conversation_id);
    }
}

fn now_rfc3339() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

/// Hardcoded parallel-safe readonly tool names.
pub fn is_parallel_safe_tool(name: &str) -> bool {
    matches!(
        name,
        "read_file"
            | "list_dir"
            | "grep"
            | "search_files"
            | "task_output"
            | "memory_search"
            | "memory_get"
    )
}

/// Max concurrent parallel_safe tools in one provider tool batch.
pub const PARALLEL_SAFE_MAX_CONCURRENCY: usize = 4;

// ---------------------------------------------------------------------------
// Backward-compatible aliases (Phase 2 SessionHarness names)
// ---------------------------------------------------------------------------

/// Historical name — prefer [`SessionCoordinator`].
pub type SessionHarness = SessionCoordinator;
/// Historical action name — prefer [`CoordinatorAction`].
pub type HarnessAction = CoordinatorAction;

#[cfg(test)]
mod tests {
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

        let action = h
            .cancel_and_send("c1", &item.id)
            .expect("cancel_and_send");
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
            assert!(matches!(action, CoordinatorAction::InjectInterjection { .. }));
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
}
