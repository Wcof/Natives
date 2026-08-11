//! Conversation hard-delete + token folding into durable `usage_stats`.
//!
//! Split out of `conversation_store` to keep each file under 1000 lines (W10).
//! Shared helpers (`store`, `id_param`) come from the parent via `super::*`.

use super::*;

pub(crate) async fn delete(params: Value) -> Result<Value, String> {
    let id = id_param(&params)?.to_string();
    // Collect this conversation + hidden children so we hard-delete the whole tree.
    let mut conversation_ids: Vec<String> = vec![id.clone()];
    if let Ok(sessions) = crate::subagent_store::list_subagent_sessions(Some(&id), true) {
        for sess in sessions {
            if !conversation_ids
                .iter()
                .any(|c| c == &sess.child_conversation_id)
            {
                conversation_ids.push(sess.child_conversation_id.clone());
            }
        }
    }

    // Cancel every live run under the tree before rows disappear.
    for cid in &conversation_ids {
        for run in crate::global_run_manager()
            .list_runs(Some(cid))
            .into_iter()
            .filter(|run| run.status.is_active())
        {
            let _ = crate::global_run_manager()
                .cancel(assistant_protocol::v2::CancelRunRequest { run_id: run.id })
                .await;
        }
    }
    if let Ok(sessions) = crate::subagent_store::list_subagent_sessions(Some(&id), true) {
        for sess in sessions {
            let _ = crate::global_run_manager()
                .runtime
                .kill_task(&sess.id)
                .await;
            let _ = crate::subagent_store::close_subagent_session(
                &sess.id,
                "cancelled",
                Some("parent conversation deleted"),
            );
        }
    }

    // Fold remaining token totals into durable usage_stats *before* CASCADE removes
    // run/message rows. usage_stats is date/model aggregate billing — no conversation content.
    let mut usage_rows_folded = 0u64;
    {
        let store = store()?;
        let conn = store.conn()?;
        for cid in &conversation_ids {
            usage_rows_folded += fold_conversation_tokens_into_usage_stats(&conn, cid)?;
        }
        // Hard-delete: conversation content gone; billing stays in usage_stats.
        // CASCADE clears messages/runs/events/subagent_session/route_policy/children.
        for cid in conversation_ids.iter().rev() {
            // Children first is not required with CASCADE from parent, but deleting each
            // id is idempotent and covers orphan child rows without parent FK path.
            let _ = conn.execute("DELETE FROM conversation WHERE id = ?1", params![cid]);
        }
        // Ensure root is gone even if children-only path ran.
        let changed = conn
            .execute("DELETE FROM conversation WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        if changed == 0 {
            // Already deleted is success (idempotent).
            let still: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM conversation WHERE id = ?1)",
                    params![id],
                    |row| row.get(0),
                )
                .unwrap_or(false);
            if still {
                return Err("conversation not found".into());
            }
        }
    }

    // Drop in-memory run rows for deleted conversations (best-effort).
    crate::global_run_manager().forget_conversations(&conversation_ids);

    Ok(serde_json::json!({
        "deleted": true,
        "hard_deleted": true,
        "usage_stats_preserved": true,
        "usage_rows_folded": usage_rows_folded,
    }))
}

/// Snapshot token totals for a conversation into `usage_stats` (billing-only aggregate).
/// Does not store messages, titles, or prompts.
fn fold_conversation_tokens_into_usage_stats(
    conn: &rusqlite::Connection,
    conversation_id: &str,
) -> Result<u64, String> {
    let _ = conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS usage_stats (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            date TEXT NOT NULL,
            source TEXT NOT NULL,
            source_path TEXT,
            model TEXT NOT NULL,
            input_tokens INTEGER NOT NULL DEFAULT 0,
            output_tokens INTEGER NOT NULL DEFAULT 0,
            cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
            cache_read_tokens INTEGER NOT NULL DEFAULT 0,
            request_count INTEGER NOT NULL DEFAULT 0,
            cost_usd REAL NOT NULL DEFAULT 0.0,
            UNIQUE(date, source, model)
        );",
    );

    // Prefer run-level totals (already projected from usage_updated events).
    // date: first 10 chars of RFC3339 / sqlite datetime → YYYY-MM-DD.
    let mut stmt = conn
        .prepare(
            "SELECT COALESCE(NULLIF(TRIM(model_id), ''), 'unknown'),
                    substr(COALESCE(started_at, created_at, datetime('now')), 1, 10),
                    COALESCE(SUM(COALESCE(total_input_tokens, 0)), 0),
                    COALESCE(SUM(COALESCE(total_output_tokens, 0)), 0),
                    COUNT(*)
             FROM run
             WHERE conversation_id = ?1
             GROUP BY 1, 2",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![conversation_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    let mut folded = 0u64;
    for row in rows.flatten() {
        let (model, date, input, output, request_count) = row;
        if input == 0 && output == 0 {
            continue;
        }
        conn.execute(
            "INSERT INTO usage_stats
                (date, source, source_path, model, input_tokens, output_tokens,
                 cache_creation_tokens, cache_read_tokens, request_count, cost_usd)
             VALUES (?1, 'natives', 'conversation.delete', ?2, ?3, ?4, 0, 0, ?5, 0.0)
             ON CONFLICT(date, source, model) DO UPDATE SET
                input_tokens = input_tokens + excluded.input_tokens,
                output_tokens = output_tokens + excluded.output_tokens,
                request_count = request_count + excluded.request_count",
            params![date, model, input, output, request_count],
        )
        .map_err(|e| e.to_string())?;
        folded += 1;
    }
    Ok(folded)
}
