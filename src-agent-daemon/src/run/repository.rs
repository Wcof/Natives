//! Persistence mapping and run query access.

use super::lifecycle::{parse_db_time, run_status_from_db};
use super::manager::RunManager;
use assistant_protocol::v2::{ReplayRunRequest, RunEventV2, RunV2};

impl RunManager {
    pub(crate) fn persist_run_row(&self, run: &RunV2) -> Result<(), String> {
        let Some(store) = &self.data_store else {
            return Ok(());
        };
        let conn = store.conn()?;
        conn.execute(
            "INSERT INTO run (
                id, conversation_id, status, trigger_message_id, provider_id, model_id,
                started_at, finished_at, error_code, step_count, max_steps,
                token_budget, total_input_tokens, total_output_tokens, created_at,
                parent_run_id, agent_profile_id, key_id, permission_profile,
                project_path, retry_count, idempotency_key, revision, capability_snapshot_json,
                retry_of_run_id, retry_of_turn_id, continued_from_run_id, branch_id,
                branch_parent_message_id, checkpoint_id, resume_of_run_id
             )
             VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, NULL, 0, 0, ?12,
                ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28
             )
             ON CONFLICT(id) DO UPDATE SET
                status = excluded.status,
                trigger_message_id = excluded.trigger_message_id,
                provider_id = excluded.provider_id,
                model_id = excluded.model_id,
                started_at = excluded.started_at,
                finished_at = excluded.finished_at,
                error_code = excluded.error_code,
                step_count = excluded.step_count,
                max_steps = excluded.max_steps,
                parent_run_id = excluded.parent_run_id,
                agent_profile_id = excluded.agent_profile_id,
                key_id = excluded.key_id,
                permission_profile = excluded.permission_profile,
                project_path = excluded.project_path,
                retry_count = excluded.retry_count,
                idempotency_key = excluded.idempotency_key,
                revision = excluded.revision,
                capability_snapshot_json = excluded.capability_snapshot_json,
                retry_of_run_id = excluded.retry_of_run_id,
                retry_of_turn_id = excluded.retry_of_turn_id,
                continued_from_run_id = excluded.continued_from_run_id,
                branch_id = excluded.branch_id,
                branch_parent_message_id = excluded.branch_parent_message_id,
                checkpoint_id = excluded.checkpoint_id,
                resume_of_run_id = excluded.resume_of_run_id",
            rusqlite::params![
                run.id,
                run.conversation_id,
                run.status.as_str(),
                run.trigger_message_id,
                run.provider_id,
                run.model_id,
                run.started_at.map(|dt| dt.to_rfc3339()),
                run.finished_at.map(|dt| dt.to_rfc3339()),
                run.error_code,
                run.step_count as i64,
                run.max_steps as i64,
                run.created_at.unwrap_or_else(chrono::Utc::now).to_rfc3339(),
                run.parent_run_id,
                run.agent_profile_id,
                run.key_id,
                run.permission_profile,
                run.project_path,
                run.retry_count as i64,
                run.idempotency_key,
                run.revision as i64,
                run.capability_snapshot.as_ref().map(|v| v.to_string()),
                run.retry_of_run_id,
                run.retry_of_turn_id,
                run.continued_from_run_id,
                run.branch_id,
                run.branch_parent_message_id,
                run.checkpoint_id,
                run.resume_of_run_id,
            ],
        )
        .map_err(|e| format!("PERSISTENCE_FAILED upsert run: {e}"))?;
        Ok(())
    }
    pub(crate) fn run_from_store_by_idempotency_key(
        &self,
        key: &str,
    ) -> Result<Option<RunV2>, String> {
        let Some(store) = &self.data_store else {
            return Ok(None);
        };
        if key.trim().is_empty() {
            return Ok(None);
        }
        let conn = store.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, conversation_id, status, parent_run_id, agent_profile_id,
                        provider_id, key_id, model_id, permission_profile,
                        trigger_message_id, started_at, finished_at, error_code,
                        step_count, max_steps, project_path, retry_count,
                        created_at, idempotency_key, COALESCE(revision, 0),
                        retry_of_run_id, retry_of_turn_id, continued_from_run_id,
                        branch_id, branch_parent_message_id, checkpoint_id, resume_of_run_id
                 FROM run WHERE idempotency_key = ?1 OR id = ?1 LIMIT 1",
            )
            .map_err(|e| e.to_string())?;
        let mut rows = stmt
            .query(rusqlite::params![key])
            .map_err(|e| e.to_string())?;
        let Some(row) = rows.next().map_err(|e| e.to_string())? else {
            return Ok(None);
        };
        Ok(Some(RunV2 {
            capability_snapshot: None,
            id: row.get(0).map_err(|e| e.to_string())?,
            conversation_id: row.get(1).map_err(|e| e.to_string())?,
            status: run_status_from_db(&row.get::<_, String>(2).map_err(|e| e.to_string())?),
            parent_run_id: row.get(3).map_err(|e| e.to_string())?,
            agent_profile_id: row.get(4).map_err(|e| e.to_string())?,
            provider_id: row.get(5).map_err(|e| e.to_string())?,
            key_id: row.get(6).map_err(|e| e.to_string())?,
            model_id: row.get(7).map_err(|e| e.to_string())?,
            permission_profile: row.get(8).map_err(|e| e.to_string())?,
            trigger_message_id: row.get(9).map_err(|e| e.to_string())?,
            started_at: parse_db_time(
                row.get::<_, Option<String>>(10)
                    .map_err(|e| e.to_string())?,
            ),
            finished_at: parse_db_time(
                row.get::<_, Option<String>>(11)
                    .map_err(|e| e.to_string())?,
            ),
            error_code: row.get(12).map_err(|e| e.to_string())?,
            step_count: row.get::<_, i64>(13).map_err(|e| e.to_string())? as u32,
            max_steps: row.get::<_, i64>(14).map_err(|e| e.to_string())? as u32,
            project_path: row.get(15).map_err(|e| e.to_string())?,
            retry_count: row.get::<_, i64>(16).map_err(|e| e.to_string())? as u32,
            created_at: parse_db_time(
                row.get::<_, Option<String>>(17)
                    .map_err(|e| e.to_string())?,
            ),
            last_event_sequence: 0,
            idempotency_key: row.get(18).map_err(|e| e.to_string())?,
            effort: None,
            runtime_id: None,
            revision: row.get::<_, i64>(19).unwrap_or(0) as u64,
            project_id: None,
            project_identity_version: None,
            retry_of_run_id: row.get(20).map_err(|e| e.to_string())?,
            retry_of_turn_id: row.get(21).map_err(|e| e.to_string())?,
            continued_from_run_id: row.get(22).map_err(|e| e.to_string())?,
            branch_id: row.get(23).map_err(|e| e.to_string())?,
            branch_parent_message_id: row.get(24).map_err(|e| e.to_string())?,
            checkpoint_id: row.get(25).map_err(|e| e.to_string())?,
            resume_of_run_id: row.get(26).map_err(|e| e.to_string())?,
        }))
    }
    pub(crate) fn delete_run_row(&self, run_id: &str) {
        if let Some(store) = &self.data_store {
            if let Ok(conn) = store.conn() {
                let _ = conn.execute("DELETE FROM run WHERE id = ?1", rusqlite::params![run_id]);
            }
        }
    }
    /// Shared DataStore handle for ProjectIdentity verify on tool path.
    pub fn data_store_ref(&self) -> Option<std::sync::Arc<crate::storage::DataStore>> {
        self.data_store.clone()
    }
    pub fn get_run(&self, run_id: &str) -> Option<RunV2> {
        self.runs.lock().ok()?.get(run_id).cloned()
    }
    pub fn list_runs(&self, conversation_id: Option<&str>) -> Vec<RunV2> {
        let runs = match self.runs.lock() {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };
        runs.values()
            .filter(|r| {
                conversation_id
                    .map(|c| r.conversation_id == c)
                    .unwrap_or(true)
            })
            .cloned()
            .collect()
    }
    /// Drop in-memory run bookkeeping after conversations were hard-deleted.
    /// Does not touch durable `usage_stats` aggregates.
    pub fn forget_conversations(&self, conversation_ids: &[String]) {
        if conversation_ids.is_empty() {
            return;
        }
        let set: std::collections::HashSet<&str> =
            conversation_ids.iter().map(|s| s.as_str()).collect();
        if let Ok(mut runs) = self.runs.lock() {
            runs.retain(|_, r| !set.contains(r.conversation_id.as_str()));
        }
        if let Ok(mut last) = self.last_content.lock() {
            // last_content is keyed by run_id; drop entries whose run is gone.
            if let Ok(runs) = self.runs.lock() {
                last.retain(|run_id, _| runs.contains_key(run_id));
            }
        }
        let _ = self.persist_runs_snapshot();
    }
    pub fn replay(&self, req: ReplayRunRequest) -> Vec<RunEventV2> {
        self.runtime
            .events
            .replay_after(&req.run_id, req.after_sequence)
    }
    /// Checked replay for authoritative consumers. A corrupt event stream is
    /// an error, not an empty replay that could make a renderer or recovery
    /// path conclude that no facts exist.
    pub fn replay_checked(&self, req: ReplayRunRequest) -> Result<Vec<RunEventV2>, String> {
        self.runtime
            .events
            .replay_after_checked(&req.run_id, req.after_sequence)
    }
}
