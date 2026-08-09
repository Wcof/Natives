//! Run start helpers — message append + preparing transition (W9 split from
//! `run/start.rs`). Kept as `impl RunManager` so callers stay unchanged.

use super::manager::RunManager;
use crate::Result;
use assistant_protocol::v2::{AttachmentRef, RunStatusV2, RunV2};

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
            crate::production::TransitionMetadata::empty()
                .with_lifecycle_hint("preparing"),
        )
    }
}
