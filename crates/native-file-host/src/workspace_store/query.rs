use rusqlite::Connection;
use serde_json::{json, Value};

use super::types::{
    OpenTab, SessionSnapshot, StoreResult, TemplateSummary, WidgetRecord, WorkspaceMeta,
    WorkspaceSnapshot,
};

pub fn global_revision(conn: &Connection) -> StoreResult<i64> {
    Ok(conn.query_row(
        "SELECT revision FROM workspace_meta WHERE id = 1",
        [],
        |row| row.get(0),
    )?)
}

pub fn workspace_revision(conn: &Connection, id: &str) -> StoreResult<i64> {
    Ok(conn.query_row(
        "SELECT revision FROM workspaces WHERE id = ?1 AND deleted_at IS NULL",
        [id],
        |row| row.get(0),
    )?)
}

pub fn query_session(conn: &Connection) -> StoreResult<SessionSnapshot> {
    let mut workspaces = Vec::new();
    let mut statement = conn.prepare(
        "SELECT w.id, w.name, w.sort_order, w.pinned, w.revision,
                (SELECT COUNT(*) FROM workspace_open_tabs t WHERE t.workspace_id = w.id) AS open_count
         FROM workspaces w WHERE w.deleted_at IS NULL ORDER BY w.pinned DESC, w.sort_order ASC, w.name ASC",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(WorkspaceMeta {
            id: row.get(0)?,
            name: row.get(1)?,
            sort_order: row.get(2)?,
            is_pinned: row.get::<_, i64>(3)? != 0,
            revision: row.get(4)?,
            is_open: row.get::<_, i64>(5)? > 0,
        })
    })?;
    for row in rows {
        workspaces.push(row?);
    }
    let mut opened_tabs = Vec::new();
    let mut statement = conn.prepare(
        "SELECT workspace_id, CAST(sort_order AS INTEGER), is_pinned FROM workspace_open_tabs ORDER BY is_pinned DESC, sort_order ASC",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(OpenTab {
            workspace_id: row.get(0)?,
            sort_order: row.get(1)?,
            is_pinned: row.get::<_, i64>(2)? != 0,
        })
    })?;
    for row in rows {
        opened_tabs.push(row?);
    }
    let active_workspace_id: Option<String> = conn
        .query_row(
            "SELECT active_workspace_id FROM workspace_meta WHERE id = 1",
            [],
            |row| row.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten()
        .filter(|id: &String| workspaces.iter().any(|workspace| &workspace.id == id))
        .or_else(|| opened_tabs.first().map(|tab| tab.workspace_id.clone()));
    Ok(SessionSnapshot {
        active_workspace_id,
        opened_tabs,
        workspaces,
        revision: global_revision(conn)?,
    })
}

pub fn query_snapshot(conn: &Connection, id: &str) -> StoreResult<WorkspaceSnapshot> {
    let (name, sort_order, pinned, background_json, template_source, revision) = conn.query_row(
        "SELECT name, sort_order, pinned, background_json, template_source, revision FROM workspaces WHERE id = ?1 AND deleted_at IS NULL",
        [id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)? != 0,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, i64>(5)?,
            ))
        },
    )?;
    let mut widgets = Vec::new();
    let mut statement = conn.prepare(
        "SELECT id, key, \"order\", enabled, config_json, display_json FROM workspace_widgets WHERE workspace_id = ?1 ORDER BY \"order\" ASC",
    )?;
    let rows = statement.query_map([id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)? != 0,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
        ))
    })?;
    for row in rows {
        let (widget_id, key, order, enabled, config_json, display_json) = row?;
        widgets.push(WidgetRecord {
            id: widget_id,
            workspace_id: id.to_string(),
            key,
            order,
            enabled,
            config_json: serde_json::from_str(&config_json).unwrap_or(Value::Null),
            display_json: serde_json::from_str(&display_json).unwrap_or(Value::Null),
        });
    }
    let _ = sort_order;
    let _ = pinned;
    Ok(WorkspaceSnapshot {
        workspace: WorkspaceMeta {
            id: id.to_string(),
            name: name.clone(),
            sort_order,
            is_pinned: pinned,
            is_open: true,
            revision,
        },
        name,
        background_json: serde_json::from_str(&background_json).unwrap_or(Value::Null),
        template_source,
        widgets,
        revision,
    })
}

pub fn query_settings(conn: &Connection, keys: &[String]) -> StoreResult<Value> {
    let mut output = json!({});
    for key in keys {
        let value: Option<String> = conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .map(Some)
            .or_else(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })?;
        if let Some(value) = value {
            output[key.as_str()] = serde_json::from_str(&value).unwrap_or(Value::String(value));
        }
    }
    Ok(output)
}

pub fn query_template_list(conn: &Connection) -> StoreResult<Vec<TemplateSummary>> {
    let mut list = vec![
        TemplateSummary {
            id: "classic".into(),
            origin: "builtin".into(),
            name: "Classic".into(),
            created_at: "builtin".into(),
        },
        TemplateSummary {
            id: "blank".into(),
            origin: "builtin".into(),
            name: "Blank".into(),
            created_at: "builtin".into(),
        },
        TemplateSummary {
            id: "focus".into(),
            origin: "builtin".into(),
            name: "Focus".into(),
            created_at: "builtin".into(),
        },
    ];
    let mut statement = conn.prepare(
        "SELECT id, origin, name, created_at FROM workspace_templates ORDER BY created_at DESC",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(TemplateSummary {
            id: row.get(0)?,
            origin: row.get(1)?,
            name: row.get(2)?,
            created_at: row.get(3)?,
        })
    })?;
    for row in rows {
        list.push(row?);
    }
    Ok(list)
}
