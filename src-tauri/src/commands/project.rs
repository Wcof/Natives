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
    pub last_opened_at: String,
}

/// List all registered projects with conversation counts.
#[tauri::command]
pub fn project_list() -> Result<Vec<ProjectInfo>> {
    let conn = db::get_assistant_db_conn().map_err(|e| e.to_string())?;

    let mut stmt = conn
        .prepare(
            "SELECT p.id, p.path, p.label,
                (SELECT COUNT(*) FROM assistant_conversations c WHERE c.project_id = p.path AND c.archived_at IS NULL),
                p.last_opened_at
             FROM assistant_projects p
             WHERE p.deleted_at IS NULL
             ORDER BY p.last_opened_at DESC, p.label COLLATE NOCASE",
        )
        .map_err(|e| format!("Failed to prepare project list query: {e}"))?;

    let projects = stmt
        .query_map([], |row| {
            let path: String = row.get(1)?;
            let label: String = row.get(2)?;
            let conv_count: i64 = row.get(3)?;
            let last_opened_at: String = row.get(4)?;
            let exists = Path::new(&path).exists();
            Ok(ProjectInfo {
                id: row.get(0)?,
                path,
                label,
                conversation_count: conv_count,
                exists,
                last_opened_at,
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
    let canonical = path_ref
        .canonicalize()
        .map_err(|e| format!("Failed to canonicalize path: {e}"))?;
    let canonical_str = canonical.to_string_lossy().to_string();

    let label = canonical
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| canonical_str.clone());

    let now = chrono::Utc::now().to_rfc3339();
    let conn = db::get_assistant_db_conn().map_err(|e| e.to_string())?;

    // Check if a soft-deleted project exists with the same path — restore it.
    let was_deleted: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM assistant_projects WHERE path = ?1 AND deleted_at IS NOT NULL",
            rusqlite::params![canonical_str],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;

    if was_deleted {
        // Restore the soft-deleted project: clear deleted_at, update label and timestamp.
        conn.execute(
            "UPDATE assistant_projects SET deleted_at = NULL, label = ?1, last_opened_at = ?2 WHERE path = ?3",
            rusqlite::params![label, now, canonical_str],
        ).map_err(|e| format!("Failed to restore project: {e}"))?;
    } else {
        conn.execute(
            "INSERT INTO assistant_projects (id, path, label, created_at, last_opened_at)
             VALUES (?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(path) DO UPDATE SET label = excluded.label, last_opened_at = excluded.last_opened_at, deleted_at = NULL",
            rusqlite::params![canonical_str, canonical_str, label, now],
        ).map_err(|e| format!("Failed to register project: {e}"))?;
    }
    Ok(ProjectInfo {
        id: canonical_str.clone(),
        path: canonical_str,
        label,
        conversation_count: 0,
        exists: true,
        last_opened_at: now,
    })
}

/// Open a project directory in the file browser / terminal.
/// For now this is a no-op placeholder; the actual directory navigation
/// is handled by the frontend ShellLayout/ContentArea.
#[tauri::command]
pub fn project_open(id: String) -> Result<()> {
    // Touch last_opened_at so assistant project order follows real opens.
    let now = chrono::Utc::now().to_rfc3339();
    let conn = db::get_assistant_db_conn().map_err(|e| e.to_string())?;
    let _ = conn.execute(
        "UPDATE assistant_projects SET last_opened_at = ?1 WHERE id = ?2 OR path = ?2",
        rusqlite::params![now, id],
    );
    Ok(())
}

/// Rename only the assistant-side project label. The source directory is never moved.
#[tauri::command]
pub fn project_rename(id: String, label: String) -> Result<()> {
    let label = label.trim();
    if label.is_empty() {
        return Err("Project name cannot be empty".into());
    }
    let conn = db::get_assistant_db_conn().map_err(|e| e.to_string())?;
    let changed = conn
        .execute(
            "UPDATE assistant_projects SET label = ?1 WHERE (id = ?2 OR path = ?2) AND deleted_at IS NULL",
            rusqlite::params![label, id],
        )
        .map_err(|e| format!("Failed to rename project: {e}"))?;
    if changed == 0 {
        return Err("Project not found".into());
    }
    Ok(())
}

/// Soft-remove a project registration (logical delete).
/// Sessions keep their project_id so they reappear when the project is re-added.
#[tauri::command]
pub fn project_remove(id: String) -> Result<()> {
    let conn = db::get_assistant_db_conn().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE assistant_projects SET deleted_at = ?1 WHERE (id = ?2 OR path = ?2) AND deleted_at IS NULL",
        rusqlite::params![now, id],
    ).map_err(|e| format!("Failed to remove project: {e}"))?;
    Ok(())
}
