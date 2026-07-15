use crate::db;
use crate::Result;
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub id: String,
    pub path: String,
    pub label: String,
    pub conversation_count: i64,
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
            let conv_count: i64 = row.get(2)?;
            let label = path
                    .trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .unwrap_or(&path)
                    .to_string();
            Ok(ProjectInfo {
                id: row.get(0)?,
                path,
                label,
                conversation_count: conv_count,
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
    let path = path.trim().to_string();
    if path.is_empty() {
        return Err("Project path cannot be empty".into());
    }
    let canonical = std::fs::canonicalize(&path)
        .map_err(|_| "Project directory does not exist".to_string())?;
    if !canonical.is_dir() {
        return Err("Project path must be a directory".into());
    }
    let path = canonical.to_string_lossy().to_string();

    let label = path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(&path)
        .to_string();

    let now = chrono::Utc::now().to_rfc3339();
    let conn = db::get_assistant_db_conn().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO assistant_projects (id, path, label, created_at, last_opened_at)
         VALUES (?1, ?2, ?3, ?4, ?4)
         ON CONFLICT(path) DO UPDATE SET label = excluded.label, last_opened_at = excluded.last_opened_at",
        rusqlite::params![path, path, label, now],
    ).map_err(|e| format!("Failed to register project: {e}"))?;
    Ok(ProjectInfo { id: path.clone(), path, label, conversation_count: 0 })
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

/// Remove a project registration (does not delete conversations).
#[tauri::command]
pub fn project_remove(id: String) -> Result<()> {
    // For now this is a metadata-only operation: the project row in
    // the assistant DB exists implicitly via session.project_id = id.
    // We do NOT delete the sessions — the plan says to keep DB tables
    // for rollback. Future iterations may add a explicit project registry table.
    let _ = id;
    Ok(())
}
