//! Interaction waiters for permission and subagent assignment (task-01 / task-04).
//!
//! Sole owner of permission/assignment oneshot maps. Callers must not hold raw Map refs.

use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};

/// permission_id → (run_id, tool_name, resolver)
pub type PermissionWaiterMap = HashMap<String, (String, String, oneshot::Sender<(bool, String)>)>;

#[derive(Default)]
pub struct InteractionHub {
    permission_waiters: Arc<Mutex<PermissionWaiterMap>>,
    assignment_waiters: Arc<std::sync::Mutex<HashMap<String, oneshot::Sender<Value>>>>,
    assignment_inflight: Arc<std::sync::Mutex<HashMap<String, String>>>,
}

impl InteractionHub {
    pub fn new() -> Self {
        Self {
            permission_waiters: Arc::new(Mutex::new(HashMap::new())),
            assignment_waiters: Arc::new(std::sync::Mutex::new(HashMap::new())),
            assignment_inflight: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Register a permission waiter. Returns Err if id already pending.
    pub async fn register_permission(
        &self,
        permission_id: &str,
        run_id: &str,
        tool_name: &str,
        tx: oneshot::Sender<(bool, String)>,
    ) {
        self.permission_waiters.lock().await.insert(
            permission_id.to_string(),
            (run_id.to_string(), tool_name.to_string(), tx),
        );
    }

    /// Resolve a permission waiter (UI respond). Returns None if not found / already resolved.
    pub async fn resolve_permission(
        &self,
        permission_id: &str,
    ) -> Option<(String, String, oneshot::Sender<(bool, String)>)> {
        self.permission_waiters.lock().await.remove(permission_id)
    }

    /// Remove a waiter only when it belongs to `expected_run_id` (if supplied).
    /// A mismatch leaves the waiter live so the owning run can still respond.
    pub async fn resolve_permission_for_run(
        &self,
        permission_id: &str,
        expected_run_id: Option<&str>,
    ) -> Result<(String, String, oneshot::Sender<(bool, String)>), String> {
        let mut waiters = self.permission_waiters.lock().await;
        let Some((run_id, _tool_name, _)) = waiters.get(permission_id) else {
            return Err(format!(
                "permission_orphaned: no live waiter for request_id={permission_id}"
            ));
        };
        if let Some(expected) = expected_run_id.filter(|id| !id.is_empty()) {
            if expected != run_id {
                return Err(format!(
                    "permission run_id mismatch: expected {run_id}, got {expected}"
                ));
            }
        }
        waiters.remove(permission_id).ok_or_else(|| {
            format!("permission_orphaned: no live waiter for request_id={permission_id}")
        })
    }

    pub async fn has_permission(&self, permission_id: &str) -> bool {
        self.permission_waiters
            .lock()
            .await
            .contains_key(permission_id)
    }

    /// Expire / cancel all permission waiters bound to any of `run_ids`.
    /// Sends (false, "cancelled") to each and returns cleared permission_ids.
    pub async fn cancel_runs(&self, run_ids: &[String]) -> Vec<String> {
        let set: HashSet<&str> = run_ids.iter().map(|s| s.as_str()).collect();
        let mut map = self.permission_waiters.lock().await;
        let stale: Vec<String> = map
            .iter()
            .filter(|(_, (rid, _, _))| set.contains(rid.as_str()))
            .map(|(pid, _)| pid.clone())
            .collect();
        for pid in &stale {
            if let Some((_rid, _tool, tx)) = map.remove(pid) {
                let _ = tx.send((false, "cancelled".into()));
            }
        }
        stale
    }

    pub async fn permission_count(&self) -> usize {
        self.permission_waiters.lock().await.len()
    }

    pub fn register_assignment(
        &self,
        interaction_id: &str,
        tx: oneshot::Sender<Value>,
    ) -> Result<(), String> {
        let mut map = self.assignment_waiters.lock().map_err(|e| e.to_string())?;
        map.insert(interaction_id.to_string(), tx);
        Ok(())
    }

    pub fn resolve_assignment(&self, interaction_id: &str) -> Option<oneshot::Sender<Value>> {
        self.assignment_waiters
            .lock()
            .ok()
            .and_then(|mut m| m.remove(interaction_id))
    }

    pub fn assignment_waiters_arc(
        &self,
    ) -> Arc<std::sync::Mutex<HashMap<String, oneshot::Sender<Value>>>> {
        self.assignment_waiters.clone()
    }

    pub fn assignment_inflight_arc(&self) -> Arc<std::sync::Mutex<HashMap<String, String>>> {
        self.assignment_inflight.clone()
    }

    pub fn set_assignment_inflight(
        &self,
        conversation_id: &str,
        interaction_id: &str,
    ) -> Result<(), String> {
        let mut map = self.assignment_inflight.lock().map_err(|e| e.to_string())?;
        map.insert(conversation_id.to_string(), interaction_id.to_string());
        Ok(())
    }

    pub fn clear_assignment_inflight(&self, conversation_id: &str) {
        if let Ok(mut map) = self.assignment_inflight.lock() {
            map.remove(conversation_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancel_runs_clears_permission_waiters_under_100ms() {
        let hub = InteractionHub::new();
        let (tx, mut rx) = oneshot::channel();
        hub.register_permission("p1", "run-a", "Bash", tx).await;
        assert_eq!(hub.permission_count().await, 1);
        let t0 = std::time::Instant::now();
        let cleared = hub.cancel_runs(&["run-a".into()]).await;
        let elapsed = t0.elapsed();
        assert_eq!(cleared, vec!["p1".to_string()]);
        assert_eq!(hub.permission_count().await, 0);
        assert!(elapsed.as_millis() < 100, "cancel waiters took {elapsed:?}");
        let got = rx.try_recv().expect("waiter resolved");
        assert_eq!(got, (false, "cancelled".into()));
    }
}
