//! Side-effect ledger for rewind coverage (Phase 4 / task-07).
//!
//! Records whether a run's tool effects are checkpoint-covered, external-only,
//! or unknown — used by workspace.restorePreview.

use serde_json::Value;

fn ledger_store() -> Result<std::sync::Arc<crate::storage::DataStore>, String> {
    if let Some(store) = crate::run_manager::global_run_manager().data_store_ref() {
        return Ok(store);
    }
    #[cfg(test)]
    {
        use std::sync::OnceLock;
        static TEST_STORE: OnceLock<std::sync::Arc<crate::storage::DataStore>> = OnceLock::new();
        let store = TEST_STORE.get_or_init(|| {
            let db = std::env::temp_dir().join(format!(
                "natives-side-effect-ledger-test-{}.db",
                std::process::id()
            ));
            let artifacts = db.with_extension("artifacts");
            std::sync::Arc::new(
                crate::storage::DataStore::new(&db, &artifacts)
                    .expect("test side-effect ledger store must migrate"),
            )
        });
        return Ok(store.clone());
    }
    #[allow(unreachable_code)]
    Err("no data store for side_effect_record".to_string())
}

/// Record a tool side-effect after execution.
pub fn record_tool_effect(
    run_id: &str,
    tool_name: &str,
    category: &str,
    summary: &Value,
    reversible: bool,
    checkpoint_id: Option<&str>,
) -> Result<(), String> {
    let store = ledger_store()?;
    let conn = store.conn()?;
    let id = uuid::Uuid::new_v4().to_string();
    let summary_s = serde_json::to_string(summary)
        .map_err(|error| format!("serialize side-effect summary: {error}"))?;
    let target = summary
        .get("path")
        .or_else(|| summary.get("command"))
        .or_else(|| summary.get("url"))
        .and_then(|v| v.as_str())
        .unwrap_or(tool_name);
    conn.execute(
        "INSERT INTO side_effect_record
         (id, run_id, tool_call_id, category, target_summary, reversible, checkpoint_id, coverage_note, created_at)
         VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, datetime('now'))",
        rusqlite::params![
            id,
            run_id,
            category,
            target,
            if reversible { 1 } else { 0 },
            checkpoint_id,
            summary_s,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Record the execution state with the call identity used by Core.  Unknown
/// completion is deliberately durable: resume code must block rather than
/// replaying a side effect it cannot prove safe.
pub fn record_tool_effect_state(
    run_id: &str,
    tool_call_id: &str,
    tool_name: &str,
    category: &str,
    status: &str,
    replay_safe: bool,
    turn_id: Option<&str>,
    summary: &Value,
) -> Result<(), String> {
    let store = ledger_store()?;
    let conn = store.conn()?;
    let target = summary
        .get("path")
        .or_else(|| summary.get("command"))
        .or_else(|| summary.get("url"))
        .and_then(Value::as_str)
        .unwrap_or(tool_name);
    let now = chrono::Utc::now().to_rfc3339();
    let terminal = matches!(status, "completed" | "failed" | "cancelled" | "uncertain");
    let changed = conn
        .execute(
            "UPDATE side_effect_record
             SET category = ?1, target_summary = ?2, coverage_note = ?3,
                 turn_id = ?4, side_effect_class = ?5, status = ?6,
                 replay_safe = ?7, resource = ?8,
                 completed_at = CASE WHEN ?9 THEN ?10 ELSE completed_at END
             WHERE run_id = ?11 AND tool_call_id = ?12
               AND (status NOT IN ('cancelled', 'uncertain') OR ?6 = 'uncertain')",
            rusqlite::params![
                category,
                target,
                serde_json::to_string(summary)
                    .map_err(|error| format!("serialize side-effect summary: {error}"))?,
                turn_id,
                category,
                status,
                if replay_safe { 1 } else { 0 },
                target,
                terminal,
                now,
                run_id,
                tool_call_id,
            ],
        )
        .map_err(|e| e.to_string())?;
    if changed > 0 {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO side_effect_record
         (id, run_id, tool_call_id, category, target_summary, reversible, coverage_note,
          turn_id, side_effect_class, status, replay_safe, idempotency_key, external_reference,
          resource, started_at, completed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7, ?8, ?9, ?10, NULL, NULL, ?11, ?12,
                 CASE WHEN ?13 THEN ?12 ELSE NULL END)",
        rusqlite::params![
            uuid::Uuid::new_v4().to_string(),
            run_id,
            tool_call_id,
            category,
            target,
            serde_json::to_string(summary)
                .map_err(|error| format!("serialize side-effect summary: {error}"))?,
            turn_id,
            category,
            status,
            if replay_safe { 1 } else { 0 },
            target,
            now,
            terminal,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Aggregate coverage label for restore preview.
pub fn coverage_for_run(run_id: &str) -> Result<String, String> {
    let store = ledger_store()?;
    let conn = store.conn()?;
    let mut stmt = conn
        .prepare("SELECT category FROM side_effect_record WHERE run_id = ?1")
        .map_err(|e| e.to_string())?;
    let cats: Vec<String> = stmt
        .query_map(rusqlite::params![run_id], |r| r.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    if cats.is_empty() {
        return Ok("unknown".into());
    }
    let has_external = cats.iter().any(|c| {
        matches!(
            c.as_str(),
            "process" | "network" | "mcp" | "external" | "git" | "database"
        )
    });
    let has_workspace = cats.iter().any(|c| c == "workspace_file");
    if has_external && has_workspace {
        Ok("partial".into())
    } else if has_external {
        Ok("external_only".into())
    } else if has_workspace {
        Ok("checkpoint".into())
    } else {
        Ok("unknown".into())
    }
}

pub fn category_for_tool(tool_name: &str) -> &'static str {
    match tool_name {
        "write_file" | "apply_patch" | "edit_file" => "workspace_file",
        "run_terminal" | "bash" => "process",
        "web_fetch" | "fetch" => "network",
        name if name == "mcp_call" || name.starts_with("mcp__") => "mcp",
        _ => "external",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_mapping() {
        assert_eq!(category_for_tool("write_file"), "workspace_file");
        assert_eq!(category_for_tool("run_terminal"), "process");
        assert_eq!(category_for_tool("mcp_call"), "mcp");
    }
}
