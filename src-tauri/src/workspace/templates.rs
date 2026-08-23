//! Built-in and personal Workspace templates.

use super::store::{new_id, now_rfc3339};
use super::types::{
    TemplateWidgetSpec, WorkspaceTemplate, WorkspaceTemplateManifestV1,
    WorkspaceTemplateSaveRequest,
};
use crate::{Error, Result};
use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};

const CLASSIC_ID: &str = "classic-personal-dashboard";

fn grid(items: &[(&str, i64, i64, i64, i64)]) -> Value {
    Value::Array(
        items
            .iter()
            .map(|(key, x, y, w, h)| json!({"i":key,"x":x,"y":y,"w":w,"h":h}))
            .collect(),
    )
}

fn builtin(
    id: &str,
    name: &str,
    widgets: Vec<TemplateWidgetSpec>,
    layouts: Value,
) -> WorkspaceTemplate {
    let now = "builtin".to_string();
    WorkspaceTemplate {
        id: id.into(),
        name: name.into(),
        origin: "builtin".into(),
        schema_version: 1,
        template_version: 1,
        name_key: Some(format!("workspace.template.{id}")),
        preview_key: Some(id.into()),
        manifest: WorkspaceTemplateManifestV1 {
            schema_version: 1,
            template_version: 1,
            name_key: Some(format!("workspace.template.{id}")),
            appearance: json!({}),
            default_layout_mode: "structured".into(),
            widgets,
            layouts,
        },
        created_at: now.clone(),
        updated_at: now,
    }
}

fn spec(key: &str, widget_type: &str) -> TemplateWidgetSpec {
    TemplateWidgetSpec {
        key: key.into(),
        widget_type: widget_type.into(),
        config_version: 1,
        config: json!({}),
        appearance: json!({}),
    }
}

pub fn builtin_templates() -> Vec<WorkspaceTemplate> {
    let classic_widgets = vec![
        spec("greeting", "greeting"),
        spec("usage", "today_usage"),
        spec("apps", "app_launcher"),
        spec("recent", "recent_files"),
        spec("ai", "ai_status"),
        spec("notes", "notes"),
        spec("storage", "storage_overview"),
        spec("quick", "quick_links"),
    ];
    let lg = grid(&[
        ("greeting", 0, 0, 8, 2),
        ("usage", 8, 0, 4, 2),
        ("apps", 0, 2, 8, 4),
        ("ai", 8, 2, 4, 4),
        ("recent", 0, 6, 6, 4),
        ("notes", 6, 6, 6, 4),
        ("storage", 0, 10, 6, 3),
        ("quick", 6, 10, 6, 3),
    ]);
    let md = grid(&[
        ("greeting", 0, 0, 5, 2),
        ("usage", 5, 0, 3, 2),
        ("apps", 0, 2, 8, 4),
        ("ai", 0, 6, 4, 4),
        ("recent", 4, 6, 4, 4),
        ("notes", 0, 10, 8, 4),
        ("storage", 0, 14, 4, 3),
        ("quick", 4, 14, 4, 3),
    ]);
    let sm = grid(&[
        ("greeting", 0, 0, 4, 2),
        ("usage", 0, 2, 4, 2),
        ("apps", 0, 4, 4, 4),
        ("ai", 0, 8, 4, 4),
        ("recent", 0, 12, 4, 4),
        ("notes", 0, 16, 4, 4),
        ("storage", 0, 20, 4, 3),
        ("quick", 0, 23, 4, 3),
    ]);
    vec![
        builtin(
            CLASSIC_ID,
            "Classic Personal Dashboard",
            classic_widgets,
            json!({"lg":lg,"md":md,"sm":sm}),
        ),
        builtin("blank", "Blank", vec![], json!({"lg":[],"md":[],"sm":[]})),
        builtin(
            "focus",
            "Focus",
            vec![
                spec("greeting", "greeting"),
                spec("notes", "notes"),
                spec("usage", "today_usage"),
            ],
            json!({
                "lg":grid(&[("greeting",0,0,8,2),("usage",8,0,4,2),("notes",0,2,12,7)]),
                "md":grid(&[("greeting",0,0,5,2),("usage",5,0,3,2),("notes",0,2,8,7)]),
                "sm":grid(&[("greeting",0,0,4,2),("usage",0,2,4,2),("notes",0,4,4,7)])
            }),
        ),
    ]
}

fn row_to_template(row: &rusqlite::Row) -> rusqlite::Result<WorkspaceTemplate> {
    let manifest: WorkspaceTemplateManifestV1 = serde_json::from_str(&row.get::<_, String>(5)?)
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e))
        })?;
    Ok(WorkspaceTemplate {
        id: row.get(0)?,
        name: row.get(1)?,
        origin: row.get(2)?,
        schema_version: row.get(3)?,
        template_version: row.get(4)?,
        name_key: manifest.name_key.clone(),
        manifest,
        preview_key: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

pub fn list_templates(conn: &Connection) -> Result<Vec<WorkspaceTemplate>> {
    let mut result = builtin_templates();
    let mut stmt = conn.prepare("SELECT id,name,origin,schema_version,template_version,manifest_json,preview_key,created_at,updated_at FROM workspace_templates WHERE deleted_at IS NULL AND origin = 'personal' ORDER BY updated_at DESC").map_err(Error::Database)?;
    let rows = stmt
        .query_map([], row_to_template)
        .map_err(Error::Database)?;
    result.extend(
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::Database)?,
    );
    Ok(result)
}

pub fn get_template(conn: &Connection, id: &str) -> Result<Option<WorkspaceTemplate>> {
    if let Some(found) = builtin_templates().into_iter().find(|t| t.id == id) {
        return Ok(Some(found));
    }
    conn.query_row("SELECT id,name,origin,schema_version,template_version,manifest_json,preview_key,created_at,updated_at FROM workspace_templates WHERE id = ?1 AND deleted_at IS NULL", [id], row_to_template).optional().map_err(Error::Database)
}

pub fn capture_workspace(
    conn: &Connection,
    workspace_id: &str,
    req: &WorkspaceTemplateSaveRequest,
) -> Result<Option<WorkspaceTemplate>> {
    let Some(snapshot) = super::snapshot::load_workspace_snapshot(conn, workspace_id)? else {
        return Ok(None);
    };
    let key_by_id = snapshot
        .widgets
        .iter()
        .enumerate()
        .map(|(i, w)| (w.id.clone(), format!("widget-{i}")))
        .collect::<std::collections::HashMap<_, _>>();
    let widgets = snapshot
        .widgets
        .iter()
        .filter(|w| w.enabled)
        .map(|w| TemplateWidgetSpec {
            key: key_by_id[&w.id].clone(),
            widget_type: w.widget_type.clone(),
            config_version: w.config_version,
            config: w.config.clone(),
            appearance: w.appearance.clone(),
        })
        .collect();
    let mut layouts = serde_json::Map::new();
    for layout in snapshot.layouts {
        let mut value = layout.layout;
        rewrite_layout_ids(&mut value, &key_by_id);
        layouts.insert(layout.breakpoint, value);
    }
    let version = 1;
    let manifest = WorkspaceTemplateManifestV1 {
        schema_version: 1,
        template_version: version,
        name_key: None,
        appearance: snapshot.workspace.appearance,
        default_layout_mode: snapshot.workspace.default_layout_mode,
        widgets,
        layouts: Value::Object(layouts),
    };
    validate_manifest(&manifest)?;
    let id = req.id.clone().unwrap_or_else(|| new_id("tpl"));
    let now = now_rfc3339();
    conn.execute("INSERT INTO workspace_templates (id,name,origin,schema_version,template_version,manifest_json,created_at,updated_at) VALUES (?1,?2,'personal',1,?3,?4,?5,?5) ON CONFLICT(id) DO UPDATE SET name=excluded.name,template_version=workspace_templates.template_version+1,manifest_json=excluded.manifest_json,updated_at=excluded.updated_at,deleted_at=NULL", rusqlite::params![id, req.name.trim(), version, serde_json::to_string(&manifest).map_err(|e| Error::Internal(e.to_string()))?, now]).map_err(Error::Database)?;
    get_template(conn, &id)
}

fn rewrite_layout_ids(value: &mut Value, ids: &std::collections::HashMap<String, String>) {
    match value {
        Value::Array(items) => {
            for item in items {
                rewrite_layout_ids(item, ids);
            }
        }
        Value::Object(map) => {
            for key in ["i", "id", "widgetId"] {
                if let Some(Value::String(id)) = map.get_mut(key) {
                    if let Some(replacement) = ids.get(id) {
                        *id = replacement.clone();
                    }
                }
            }
            for value in map.values_mut() {
                rewrite_layout_ids(value, ids);
            }
        }
        _ => {}
    }
}

pub fn validate_manifest(manifest: &WorkspaceTemplateManifestV1) -> Result<()> {
    if manifest.schema_version != 1 {
        return Err(Error::InvalidInput(
            "unsupported template schema version".into(),
        ));
    }
    let text = serde_json::to_string(manifest)
        .map_err(|e| Error::InvalidInput(e.to_string()))?
        .to_ascii_lowercase();
    for forbidden in [
        "<script",
        "javascript:",
        "secret",
        "password",
        "token\"",
        "/users/",
        "c:\\\\users\\\\",
    ] {
        if text.contains(forbidden) {
            return Err(Error::InvalidInput(format!(
                "template contains forbidden value: {forbidden}"
            )));
        }
    }
    Ok(())
}

pub fn classic_template_id() -> &'static str {
    CLASSIC_ID
}

pub fn apply_template(conn: &Connection, workspace_id: &str, template_id: &str) -> Result<bool> {
    let Some(template) = get_template(conn, template_id)? else {
        return Ok(false);
    };
    validate_manifest(&template.manifest)?;
    if super::store::get_workspace(conn, workspace_id)?.is_none() {
        return Ok(false);
    }
    let tx = conn.unchecked_transaction().map_err(Error::Database)?;
    for table in [
        "workspace_widgets",
        "workspace_layouts",
        "workspace_view_states",
    ] {
        tx.execute(
            &format!("DELETE FROM {table} WHERE workspace_id = ?1"),
            [workspace_id],
        )
        .map_err(Error::Database)?;
    }
    let now = now_rfc3339();
    let mut ids = std::collections::HashMap::new();
    for (position, widget) in template.manifest.widgets.iter().enumerate() {
        let id = new_id("wgt");
        ids.insert(widget.key.clone(), id.clone());
        tx.execute("INSERT INTO workspace_widgets (id,workspace_id,widget_type,config_version,config_json,appearance_json,hidden,enabled,z_index,position,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,0,1,?7,?8,?9,?9)", rusqlite::params![id,workspace_id,widget.widget_type,widget.config_version,widget.config.to_string(),widget.appearance.to_string(),position as i64,position as i64,now]).map_err(Error::Database)?;
    }
    let layouts = template
        .manifest
        .layouts
        .as_object()
        .cloned()
        .unwrap_or_default();
    for (breakpoint, mut layout) in layouts {
        rewrite_layout_ids(&mut layout, &ids);
        let mode = if breakpoint == "free" {
            "free"
        } else {
            "structured"
        };
        tx.execute("INSERT INTO workspace_layouts (id,workspace_id,breakpoint,layout_mode,layout_version,layout_json,is_active,created_at,updated_at) VALUES (?1,?2,?3,?4,1,?5,1,?6,?6)", rusqlite::params![new_id("lay"),workspace_id,breakpoint,mode,layout.to_string(),now]).map_err(Error::Database)?;
    }
    tx.execute("UPDATE workspaces SET default_layout_mode=?1,appearance_json=?2,template_source_id=?3,template_version=?4,updated_at=?5 WHERE id=?6", rusqlite::params![template.manifest.default_layout_mode,template.manifest.appearance.to_string(),template.id,template.template_version,now,workspace_id]).map_err(Error::Database)?;
    tx.commit().map_err(Error::Database)?;
    Ok(true)
}

pub fn delete_personal_template(conn: &Connection, template_id: &str) -> Result<bool> {
    let now = now_rfc3339();
    Ok(conn.execute("UPDATE workspace_templates SET deleted_at=?1,updated_at=?1 WHERE id=?2 AND origin='personal' AND deleted_at IS NULL", rusqlite::params![now,template_id]).map_err(Error::Database)? > 0)
}

pub fn reset_widget(
    conn: &Connection,
    workspace_id: &str,
    widget_id: &str,
    template_id: Option<&str>,
) -> Result<bool> {
    let Some(snapshot) = super::snapshot::load_workspace_snapshot(conn, workspace_id)? else {
        return Ok(false);
    };
    let Some(widget) = snapshot.widgets.iter().find(|item| item.id == widget_id) else {
        return Ok(false);
    };
    let source_id = template_id
        .or(snapshot.workspace.template_source_id.as_deref())
        .unwrap_or(CLASSIC_ID);
    let Some(template) = get_template(conn, source_id)? else {
        return Ok(false);
    };
    let Some(spec) = template
        .manifest
        .widgets
        .iter()
        .find(|item| item.widget_type == widget.widget_type)
    else {
        return Ok(false);
    };
    let tx = conn.unchecked_transaction().map_err(Error::Database)?;
    let now = now_rfc3339();
    tx.execute("UPDATE workspace_widgets SET config_version=?1,config_json=?2,appearance_json=?3,enabled=1,updated_at=?4 WHERE id=?5 AND workspace_id=?6", rusqlite::params![spec.config_version,spec.config.to_string(),spec.appearance.to_string(),now,widget_id,workspace_id]).map_err(Error::Database)?;
    if let Some(template_layouts) = template.manifest.layouts.as_object() {
        for layout in snapshot.layouts {
            let Some(default_layout) = template_layouts.get(&layout.breakpoint) else {
                continue;
            };
            let Some(default_item) = default_layout.as_array().and_then(|items| {
                items
                    .iter()
                    .find(|item| item.get("i").and_then(Value::as_str) == Some(spec.key.as_str()))
            }) else {
                continue;
            };
            let Some(mut current) = layout.layout.as_array().cloned() else {
                continue;
            };
            current.retain(|item| item.get("i").and_then(Value::as_str) != Some(widget_id));
            let mut replacement = default_item.clone();
            if let Some(object) = replacement.as_object_mut() {
                object.insert("i".into(), Value::String(widget_id.into()));
            }
            current.push(replacement);
            tx.execute(
                "UPDATE workspace_layouts SET layout_json=?1,updated_at=?2 WHERE id=?3",
                rusqlite::params![Value::Array(current).to_string(), now, layout.id],
            )
            .map_err(Error::Database)?;
        }
    }
    tx.commit().map_err(Error::Database)?;
    Ok(true)
}
