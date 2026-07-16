use crate::db;
use crate::Result;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub id: String,
    pub path: String,
    pub label: String,
    pub conversation_count: i64,
    pub exists: bool,
}

/// List all registered projects with conversation counts.
#[tauri::command]
pub fn project_list() -> Result<Vec<ProjectInfo>> {
    let conn = db::get_assistant_db_conn().map_err(|e| e.to_string())?;

    let mut stmt = conn
        .prepare(
            "SELECT p.id, p.path, p.label,
                (SELECT COUNT(*) FROM assistant_conversations c WHERE c.project_id = p.path AND c.archived_at IS NULL)
             FROM assistant_projects p
             ORDER BY p.last_opened_at DESC, p.label COLLATE NOCASE",
        )
        .map_err(|e| format!("Failed to prepare project list query: {e}"))?;

    let projects = stmt
        .query_map([], |row| {
            let path: String = row.get(1)?;
            let label: String = row.get(2)?;
            let conv_count: i64 = row.get(3)?;
            let exists = Path::new(&path).exists();
            Ok(ProjectInfo {
                id: row.get(0)?,
                path,
                label,
                conversation_count: conv_count,
                exists,
            })
        })
        .map_err(|e| format!("Failed to query projects: {e}"))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to collect projects: {e}"))?;

    Ok(projects)
}

/// Register a project path.
#[tauri::command]
pub fn project_register(path: String) -> Result<ProjectInfo> {
    let trimmed = path.trim().to_string();
    if trimmed.is_empty() {
        return Err("Project path cannot be empty".into());
    }

    // Expand ~ to home directory
    let expanded = if trimmed.starts_with('~') {
        if let Some(home) = dirs::home_dir() {
            trimmed.replacen('~', &home.to_string_lossy(), 1)
        } else {
            trimmed.clone()
        }
    } else {
        trimmed.clone()
    };

    // Check the path exists and is a directory
    let path_ref = Path::new(&expanded);
    if !path_ref.exists() {
        return Err("Project path does not exist".into());
    }
    if !path_ref.is_dir() {
        return Err("Project path is not a directory".into());
    }

    // Canonicalize to get the absolute, normalized path
    let canonical = path_ref.canonicalize()
        .map_err(|e| format!("Failed to canonicalize path: {e}"))?;
    let canonical_str = canonical.to_string_lossy().to_string();

    let label = canonical
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| canonical_str.clone());

    let now = chrono::Utc::now().to_rfc3339();
    let conn = db::get_assistant_db_conn().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO assistant_projects (id, path, label, created_at, last_opened_at)
         VALUES (?1, ?2, ?3, ?4, ?4)
         ON CONFLICT(path) DO UPDATE SET label = excluded.label, last_opened_at = excluded.last_opened_at",
        rusqlite::params![canonical_str, canonical_str, label, now],
    ).map_err(|e| format!("Failed to register project: {e}"))?;
    Ok(ProjectInfo {
        id: canonical_str.clone(),
        path: canonical_str,
        label,
        conversation_count: 0,
        exists: true,
    })
}

/// Open a project directory in the file browser / terminal.
/// For now this is a no-op placeholder; the actual directory navigation
/// is handled by the frontend ShellLayout/ContentArea.
#[tauri::command]
pub fn project_open(id: String) -> Result<()> {
    // Currently a no-op — the frontend navigates to the project directory
    // via its own file-browser state. This command exists for future expansion.
    let _ = id;
    Ok(())
}

/// Remove a project registration without deleting its conversations.
/// Existing conversations are moved to the explicit unassigned group.
#[tauri::command]
pub fn project_remove(id: String) -> Result<()> {
    let mut conn = db::get_assistant_db_conn().map_err(|e| e.to_string())?;
    let transaction = conn.transaction()
        .map_err(|e| format!("Failed to start project removal: {e}"))?;
    transaction.execute(
        "UPDATE assistant_conversations SET project_id = NULL WHERE project_id = ?1",
        rusqlite::params![id],
    ).map_err(|e| format!("Failed to unassign project conversations: {e}"))?;
    transaction.execute(
        "DELETE FROM assistant_projects WHERE id = ?1 OR path = ?1",
        rusqlite::params![id],
    ).map_err(|e| format!("Failed to remove project: {e}"))?;
    transaction.commit()
        .map_err(|e| format!("Failed to commit project removal: {e}"))?;
    Ok(())
}
