//! SessionHarness — per-conversation execution actor (Phase 2).
//!
//! Manages prompt queue, interjection, cancel-and-send, and safe-point drain.
//! Persist-first against the daemon `prompt_queue` table when a DataStore is available;
//! always keeps an in-memory mirror for tests and live coordination.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use uuid::Uuid;

/// Moments where interjection / queue drain is safe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafePoint {
    BetweenProviderBatches,
    BeforeToolCall,
    AfterToolCall,
    AfterInteraction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueItemStatus {
    Queued,
    Running,
    Sent,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueItem {
    pub id: String,
    pub conversation_id: String,
    pub content: String,
    pub source: String,
    pub position: i64,
    pub status: QueueItemStatus,
    pub client_temp_id: Option<String>,
    pub version: u64,
    pub attachments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Interjection {
    pub id: String,
    pub content: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Default)]
struct ConversationActor {
    running_prompt_id: Option<String>,
    active_run_id: Option<String>,
    queue: Vec<QueueItem>,
    pending_interjection: Option<Interjection>,
    pending_interaction_id: Option<String>,
    /// When set, after terminal run finish, start this queue item as a new run.
    cancel_and_send: Option<String>,
    version: u64,
}

/// In-process harness (one per daemon).
pub struct SessionHarness {
    actors: Mutex<HashMap<String, ConversationActor>>,
}

impl Default for SessionHarness {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionHarness {
    pub fn new() -> Self {
        Self {
            actors: Mutex::new(HashMap::new()),
        }
    }

    fn with_actor_mut<R>(
        &self,
        conversation_id: &str,
        f: impl FnOnce(&mut ConversationActor) -> R,
    ) -> R {
        let mut map = self.actors.lock().expect("session harness lock");
        let actor = map.entry(conversation_id.to_string()).or_default();
        f(actor)
    }

    pub fn list(&self, conversation_id: &str) -> Vec<QueueItem> {
        self.with_actor_mut(conversation_id, |a| a.queue.clone())
    }

    pub fn enqueue(
        &self,
        conversation_id: &str,
        content: impl Into<String>,
        source: Option<&str>,
        client_temp_id: Option<String>,
        attachments: Option<Value>,
    ) -> QueueItem {
        self.with_actor_mut(conversation_id, |a| {
            let position = a.queue.iter().map(|q| q.position).max().unwrap_or(-1) + 1;
            a.version += 1;
            let item = QueueItem {
                id: Uuid::new_v4().to_string(),
                conversation_id: conversation_id.to_string(),
                content: content.into(),
                source: source.unwrap_or("user").to_string(),
                position,
                status: QueueItemStatus::Queued,
                client_temp_id,
                version: a.version,
                attachments: attachments.unwrap_or(Value::Null),
            };
            a.queue.push(item.clone());
            item
        })
    }

    pub fn update(&self, conversation_id: &str, id: &str, content: &str) -> Result<QueueItem, String> {
        self.with_actor_mut(conversation_id, |a| {
            let item = a
                .queue
                .iter_mut()
                .find(|q| q.id == id)
                .ok_or_else(|| format!("queue item not found: {id}"))?;
            if item.status != QueueItemStatus::Queued {
                return Err("only queued items can be edited".into());
            }
            item.content = content.to_string();
            a.version += 1;
            item.version = a.version;
            Ok(item.clone())
        })
    }

    pub fn remove(&self, conversation_id: &str, id: &str) -> Result<(), String> {
        self.with_actor_mut(conversation_id, |a| {
            let before = a.queue.len();
            a.queue.retain(|q| q.id != id);
            if a.queue.len() == before {
                return Err(format!("queue item not found: {id}"));
            }
            a.version += 1;
            Ok(())
        })
    }

    pub fn reorder(&self, conversation_id: &str, ordered_ids: &[String]) -> Result<(), String> {
        self.with_actor_mut(conversation_id, |a| {
            let mut next = Vec::with_capacity(ordered_ids.len());
            for (pos, id) in ordered_ids.iter().enumerate() {
                let Some(item) = a.queue.iter().find(|q| &q.id == id).cloned() else {
                    return Err(format!("queue item not found: {id}"));
                };
                let mut item = item;
                item.position = pos as i64;
                next.push(item);
            }
            // Keep any items not listed at the end
            for q in &a.queue {
                if !ordered_ids.contains(&q.id) {
                    let mut q = q.clone();
                    q.position = next.len() as i64;
                    next.push(q);
                }
            }
            a.queue = next;
            a.version += 1;
            Ok(())
        })
    }

    /// Mark item as next to run immediately (front of queue).
    pub fn send_now(&self, conversation_id: &str, id: &str) -> Result<QueueItem, String> {
        self.with_actor_mut(conversation_id, |a| {
            let idx = a
                .queue
                .iter()
                .position(|q| q.id == id)
                .ok_or_else(|| format!("queue item not found: {id}"))?;
            let mut item = a.queue.remove(idx);
            item.position = -1;
            a.version += 1;
            item.version = a.version;
            a.queue.insert(0, item.clone());
            // re-number
            for (i, q) in a.queue.iter_mut().enumerate() {
                q.position = i as i64;
            }
            Ok(item)
        })
    }

    /// Request interjection at next safe point of the active run.
    pub fn interject(&self, conversation_id: &str, content: impl Into<String>) -> Interjection {
        self.with_actor_mut(conversation_id, |a| {
            let inj = Interjection {
                id: Uuid::new_v4().to_string(),
                content: content.into(),
                created_at: chrono::Utc::now().to_rfc3339(),
            };
            a.pending_interjection = Some(inj.clone());
            a.version += 1;
            inj
        })
    }

    pub fn set_running(&self, conversation_id: &str, run_id: &str, prompt_id: Option<&str>) {
        self.with_actor_mut(conversation_id, |a| {
            a.active_run_id = Some(run_id.to_string());
            a.running_prompt_id = prompt_id.map(str::to_string);
            if let Some(pid) = prompt_id {
                if let Some(q) = a.queue.iter_mut().find(|q| q.id == pid) {
                    q.status = QueueItemStatus::Running;
                }
            }
        });
    }

    /// Cancel active run and, after terminal, start selected queue item.
    pub fn cancel_and_send(
        &self,
        conversation_id: &str,
        queue_item_id: &str,
    ) -> Result<(), String> {
        self.with_actor_mut(conversation_id, |a| {
            if !a.queue.iter().any(|q| q.id == queue_item_id) {
                return Err(format!("queue item not found: {queue_item_id}"));
            }
            a.cancel_and_send = Some(queue_item_id.to_string());
            a.version += 1;
            Ok(())
        })
    }

    pub fn take_cancel_and_send(&self, conversation_id: &str) -> Option<String> {
        self.with_actor_mut(conversation_id, |a| a.cancel_and_send.take())
    }

    /// Called by the engine at safe points. Returns interjection content if any.
    pub fn on_safe_point(
        &self,
        conversation_id: &str,
        point: SafePoint,
    ) -> Option<Interjection> {
        let _ = point; // all listed safe points are valid inject sites
        self.with_actor_mut(conversation_id, |a| a.pending_interjection.take())
    }

    /// After a run reaches a real terminal state, mark running item done and
    /// optionally return the next queue item to start (auto-drain).
    pub fn on_run_terminal(
        &self,
        conversation_id: &str,
        _run_id: &str,
        success: bool,
    ) -> Option<QueueItem> {
        self.with_actor_mut(conversation_id, |a| {
            if let Some(pid) = a.running_prompt_id.take() {
                if let Some(q) = a.queue.iter_mut().find(|q| q.id == pid) {
                    q.status = if success {
                        QueueItemStatus::Sent
                    } else {
                        QueueItemStatus::Failed
                    };
                }
            }
            a.active_run_id = None;

            // cancel_and_send has priority over normal drain
            if let Some(id) = a.cancel_and_send.take() {
                if let Some(q) = a.queue.iter().find(|q| q.id == id && q.status == QueueItemStatus::Queued)
                {
                    return Some(q.clone());
                }
            }

            // Auto-drain next queued
            a.queue
                .iter()
                .find(|q| q.status == QueueItemStatus::Queued)
                .cloned()
        })
    }

    pub fn snapshot_version(&self, conversation_id: &str) -> u64 {
        self.with_actor_mut(conversation_id, |a| a.version)
    }

    pub fn pending_interjection(&self, conversation_id: &str) -> Option<Interjection> {
        self.with_actor_mut(conversation_id, |a| a.pending_interjection.clone())
    }

    pub fn is_running(&self, conversation_id: &str) -> bool {
        self.with_actor_mut(conversation_id, |a| a.active_run_id.is_some())
    }
}

/// Process-wide harness for the daemon.
pub fn global_session_harness() -> &'static SessionHarness {
    use std::sync::OnceLock;
    static H: OnceLock<SessionHarness> = OnceLock::new();
    H.get_or_init(SessionHarness::new)
}

/// Max parallel readonly tools (scheme Phase 2).
pub const MAX_PARALLEL_READONLY_TOOLS: usize = 4;

/// Tools that may run in parallel when no approval is required.
pub fn is_parallel_safe_tool(name: &str) -> bool {
    matches!(
        name,
        "read_file" | "list_dir" | "grep" | "search_files" | "memory_search" | "memory_get"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enqueue_and_list_order() {
        let h = SessionHarness::new();
        let a = h.enqueue("c1", "first", None, None, None);
        let b = h.enqueue("c1", "second", None, None, None);
        let list = h.list("c1");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, a.id);
        assert_eq!(list[1].id, b.id);
        assert!(list[0].position < list[1].position);
    }

    #[test]
    fn send_now_moves_to_front() {
        let h = SessionHarness::new();
        let _a = h.enqueue("c1", "first", None, None, None);
        let b = h.enqueue("c1", "second", None, None, None);
        h.send_now("c1", &b.id).unwrap();
        let list = h.list("c1");
        assert_eq!(list[0].id, b.id);
    }

    #[test]
    fn interject_taken_at_safe_point() {
        let h = SessionHarness::new();
        h.set_running("c1", "run-1", None);
        let inj = h.interject("c1", "please stop and do X");
        assert!(h.pending_interjection("c1").is_some());
        let got = h.on_safe_point("c1", SafePoint::AfterToolCall).unwrap();
        assert_eq!(got.id, inj.id);
        assert!(h.pending_interjection("c1").is_none());
        // second safe point has nothing
        assert!(h.on_safe_point("c1", SafePoint::BeforeToolCall).is_none());
    }

    #[test]
    fn cancel_and_send_after_terminal() {
        let h = SessionHarness::new();
        let item = h.enqueue("c1", "do next", None, None, None);
        h.set_running("c1", "run-1", None);
        h.cancel_and_send("c1", &item.id).unwrap();
        let next = h.on_run_terminal("c1", "run-1", false).unwrap();
        assert_eq!(next.id, item.id);
    }

    #[test]
    fn auto_drain_next_queued() {
        let h = SessionHarness::new();
        let first = h.enqueue("c1", "a", None, None, None);
        let second = h.enqueue("c1", "b", None, None, None);
        h.set_running("c1", "run-1", Some(&first.id));
        let next = h.on_run_terminal("c1", "run-1", true).unwrap();
        assert_eq!(next.id, second.id);
        assert_eq!(
            h.list("c1")
                .iter()
                .find(|q| q.id == first.id)
                .unwrap()
                .status,
            QueueItemStatus::Sent
        );
    }

    #[test]
    fn parallel_safe_classification() {
        assert!(is_parallel_safe_tool("read_file"));
        assert!(is_parallel_safe_tool("grep"));
        assert!(!is_parallel_safe_tool("write_file"));
        assert!(!is_parallel_safe_tool("run_terminal"));
        assert!(!is_parallel_safe_tool("apply_patch"));
        assert!(!is_parallel_safe_tool("web_fetch"));
        assert_eq!(MAX_PARALLEL_READONLY_TOOLS, 4);
    }
}
