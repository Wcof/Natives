//! Conversation forking (`fork` / `copy_transcript`).
//!
//! Split out of `conversation_store` to keep each file under 1000 lines. These
//! items are re-exported from `conversation_store` so the public interface is
//! unchanged.

use rusqlite::{params, OptionalExtension};
use serde_json::Value;

use super::*;

pub(crate) fn fork(params: Value) -> Result<Value, String> {
    let source_id = required_str(&params, "conversation_id")?;
    // W8: optional `through_message_id` — fork up to and including the selected
    // persisted user turn (assistant/tool pairs complete to that turn). When
    // absent the full transcript is copied (legacy behavior).
    let through_message_id = params
        .get("through_message_id")
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
        .map(str::to_string);
    let source = get(serde_json::json!({ "id": source_id }))?;
    let mut params = source;
    params["title"] = serde_json::json!(format!(
        "Fork of {}",
        params
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("Conversation")
    ));
    // A fork inherits transcript metadata only; permissions/credentials are
    // re-resolved for the new branch and never copy a pending grant.
    params["permission_profile_id"] = serde_json::json!("ask");
    let forked = create(params)?;
    let fork_id = required_str(&forked, "id")?;
    let branch_id = uuid::Uuid::new_v4().to_string();
    let store = store()?;
    let conn = store.conn()?;
    let parent_message_id = {
        let mut stmt = conn
            .prepare(
                "SELECT id FROM message WHERE conversation_id = ?1
                 ORDER BY created_at DESC, id DESC LIMIT 1",
            )
            .map_err(|e| e.to_string())?;
        match through_message_id.as_deref() {
            Some(selected) => {
                // Fork head: the selected message itself when it exists.
                conn.query_row(
                    "SELECT id FROM message WHERE conversation_id = ?1 AND id = ?2",
                    rusqlite::params![source_id, selected],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(|e| e.to_string())?
            }
            None => stmt
                .query_row(rusqlite::params![source_id], |row| row.get::<_, String>(0))
                .optional()
                .map_err(|e| e.to_string())?,
        }
    };
    // W8: the selected turn must be a persisted user message (never a run
    // artifact); reject forks from an ephemeral or non-user message.
    if let Some(selected) = through_message_id.as_deref() {
        let role: Option<String> = conn
            .query_row(
                "SELECT role FROM message WHERE conversation_id = ?1 AND id = ?2",
                rusqlite::params![source_id, selected],
                |row| row.get(0),
            )
            .ok()
            .flatten()
            .or_else(|| {
                // Message id may be namespaced (fork:...:old) — still verify role.
                conn.query_row(
                    "SELECT role FROM message WHERE id = ?1",
                    rusqlite::params![selected],
                    |row| row.get(0),
                )
                .ok()
                .flatten()
            });
        if role.as_deref() != Some("user") {
            return Err(format!(
                "fork through_message_id must be a persisted user message, got role={role:?}"
            ));
        }
    }
    copy_transcript(&conn, source_id, fork_id, through_message_id.as_deref())?;
    conn.execute(
        "UPDATE conversation
         SET branch_id = ?1, parent_conversation_id = ?2,
             branch_parent_message_id = ?3
         WHERE id = ?4",
        rusqlite::params![branch_id, source_id, parent_message_id, fork_id],
    )
    .map_err(|e| e.to_string())?;
    let mut forked = forked;
    forked["branch_id"] = serde_json::json!(branch_id);
    forked["parent_conversation_id"] = serde_json::json!(source_id);
    forked["branch_parent_message_id"] = serde_json::json!(parent_message_id);
    Ok(forked)
}

/// Copy the durable typed transcript into a fork with new message identities.
/// Parent links are remapped so the fork is independent while tool-call IDs
/// inside content blocks remain stable for replay and audit correlation.
///
/// W8: with `through_message_id`, the copy is bounded to the selected turn —
/// rows are ordered by `(created_at, id)` and only rows up to and including
/// the selected message are copied (assistant/tool pairs complete to it).
/// The copy is single-transaction and idempotent (fork-prefixed ids).
fn copy_transcript(
    conn: &rusqlite::Connection,
    source_conversation_id: &str,
    fork_conversation_id: &str,
    through_message_id: Option<&str>,
) -> Result<(), String> {
    let mut messages = Vec::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT id, parent_message_id, role, status, input_tokens, output_tokens,
                        reasoning_tokens, cost_usd, created_at, turn_id, run_id, stop_reason,
                        legacy_marker, truncated
                 FROM message WHERE conversation_id = ?1
                 ORDER BY created_at ASC, id ASC",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![source_conversation_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, Option<f64>>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, Option<String>>(10)?,
                    row.get::<_, Option<String>>(11)?,
                    row.get::<_, Option<String>>(12)?,
                    row.get::<_, i64>(13)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        for row in rows {
            messages.push(row.map_err(|e| e.to_string())?);
        }
    }
    // W8: bound to the selected turn when provided. Stable `(created_at, id)`
    // order; stop after the selected message id (inclusive).
    if let Some(selected) = through_message_id {
        let mut bounded = Vec::new();
        for msg in &messages {
            bounded.push(msg.clone());
            if msg.0 == selected {
                break;
            }
        }
        if bounded.last().map(|m| m.0.as_str()) != Some(selected) {
            return Err(format!("fork through_message_id not found: {selected}"));
        }
        messages = bounded;
    }
    let mut id_map = std::collections::HashMap::new();
    for (old_id, ..) in &messages {
        id_map.insert(
            old_id.clone(),
            format!("fork:{fork_conversation_id}:{old_id}"),
        );
    }
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    for (
        old_id,
        parent_id,
        role,
        status,
        input_tokens,
        output_tokens,
        reasoning_tokens,
        cost_usd,
        created_at,
        _turn_id,
        _run_id,
        stop_reason,
        legacy_marker,
        truncated,
    ) in messages
    {
        let new_id = id_map
            .get(&old_id)
            .ok_or_else(|| "fork message identity missing".to_string())?;
        let new_parent = parent_id.as_deref().and_then(|id| id_map.get(id));
        tx.execute(
            "INSERT INTO message
             (id, conversation_id, parent_message_id, role, status, input_tokens, output_tokens,
              reasoning_tokens, cost_usd, created_at, turn_id, run_id, stop_reason, legacy_marker, truncated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                new_id,
                fork_conversation_id,
                new_parent,
                role,
                status,
                input_tokens,
                output_tokens,
                reasoning_tokens,
                cost_usd,
                created_at,
                // Runs/turns belong to the source execution lineage; the
                // fork starts a fresh run while retaining the transcript.
                Option::<String>::None,
                Option::<String>::None,
                stop_reason,
                legacy_marker,
                truncated,
            ],
        )
        .map_err(|e| e.to_string())?;
        let block_rows = {
            let mut blocks = tx
                .prepare(
                    "SELECT sort_order, block_type, block_json, artifact_id, truncated
                     FROM message_block WHERE message_id = ?1 ORDER BY sort_order ASC, id ASC",
                )
                .map_err(|e| e.to_string())?;
            let rows = blocks
                .query_map(params![old_id], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                })
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?
        };
        for (sort_order, block_type, block_json, artifact_id, block_truncated) in block_rows {
            tx.execute(
                "INSERT INTO message_block
                 (message_id, sort_order, block_type, block_json, artifact_id, truncated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    new_id,
                    sort_order,
                    block_type,
                    block_json,
                    artifact_id,
                    block_truncated,
                ],
            )
            .map_err(|e| e.to_string())?;
        }
    }
    tx.commit().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation_store::test_support::*;
    use agent_core::{AgentMessage, ContentBlock};

    #[test]
    fn fork_copies_typed_transcript_with_new_message_ids_and_ask_profile() {
        let _guard = env_lock();
        let _restore = EnvRestore {
            db: std::env::var("NATIVES_DB_PATH").ok(),
            asst: std::env::var("NATIVES_ASSISTANT_DB_PATH").ok(),
            rt: std::env::var("NATIVES_RUNTIME_DIR").ok(),
        };
        let _clear_db = ClearTestDb;
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("fork.db");
        std::env::set_var("NATIVES_DB_PATH", &db);
        std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
        std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
        crate::storage::set_test_db_override(Some(db.clone()), Some(dir.path().join("artifacts")));
        let _store = crate::storage::DataStore::new(&db, &dir.path().join("artifacts")).unwrap();

        let source = create(serde_json::json!({
            "mode": "agent",
            "title": "Source",
            "provider_id": "openai",
            "model_id": "gpt-4o",
            "project_id": "project-1",
            "permission_profile_id": "full_access"
        }))
        .unwrap();
        let source_id = source["id"].as_str().unwrap();
        append_agent_message(
            source_id,
            None,
            None,
            &AgentMessage::User(agent_core::UserMessage {
                message_id: agent_core::MessageId::from("source-user"),
                content: vec![ContentBlock::Text {
                    text: "hello".into(),
                }],
            }),
        )
        .unwrap();

        let forked = fork(serde_json::json!({ "conversation_id": source_id })).unwrap();
        let fork_id = forked["id"].as_str().unwrap();
        assert_ne!(fork_id, source_id);
        assert_eq!(forked["permission_profile_id"], "ask");
        assert_eq!(forked["parent_conversation_id"], source_id);
        assert!(forked["branch_id"].as_str().is_some());

        let source_message_ids: Vec<String> = load_agent_messages(source_id)
            .unwrap()
            .into_iter()
            .map(|message| match message {
                AgentMessage::User(message) => message.message_id.to_string(),
                AgentMessage::Assistant(message) => message.message_id.to_string(),
                AgentMessage::ToolResult(message) => message.message_id.to_string(),
                AgentMessage::System(message) => message.message_id.to_string(),
                AgentMessage::Custom(message) => message.message_id.to_string(),
            })
            .collect();
        let fork_messages = load_agent_messages(fork_id).unwrap();
        assert_eq!(fork_messages.len(), 1);
        assert!(
            !source_message_ids.iter().any(|id| match &fork_messages[0] {
                AgentMessage::User(message) => id == &message.message_id.to_string(),
                AgentMessage::Assistant(message) => id == &message.message_id.to_string(),
                AgentMessage::ToolResult(message) => id == &message.message_id.to_string(),
                AgentMessage::System(message) => id == &message.message_id.to_string(),
                AgentMessage::Custom(message) => id == &message.message_id.to_string(),
            })
        );
        assert!(matches!(
            &fork_messages[0],
            AgentMessage::User(message)
                if matches!(&message.content[0], ContentBlock::Text { text } if text == "hello")
        ));
    }
}
