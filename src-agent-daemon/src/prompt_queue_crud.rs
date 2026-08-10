//! promptQueue.* RPC handlers (W9 split from prompt_queue_store.rs).
//! `super` here is the `prompt_queue_store` module.

use super::{
    ensure_conversation_for_queue, global_harness, load_actor_snapshot, persist_actor_snapshot,
    row_to_item, store, value_to_queue_item,
};
use crate::run_manager::global_run_manager;
use agent_core::{CoordinatorAction, PromptSource, QueueItem, QueueItemStatus};
use assistant_protocol::v2::methods::names;
use assistant_protocol::v2::{CancelRunRequest, StartRunRequest};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use uuid::Uuid;

pub async fn request(method: &str, params: Value) -> Result<Value, String> {
    match method {
        names::PROMPT_QUEUE_LIST => list(params),
        names::PROMPT_QUEUE_ENQUEUE => enqueue(params),
        names::PROMPT_QUEUE_UPDATE => update(params),
        names::PROMPT_QUEUE_REMOVE => remove(params),
        names::PROMPT_QUEUE_REORDER => reorder(params),
        names::PROMPT_QUEUE_SEND_NOW => send_now(params).await,
        names::PROMPT_QUEUE_INTERJECT => interject(params),
        _ => Err(format!("unsupported promptQueue method: {method}")),
    }
}

pub(crate) fn list(params: Value) -> Result<Value, String> {
    let conversation_id = params
        .get("conversation_id")
        .or_else(|| params.get("conversationId"))
        .and_then(Value::as_str)
        .ok_or_else(|| "conversation_id is required".to_string())?;
    let store = store()?;
    let conn = store.conn()?;
    let sql_with_status = "SELECT id, conversation_id, content, source, attachments, position,
                                  client_temp_id, created_at, updated_at, status
                           FROM prompt_queue
                           WHERE conversation_id = ?1
                           ORDER BY position ASC, created_at ASC";
    let sql_legacy = "SELECT id, conversation_id, content, source, attachments, position,
                             client_temp_id, created_at, updated_at, 'queued' AS status
                      FROM prompt_queue
                      WHERE conversation_id = ?1
                      ORDER BY position ASC, created_at ASC";
    let mut stmt = conn
        .prepare(sql_with_status)
        .or_else(|_| conn.prepare(sql_legacy))
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![conversation_id], row_to_item)
        .map_err(|e| e.to_string())?;
    let items: Vec<Value> = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    // SQLite is source of truth — always rehydrate coordinator from durable rows.
    let q_items: Vec<QueueItem> = items
        .iter()
        .map(value_to_queue_item)
        .collect::<Result<Vec<_>, _>>()?;
    let harness = global_harness();
    harness.reload_queue(conversation_id, q_items);
    if let Some(snap) = load_actor_snapshot(conversation_id)? {
        harness.restore_snapshot(snap);
    }

    Ok(Value::Array(items))
}

pub(crate) fn enqueue(params: Value) -> Result<Value, String> {
    let conversation_id = params
        .get("conversation_id")
        .or_else(|| params.get("conversationId"))
        .and_then(Value::as_str)
        .ok_or_else(|| "conversation_id is required".to_string())?;
    let content = params
        .get("content")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "content is required".to_string())?
        .to_string();
    let source = params
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or("user");
    let client_temp_id = params
        .get("client_temp_id")
        .or_else(|| params.get("clientTempId"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let attachments = params
        .get("attachments")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "null".into());
    let kind = if source.eq_ignore_ascii_case("interjection") {
        "steering"
    } else {
        "follow_up"
    };
    let drain_mode = params
        .get("drain_mode")
        .or_else(|| params.get("drainMode"))
        .and_then(Value::as_str)
        .unwrap_or("all");
    if !matches!(drain_mode, "one" | "all") {
        return Err("drain_mode must be one or all".into());
    }

    ensure_conversation_for_queue(conversation_id, &params)?;

    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let store = store()?;
    let conn = store.conn()?;
    let position: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM prompt_queue WHERE conversation_id = ?1",
            params![conversation_id],
            |row| row.get(0),
        )
        .unwrap_or(0);

    conn.execute(
        "INSERT INTO prompt_queue
            (id, conversation_id, content, source, attachments, position, client_temp_id, created_at, updated_at, status, kind, drain_mode)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, 'queued', ?9, ?10)",
        params![
            id,
            conversation_id,
            content,
            source,
            attachments,
            position,
            client_temp_id,
            now,
            kind,
            drain_mode
        ],
    )
    .or_else(|e| {
        if e.to_string().contains("no such column: status") {
            conn.execute(
                "INSERT INTO prompt_queue
                    (id, conversation_id, content, source, attachments, position, client_temp_id, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                params![
                    id,
                    conversation_id,
                    content,
                    source,
                    attachments,
                    position,
                    client_temp_id,
                    now
                ],
            )
        } else {
            Err(e)
        }
    })
    .map_err(|e| format!("prompt_queue insert: {e}"))?;

    let item = global_harness().enqueue(
        conversation_id,
        content.clone(),
        PromptSource::parse(source),
        client_temp_id.clone(),
        Some(id.clone()),
    );
    persist_actor_snapshot(conversation_id)?;

    Ok(json!({
        "id": item.id,
        "conversation_id": conversation_id,
        "content": content,
        "source": source,
        "order": position,
        "position": position,
        "client_temp_id": client_temp_id,
        "created_at": now,
        "status": "queued",
    }))
}

pub(crate) fn update(params: Value) -> Result<Value, String> {
    let id = params
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "id is required".to_string())?;
    let content = params
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| "content is required".to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    let store = store()?;
    let conn = store.conn()?;
    let conversation_id: String = conn
        .query_row(
            "SELECT conversation_id FROM prompt_queue WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "prompt queue item not found".to_string())?;

    let n = conn
        .execute(
            "UPDATE prompt_queue SET content = ?1, updated_at = ?2 WHERE id = ?3",
            params![content, now, id],
        )
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err("prompt queue item not found".into());
    }

    let _ = global_harness().update(&conversation_id, id, content);
    persist_actor_snapshot(&conversation_id)?;
    Ok(json!({ "id": id, "updated": true }))
}

pub(crate) fn remove(params: Value) -> Result<Value, String> {
    let id = params
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "id is required".to_string())?;
    let store = store()?;
    let conn = store.conn()?;
    let conversation_id: Option<String> = conn
        .query_row(
            "SELECT conversation_id FROM prompt_queue WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;

    let n = conn
        .execute("DELETE FROM prompt_queue WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err("prompt queue item not found".into());
    }
    if let Some(cid) = conversation_id {
        let _ = global_harness().remove(&cid, id);
        persist_actor_snapshot(&cid)?;
    }
    Ok(json!({ "id": id, "removed": true }))
}

pub(crate) fn reorder(params: Value) -> Result<Value, String> {
    let conversation_id = params
        .get("conversation_id")
        .or_else(|| params.get("conversationId"))
        .and_then(Value::as_str)
        .ok_or_else(|| "conversation_id is required".to_string())?;
    let ids = params
        .get("ids")
        .and_then(Value::as_array)
        .ok_or_else(|| "ids is required".to_string())?;
    let id_list: Vec<String> = ids
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect();

    let store = store()?;
    let conn = store.conn()?;
    let now = chrono::Utc::now().to_rfc3339();
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    for (position, id) in id_list.iter().enumerate() {
        tx.execute(
            "UPDATE prompt_queue SET position = ?1, updated_at = ?2
             WHERE id = ?3 AND conversation_id = ?4",
            params![position as i64, now, id, conversation_id],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;

    let _ = global_harness().reorder(conversation_id, &id_list);
    persist_actor_snapshot(conversation_id)?;
    Ok(json!({ "conversation_id": conversation_id, "reordered": true }))
}

pub(crate) fn interject(params: Value) -> Result<Value, String> {
    let conversation_id = params
        .get("conversation_id")
        .or_else(|| params.get("conversationId"))
        .and_then(Value::as_str)
        .ok_or_else(|| "conversation_id is required".to_string())?
        .to_string();
    let content = params
        .get("content")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "content is required".to_string())?
        .to_string();

    let mut queued = params;
    if let Some(object) = queued.as_object_mut() {
        object.insert("source".into(), Value::String("interjection".into()));
        object.insert("drain_mode".into(), Value::String("all".into()));
    }
    let result = enqueue(queued)?;
    Ok(json!({
        "conversation_id": conversation_id,
        "interjected": true,
        "content": content,
        "id": result.get("id"),
        "status": "queued",
        "note": "leased and acknowledged by the AgentEngine at a safe point",
    }))
}

async fn send_now(params: Value) -> Result<Value, String> {
    let id = params
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "id is required".to_string())?;

    let store = store()?;
    let (conversation_id, content, _attachments_raw, provider_id, model_id, project_path) = {
        let conn = store.conn()?;
        conn.query_row(
            "SELECT q.conversation_id, q.content, q.attachments,
                    c.provider_id, c.model_id, c.project_id
             FROM prompt_queue q
             JOIN conversation c ON c.id = q.conversation_id
             WHERE q.id = ?1",
            params![id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "prompt queue item not found".to_string())?
    };

    // Ensure coordinator knows about this item (may already).
    let harness = global_harness();
    if !harness.list(&conversation_id).iter().any(|i| i.id == id) {
        harness.enqueue(
            &conversation_id,
            &content,
            PromptSource::User,
            None,
            Some(id.to_string()),
        );
    }

    let action = harness.send_now(&conversation_id, id)?;
    persist_actor_snapshot(&conversation_id)?;

    // Cancel active runs when needed. Only the winner of finish_run(expected_run_id)
    // may start the next prompt — either us (after cancel settles) or on_run_terminal.
    if matches!(&action, CoordinatorAction::CancelThenStart { .. }) {
        let rm = global_run_manager();
        let active_ids: Vec<String> = rm
            .list_runs(Some(&conversation_id))
            .into_iter()
            .filter(|r| !r.status.is_terminal())
            .map(|r| r.id)
            .collect();
        for run_id in &active_ids {
            rm.cancel(CancelRunRequest {
                run_id: run_id.clone(),
            })
            .await
            .map_err(|error| format!("cancel active run before send_now: {error}"))?;
            for _ in 0..20 {
                if rm
                    .get_run(run_id)
                    .map(|r| r.status.is_terminal())
                    .unwrap_or(true)
                {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
        }

        // Prefer the coordinator's recorded active run id (exact match for finish_run).
        let expected = harness
            .snapshot(&conversation_id)
            .active_run_id
            .or_else(|| active_ids.first().cloned());

        if let Some(expected_run_id) = expected {
            if let Some(CoordinatorAction::StartPrompt { item }) =
                harness.finish_run(&conversation_id, &expected_run_id, false)
            {
                let start_req = StartRunRequest {
                    agent_profile_id: None,
                    capability_selection: None,
                    run_id: None,
                    conversation_id: Some(conversation_id.clone()),
                    provider_id: Some(provider_id.clone()),
                    model_id: Some(model_id.clone()),
                    key_id: None,
                    content: Some(item.content.clone()),
                    attachments: None,
                    trigger_message_id: None,
                    permission_profile: None,
                    max_steps: None,
                    project_path: project_path.clone(),
                    idempotency_key: Some(format!("prompt-queue:{}", item.id)),
                    effort: None,
                    runtime_id: None,
                };
                let run = match crate::run_manager::RunManager::start_detached_global(start_req) {
                    Ok(run) => run,
                    Err(error) => {
                        harness.requeue(&conversation_id, item.clone());
                        persist_actor_snapshot(&conversation_id)?;
                        return Err(error);
                    }
                };
                let conn = store.conn()?;
                let now = chrono::Utc::now().to_rfc3339();
                harness.mark_running_item(&conversation_id, &run.id, Some(&item.id), &item.content);
                let changed = conn
                    .execute(
                        "UPDATE prompt_queue SET status = 'sent', updated_at = ?1 WHERE id = ?2",
                        params![now, item.id],
                    )
                    .map_err(|e| format!("mark queued prompt sent: {e}"))?;
                if changed != 1 {
                    return Err("queued prompt was not transitioned to sent".into());
                }
                conn.execute("DELETE FROM prompt_queue WHERE id = ?1", params![item.id])
                    .map_err(|e| format!("remove sent prompt: {e}"))?;
                persist_actor_snapshot(&conversation_id)?;
                return serde_json::to_value(run)
                    .map_err(|e| format!("serialize started run: {e}"));
            }
        }

        // on_run_terminal already claimed finish and started next (or still mid-cancel).
        persist_actor_snapshot(&conversation_id)?;
        return Ok(json!({
            "conversation_id": conversation_id,
            "content": content,
            "started": false,
            "cancelling": true,
            "queue_item_id": id,
            "note": "cancel requested; next prompt starts via sole finish_run winner",
        }));
    }

    // Idle path: StartPrompt immediately. Keep the durable row until the
    // RunManager accepts the new run so a synchronous start failure can be
    // retried instead of silently dropping the prompt.
    let item = match action {
        CoordinatorAction::StartPrompt { item } => item,
        _ => return Err("queue coordinator returned no start action".into()),
    };

    let start_req = StartRunRequest {
        agent_profile_id: None,
        capability_selection: None,
        run_id: None,
        conversation_id: Some(conversation_id.clone()),
        provider_id: Some(provider_id),
        model_id: Some(model_id),
        key_id: None,
        content: Some(item.content.clone()),
        attachments: None,
        trigger_message_id: None,
        permission_profile: None,
        max_steps: None,
        project_path,
        // Unified queue idempotency key (also used by drain / cancel-and-send).
        idempotency_key: Some(format!("prompt-queue:{}", item.id)),
        effort: None,
        runtime_id: None,
    };

    let run = match crate::run_manager::RunManager::start_detached_global(start_req) {
        Ok(run) => run,
        Err(error) => {
            harness.requeue(&conversation_id, item.clone());
            persist_actor_snapshot(&conversation_id)?;
            return Err(error);
        }
    };
    let conn = store.conn()?;
    let now = chrono::Utc::now().to_rfc3339();
    harness.mark_running_item(&conversation_id, &run.id, Some(&item.id), &item.content);
    let changed = conn
        .execute(
            "UPDATE prompt_queue SET status = 'sent', updated_at = ?1 WHERE id = ?2",
            params![now, item.id],
        )
        .map_err(|e| format!("mark queued prompt sent: {e}"))?;
    if changed != 1 {
        return Err("queued prompt was not transitioned to sent".into());
    }
    conn.execute("DELETE FROM prompt_queue WHERE id = ?1", params![item.id])
        .map_err(|e| format!("remove sent prompt: {e}"))?;
    persist_actor_snapshot(&conversation_id)?;

    serde_json::to_value(run).map_err(|e| format!("serialize started run: {e}"))
}
