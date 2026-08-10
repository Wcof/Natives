//! Durable input/safe-point receivers (W9 split from prompt_queue_store.rs).
//!
//! The engine leases queued prompt inputs and reports safe points through
//! these SQLite-backed bridges; durable queue/actor rows stay owned by this
//! module tree (Daemon assistant.db authority).

use super::{global_harness, on_safe_point_checked, persist_actor_snapshot, store};
use crate::conversation_store;
use crate::storage::DataStore;
use agent_core::{
    CoordinatorAction, DrainMode, EngineInputReceiver, EngineSafePointReceiver, InputSafePoint,
    PendingInput, PendingInputKind, SafePoint,
};
use rusqlite::{params, OptionalExtension};
use uuid::Uuid;

pub struct DurableInputReceiver {
    conversation_id: String,
    run_id: String,
}

impl DurableInputReceiver {
    pub fn new(conversation_id: impl Into<String>, run_id: impl Into<String>) -> Self {
        Self {
            conversation_id: conversation_id.into(),
            run_id: run_id.into(),
        }
    }
}

/// Durable safe-point bridge used by the production AgentEngine. The engine
/// must not mutate the in-memory actor directly: a claimed interjection is
/// restored when its post-claim snapshot cannot be persisted.
pub struct DurableSafePointReceiver {
    conversation_id: String,
}

impl DurableSafePointReceiver {
    pub fn new(conversation_id: impl Into<String>) -> Self {
        Self {
            conversation_id: conversation_id.into(),
        }
    }
}

#[async_trait::async_trait]
impl EngineSafePointReceiver for DurableSafePointReceiver {
    async fn on_safe_point(&self, point: InputSafePoint) -> Result<Option<String>, String> {
        let point = match point {
            InputSafePoint::AfterToolBatch => SafePoint::AfterTool,
            InputSafePoint::BeforeProvider => SafePoint::ProviderBatchBoundary,
            InputSafePoint::BeforeRunEnd => SafePoint::ProviderBatchBoundary,
        };
        match on_safe_point_checked(&self.conversation_id, point)? {
            CoordinatorAction::InjectInterjection { content } => Ok(Some(content)),
            _ => Ok(None),
        }
    }
}

#[async_trait::async_trait]
impl EngineInputReceiver for DurableInputReceiver {
    async fn drain(
        &self,
        kind: PendingInputKind,
        mode: DrainMode,
        _point: InputSafePoint,
    ) -> Result<Vec<PendingInput>, String> {
        let kind = match kind {
            PendingInputKind::Steering => "steering",
            PendingInputKind::FollowUp => "follow_up",
        };
        let limit = match mode {
            DrainMode::One => 1,
            DrainMode::All => i64::MAX,
        };
        let store = store()?;
        let mut conn = store.conn()?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let mut items = Vec::new();
        let queued = {
            let mut stmt = tx
                .prepare(
                    "SELECT id, content, COALESCE(drain_mode, 'all') FROM prompt_queue
                 WHERE conversation_id = ?1 AND kind = ?2 AND status = 'queued'
                 ORDER BY position, created_at LIMIT ?3",
                )
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![self.conversation_id, kind, limit], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })
                .map_err(|e| e.to_string())?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|e| e.to_string())?
        };
        let leased_at = chrono::Utc::now().to_rfc3339();
        for row in queued {
            // C02: a `drain_mode='one'` row is a per-row batch boundary — the
            // user asked to process just that input, so an All-mode drain stops
            // before a 'one' row that follows already-leased 'all' rows (that
            // row is leased alone in the NEXT drain). One/all is defined per
            // row (FIFO), never by the first row alone.
            if matches!(mode, DrainMode::All) && row.2 == "one" && !items.is_empty() {
                break;
            }
            let token = Uuid::new_v4().to_string();
            let changed = tx
                .execute(
                    "UPDATE prompt_queue SET status = 'leased', lease_token = ?1,
                    lease_run_id = ?2, leased_at = ?3, updated_at = ?3
                 WHERE id = ?4 AND status = 'queued'",
                    params![token, self.run_id, leased_at, row.0],
                )
                .map_err(|e| format!("lease prompt input failed: {e}"))?;
            if changed != 1 {
                return Err(format!("prompt input {} was no longer queued", row.0));
            }
            items.push(PendingInput {
                id: row.0,
                kind: if kind == "steering" {
                    PendingInputKind::Steering
                } else {
                    PendingInputKind::FollowUp
                },
                content: row.1,
                lease_token: Some(token),
            });
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(items)
    }

    async fn ack(&self, input: &PendingInput, turn_id: Option<&str>) -> Result<(), String> {
        let input_id = &input.id;
        let store = store()?;
        let conn = store.conn()?;
        let content = conn
            .query_row(
                "SELECT content, lease_token FROM prompt_queue
                 WHERE id = ?1 AND conversation_id = ?2 AND lease_run_id = ?3 AND status = 'leased'",
                params![input_id, self.conversation_id, self.run_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
            );
        drop(conn);
        let (queued_content, lease_token) =
            content.map_err(|error| format!("load queued input for ack: {error}"))?;
        if input.lease_token.as_deref() != lease_token.as_deref() {
            return Err("prompt input lease token mismatch".into());
        }
        let content = format!(
            "[{}]\n{}",
            match input.kind {
                PendingInputKind::Steering => "steering",
                PendingInputKind::FollowUp => "follow_up",
            },
            queued_content
        );
        conversation_store::persist_queued_input_and_ack(
            &self.conversation_id,
            &self.run_id,
            input_id,
            &content,
            turn_id,
            input.lease_token.as_deref(),
        )?;
        // SQLite is authoritative, but the live actor must not retain an
        // already-acked item that terminal queue draining could start again.
        let _ = global_harness().remove(&self.conversation_id, input_id);
        persist_actor_snapshot(&self.conversation_id)
    }
}
