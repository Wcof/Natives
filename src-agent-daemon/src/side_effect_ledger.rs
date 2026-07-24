//! Side-effect ledger for rewind coverage (Phase 4 / task-07).
//!
//! Records whether a run's tool effects are checkpoint-covered, external-only,
//! or unknown — used by workspace.restorePreview.

use serde_json::Value;

/// Record a tool side-effect after execution.
pub fn record_tool_effect(
    run_id: &str,
    tool_name: &str,
    category: &str,
    summary: &Value,
    reversible: bool,
    checkpoint_id: Option<&str>,
) -> Result<(), String> {
    let store = crate::run_manager::global_run_manager()
        .data_store_ref()
        .ok_or_else(|| "no data store for side_effect_record".to_string())?;
    let conn = store.conn()?;
    let id = uuid::Uuid::new_v4().to_string();
    let summary_s = serde_json::to_string(summary).unwrap_or_else(|_| "{}".into());
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

/// Aggregate coverage label for restore preview.
pub fn coverage_for_run(run_id: &str) -> Option<String> {
    let store = crate::run_manager::global_run_manager().data_store_ref()?;
    let conn = store.conn().ok()?;
    let mut stmt = conn
        .prepare("SELECT category FROM side_effect_record WHERE run_id = ?1")
        .ok()?;
    let cats: Vec<String> = stmt
        .query_map(rusqlite::params![run_id], |r| r.get(0))
        .ok()?
        .filter_map(|r| r.ok())
        .collect();
    if cats.is_empty() {
        return Some("unknown".into());
    }
    let has_external = cats.iter().any(|c| {
        matches!(
            c.as_str(),
            "process" | "network" | "mcp" | "external" | "git" | "database"
        )
    });
    let has_workspace = cats.iter().any(|c| c == "workspace_file");
    if has_external && has_workspace {
        Some("partial".into())
    } else if has_external {
        Some("external_only".into())
    } else if has_workspace {
        Some("checkpoint".into())
    } else {
        Some("unknown".into())
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
