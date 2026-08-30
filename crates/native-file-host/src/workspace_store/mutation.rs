use rusqlite::Connection;
use serde_json::Value;

use super::query::{global_revision, query_snapshot, workspace_revision};
use super::schema::{
    table_has_column, uuid_v4, validate_background_key, validate_json_size, validate_settings_key,
    validate_widget_record, MAX_WIDGETS_PER_WORKSPACE,
};
use super::types::{StoreResult, TemplatePayload, WidgetRecord, WorkspaceError};

fn check_revision(expected: Option<i64>, actual: i64) -> StoreResult<()> {
    match expected {
        Some(expected) if expected == actual => Ok(()),
        Some(expected) => Err(WorkspaceError::RevisionConflict { expected, actual }),
        None => Err(WorkspaceError::InvalidInput(
            "expectedRevision is required for all workspace mutations".into(),
        )),
    }
}

fn bump_workspace(conn: &Connection, id: &str) -> StoreResult<i64> {
    conn.execute(
        "UPDATE workspaces SET revision = revision + 1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1",
        [id],
    )?;
    workspace_revision(conn, id)
}

pub fn insert_widgets(
    conn: &Connection,
    workspace_id: &str,
    widgets: &[WidgetRecord],
) -> StoreResult<()> {
    for (index, widget) in widgets.iter().enumerate() {
        validate_widget_record(widget)?;
        let widget_id = uuid_v4();
        let legacy = table_has_column(conn, "workspace_widgets", "widget_type")?;
        conn.execute(
            if legacy {
                "INSERT INTO workspace_widgets (id, workspace_id, widget_type, key, \"order\", position, enabled, config_json, display_json, appearance_json, created_at, updated_at) VALUES (?1, ?2, ?3, ?3, ?4, ?4, ?5, ?6, ?7, ?7, strftime('%Y-%m-%dT%H:%M:%fZ','now'), strftime('%Y-%m-%dT%H:%M:%fZ','now'))"
            } else {
                "INSERT INTO workspace_widgets (id, workspace_id, key, \"order\", enabled, config_json, display_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"
            },
            rusqlite::params![
                widget_id,
                workspace_id,
                widget.key,
                widget.order.max(index as i64),
                widget.enabled as i64,
                widget.config_json.to_string(),
                widget.display_json.to_string()
            ],
        )?;
    }
    Ok(())
}

pub fn mutate_create(conn: &Connection, name: &str, template: Option<&str>) -> StoreResult<String> {
    if name.trim().is_empty() || name.len() > 120 {
        return Err(WorkspaceError::InvalidInput(
            "workspace name must be 1..=120 chars".into(),
        ));
    }
    let template_key = template.unwrap_or("classic");
    let payload = super::import::builtin_template_payload(template_key)?;
    mutate_create_from_payload(conn, name, &payload, template_key)
}

pub fn mutate_create_from_payload(
    conn: &Connection,
    name: &str,
    payload: &TemplatePayload,
    template_source: &str,
) -> StoreResult<String> {
    if name.trim().is_empty() || name.len() > 120 {
        return Err(WorkspaceError::InvalidInput(
            "workspace name must be 1..=120 chars".into(),
        ));
    }
    let id = uuid_v4();
    conn.execute(
        "INSERT INTO workspaces (id, name, sort_order, background_json, template_source, created_at, updated_at) VALUES (?1, ?2, (SELECT COALESCE(MAX(sort_order),0)+1 FROM workspaces WHERE deleted_at IS NULL), ?3, ?4, strftime('%Y-%m-%dT%H:%M:%fZ','now'), strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        rusqlite::params![id, name.trim(), payload.background_json.to_string(), template_source],
    )?;
    conn.execute(
        "INSERT INTO workspace_open_tabs (workspace_id, opened_at, last_active_at) VALUES (?1, strftime('%Y-%m-%dT%H:%M:%fZ','now'), strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        [&id],
    )?;
    insert_widgets(conn, &id, &payload.widgets)?;
    conn.execute(
        "UPDATE workspace_meta SET revision = revision + 1, active_workspace_id = ?1 WHERE id = 1",
        [&id],
    )?;
    Ok(id)
}

pub fn mutate_rename(
    conn: &Connection,
    id: &str,
    name: &str,
    expected_revision: Option<i64>,
) -> StoreResult<()> {
    if name.trim().is_empty() || name.len() > 120 {
        return Err(WorkspaceError::InvalidInput(
            "workspace name must be 1..=120 chars".into(),
        ));
    }
    let actual = workspace_revision(conn, id)?;
    check_revision(expected_revision, actual)?;
    conn.execute(
        "UPDATE workspaces SET name = ?1, revision = revision + 1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?2",
        rusqlite::params![name.trim(), id],
    )?;
    Ok(())
}

pub fn mutate_reorder(
    conn: &Connection,
    ordered_ids: &[String],
    expected_revision: Option<i64>,
) -> StoreResult<()> {
    let actual = global_revision(conn)?;
    check_revision(expected_revision, actual)?;
    for (index, id) in ordered_ids.iter().enumerate() {
        conn.execute(
            "UPDATE workspaces SET sort_order = ?1, revision = revision + 1 WHERE id = ?2 AND deleted_at IS NULL",
            rusqlite::params![index as i64, id],
        )?;
    }
    conn.execute(
        "UPDATE workspace_meta SET revision = revision + 1 WHERE id = 1",
        [],
    )?;
    Ok(())
}

pub fn mutate_pin(
    conn: &Connection,
    id: &str,
    pinned: bool,
    expected_revision: Option<i64>,
) -> StoreResult<()> {
    let actual = workspace_revision(conn, id)?;
    check_revision(expected_revision, actual)?;
    conn.execute(
        "UPDATE workspaces SET pinned = ?1, revision = revision + 1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?2",
        rusqlite::params![pinned as i64, id],
    )?;
    Ok(())
}

pub fn mutate_duplicate(
    conn: &Connection,
    id: &str,
    expected_revision: Option<i64>,
) -> StoreResult<String> {
    let actual = workspace_revision(conn, id)?;
    check_revision(expected_revision, actual)?;
    let (name, background_json, template_source) = conn.query_row(
        "SELECT name, background_json, template_source FROM workspaces WHERE id = ?1 AND deleted_at IS NULL",
        [id],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?)),
    )?;
    let new_id = uuid_v4();
    conn.execute(
        "INSERT INTO workspaces (id, name, sort_order, background_json, template_source, created_at, updated_at) VALUES (?1, ?2, (SELECT COALESCE(MAX(sort_order),0)+1 FROM workspaces WHERE deleted_at IS NULL), ?3, ?4, strftime('%Y-%m-%dT%H:%M:%fZ','now'), strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        rusqlite::params![new_id, format!("{name} (copy)"), background_json, template_source],
    )?;
    conn.execute(
        "INSERT INTO workspace_open_tabs (workspace_id, opened_at, last_active_at) VALUES (?1, strftime('%Y-%m-%dT%H:%M:%fZ','now'), strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        [&new_id],
    )?;
    let mut statement = conn.prepare(
        "SELECT key, \"order\", enabled, config_json, display_json FROM workspace_widgets WHERE workspace_id = ?1 ORDER BY \"order\" ASC",
    )?;
    let rows = statement.query_map([id], |row| {
        Ok(WidgetRecord {
            id: String::new(),
            workspace_id: new_id.clone(),
            key: row.get(0)?,
            order: row.get(1)?,
            enabled: row.get::<_, i64>(2)? != 0,
            config_json: serde_json::from_str(&row.get::<_, String>(3)?).unwrap_or(Value::Null),
            display_json: serde_json::from_str(&row.get::<_, String>(4)?).unwrap_or(Value::Null),
        })
    })?;
    let mut cloned_widgets = Vec::new();
    for row in rows {
        cloned_widgets.push(row?);
    }
    insert_widgets(conn, &new_id, &cloned_widgets)?;
    conn.execute(
        "UPDATE workspace_meta SET revision = revision + 1, active_workspace_id = ?1 WHERE id = 1",
        [&new_id],
    )?;
    Ok(new_id)
}

pub fn mutate_delete(
    conn: &Connection,
    id: &str,
    expected_revision: Option<i64>,
) -> StoreResult<()> {
    let actual = workspace_revision(conn, id)?;
    check_revision(expected_revision, actual)?;
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM workspaces WHERE deleted_at IS NULL",
        [],
        |row| row.get(0),
    )?;
    if count <= 1 {
        return Err(WorkspaceError::InvalidInput(
            "at least one workspace is required".into(),
        ));
    }
    conn.execute(
        "UPDATE workspaces SET deleted_at = strftime('%Y-%m-%dT%H:%M:%fZ','now'), revision = revision + 1 WHERE id = ?1",
        [id],
    )?;
    conn.execute(
        "DELETE FROM workspace_open_tabs WHERE workspace_id = ?1",
        [id],
    )?;
    let active_id: Option<String> = conn
        .query_row(
            "SELECT active_workspace_id FROM workspace_meta WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .ok();
    if active_id.as_deref() == Some(id) {
        let next_id: Option<String> = conn
            .query_row(
                "SELECT workspace_id FROM workspace_open_tabs ORDER BY is_pinned DESC, sort_order ASC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .ok();
        conn.execute(
            "UPDATE workspace_meta SET revision = revision + 1, active_workspace_id = ?1 WHERE id = 1",
            [next_id],
        )?;
    } else {
        conn.execute(
            "UPDATE workspace_meta SET revision = revision + 1 WHERE id = 1",
            [],
        )?;
    }
    Ok(())
}

pub fn mutate_open_tab(conn: &Connection, id: &str) -> StoreResult<()> {
    workspace_revision(conn, id)?;
    conn.execute(
        "INSERT OR IGNORE INTO workspace_open_tabs (workspace_id, sort_order, opened_at, last_active_at) VALUES (?1, (SELECT COALESCE(MAX(sort_order),0)+1 FROM workspace_open_tabs), strftime('%Y-%m-%dT%H:%M:%fZ','now'), strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        [id],
    )?;
    conn.execute(
        "UPDATE workspace_open_tabs SET last_active_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE workspace_id = ?1",
        [id],
    )?;
    conn.execute(
        "UPDATE workspace_meta SET revision = revision + 1, active_workspace_id = ?1 WHERE id = 1",
        [id],
    )?;
    Ok(())
}

pub fn mutate_close_tab(conn: &Connection, id: &str) -> StoreResult<()> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM workspace_open_tabs", [], |row| {
        row.get(0)
    })?;
    if count <= 1 {
        return Err(WorkspaceError::InvalidInput(
            "at least one workspace must remain open".into(),
        ));
    }
    conn.execute(
        "DELETE FROM workspace_open_tabs WHERE workspace_id = ?1",
        [id],
    )?;
    let active_id: Option<String> = conn
        .query_row(
            "SELECT active_workspace_id FROM workspace_meta WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .ok();
    if active_id.as_deref() == Some(id) {
        let next_id: Option<String> = conn
            .query_row(
                "SELECT workspace_id FROM workspace_open_tabs ORDER BY is_pinned DESC, sort_order ASC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .ok();
        conn.execute(
            "UPDATE workspace_meta SET revision = revision + 1, active_workspace_id = ?1 WHERE id = 1",
            [next_id],
        )?;
    } else {
        conn.execute(
            "UPDATE workspace_meta SET revision = revision + 1 WHERE id = 1",
            [],
        )?;
    }
    Ok(())
}

pub fn mutate_reorder_tabs(
    conn: &Connection,
    ordered_ids: &[String],
    expected_revision: Option<i64>,
) -> StoreResult<()> {
    let actual = global_revision(conn)?;
    check_revision(expected_revision, actual)?;
    for (index, id) in ordered_ids.iter().enumerate() {
        conn.execute(
            "UPDATE workspace_open_tabs SET sort_order = ?1 WHERE workspace_id = ?2",
            rusqlite::params![index as i64, id],
        )?;
    }
    conn.execute(
        "UPDATE workspace_meta SET revision = revision + 1 WHERE id = 1",
        [],
    )?;
    Ok(())
}

pub fn mutate_widget_upsert(
    conn: &Connection,
    workspace_id: &str,
    widget: &WidgetRecord,
    expected_revision: Option<i64>,
) -> StoreResult<()> {
    validate_widget_record(widget)?;
    let actual = workspace_revision(conn, workspace_id)?;
    check_revision(expected_revision, actual)?;
    let widget_id = if widget.id.is_empty() {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM workspace_widgets WHERE workspace_id = ?1",
            [workspace_id],
            |row| row.get(0),
        )?;
        if count >= MAX_WIDGETS_PER_WORKSPACE as i64 {
            return Err(WorkspaceError::InvalidInput(format!(
                "workspace already has {count} widgets (max {MAX_WIDGETS_PER_WORKSPACE})"
            )));
        }
        uuid_v4()
    } else {
        let owner: Option<String> = conn
            .query_row(
                "SELECT workspace_id FROM workspace_widgets WHERE id = ?1",
                [&widget.id],
                |row| row.get(0),
            )
            .map(Some)
            .or_else(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })?;
        if let Some(owner) = owner {
            if owner != workspace_id {
                return Err(WorkspaceError::InvalidInput(format!(
                    "widget {} belongs to workspace {}, not {}",
                    widget.id, owner, workspace_id
                )));
            }
        }
        widget.id.clone()
    };
    let legacy = table_has_column(conn, "workspace_widgets", "widget_type")?;
    conn.execute(
        if legacy {
            "INSERT INTO workspace_widgets (id, workspace_id, widget_type, key, \"order\", position, enabled, config_json, display_json, appearance_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?3, ?4, ?4, ?5, ?6, ?7, ?7, strftime('%Y-%m-%dT%H:%M:%fZ','now'), strftime('%Y-%m-%dT%H:%M:%fZ','now'))
             ON CONFLICT(id) DO UPDATE SET widget_type = excluded.widget_type, key = excluded.key, \"order\" = excluded.\"order\", position = excluded.position, enabled = excluded.enabled,
               config_json = excluded.config_json, display_json = excluded.display_json, appearance_json = excluded.appearance_json, updated_at = excluded.updated_at
             WHERE workspace_widgets.workspace_id = excluded.workspace_id"
        } else {
            "INSERT INTO workspace_widgets (id, workspace_id, key, \"order\", enabled, config_json, display_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET key = excluded.key, \"order\" = excluded.\"order\", enabled = excluded.enabled,
               config_json = excluded.config_json, display_json = excluded.display_json
             WHERE workspace_widgets.workspace_id = excluded.workspace_id"
        },
        rusqlite::params![
            widget_id,
            workspace_id,
            widget.key,
            widget.order,
            widget.enabled as i64,
            widget.config_json.to_string(),
            widget.display_json.to_string()
        ],
    )?;
    bump_workspace(conn, workspace_id)?;
    Ok(())
}

pub fn mutate_widget_remove(
    conn: &Connection,
    workspace_id: &str,
    widget_id: &str,
    expected_revision: Option<i64>,
) -> StoreResult<()> {
    let actual = workspace_revision(conn, workspace_id)?;
    check_revision(expected_revision, actual)?;
    conn.execute(
        "DELETE FROM workspace_widgets WHERE id = ?1 AND workspace_id = ?2",
        rusqlite::params![widget_id, workspace_id],
    )?;
    bump_workspace(conn, workspace_id)?;
    Ok(())
}

pub fn mutate_widget_reorder(
    conn: &Connection,
    workspace_id: &str,
    ordered_ids: &[String],
    expected_revision: Option<i64>,
) -> StoreResult<()> {
    let actual = workspace_revision(conn, workspace_id)?;
    check_revision(expected_revision, actual)?;
    for (index, widget_id) in ordered_ids.iter().enumerate() {
        conn.execute(
            "UPDATE workspace_widgets SET \"order\" = ?1 WHERE id = ?2 AND workspace_id = ?3",
            rusqlite::params![index as i64, widget_id, workspace_id],
        )?;
    }
    bump_workspace(conn, workspace_id)?;
    Ok(())
}

pub fn mutate_background_save(
    conn: &Connection,
    workspace_id: &str,
    background_json: &Value,
    expected_revision: Option<i64>,
) -> StoreResult<()> {
    if let Some(key) = background_json.get("key").and_then(Value::as_str) {
        validate_background_key(key)?;
    } else {
        return Err(WorkspaceError::InvalidInput(
            "background must carry a key field".into(),
        ));
    }
    validate_json_size(background_json, "background")?;
    let actual = workspace_revision(conn, workspace_id)?;
    check_revision(expected_revision, actual)?;
    conn.execute(
        "UPDATE workspaces SET background_json = ?1, revision = revision + 1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?2",
        rusqlite::params![background_json.to_string(), workspace_id],
    )?;
    Ok(())
}

pub fn mutate_template_save(
    conn: &Connection,
    name: &str,
    workspace_id: &str,
) -> StoreResult<String> {
    if name.trim().is_empty() || name.len() > 120 {
        return Err(WorkspaceError::InvalidInput(
            "template name must be 1..=120 chars".into(),
        ));
    }
    let snapshot = query_snapshot(conn, workspace_id)?;
    let payload = TemplatePayload {
        background_json: snapshot.background_json,
        widgets: snapshot.widgets,
    };
    let payload_str =
        serde_json::to_string(&payload).map_err(|e| WorkspaceError::InvalidInput(e.to_string()))?;
    let id = uuid_v4();
    conn.execute(
        "INSERT INTO workspace_templates (id, origin, name, payload_json, created_at, updated_at) VALUES (?1, 'personal', ?2, ?3, strftime('%Y-%m-%dT%H:%M:%fZ','now'), strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        rusqlite::params![id, name.trim(), payload_str],
    )?;
    Ok(id)
}

pub fn mutate_template_delete(conn: &Connection, template_id: &str) -> StoreResult<()> {
    let affected = conn.execute(
        "DELETE FROM workspace_templates WHERE id = ?1 AND origin = 'personal'",
        [template_id],
    )?;
    if affected == 0 {
        Err(WorkspaceError::NotFound)
    } else {
        Ok(())
    }
}

pub fn mutate_settings_set(conn: &Connection, entries: &Value) -> StoreResult<()> {
    let map = entries
        .as_object()
        .ok_or_else(|| WorkspaceError::InvalidInput("settings body must be an object".into()))?;
    for (key, value) in map {
        validate_settings_key(key)?;
        validate_json_size(value, key)?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')",
            rusqlite::params![key, value.to_string()],
        )?;
    }
    Ok(())
}
