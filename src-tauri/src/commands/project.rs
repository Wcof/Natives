use crate::db;
use crate::{Error, Result};
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
    let conn = db::get_assistant_db_conn()
        .map_err(|e| Error::Internal(format!("DB connection: {e}")))?;

    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT
                s.project_id,
                s.project_id,
                COALESCE(
                    (SELECT COUNT(*) FROM assistant_sessions s2 WHERE s2.project_id = s.project_id),
                    0
                ) as conv_count
             FROM assistant_sessions s
             WHERE s.project_id IS NOT NULL AND s.project_id != ''
             ORDER BY s.project_id",
        )
        .map_err(|e| Error::Internal(format!("Failed to prepare project list query: {e}")))?;

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
        .map_err(|e| Error::Internal(format!("Failed to query projects: {e}")))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| Error::Internal(format!("Failed to collect projects: {e}")))?;

    Ok(projects)
}

/// Register a project path.
#[tauri::command]
pub fn project_register(path: String) -> Result<ProjectInfo> {
    let path = path.trim().to_string();
    if path.is_empty() {
        return Err(Error::InvalidInput("Project path cannot be empty".into()));
    }

    let label = path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(&path)
        .to_string();

    Ok(ProjectInfo {
        id: path.clone(),
        path,
        label,
        conversation_count: 0,
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
