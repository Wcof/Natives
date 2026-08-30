use serde_json::{json, Map, Value};

use super::schema::{validate_background_key, validate_widget_key};
use super::types::{StoreResult, TemplatePayload, WidgetRecord, WorkspaceError, WorkspaceSnapshot};

pub fn builtin_template_payload(key: &str) -> StoreResult<TemplatePayload> {
    let widgets: Vec<WidgetRecord> = match key {
        "blank" => Vec::new(),
        "focus" => vec![
            widget_record("widget/time", 0),
            widget_record("widget/todo", 1),
            widget_record("widget/notes", 2),
        ],
        "classic" => vec![
            widget_record("widget/time", 0),
            widget_record("widget/greeting", 1),
            widget_record("widget/quote", 2),
        ],
        other => {
            return Err(WorkspaceError::InvalidInput(format!(
                "unknown builtin template: {other}"
            )))
        }
    };
    let background_json = if key == "blank" {
        json!({ "key": "background/colour", "display": { "colour": "#101010" } })
    } else {
        json!({ "key": "background/gradient", "display": { "from": "#1a1a2e", "to": "#16213e", "angle": 135 } })
    };
    Ok(TemplatePayload {
        background_json,
        widgets,
    })
}

fn widget_record(key: &str, order: i64) -> WidgetRecord {
    WidgetRecord {
        id: String::new(),
        workspace_id: String::new(),
        key: key.to_string(),
        order,
        enabled: true,
        config_json: json!({}),
        display_json: json!({ "position": "middleCentre" }),
    }
}

pub fn parse_tabliss_config(value: &Value) -> StoreResult<TemplatePayload> {
    let map = value
        .as_object()
        .ok_or_else(|| WorkspaceError::InvalidInput("tabliss config must be an object".into()))?;
    let background_json = map
        .get("background")
        .cloned()
        .unwrap_or_else(|| json!({ "key": "background/colour" }));
    let background_key = background_json
        .get("key")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            WorkspaceError::InvalidInput("background entry must carry a key field".into())
        })?;
    validate_background_key(background_key)?;

    let mut widgets = Vec::new();
    for (field, entry) in map {
        let key = if let Some(key) = field.strip_prefix("widget/") {
            if let Some(explicit_key) = entry.get("key").and_then(Value::as_str) {
                explicit_key.to_string()
            } else {
                format!("widget/{key}")
            }
        } else {
            continue;
        };
        if key == "widget/css"
            || key == "widget/js"
            || key == "widget/nba"
            || key == "widget/randomMessage"
        {
            continue;
        }
        if validate_widget_key(&key).is_err() {
            continue;
        }
        let order = entry
            .get("order")
            .and_then(Value::as_i64)
            .unwrap_or(widgets.len() as i64);
        let display = entry
            .get("display")
            .cloned()
            .unwrap_or_else(|| json!({ "position": "middleCentre" }));
        let config = entry
            .get("data")
            .cloned()
            .or_else(|| entry.get("config").cloned())
            .unwrap_or_else(|| json!({}));
        widgets.push(WidgetRecord {
            id: String::new(),
            workspace_id: String::new(),
            key,
            order,
            enabled: !entry
                .get("display")
                .and_then(|display| display.get("disabled"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
            config_json: config,
            display_json: display,
        });
    }
    widgets.sort_by_key(|widget| widget.order);
    Ok(TemplatePayload {
        background_json,
        widgets,
    })
}

pub fn export_tabliss_config(snapshot: &WorkspaceSnapshot) -> Value {
    let mut map = Map::new();
    map.insert("background".into(), snapshot.background_json.clone());
    for widget in &snapshot.widgets {
        let mut entry = Map::new();
        entry.insert("order".into(), json!(widget.order));
        entry.insert("data".into(), widget.config_json.clone());
        let mut display = widget.display_json.as_object().cloned().unwrap_or_default();
        if !widget.enabled {
            display.insert("disabled".into(), json!(true));
        }
        entry.insert("display".into(), Value::Object(display));
        let field_key = format!("widget/{}", widget.key.replace("widget/", ""));
        map.insert(field_key, Value::Object(entry));
    }
    Value::Object(map)
}
