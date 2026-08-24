//! Workspace session tabs persistence operations.

use rusqlite::{Connection, OptionalExtension};

use crate::workspace::store::{get_workspace, now_rfc3339};
use crate::workspace::types::{WorkspaceOpenTab, WorkspaceSummary};
use crate::{Error, Result};

#[allow(dead_code)]
pub fn activate_workspace_session(
    conn: &Connection,
    workspace_id: &str,
) -> Result<Option<WorkspaceSummary>> {
    if get_workspace(conn, workspace_id)?.is_none() {
        return Ok(None);
    }
    let is_open: bool = conn
        .query_row(
            "SELECT 1 FROM workspace_open_tabs WHERE workspace_id = ?1",
            [workspace_id],
            |_| Ok(()),
        )
        .optional()
        .map_err(Error::Database)?
        .is_some();
    if !is_open {
        return Err(Error::InvalidInput(format!(
            "cannot activate workspace {workspace_id}: it has no open session tab; open it first"
        )));
    }
    let now = now_rfc3339();
    let tx = conn.unchecked_transaction().map_err(Error::Database)?;
    tx.execute(
        "UPDATE workspaces SET is_active = 0, updated_at = ?1",
        [&now],
    )
    .map_err(Error::Database)?;
    tx.execute(
        "UPDATE workspaces SET is_active = 1, updated_at = ?1 WHERE id = ?2 AND deleted_at IS NULL",
        rusqlite::params![&now, workspace_id],
    )
    .map_err(Error::Database)?;
    tx.execute(
        "UPDATE workspace_open_tabs SET last_active_at = ?1 WHERE workspace_id = ?2",
        rusqlite::params![&now, workspace_id],
    )
    .map_err(Error::Database)?;
    tx.commit().map_err(Error::Database)?;
    get_workspace(conn, workspace_id)
}

pub fn list_open_tabs(conn: &Connection) -> Result<Vec<WorkspaceOpenTab>> {
    let mut stmt = conn
        .prepare(
            "SELECT t.workspace_id, t.sort_order, t.is_pinned, t.opened_at, t.last_active_at
             FROM workspace_open_tabs t JOIN workspaces w ON w.id = t.workspace_id
             WHERE w.deleted_at IS NULL ORDER BY t.is_pinned DESC, t.sort_order ASC, t.opened_at ASC",
        )
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(WorkspaceOpenTab {
                workspace_id: row.get(0)?,
                sort_order: row.get(1)?,
                is_pinned: row.get::<_, i64>(2)? != 0,
                opened_at: row.get(3)?,
                last_active_at: row.get(4)?,
            })
        })
        .map_err(Error::Database)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::Database)
}

pub fn open_workspace_tab(conn: &Connection, workspace_id: &str) -> Result<bool> {
    if get_workspace(conn, workspace_id)?.is_none() {
        return Ok(false);
    }
    let now = now_rfc3339();
    let next: f64 = conn
        .query_row(
            "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM workspace_open_tabs",
            [],
            |r| r.get(0),
        )
        .map_err(Error::Database)?;
    conn.execute(
        "INSERT INTO workspace_open_tabs (workspace_id, sort_order, is_pinned, opened_at, last_active_at)
         VALUES (?1, ?2, 0, ?3, ?3)
         ON CONFLICT(workspace_id) DO UPDATE SET last_active_at = excluded.last_active_at",
        rusqlite::params![workspace_id, next, now],
    ).map_err(Error::Database)?;
    Ok(true)
}

pub fn close_workspace_tab(conn: &Connection, workspace_id: &str) -> Result<bool> {
    let tx = conn.unchecked_transaction().map_err(Error::Database)?;
    let changed = tx
        .execute(
            "DELETE FROM workspace_open_tabs WHERE workspace_id = ?1",
            [workspace_id],
        )
        .map_err(Error::Database)?;
    let was_active: bool = tx
        .query_row(
            "SELECT is_active FROM workspaces WHERE id=?1",
            [workspace_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(Error::Database)?
        .unwrap_or(0)
        != 0;
    if was_active {
        tx.execute(
            "UPDATE workspaces SET is_active=0 WHERE id=?1",
            [workspace_id],
        )
        .map_err(Error::Database)?;
        let fallback: Option<String> = tx.query_row("SELECT workspace_id FROM workspace_open_tabs ORDER BY is_pinned DESC,sort_order ASC LIMIT 1", [], |row| row.get(0)).optional().map_err(Error::Database)?;
        if let Some(id) = fallback {
            tx.execute(
                "UPDATE workspaces SET is_active=1,updated_at=?1 WHERE id=?2",
                rusqlite::params![now_rfc3339(), id],
            )
            .map_err(Error::Database)?;
        }
    }
    tx.commit().map_err(Error::Database)?;
    Ok(changed > 0)
}

pub fn reorder_workspace_tabs(
    conn: &Connection,
    ordered_ids: &[String],
) -> Result<Vec<WorkspaceOpenTab>> {
    let tx = conn.unchecked_transaction().map_err(Error::Database)?;
    for (index, id) in ordered_ids.iter().enumerate() {
        tx.execute(
            "UPDATE workspace_open_tabs SET sort_order = ?1 WHERE workspace_id = ?2",
            rusqlite::params![index as f64, id],
        )
        .map_err(Error::Database)?;
    }
    tx.commit().map_err(Error::Database)?;
    list_open_tabs(conn)
}
