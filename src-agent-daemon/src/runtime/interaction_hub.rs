//! Interaction waiters for permission and subagent assignment (task-01 / task-04).

use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};

/// permission_id → (run_id, tool_name, resolver)
pub type PermissionWaiterMap =
    HashMap<String, (String, String, oneshot::Sender<(bool, String)>)>;

#[derive(Default)]
pub struct InteractionHub {
    pub permission_waiters: Arc<Mutex<PermissionWaiterMap>>,
    pub assignment_waiters: Arc<std::sync::Mutex<HashMap<String, oneshot::Sender<Value>>>>,
    pub assignment_inflight: Arc<std::sync::Mutex<HashMap<String, String>>>,
}

impl InteractionHub {
    pub fn new() -> Self {
        Self {
            permission_waiters: Arc::new(Mutex::new(HashMap::new())),
            assignment_waiters: Arc::new(std::sync::Mutex::new(HashMap::new())),
            assignment_inflight: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }
}
