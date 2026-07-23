//! Per-run CancellationToken tree + join handles + managed resource ids.
//!
//! Invariant (task-03 / 01-目标架构):
//! - Each Run has exactly one root CancellationToken.
//! - Child runs use `parent.child_token()`.
//! - Cancel order: signal tree → graceful wait → force cleanup → (status commit by RunManager).
//! - `Cancelled` means this registry has no managed resources for the tree.
//! - Daemon shutdown calls [`ExecutionRegistry::cancel_all_execution_roots`].
//!
//! Agent C wiring points:
//! - SidecarSupervisor / Daemon grace shutdown → `cancel_all_execution_roots`
//! - task-01 splits this out of ProductionRuntime

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Mutex, Notify};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Default graceful cancel budget before force kill/abort (task-03).
pub const DEFAULT_CANCEL_GRACE_MS: u64 = 5_000;

/// Resource kinds tracked under a run for force cleanup.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ManagedResource {
    /// `LocalProcessSupervisor` / terminal task id.
    ProcessTask(String),
    /// MCP server session id (stdio child).
    McpServer(String),
    /// Opaque id for future adapters (CLI pid bookkeeping, etc.).
    External(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelPhase {
    Idle,
    Signalled,
    Force,
    Clean,
    CleanupFailed,
}

/// Snapshot returned after tree cancel cleanup.
#[derive(Debug, Clone)]
pub struct CancelCleanupOutcome {
    pub root_run_id: String,
    pub run_ids: Vec<String>,
    pub phase: CancelPhase,
    /// True when all JoinHandles finished and process resources were cancelled.
    pub quiet: bool,
    pub errors: Vec<String>,
}

struct RunExecution {
    parent_run_id: Option<String>,
    token: CancellationToken,
    join: Option<JoinHandle<()>>,
    resources: HashSet<ManagedResource>,
    terminal: Arc<Notify>,
    cancel_phase: CancelPhase,
}

impl RunExecution {
    fn new(parent_run_id: Option<String>, token: CancellationToken) -> Self {
        Self {
            parent_run_id,
            token,
            join: None,
            resources: HashSet::new(),
            terminal: Arc::new(Notify::new()),
            cancel_phase: CancelPhase::Idle,
        }
    }
}

/// Registration handle returned to start paths.
#[derive(Debug, Clone)]
pub struct ExecutionRegistration {
    pub run_id: String,
    pub parent_run_id: Option<String>,
    pub token: CancellationToken,
    pub terminal: Arc<Notify>,
}

/// In-memory execution registry — sole owner of cancel tokens and managed resources.
pub struct ExecutionRegistry {
    runs: Mutex<HashMap<String, RunExecution>>,
    grace: Duration,
    /// Optional process cancel hook (task_id) — injected so tests avoid OS spawn.
    /// std mutex: install is sync-safe from non-async constructors.
    process_cancel: std::sync::Mutex<Option<Arc<dyn ProcessCancelHook>>>,
}

/// Seam for force-killing process tasks without coupling to capability-gateway types.
#[async_trait::async_trait]
pub trait ProcessCancelHook: Send + Sync {
    async fn cancel_task(&self, task_id: &str) -> Result<(), String>;
    async fn cancel_tasks_for_run(&self, run_id: &str) -> Result<(), String>;
}

impl ExecutionRegistry {
    pub fn new() -> Self {
        Self {
            runs: Mutex::new(HashMap::new()),
            grace: Duration::from_millis(DEFAULT_CANCEL_GRACE_MS),
            process_cancel: std::sync::Mutex::new(None),
        }
    }

    pub fn with_grace_ms(ms: u64) -> Self {
        Self {
            runs: Mutex::new(HashMap::new()),
            grace: Duration::from_millis(ms.max(1)),
            process_cancel: std::sync::Mutex::new(None),
        }
    }

    pub fn set_process_cancel_hook(&self, hook: Arc<dyn ProcessCancelHook>) {
        if let Ok(mut slot) = self.process_cancel.lock() {
            *slot = Some(hook);
        }
    }

    /// Register a root run with a fresh token tree root.
    pub async fn register_root(&self, run_id: &str) -> Result<ExecutionRegistration, String> {
        self.register_inner(run_id, None, CancellationToken::new())
            .await
    }

    /// Register a child run under parent; uses `parent.child_token()`.
    pub async fn register_child(
        &self,
        run_id: &str,
        parent_run_id: &str,
    ) -> Result<ExecutionRegistration, String> {
        let parent_token = {
            let runs = self.runs.lock().await;
            let parent = runs
                .get(parent_run_id)
                .ok_or_else(|| format!("parent run not registered: {parent_run_id}"))?;
            parent.token.clone()
        };
        let child = parent_token.child_token();
        self.register_inner(run_id, Some(parent_run_id.to_string()), child)
            .await
    }

    /// Register with an externally provided token (e.g. tests). Parent optional.
    pub async fn register_with_token(
        &self,
        run_id: &str,
        parent_run_id: Option<String>,
        token: CancellationToken,
    ) -> Result<ExecutionRegistration, String> {
        if let Some(ref parent) = parent_run_id {
            // Ensure parent exists so tree walks stay consistent.
            let runs = self.runs.lock().await;
            if !runs.contains_key(parent) {
                return Err(format!("parent run not registered: {parent}"));
            }
        }
        self.register_inner(run_id, parent_run_id, token).await
    }

    async fn register_inner(
        &self,
        run_id: &str,
        parent_run_id: Option<String>,
        token: CancellationToken,
    ) -> Result<ExecutionRegistration, String> {
        if run_id.trim().is_empty() {
            return Err("run_id required".into());
        }
        let mut runs = self.runs.lock().await;
        if runs.contains_key(run_id) {
            // Idempotent: return existing registration (start retries).
            let existing = runs.get(run_id).expect("just checked");
            return Ok(ExecutionRegistration {
                run_id: run_id.to_string(),
                parent_run_id: existing.parent_run_id.clone(),
                token: existing.token.clone(),
                terminal: existing.terminal.clone(),
            });
        }
        let exec = RunExecution::new(parent_run_id.clone(), token.clone());
        let terminal = exec.terminal.clone();
        runs.insert(run_id.to_string(), exec);
        Ok(ExecutionRegistration {
            run_id: run_id.to_string(),
            parent_run_id,
            token,
            terminal,
        })
    }

    pub async fn token(&self, run_id: &str) -> Option<CancellationToken> {
        self.runs.lock().await.get(run_id).map(|r| r.token.clone())
    }

    pub async fn is_registered(&self, run_id: &str) -> bool {
        self.runs.lock().await.contains_key(run_id)
    }

    pub async fn attach_join(&self, run_id: &str, handle: JoinHandle<()>) -> Result<(), String> {
        let mut runs = self.runs.lock().await;
        let exec = runs
            .get_mut(run_id)
            .ok_or_else(|| format!("run not registered: {run_id}"))?;
        if let Some(prev) = exec.join.replace(handle) {
            // Previous handle still running — abort to avoid leak.
            prev.abort();
        }
        Ok(())
    }

    pub async fn track_resource(
        &self,
        run_id: &str,
        resource: ManagedResource,
    ) -> Result<(), String> {
        let mut runs = self.runs.lock().await;
        let exec = runs
            .get_mut(run_id)
            .ok_or_else(|| format!("run not registered: {run_id}"))?;
        exec.resources.insert(resource);
        Ok(())
    }

    pub async fn untrack_resource(&self, run_id: &str, resource: &ManagedResource) {
        if let Some(exec) = self.runs.lock().await.get_mut(run_id) {
            exec.resources.remove(resource);
        }
    }

    /// All run_ids in the subtree rooted at `run_id` (root first, then DFS children).
    pub async fn list_tree(&self, run_id: &str) -> Vec<String> {
        let runs = self.runs.lock().await;
        if !runs.contains_key(run_id) {
            return vec![run_id.to_string()];
        }
        let mut out = Vec::new();
        let mut stack = vec![run_id.to_string()];
        let mut seen = HashSet::new();
        while let Some(id) = stack.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            out.push(id.clone());
            for (child_id, exec) in runs.iter() {
                if exec.parent_run_id.as_deref() == Some(id.as_str()) {
                    stack.push(child_id.clone());
                }
            }
        }
        out
    }

    /// Signal cancel on the entire tree (cooperative). Does not wait or force.
    pub async fn signal_tree(&self, run_id: &str) -> Vec<String> {
        let tree = self.list_tree(run_id).await;
        let mut runs = self.runs.lock().await;
        for id in &tree {
            if let Some(exec) = runs.get_mut(id) {
                exec.token.cancel();
                if exec.cancel_phase == CancelPhase::Idle {
                    exec.cancel_phase = CancelPhase::Signalled;
                }
            }
        }
        tree
    }

    /// Two-phase cancel for one tree: signal → grace → force abort/kill → clear registry entries.
    ///
    /// Does **not** commit Run status — RunManager owns that (task-02 contract).
    pub async fn cancel_tree(&self, run_id: &str) -> CancelCleanupOutcome {
        let tree = self.signal_tree(run_id).await;
        let grace = self.grace;

        // Graceful: wait for join handles or terminal notify.
        let wait_futs: Vec<_> = {
            let runs = self.runs.lock().await;
            tree.iter()
                .filter_map(|id| {
                    runs.get(id).map(|e| {
                        let term = e.terminal.clone();
                        let token = e.token.clone();
                        async move {
                            tokio::select! {
                                _ = term.notified() => {}
                                _ = token.cancelled() => {
                                    // already cancelled; still give grace for joins
                                }
                            }
                        }
                    })
                })
                .collect()
        };
        let _ = tokio::time::timeout(grace, futures_util::future::join_all(wait_futs)).await;

        // Force phase: abort joins + cancel process resources.
        let mut errors = Vec::new();
        let hook = self
            .process_cancel
            .lock()
            .ok()
            .and_then(|g| g.clone());
        // Collect resources first so we can await hooks without holding run map.
        let mut force_jobs: Vec<(String, Option<JoinHandle<()>>, Vec<ManagedResource>)> = {
            let mut runs = self.runs.lock().await;
            let mut jobs = Vec::new();
            for id in &tree {
                if let Some(exec) = runs.get_mut(id) {
                    exec.cancel_phase = CancelPhase::Force;
                    let join = exec.join.take();
                    let resources: Vec<_> = exec.resources.drain().collect();
                    jobs.push((id.clone(), join, resources));
                }
            }
            jobs
        };
        for (id, join, resources) in force_jobs.drain(..) {
            if let Some(join) = join {
                if !join.is_finished() {
                    join.abort();
                }
            }
            for res in resources {
                match res {
                    ManagedResource::ProcessTask(task_id) => {
                        if let Some(ref h) = hook {
                            if let Err(e) = h.cancel_task(&task_id).await {
                                errors.push(format!("process {task_id}: {e}"));
                            }
                        }
                    }
                    ManagedResource::McpServer(sid) => {
                        let _ = sid;
                    }
                    ManagedResource::External(_) => {}
                }
            }
            if let Some(ref h) = hook {
                if let Err(e) = h.cancel_tasks_for_run(&id).await {
                    errors.push(format!("run processes {id}: {e}"));
                }
            }
        }

        // Remove quiet runs from registry (Cancelled means empty).
        let quiet = errors.is_empty();
        let phase = if quiet {
            CancelPhase::Clean
        } else {
            CancelPhase::CleanupFailed
        };
        {
            let mut runs = self.runs.lock().await;
            for id in &tree {
                if let Some(exec) = runs.get_mut(id) {
                    exec.cancel_phase = phase;
                    exec.terminal.notify_waiters();
                }
                // Drop entry only when clean so Failed(cancel_cleanup_failed) can retry force.
                if quiet {
                    runs.remove(id);
                }
            }
        }

        CancelCleanupOutcome {
            root_run_id: run_id.to_string(),
            run_ids: tree,
            phase,
            quiet,
            errors,
        }
    }

    /// Mark a run's execution finished successfully (or after natural terminal).
    pub async fn mark_finished(&self, run_id: &str) {
        let mut runs = self.runs.lock().await;
        if let Some(mut exec) = runs.remove(run_id) {
            if let Some(join) = exec.join.take() {
                // Detach — caller already observed completion.
                drop(join);
            }
            exec.terminal.notify_waiters();
        }
    }

    /// Cancel every root (no parent) and wait for quiet — Daemon shutdown entry (task-13).
    pub async fn cancel_all_execution_roots(&self) -> Vec<CancelCleanupOutcome> {
        let roots: Vec<String> = {
            let runs = self.runs.lock().await;
            runs.iter()
                .filter(|(_, e)| e.parent_run_id.is_none())
                .map(|(id, _)| id.clone())
                .collect()
        };
        let mut out = Vec::with_capacity(roots.len());
        for root in roots {
            out.push(self.cancel_tree(&root).await);
        }
        // Orphan children without parent registration.
        let leftovers: Vec<String> = {
            let runs = self.runs.lock().await;
            runs.keys().cloned().collect()
        };
        for id in leftovers {
            out.push(self.cancel_tree(&id).await);
        }
        out
    }

    pub async fn active_count(&self) -> usize {
        self.runs.lock().await.len()
    }

    pub async fn cancel_phase(&self, run_id: &str) -> Option<CancelPhase> {
        self.runs
            .lock()
            .await
            .get(run_id)
            .map(|e| e.cancel_phase)
    }
}

impl Default for ExecutionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingHook {
        kills: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl ProcessCancelHook for CountingHook {
        async fn cancel_task(&self, _task_id: &str) -> Result<(), String> {
            self.kills.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        async fn cancel_tasks_for_run(&self, _run_id: &str) -> Result<(), String> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn parent_child_shares_token_tree() {
        let reg = ExecutionRegistry::with_grace_ms(50);
        let parent = reg.register_root("p").await.unwrap();
        let child = reg.register_child("c", "p").await.unwrap();
        let grand = reg.register_child("g", "c").await.unwrap();

        assert!(!child.token.is_cancelled());
        parent.token.cancel();
        assert!(child.token.is_cancelled());
        assert!(grand.token.is_cancelled());
    }

    #[tokio::test]
    async fn cancel_tree_clears_registry_when_clean() {
        let reg = ExecutionRegistry::with_grace_ms(20);
        let hook = Arc::new(CountingHook {
            kills: AtomicUsize::new(0),
        });
        reg.set_process_cancel_hook(hook.clone());
        reg.register_root("p").await.unwrap();
        reg.register_child("c", "p").await.unwrap();
        reg.track_resource("c", ManagedResource::ProcessTask("t1".into()))
            .await
            .unwrap();

        let out = reg.cancel_tree("p").await;
        assert!(out.quiet, "{out:?}");
        assert_eq!(out.phase, CancelPhase::Clean);
        assert!(out.run_ids.contains(&"p".into()));
        assert!(out.run_ids.contains(&"c".into()));
        assert_eq!(reg.active_count().await, 0);
        assert!(hook.kills.load(Ordering::SeqCst) >= 1);
    }

    #[tokio::test]
    async fn cancel_all_roots() {
        let reg = ExecutionRegistry::with_grace_ms(10);
        reg.register_root("a").await.unwrap();
        reg.register_root("b").await.unwrap();
        reg.register_child("a1", "a").await.unwrap();
        let results = reg.cancel_all_execution_roots().await;
        assert!(results.len() >= 2);
        assert_eq!(reg.active_count().await, 0);
    }

    #[tokio::test]
    async fn list_tree_order_includes_descendants() {
        let reg = ExecutionRegistry::new();
        reg.register_root("p").await.unwrap();
        reg.register_child("c1", "p").await.unwrap();
        reg.register_child("c2", "p").await.unwrap();
        reg.register_child("g", "c1").await.unwrap();
        let tree = reg.list_tree("p").await;
        assert_eq!(tree[0], "p");
        assert!(tree.contains(&"c1".into()));
        assert!(tree.contains(&"c2".into()));
        assert!(tree.contains(&"g".into()));
        assert_eq!(tree.len(), 4);
    }
}
