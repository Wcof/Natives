//! Run creation and idempotency.

use super::manager::RunManager;
use assistant_protocol::v2::{CreateRunRequest, RunEventKind, RunStatusV2, RunV2};
use uuid::Uuid;

impl RunManager {
    pub fn create_run(&self, req: CreateRunRequest) -> Result<RunV2, String> {
        if let Some(key) = &req.idempotency_key {
            {
                let map = self.idempotency.lock().map_err(|e| e.to_string())?;
                if let Some(existing) = map.get(key) {
                    let runs = self.runs.lock().map_err(|e| e.to_string())?;
                    if let Some(run) = runs.get(existing) {
                        return Ok(run.clone());
                    }
                }
            }
            if let Some(run) = self.run_from_store_by_idempotency_key(key)? {
                self.runs
                    .lock()
                    .map_err(|e| e.to_string())?
                    .insert(run.id.clone(), run.clone());
                self.idempotency
                    .lock()
                    .map_err(|e| e.to_string())?
                    .insert(key.clone(), run.id.clone());
                self.store_project_path(&run.id, run.project_path.as_deref());
                return Ok(run);
            }
        }

        // Resolve stable ProjectIdentity when a path is provided (task-10).
        // Never store raw path as conversation.project_id.
        let mut bound_project_id: Option<String> = None;
        let mut bound_identity_version: Option<u32> = None;
        let mut bound_canonical: Option<String> = None;
        if let Some(pp) = req
            .project_path
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            if let Some(store) = &self.data_store {
                if let Ok(conn) = store.conn() {
                    match crate::project_identity::store::register_or_get(&conn, pp) {
                        Ok(identity) => {
                            bound_project_id = Some(identity.project_id.clone());
                            bound_identity_version = Some(identity.identity_version);
                            bound_canonical = Some(identity.canonical_path.clone());
                        }
                        Err(e) => {
                            // Missing path: keep diagnostic snapshot path, leave project_id
                            // unbound (orphaned). Side-effect tools must re-bind first.
                            eprintln!(
                                "[run_manager] project identity not bound for '{}': {e}",
                                std::path::Path::new(pp)
                                    .file_name()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("<project>")
                            );
                            bound_canonical = Some(pp.to_string());
                        }
                    }
                }
            } else {
                bound_canonical = Some(pp.to_string());
            }
        }

        // Host-mediated or daemon-owned: ensure conversation row exists for FK integrity
        // on the SAME store used by persist_run_row (never a different env path).
        // conversation.project_id stores stable ProjectIdentity UUID (not path).
        self.ensure_conversation_for_run(
            &req.conversation_id,
            &req.provider_id,
            &req.model_id,
            req.permission_profile.as_deref(),
            bound_project_id.as_deref(),
        )?;

        // When the UI supplies an idempotency key, use it as the run id so
        // assistant.db rows and daemon events share one identifier.
        let id = req
            .idempotency_key
            .clone()
            .filter(|k| !k.is_empty())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let run = RunV2 {
            capability_snapshot: None,
            retry_of_run_id: None,
            retry_of_turn_id: None,
            continued_from_run_id: None,
            branch_id: None,
            branch_parent_message_id: None,
            checkpoint_id: None,
            resume_of_run_id: None,
            id: id.clone(),
            conversation_id: req.conversation_id,
            status: RunStatusV2::Queued,
            parent_run_id: req.parent_run_id,
            agent_profile_id: req.agent_profile_id,
            provider_id: req.provider_id,
            key_id: req.key_id,
            model_id: req.model_id,
            permission_profile: req.permission_profile.unwrap_or_else(|| "ask".into()),
            trigger_message_id: None,
            started_at: None,
            finished_at: None,
            error_code: None,
            step_count: 0,
            max_steps: req.max_steps.unwrap_or(50),
            project_path: bound_canonical.clone().or(req.project_path.clone()),
            project_id: bound_project_id.clone(),
            project_identity_version: bound_identity_version,
            retry_count: 0,
            created_at: Some(chrono::Utc::now()),
            last_event_sequence: 0,
            idempotency_key: req.idempotency_key.clone(),
            effort: req.effort.clone(),
            runtime_id: req.runtime_id.clone().or_else(|| Some("native".into())),
            revision: 0,
        };
        self.persist_run_row(&run)?;
        {
            let mut runs = self.runs.lock().map_err(|e| e.to_string())?;
            runs.insert(id.clone(), run.clone());
        }
        let _ = self.persist_runs_snapshot();
        if let Some(key) = req.idempotency_key {
            self.idempotency
                .lock()
                .map_err(|e| e.to_string())?
                .insert(key, id.clone());
        }
        if let Some(content) = req.content {
            self.last_content
                .lock()
                .map_err(|e| e.to_string())?
                .insert(run.id.clone(), content);
        }
        self.store_project_path(&run.id, req.project_path.as_deref());
        let queued = self.runtime.events.append(&run.id, RunEventKind::Queued);
        if let RunEventKind::Failed { error, code } = queued.payload {
            if code == "PERSISTENCE_FAILED" {
                if let Ok(mut runs) = self.runs.lock() {
                    runs.remove(&run.id);
                }
                self.delete_run_row(&run.id);
                return Err(error);
            }
        }
        Ok(run)
    }
    /// Ensure a conversation row exists on this manager's DataStore (or env store).
    fn ensure_conversation_for_run(
        &self,
        conversation_id: &str,
        provider_id: &str,
        model_id: &str,
        permission_profile: Option<&str>,
        project_id: Option<&str>,
    ) -> Result<(), String> {
        let id = conversation_id.trim();
        if id.is_empty() {
            return Err("conversation_id is required".into());
        }
        if let Some(store) = &self.data_store {
            let conn = store.conn()?;
            let exists: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM conversation WHERE id = ?1)",
                    rusqlite::params![id],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            if exists {
                return Ok(());
            }
            let now = chrono::Utc::now().to_rfc3339();
            let permission = permission_profile
                .filter(|p| matches!(*p, "readonly" | "ask" | "full_access"))
                .unwrap_or("ask");
            let provider = if provider_id.trim().is_empty() {
                "unknown"
            } else {
                provider_id.trim()
            };
            let model = if model_id.trim().is_empty() {
                "unknown"
            } else {
                model_id.trim()
            };
            conn.execute(
                "INSERT INTO conversation (id, mode, project_id, title, provider_id, model_id, permission_profile_id, created_at, updated_at)
                 VALUES (?1, 'agent', ?2, ?3, ?4, ?5, ?6, ?7, ?7)
                 ON CONFLICT(id) DO NOTHING",
                rusqlite::params![
                    id,
                    project_id,
                    "Daemon-mediated conversation",
                    provider,
                    model,
                    permission,
                    now,
                ],
            )
            .map_err(|e| format!("ensure_conversation_for_run failed: {e}"))?;
            return Ok(());
        }
        // Memory-only manager: no FK store — skip. Production always has data_store.
        let _ = (provider_id, model_id, permission_profile, project_id);
        Ok(())
    }
}
