//! Run start helpers — message append + preparing transition + detached start (W9 split from
//! `run/start.rs`). Kept as `impl RunManager` so callers stay unchanged.

use super::manager::{global_run_manager, RunManager};
use assistant_protocol::v2::{
    AttachmentRef, CreateRunRequest, RunStatusV2, RunV2, StartRunRequest,
};
use std::sync::Arc;

impl RunManager {
    /// Insert a user message on **this** manager's DataStore (never env-open another DB).
    pub(crate) fn append_user_message_on_store(
        &self,
        conversation_id: &str,
        content: Option<&str>,
        attachments: Option<&[AttachmentRef]>,
    ) -> Result<Option<String>, String> {
        let Some(store) = &self.data_store else {
            return Ok(None);
        };
        let mut blocks = Vec::new();
        if let Some(content) = content.filter(|s| !s.trim().is_empty()) {
            blocks.push(serde_json::json!({ "type": "text", "text": content }));
        }
        for attachment in attachments.unwrap_or(&[]) {
            if attachment.path.trim().is_empty() {
                continue;
            }
            blocks.push(serde_json::json!({
                "type": "file_reference",
                "path": attachment.path,
                "name": attachment.name.clone(),
                "mime_type": attachment.mime_type.clone(),
                "size": attachment.size,
            }));
        }
        if blocks.is_empty() {
            return Ok(None);
        }
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let conn = store.conn()?;
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO message (id, conversation_id, role, status, created_at)
             VALUES (?1, ?2, 'user', 'complete', ?3)",
            rusqlite::params![id, conversation_id, now],
        )
        .map_err(|e| e.to_string())?;
        for (index, block) in blocks.iter().enumerate() {
            let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("text");
            let content = if block_type == "text" {
                serde_json::json!({
                    "text": block.get("text").and_then(|v| v.as_str()).unwrap_or_default()
                })
            } else {
                block.clone()
            };
            tx.execute(
                "INSERT INTO message_block (message_id, sort_order, block_type, block_json)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![id, index as i64, block_type, content.to_string()],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.execute(
            "UPDATE conversation SET updated_at = ?1 WHERE id = ?2",
            rusqlite::params![now, conversation_id],
        )
        .and_then(|_| tx.commit())
        .map_err(|e| e.to_string())?;
        Ok(Some(id))
    }

    pub(crate) fn mark_preparing(&self, run_id: &str) -> Result<RunV2, String> {
        self.commit_status(
            run_id,
            RunStatusV2::Preparing,
            agent_core::TransitionMetadata::empty().with_lifecycle_hint("preparing"),
        )
    }

    /// Non-blocking start for RPC / UI: returns immediately with Preparing status.
    /// Engine execution continues on a background task; clients poll events / cancel
    /// on the same session without waiting for completion.
    ///
    /// Idempotent: if the run is already active (or terminal), does **not** spawn a
    /// second engine — returns the current row (duplicate Start is a no-op).
    pub fn start_detached(self: &Arc<Self>, req: StartRunRequest) -> Result<RunV2, String> {
        let run = self.ensure_run_for_start(&req)?;

        // Honest runtime gate (REQ-T01/T02): codex never executable; claude_cli only if binary present.
        if let Some(rt) = req
            .runtime_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            match rt {
                "native" | "" => {}
                "codex_cli" => {
                    // Single source of truth for T02 red line.
                    if !crate::codex_runtime_bridge::codex_cli_available() {
                        return Err(
                            "runtime codex_cli is unavailable (app-server not implemented)".into(),
                        );
                    }
                }
                "claude_cli" => {
                    if !crate::cli_runtime_bridge::claude_cli_available() {
                        return Err(
                            "runtime claude_cli unavailable (claude binary not found)".into()
                        );
                    }
                }
                other => {
                    return Err(format!("unknown runtime_id: {other}"));
                }
            }
        }
        if run.status.is_active() {
            return Ok(run);
        }
        if run.status.is_terminal() {
            return Err(format!(
                "run {} is terminal ({:?}); use run.retry",
                run.id,
                run.status.as_str()
            ));
        }
        let mut req = req;
        req.run_id = Some(run.id.clone());
        if let Some(content) = &req.content {
            self.last_content
                .lock()
                .map_err(|e| e.to_string())?
                .insert(run.id.clone(), content.clone());
        }
        self.store_project_path(&run.id, req.project_path.as_deref());
        let preparing = self.mark_preparing(&run.id)?;
        self.persist_runs_snapshot()?;
        let rm = Arc::clone(self);
        let run_id_for_fail = preparing.id.clone();
        tokio::spawn(async move {
            if let Err(err) = rm.start(req).await {
                rm.fail_run_if_active(&run_id_for_fail, err, "START_FAILED");
            }
        });
        Ok(preparing)
    }

    /// Process-wide non-blocking start (sidecar RPC uses this via `global_run_manager`).
    /// Same idempotency rules as [`start_detached`].
    pub fn start_detached_global(req: StartRunRequest) -> Result<RunV2, String> {
        let rm = global_run_manager();
        let run = rm.ensure_run_for_start(&req)?;

        // Honest runtime gate (REQ-T01/T02): codex never executable; claude_cli only if binary present.
        if let Some(rt) = req
            .runtime_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            match rt {
                "native" | "" => {}
                "codex_cli" => {
                    // Single source of truth for T02 red line.
                    if !crate::codex_runtime_bridge::codex_cli_available() {
                        return Err(
                            "runtime codex_cli is unavailable (app-server not implemented)".into(),
                        );
                    }
                }
                "claude_cli" => {
                    if !crate::cli_runtime_bridge::claude_cli_available() {
                        return Err(
                            "runtime claude_cli unavailable (claude binary not found)".into()
                        );
                    }
                }
                other => {
                    return Err(format!("unknown runtime_id: {other}"));
                }
            }
        }
        if run.status.is_active() {
            return Ok(run);
        }
        if run.status.is_terminal() {
            return Err(format!(
                "run {} is terminal ({:?}); use run.retry",
                run.id,
                run.status.as_str()
            ));
        }
        let mut req = req;
        req.run_id = Some(run.id.clone());
        if let Some(content) = &req.content {
            rm.last_content
                .lock()
                .map_err(|e| e.to_string())?
                .insert(run.id.clone(), content.clone());
        }
        rm.store_project_path(&run.id, req.project_path.as_deref());
        let preparing = rm.mark_preparing(&run.id)?;
        rm.persist_runs_snapshot()?;
        let run_id_for_fail = preparing.id.clone();
        tokio::spawn(async move {
            let rm = global_run_manager();
            if let Err(err) = rm.start(req).await {
                rm.fail_run_if_active(&run_id_for_fail, err, "START_FAILED");
            }
        });
        Ok(preparing)
    }

    /// Ensure a run row exists for `start` / `start_detached` (create if `run_id` absent).
    ///
    /// Always ensures the current user turn is recorded in the daemon conversation store
    /// under a daemon-owned `trigger_message_id` (never reuses host message ids as FKs).
    /// Ensure a run exists for start. Appends a daemon-local user message when content
    /// is present. When this manager is memory-only (`data_store` is None), skip SQLite
    /// message writes so pure unit tests never touch the developer's assistant.db.
    pub fn ensure_run_for_start(&self, req: &StartRunRequest) -> Result<RunV2, String> {
        let can_write_messages = self.data_store.is_some();
        // Existing run (host create_run + start path): still append daemon-local user message.
        if let Some(run_id) = &req.run_id {
            let mut run = self
                .get_run(run_id)
                .ok_or_else(|| "run not found".to_string())?;
            if can_write_messages
                && run.trigger_message_id.is_none()
                && (req.content.as_ref().is_some_and(|c| !c.trim().is_empty())
                    || req.attachments.as_ref().is_some_and(|a| !a.is_empty()))
            {
                if let Some(id) = self.append_user_message_on_store(
                    &run.conversation_id,
                    req.content.as_deref(),
                    req.attachments.as_deref(),
                )? {
                    {
                        let mut runs = self.runs.lock().map_err(|e| e.to_string())?;
                        if let Some(stored) = runs.get_mut(&run.id) {
                            stored.trigger_message_id = Some(id.clone());
                            run = stored.clone();
                        }
                    }
                    self.persist_run_row(&run)?;
                    self.persist_runs_snapshot()?;
                }
            }
            return Ok(run);
        }
        if let Some(key) = &req.idempotency_key {
            let map = self.idempotency.lock().map_err(|e| e.to_string())?;
            if let Some(existing) = map.get(key) {
                let runs = self.runs.lock().map_err(|e| e.to_string())?;
                if let Some(run) = runs.get(existing) {
                    return Ok(run.clone());
                }
            }
        }
        let conversation_id = req
            .conversation_id
            .clone()
            .ok_or_else(|| "conversation_id required".to_string())?;
        let trigger_message_id = if can_write_messages {
            self.append_user_message_on_store(
                &conversation_id,
                req.content.as_deref(),
                req.attachments.as_deref(),
            )?
        } else {
            None
        };
        let mut run = match self.create_run(CreateRunRequest {
            // Seam A (ADR-0016): profile + selection flow through instead of
            // being dropped at the gateway boundary.
            capability_selection: req.capability_selection.clone(),
            disabled_tools: None,
            conversation_id: conversation_id.clone(),
            provider_id: req.provider_id.clone().unwrap_or_default(),
            model_id: req.model_id.clone().unwrap_or_default(),
            key_id: req.key_id.clone(),
            agent_profile_id: req.agent_profile_id.clone(),
            permission_profile: req.permission_profile.clone().or_else(|| {
                // Prefer conversation-row profile when available on this store.
                if let Some(store) = &self.data_store {
                    store.conn().ok().and_then(|conn| {
                        conn.query_row(
                            "SELECT COALESCE(permission_profile_id, 'ask') FROM conversation WHERE id = ?1",
                            rusqlite::params![conversation_id],
                            |row| row.get::<_, String>(0),
                        )
                        .ok()
                    })
                } else {
                    None
                }
            }),
            content: req.content.clone(),
            attachments: req.attachments.clone(),
            max_steps: req.max_steps,
            parent_run_id: None,
            project_path: req.project_path.clone(),
            idempotency_key: req.idempotency_key.clone(),
            effort: req.effort.clone(),
            runtime_id: req.runtime_id.clone(),
        }) {
            Ok(run) => run,
            Err(error) => {
                // Roll back the pre-created trigger message if create_run failed.
                if let (Some(store), Some(id)) = (&self.data_store, trigger_message_id.as_deref()) {
                    let _ = store.conn().and_then(|conn| {
                        conn.execute("DELETE FROM message WHERE id = ?1", rusqlite::params![id])
                            .map_err(|e| e.to_string())
                    });
                }
                return Err(error);
            }
        };
        if let Some(trigger_message_id) = trigger_message_id {
            {
                let mut runs = self.runs.lock().map_err(|e| e.to_string())?;
                if let Some(stored) = runs.get_mut(&run.id) {
                    stored.trigger_message_id = Some(trigger_message_id.clone());
                    run = stored.clone();
                }
            }
            self.persist_run_row(&run)?;
            self.persist_runs_snapshot()?;
        }
        Ok(run)
    }
}
