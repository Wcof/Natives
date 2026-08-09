//! Checkpoint-based resume and side-effect resume gate.

use super::manager::RunManager;
use assistant_protocol::v2::{CreateRunRequest, ResumeDecision, RunV2, ResumeRunRequest, ResumeRunResponse};
use rusqlite::OptionalExtension;
use uuid::Uuid;

/// Load a checkpoint and verify it carries the committed snapshot, turn, and
/// ledger watermarks a resumable restore needs. Returns
/// `(id, turn_id, active_context_snapshot_id, side_effect_ledger_cursor)`.
#[allow(clippy::type_complexity)] // pre-existing: factored type alias deferred
pub(crate) fn load_resumable_checkpoint(
    conn: &rusqlite::Connection,
    run_id: &str,
    checkpoint_id: Option<&str>,
) -> Result<(String, Option<String>, Option<String>, Option<String>), String> {
    let checkpoint = if let Some(checkpoint_id) = checkpoint_id {
        conn.query_row(
            "SELECT id, turn_id, active_context_snapshot_id, side_effect_ledger_cursor
             FROM checkpoint WHERE id = ?1 AND run_id = ?2",
            rusqlite::params![checkpoint_id, run_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .map_err(|_| "checkpoint not found for source run".to_string())?
    } else {
        conn.query_row(
            "SELECT id, turn_id, active_context_snapshot_id, side_effect_ledger_cursor
             FROM checkpoint WHERE run_id = ?1 ORDER BY created_at DESC LIMIT 1",
            rusqlite::params![run_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .map_err(|_| "source run has no durable checkpoint".to_string())?
    };
    // Exact restore requires the checkpoint to name a committed active context
    // snapshot. A NULL snapshot would make the resumed run fall back to a newer
    // conversation snapshot and silently change the context being restored.
    let snapshot_id = checkpoint.2.as_deref().ok_or_else(|| {
        "checkpoint has no active context snapshot; restore requires an exact committed snapshot"
            .to_string()
    })?;
    let exists: i64 = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM context_snapshot WHERE id = ?1)",
            rusqlite::params![snapshot_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if exists == 0 {
        return Err("checkpoint references a missing context snapshot".into());
    }
    // A resumable checkpoint must also carry the turn it committed and the
    // side-effect ledger watermark it was captured at; without them the restore
    // has no defined point in the run's history.
    if checkpoint
        .1
        .as_deref()
        .is_none_or(|turn| turn.trim().is_empty())
    {
        return Err("checkpoint has no committed turn; restore requires a turn watermark".into());
    }
    if checkpoint
        .3
        .as_deref()
        .is_none_or(|cursor| cursor.trim().is_empty())
    {
        return Err(
            "checkpoint has no side-effect ledger cursor; restore requires a ledger watermark"
                .into(),
        );
    }
    Ok(checkpoint)
}
/// Effects that a resume/continue from a given checkpoint cannot prove safe:
/// unresolved effects (`started`/`uncertain`) plus effects recorded **after**
/// the checkpoint's side-effect ledger cursor — the checkpoint was captured
/// before them, so their external outcome is unknown and must not be silently
/// replayed (G01). Returns the effects and whether any is non-replay-safe
/// (external/process/network/MCP), which hard-blocks resume.
pub(crate) fn unresolved_effects_for_checkpoint(
    conn: &rusqlite::Connection,
    run_id: &str,
    checkpoint_ledger_cursor: Option<&str>,
) -> Result<(Vec<serde_json::Value>, bool), String> {
    let cursor: i64 = checkpoint_ledger_cursor
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    let mut stmt = conn
        .prepare(
            "SELECT id, tool_call_id, category, replay_safe
             FROM side_effect_record
             WHERE run_id = ?1
               AND (
                   status IN ('started', 'uncertain')
                   OR (status = 'completed' AND (ledger_sequence IS NULL OR ledger_sequence > ?2))
               )",
        )
        .map_err(|e| e.to_string())?;
    let effects: Vec<serde_json::Value> = stmt
        .query_map(rusqlite::params![run_id, cursor], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "tool_call_id": row.get::<_, Option<String>>(1)?,
                "category": row.get::<_, String>(2)?,
                "replay_safe": row.get::<_, i64>(3)? != 0,
            }))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let hard_blocked = effects.iter().any(|effect| {
        effect
            .get("replay_safe")
            .and_then(serde_json::Value::as_bool)
            == Some(false)
    });
    Ok((effects, hard_blocked))
}
impl RunManager {
    /// Create an independent run from a durable checkpoint. This is a
    /// planning operation: the caller starts the returned run separately, so
    /// no source Future, permission waiter, or credential lease is revived.
    pub fn resume_run(&self, req: ResumeRunRequest) -> Result<ResumeRunResponse, String> {
        let source = self
            .get_run(&req.run_id)
            .ok_or_else(|| "run not found".to_string())?;
        if !source.status.is_terminal() {
            return Err("run must be terminal before resume".into());
        }
        let Some(store) = &self.data_store else {
            return Err("resume requires durable daemon storage".into());
        };
        let conn = store.conn()?;
        let checkpoint =
            load_resumable_checkpoint(&conn, &source.id, req.checkpoint_id.as_deref())?;
        // Resolve every effect the checkpoint cannot prove safe: `started`/
        // `uncertain` effects plus effects recorded after the checkpoint's
        // ledger cursor (the checkpoint predates their external outcome).
        // Non-replay-safe unresolved effects hard-block (Blocked), replay-safe
        // ones require explicit caller confirmation (ConfirmationRequired).
        // No provider or tool is invoked until then.
        let (uncertain_effects, hard_blocked) =
            unresolved_effects_for_checkpoint(&conn, &source.id, checkpoint.3.as_deref())?;
        let has_uncertain = !uncertain_effects.is_empty();
        if hard_blocked {
            let _ = conn.execute(
                "INSERT INTO resume_plan
                 (id, source_run_id, action, checkpoint_id, status, decision, unresolved_effects_json)
                 VALUES (?1, ?2, 'resume', ?3, 'blocked', 'Blocked', ?4)",
                rusqlite::params![
                    Uuid::new_v4().to_string(),
                    &source.id,
                    &checkpoint.0,
                    serde_json::to_string(&uncertain_effects).unwrap_or_default(),
                ],
            );
            return Ok(ResumeRunResponse {
                decision: ResumeDecision::Blocked,
                reason: "side-effect ledger has effects not covered by the checkpoint cursor that are not replay-safe; resume is not possible".into(),
                reason_code: "uncovered_side_effects_blocked".into(),
                unresolved_effects: uncertain_effects,
                new_run_id: None,
            });
        }
        if has_uncertain && !req.confirmed {
            let _ = conn.execute(
                "INSERT INTO resume_plan
                 (id, source_run_id, action, checkpoint_id, status, decision, unresolved_effects_json)
                 VALUES (?1, ?2, 'resume', ?3, 'blocked', 'ConfirmationRequired', ?4)",
                rusqlite::params![
                    Uuid::new_v4().to_string(),
                    &source.id,
                    &checkpoint.0,
                    serde_json::to_string(&uncertain_effects).unwrap_or_default(),
                ],
            );
            return Ok(ResumeRunResponse {
                decision: ResumeDecision::ConfirmationRequired,
                reason:
                    "run has effects not covered by the checkpoint cursor; confirm before resume"
                        .into(),
                reason_code: "uncovered_side_effects_confirmation_required".into(),
                unresolved_effects: uncertain_effects,
                new_run_id: None,
            });
        }
        drop(conn);
        // Safe or explicitly confirmed: create a fresh independent run. This
        // revives no old Future, permission waiter, or credential lease.
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
             VALUES (?1, ?2, ?3, 'resume', ?4, 'approved', 'SafeToContinue')",
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
            return Err(format!("persist resume resume plan failed: {error}"));
        }
        Ok(ResumeRunResponse {
            decision: ResumeDecision::SafeToContinue,
            reason: "resume approved".into(),
            reason_code: "safe_to_continue".into(),
            unresolved_effects: Vec::new(),
            new_run_id: Some(new_run.id),
        })
    }
    /// Create a fresh independent continuation run bound to a resumable
    /// checkpoint. Shared by `continue_run` and `resume_run` so there is a
    /// single run-creation path; the new run always has its own identity and
    /// never revives a source Future, permission waiter, or credential lease.
    pub(crate) fn create_continued_run(
        &self,
        source: &RunV2,
        checkpoint: &(String, Option<String>, Option<String>, Option<String>),
        content: Option<String>,
    ) -> Result<RunV2, String> {
        let Some(store) = &self.data_store else {
            return Err("continue requires durable daemon storage".into());
        };
        let conn = store.conn()?;
        let branch_parent_message_id = conn
            .query_row(
                "SELECT id FROM message WHERE conversation_id = ?1
                 ORDER BY created_at DESC, id DESC LIMIT 1",
                rusqlite::params![&source.conversation_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("load branch parent message: {error}"))?;
        drop(conn);
        let mut new_run = self.create_run(CreateRunRequest {
            capability_selection: None,
            disabled_tools: None,
            conversation_id: source.conversation_id.clone(),
            provider_id: source.provider_id.clone(),
            model_id: source.model_id.clone(),
            key_id: source.key_id.clone(),
            agent_profile_id: source.agent_profile_id.clone(),
            permission_profile: Some(source.permission_profile.clone()),
            content: content.clone(),
            attachments: None,
            max_steps: Some(source.max_steps),
            parent_run_id: None,
            project_path: source.project_path.clone(),
            idempotency_key: None,
            effort: source.effort.clone(),
            runtime_id: source.runtime_id.clone(),
        })?;
        new_run.continued_from_run_id = Some(source.id.clone());
        new_run.resume_of_run_id = Some(source.id.clone());
        new_run.checkpoint_id = Some(checkpoint.0.clone());
        new_run.retry_of_turn_id = checkpoint.1.clone();
        new_run.branch_id = source
            .branch_id
            .clone()
            .or_else(|| Some(source.conversation_id.clone()));
        new_run.branch_parent_message_id = branch_parent_message_id;
        self.persist_run_row(&new_run)?;
        self.runs
            .lock()
            .map_err(|e| e.to_string())?
            .insert(new_run.id.clone(), new_run.clone());
        if let Some(content) = content {
            self.last_content
                .lock()
                .map_err(|e| e.to_string())?
                .insert(new_run.id.clone(), content);
        }
        Ok(new_run)
    }
    pub fn mark_resume_plan_executed(
        &self,
        source_run_id: &str,
        new_run_id: &str,
    ) -> Result<(), String> {
        let Some(store) = &self.data_store else {
            return Ok(());
        };
        let changed = store
            .conn()?
            .execute(
                "UPDATE resume_plan
             SET status = 'executed', resolved_at = datetime('now')
             WHERE source_run_id = ?1 AND new_run_id = ?2 AND status = 'approved'",
                rusqlite::params![source_run_id, new_run_id],
            )
            .map_err(|error| error.to_string())?;
        if changed == 0 {
            return Err("approved resume plan not found".into());
        }
        Ok(())
    }
    pub(crate) fn mark_resume_plan_executed_for_run(&self, run: &RunV2) -> Result<(), String> {
        let source_run_id = run
            .continued_from_run_id
            .as_deref()
            .or(run.resume_of_run_id.as_deref())
            .or(run.retry_of_run_id.as_deref());
        if let Some(source_run_id) = source_run_id {
            self.mark_resume_plan_executed(source_run_id, &run.id)?;
        }
        Ok(())
    }
    pub async fn respond_permission(&self, request_id: &str, approved: bool) -> Result<(), String> {
        self.respond_permission_for_run(request_id, approved, None, None)
            .await
    }
    pub async fn respond_permission_for_run(
        &self,
        request_id: &str,
        approved: bool,
        run_id: Option<&str>,
        scope: Option<&str>,
    ) -> Result<(), String> {
        self.runtime
            .respond_permission(request_id, approved, run_id, scope)
            .await
    }
}
