//! Retry and continue lineage.

use super::manager::RunManager;
use super::resume::{load_resumable_checkpoint, unresolved_effects_for_checkpoint};
use assistant_protocol::v2::{ContinueRunRequest, CreateRunRequest, RetryRunRequest, RunV2};
use rusqlite::OptionalExtension;
use uuid::Uuid;

impl RunManager {
    pub fn retry(&self, req: RetryRunRequest) -> Result<RunV2, String> {
        let original = self
            .get_run(&req.run_id)
            .ok_or_else(|| "run not found".to_string())?;
        if original.status.is_active() {
            return Err("active run cannot be retried; cancel it first".into());
        }
        if let Some(store) = &self.data_store {
            let conn = store.conn()?;
            let uncertain: i64 = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM side_effect_record WHERE run_id = ?1 AND status = 'uncertain')",
                    rusqlite::params![&req.run_id],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            if uncertain != 0 {
                store.conn()?.execute(
                    "INSERT INTO resume_plan
                     (id, source_run_id, action, status, decision, unresolved_effects_json)
                     VALUES (?1, ?2, 'retry', 'blocked', 'Blocked', ?3)",
                    rusqlite::params![
                        uuid::Uuid::new_v4().to_string(),
                        &req.run_id,
                        serde_json::json!({"reason": "uncertain_side_effect"}).to_string()
                    ],
                )
                .map_err(|error| {
                    format!(
                        "run has uncertain side effects and blocked resume plan could not be persisted: {error}"
                    )
                })?;
                return Err(
                    "run has uncertain side effects; inspect or compensate before retrying".into(),
                );
            }
        }
        let content = self
            .last_content
            .lock()
            .map_err(|e| e.to_string())?
            .get(&req.run_id)
            .cloned();
        let project_path = self
            .project_paths
            .lock()
            .map_err(|e| e.to_string())?
            .get(&req.run_id)
            .map(|p| p.to_string_lossy().to_string());
        let mut new_run = self.create_run(CreateRunRequest {
            capability_selection: None,
            disabled_tools: None,
            conversation_id: original.conversation_id,
            provider_id: original.provider_id,
            model_id: original.model_id,
            key_id: original.key_id,
            agent_profile_id: original.agent_profile_id,
            permission_profile: Some(original.permission_profile),
            content: content.clone(),
            attachments: None,
            max_steps: Some(original.max_steps),
            parent_run_id: None,
            project_path,
            idempotency_key: None,
            effort: original.effort.clone(),
            runtime_id: original.runtime_id.clone(),
        })?;
        let (checkpoint_id, retry_of_turn_id) = if let Some(store) = &self.data_store {
            let conn = store.conn()?;
            conn.query_row(
                "SELECT id, turn_id FROM checkpoint WHERE run_id = ?1
                 ORDER BY created_at DESC LIMIT 1",
                rusqlite::params![&req.run_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
            )
            .optional()
            .map_err(|e| format!("load retry checkpoint: {e}"))?
            .map_or((None, None), |(id, turn)| (Some(id), turn))
        } else {
            (None, None)
        };
        new_run.retry_of_run_id = Some(req.run_id.clone());
        new_run.retry_of_turn_id = retry_of_turn_id;
        new_run.checkpoint_id = checkpoint_id.clone();
        self.persist_run_row(&new_run)?;
        self.runs
            .lock()
            .map_err(|e| e.to_string())?
            .insert(new_run.id.clone(), new_run.clone());
        if let Some(store) = &self.data_store {
            if let Err(error) = store.conn()?.execute(
                "INSERT INTO resume_plan
                 (id, source_run_id, new_run_id, action, checkpoint_id, status, decision)
                 VALUES (?1, ?2, ?3, 'retry',
                         ?4,
                         'approved', 'SafeToContinue')",
                rusqlite::params![
                    uuid::Uuid::new_v4().to_string(),
                    &req.run_id,
                    &new_run.id,
                    checkpoint_id,
                ],
            ) {
                self.fail_run_if_active(
                    &new_run.id,
                    error.to_string(),
                    "RESUME_PLAN_PERSISTENCE_FAILED",
                );
                return Err(format!("persist retry resume plan failed: {error}"));
            }
        }
        if let Some(c) = content {
            self.last_content
                .lock()
                .map_err(|e| e.to_string())?
                .insert(new_run.id.clone(), c);
        }
        Ok(new_run)
    }
    /// Create an independent run from a durable checkpoint. This is a
    /// planning operation: the caller starts the returned run separately, so
    /// no source Future, permission waiter, or credential lease is revived.
    pub fn continue_run(&self, req: ContinueRunRequest) -> Result<RunV2, String> {
        let source = self
            .get_run(&req.run_id)
            .ok_or_else(|| "run not found".to_string())?;
        if !source.status.is_terminal() {
            return Err("run must be terminal before continue".into());
        }
        let Some(store) = &self.data_store else {
            return Err("continue requires durable daemon storage".into());
        };
        let conn = store.conn()?;
        let checkpoint =
            load_resumable_checkpoint(&conn, &source.id, req.checkpoint_id.as_deref())?;
        let (unresolved, hard_blocked) =
            unresolved_effects_for_checkpoint(&conn, &source.id, checkpoint.3.as_deref())?;
        if !unresolved.is_empty() {
            let decision = if hard_blocked {
                "Blocked"
            } else {
                "ConfirmationRequired"
            };
            conn.execute(
                "INSERT INTO resume_plan
                 (id, source_run_id, action, checkpoint_id, status, decision, unresolved_effects_json)
                 VALUES (?1, ?2, 'continue', ?3, 'blocked', ?4, ?5)",
                rusqlite::params![
                    Uuid::new_v4().to_string(),
                    &source.id,
                    &checkpoint.0,
                    decision,
                    serde_json::to_string(&unresolved).unwrap_or_default(),
                ],
            )
            .map_err(|error| {
                format!(
                    "run has unresolved side effects and blocked resume plan could not be persisted: {error}"
                )
            })?;
            return Err(format!(
                "run has side effects not covered by the checkpoint (cursor {}); continue requires confirmation",
                checkpoint.3.as_deref().unwrap_or("0")
            ));
        }
        drop(conn);
        let content = match req.content {
            Some(content) => Some(content),
            None => self
                .last_content
                .lock()
                .map_err(|e| e.to_string())?
                .get(&source.id)
                .cloned(),
        };
        let new_run = self.create_continued_run(&source, &checkpoint, content.clone())?;
        let conn = store.conn()?;
        if let Err(error) = conn.execute(
            "INSERT INTO resume_plan
             (id, source_run_id, new_run_id, action, checkpoint_id, status, decision)
             VALUES (?1, ?2, ?3, 'continue', ?4, 'approved', 'SafeToContinue')",
            rusqlite::params![
                Uuid::new_v4().to_string(),
                &source.id,
                &new_run.id,
                &checkpoint.0,
            ],
        ) {
            self.fail_run_if_active(
                &new_run.id,
                error.to_string(),
                "RESUME_PLAN_PERSISTENCE_FAILED",
            );
            return Err(format!("persist continue resume plan failed: {error}"));
        }
        Ok(new_run)
    }
}
