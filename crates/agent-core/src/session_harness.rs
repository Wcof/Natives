//! SessionHarness — per-conversation prompt queue / interjection actor (Phase 2 skeleton).
//!
//! One logical actor state per `conversation_id`. Not a full async actor runtime:
//! methods are synchronous and thread-safe via interior mutex so daemon RPC and
//! engine safe-point hooks can share the same map.
//!
//! ## Safe points (only these may inject interjection or drain queue)
//! - Provider batch boundary (between provider turns)
//! - Before / after tool execution
//! - After permission resolution
//!
//! Engine integration: call [`SessionHarness::on_safe_point`] at those seams
//! (or simulate in unit tests). Full rewake of the run loop is future work.

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

/// One item in the in-memory prompt queue (mirrors DB row shape loosely).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueItem {
    pub id: String,
    pub conversation_id: String,
    pub content: String,
    pub source: PromptSource,
    pub position: i64,
    pub client_temp_id: Option<String>,
    pub created_at: String,
}

/// Action returned by safe-point / send_now / cancel_and_send handlers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HarnessAction {
    /// No pending work at this safe point.
    None,
    /// Inject interjection content into the running turn (highest priority).
    InjectInterjection { content: String },
    /// Start the next queued prompt (after current run finishes or was cancelled).
    StartPrompt { item: QueueItem },
    /// Cancel was requested; wait for terminal then start this item.
    CancelThenStart { item: QueueItem },
}

/// Actor state for a single conversation.
#[derive(Debug, Default)]
struct ConversationActor {
    /// Content of the currently running user prompt (if any).
    running_prompt: Option<String>,
    /// Active run id when a prompt is in flight.
    running_run_id: Option<String>,
    /// FIFO prompt queue (position-ordered).
    prompt_queue: VecDeque<QueueItem>,
    /// At most one pending interjection (latest wins).
    pending_interjection: Option<String>,
    /// Pending human interaction id (permission / ask), if any.
    pending_interaction: Option<String>,
    /// Soft cancel flag for the active run (engine also owns CancellationToken).
    cancel_requested: bool,
    /// When the current run finishes, drain the next queue item automatically.
    drain_on_finish: bool,
    /// Item waiting to start after cancel reaches terminal.
    pending_after_cancel: Option<QueueItem>,
}

/// Multi-conversation harness registry.
#[derive(Debug, Default)]
pub struct SessionHarness {
    inner: Mutex<HashMap<String, ConversationActor>>,
}

impl SessionHarness {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    pub fn shared() -> Arc<Self> {
        Arc::new(Self::new())
    }

    fn with_actor<R>(&self, conversation_id: &str, f: impl FnOnce(&mut ConversationActor) -> R) -> R {
        let mut map = self.inner.lock().expect("session harness lock");
        let actor = map.entry(conversation_id.to_string()).or_default();
        f(actor)
    }

    /// Enqueue a prompt for later delivery (does not start a run).
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
            let item = QueueItem {
                id: id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                conversation_id: conversation_id.to_string(),
                content,
                source,
                position,
                client_temp_id,
                created_at: now_rfc3339(),
            };
            actor.prompt_queue.push_back(item.clone());
            actor.drain_on_finish = true;
            item
        })
    }

    /// List queue items in position order (clone).
    pub fn list(&self, conversation_id: &str) -> Vec<QueueItem> {
        self.with_actor(conversation_id, |actor| {
            actor.prompt_queue.iter().cloned().collect()
        })
    }

    /// Update content of a queued item by id.
    pub fn update(&self, conversation_id: &str, id: &str, content: &str) -> Result<QueueItem, String> {
        self.with_actor(conversation_id, |actor| {
            for item in actor.prompt_queue.iter_mut() {
                if item.id == id {
                    item.content = content.to_string();
                    return Ok(item.clone());
                }
            }
            Err(format!("queue item not found: {id}"))
        })
    }

    /// Remove a queued item by id.
    pub fn remove(&self, conversation_id: &str, id: &str) -> Result<(), String> {
        self.with_actor(conversation_id, |actor| {
            let before = actor.prompt_queue.len();
            actor.prompt_queue.retain(|i| i.id != id);
            if actor.prompt_queue.len() == before {
                return Err(format!("queue item not found: {id}"));
            }
            Ok(())
        })
    }

    /// Reorder queue to match `ids` (items not listed keep relative tail order).
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
            // Append any leftover items not in ids.
            let mut leftovers: Vec<QueueItem> = by_id.into_values().collect();
            leftovers.sort_by_key(|i| i.position);
            let mut pos = ordered.len() as i64;
            for mut item in leftovers {
                item.position = pos;
                pos += 1;
                ordered.push_back(item);
            }
            actor.prompt_queue = ordered;
            Ok(actor.prompt_queue.iter().cloned().collect())
        })
    }

    /// Mark a run as active for this conversation.
    pub fn mark_running(&self, conversation_id: &str, run_id: &str, prompt: &str) {
        self.with_actor(conversation_id, |actor| {
            actor.running_prompt = Some(prompt.to_string());
            actor.running_run_id = Some(run_id.to_string());
            actor.cancel_requested = false;
        });
    }

    /// Mark run finished (terminal). Optionally returns next drain action.
    pub fn mark_finished(&self, conversation_id: &str) -> HarnessAction {
        self.with_actor(conversation_id, |actor| {
            actor.running_prompt = None;
            actor.running_run_id = None;
            actor.cancel_requested = false;
            actor.pending_interaction = None;

            if let Some(item) = actor.pending_after_cancel.take() {
                actor.running_prompt = Some(item.content.clone());
                return HarnessAction::StartPrompt { item };
            }

            if actor.drain_on_finish {
                if let Some(item) = actor.prompt_queue.pop_front() {
                    // reindex positions
                    for (i, q) in actor.prompt_queue.iter_mut().enumerate() {
                        q.position = i as i64;
                    }
                    actor.running_prompt = Some(item.content.clone());
                    return HarnessAction::StartPrompt { item };
                }
            }
            HarnessAction::None
        })
    }

    /// Send a queue item immediately: cancel active run if needed, then start item.
    pub fn send_now(&self, conversation_id: &str, queue_item_id: &str) -> Result<HarnessAction, String> {
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

            if actor.running_run_id.is_some() {
                actor.cancel_requested = true;
                actor.pending_after_cancel = Some(item.clone());
                actor.drain_on_finish = false;
                Ok(HarnessAction::CancelThenStart { item })
            } else {
                actor.running_prompt = Some(item.content.clone());
                Ok(HarnessAction::StartPrompt { item })
            }
        })
    }

    /// Mark pending interjection (injected at next safe point). Latest wins.
    pub fn interject(&self, conversation_id: &str, content: impl Into<String>) {
        let content = content.into();
        self.with_actor(conversation_id, |actor| {
            actor.pending_interjection = Some(content);
        });
    }

    /// Record a pending interaction (e.g. permission request id).
    pub fn set_pending_interaction(&self, conversation_id: &str, interaction_id: Option<String>) {
        self.with_actor(conversation_id, |actor| {
            actor.pending_interaction = interaction_id;
        });
    }

    /// Cancel current run and schedule a specific queue item after terminal.
    pub fn cancel_and_send(
        &self,
        conversation_id: &str,
        queue_item_id: &str,
    ) -> Result<HarnessAction, String> {
        self.send_now(conversation_id, queue_item_id)
    }

    /// Safe-point handler: inject interjection first, else drain queue if idle.
    ///
    /// When a run is active, only interjection is eligible; queue drain waits
    /// for finish (or cancel_and_send path).
    pub fn on_safe_point(&self, conversation_id: &str, point: SafePoint) -> HarnessAction {
        let _ = point; // documented for callers / future filtering
        self.with_actor(conversation_id, |actor| {
            if let Some(content) = actor.pending_interjection.take() {
                return HarnessAction::InjectInterjection { content };
            }

            // Queue drain only when nothing is running and drain_on_finish is set.
            if actor.running_run_id.is_none() && actor.drain_on_finish {
                if let Some(item) = actor.prompt_queue.pop_front() {
                    for (i, q) in actor.prompt_queue.iter_mut().enumerate() {
                        q.position = i as i64;
                    }
                    actor.running_prompt = Some(item.content.clone());
                    return HarnessAction::StartPrompt { item };
                }
            }
            HarnessAction::None
        })
    }

    /// Snapshot helpers for tests / diagnostics.
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
}

fn now_rfc3339() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Lightweight timestamp; daemon persistence uses chrono RFC3339.
    format!("{secs}")
}

/// Hardcoded parallel-safe readonly tool names (Phase 2 simplification).
/// Write / process / network tools must stay serial.
pub fn is_parallel_safe_tool(name: &str) -> bool {
    matches!(
        name,
        "read_file" | "list_dir" | "grep" | "search_files" | "task_output"
    )
}

/// Max concurrent parallel_safe tools in one provider tool batch.
pub const PARALLEL_SAFE_MAX_CONCURRENCY: usize = 4;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enqueue_preserves_fifo_order() {
        let h = SessionHarness::new();
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
        let h = SessionHarness::new();
        h.mark_running("c1", "run-1", "hello");
        h.interject("c1", "stop and do X instead");
        assert!(h.pending_interjection("c1").is_some());

        // Queue drain must NOT happen while running — only interjection.
        h.enqueue("c1", "queued", PromptSource::User, None, None);
        let action = h.on_safe_point("c1", SafePoint::AfterTool);
        match action {
            HarnessAction::InjectInterjection { content } => {
                assert_eq!(content, "stop and do X instead");
            }
            other => panic!("expected InjectInterjection, got {other:?}"),
        }
        assert!(h.pending_interjection("c1").is_none());
        // Interjection consumed; second safe point does not re-inject.
        assert_eq!(
            h.on_safe_point("c1", SafePoint::ProviderBatchBoundary),
            HarnessAction::None
        );
        // Queue still waiting.
        assert_eq!(h.queue_len("c1"), 1);
    }

    #[test]
    fn cancel_and_send_waits_for_terminal_then_starts() {
        let h = SessionHarness::new();
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
            HarnessAction::CancelThenStart { item: i } => {
                assert_eq!(i.content, "urgent");
                assert_eq!(i.id, "q-urgent");
            }
            other => panic!("expected CancelThenStart, got {other:?}"),
        }
        assert!(h.cancel_requested("c1"));
        assert!(h.is_running("c1"));

        // Simulate engine terminal → mark_finished drains pending_after_cancel.
        let next = h.mark_finished("c1");
        match next {
            HarnessAction::StartPrompt { item: i } => {
                assert_eq!(i.id, "q-urgent");
                assert_eq!(i.content, "urgent");
            }
            other => panic!("expected StartPrompt after terminal, got {other:?}"),
        }
        assert!(!h.cancel_requested("c1"));
    }

    #[test]
    fn send_now_when_idle_starts_immediately() {
        let h = SessionHarness::new();
        let item = h.enqueue("c1", "go", PromptSource::User, None, Some("q1".into()));
        let action = h.send_now("c1", &item.id).unwrap();
        assert!(matches!(action, HarnessAction::StartPrompt { .. }));
        assert_eq!(h.queue_len("c1"), 0);
    }

    #[test]
    fn drain_on_finish_starts_next_queue_item() {
        let h = SessionHarness::new();
        h.mark_running("c1", "r1", "p1");
        h.enqueue("c1", "next-1", PromptSource::User, None, Some("n1".into()));
        h.enqueue("c1", "next-2", PromptSource::User, None, Some("n2".into()));
        let action = h.mark_finished("c1");
        match action {
            HarnessAction::StartPrompt { item } => {
                assert_eq!(item.id, "n1");
            }
            other => panic!("expected drain StartPrompt, got {other:?}"),
        }
        assert_eq!(h.queue_len("c1"), 1);
    }

    #[test]
    fn reorder_and_remove() {
        let h = SessionHarness::new();
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
        assert!(!is_parallel_safe_tool("write_file"));
        assert!(!is_parallel_safe_tool("run_terminal"));
        assert!(!is_parallel_safe_tool("mcp_call"));
        assert_eq!(PARALLEL_SAFE_MAX_CONCURRENCY, 4);
    }

    #[test]
    fn all_safe_point_variants_accept_interjection() {
        let h = SessionHarness::new();
        for point in [
            SafePoint::ProviderBatchBoundary,
            SafePoint::BeforeTool,
            SafePoint::AfterTool,
            SafePoint::AfterPermissionResolved,
        ] {
            h.interject("c1", format!("inj-{point:?}"));
            let action = h.on_safe_point("c1", point);
            assert!(matches!(action, HarnessAction::InjectInterjection { .. }));
        }
    }
}
