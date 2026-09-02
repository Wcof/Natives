use crate::protocol::Request;
use crate::workspace_store::{WidgetRecord, WorkspaceStore};
use serde_json::Value;

pub(crate) fn workspace_dispatch(
    store: &WorkspaceStore,
    request: &Request,
) -> Result<Value, String> {
    let params = &request.params;
    let expected = |params: &Value| params.get("expectedRevision").and_then(Value::as_i64);
    macro_rules! handle_result {
        ($result:expr) => {
            match $result {
                Ok(val) => serde_json::to_value(val).map_err(|e| e.to_string()),
                Err(error) => Err(error.to_string()),
            }
        };
    }
    match request.method.as_str() {
        "workspace_session" => handle_result!(store.session()),
        "workspace_snapshot" => {
            let id = params
                .get("workspaceId")
                .and_then(Value::as_str)
                .ok_or("workspaceId is required")?;
            handle_result!(store.snapshot(id))
        }
        "workspace_create" => {
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .ok_or("name is required")?;
            let template = params.get("template").and_then(Value::as_str);
            handle_result!(store.create(name, template))
        }
        "workspace_rename" => {
            let id = params
                .get("workspaceId")
                .and_then(Value::as_str)
                .ok_or("workspaceId is required")?;
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .ok_or("name is required")?;
            handle_result!(store.rename(id, name, expected(params)))
        }
        "workspace_reorder" => {
            let ids = parse_id_list(params.get("orderedIds").ok_or("orderedIds is required")?)?;
            handle_result!(store.reorder(&ids, expected(params)))
        }
        "workspace_pin" => {
            let id = params
                .get("workspaceId")
                .and_then(Value::as_str)
                .ok_or("workspaceId is required")?;
            let pinned = params
                .get("pinned")
                .and_then(Value::as_bool)
                .ok_or("pinned is required")?;
            handle_result!(store.pin(id, pinned, expected(params)))
        }
        "workspace_duplicate" => {
            let id = params
                .get("workspaceId")
                .and_then(Value::as_str)
                .ok_or("workspaceId is required")?;
            handle_result!(store.duplicate(id, expected(params)))
        }
        "workspace_delete" => {
            let id = params
                .get("workspaceId")
                .and_then(Value::as_str)
                .ok_or("workspaceId is required")?;
            handle_result!(store.delete(id, expected(params)))
        }
        "workspace_open_tab" => {
            let id = params
                .get("workspaceId")
                .and_then(Value::as_str)
                .ok_or("workspaceId is required")?;
            handle_result!(store.open_tab(id))
        }
        "workspace_close_tab" => {
            let id = params
                .get("workspaceId")
                .and_then(Value::as_str)
                .ok_or("workspaceId is required")?;
            handle_result!(store.close_tab(id))
        }
        "workspace_reorder_tabs" => {
            let ids = parse_id_list(params.get("orderedIds").ok_or("orderedIds is required")?)?;
            handle_result!(store.reorder_tabs(&ids, expected(params)))
        }
        "workspace_widget_upsert" => {
            let id = params
                .get("workspaceId")
                .and_then(Value::as_str)
                .ok_or("workspaceId is required")?;
            let widget: WidgetRecord =
                serde_json::from_value(params.get("widget").cloned().ok_or("widget is required")?)
                    .map_err(|e| e.to_string())?;
            handle_result!(store.widget_upsert(id, &widget, expected(params)))
        }
        "workspace_widget_remove" => {
            let id = params
                .get("workspaceId")
                .and_then(Value::as_str)
                .ok_or("workspaceId is required")?;
            let widget_id = params
                .get("widgetId")
                .and_then(Value::as_str)
                .ok_or("widgetId is required")?;
            handle_result!(store.widget_remove(id, widget_id, expected(params)))
        }
        "workspace_widget_reorder" => {
            let id = params
                .get("workspaceId")
                .and_then(Value::as_str)
                .ok_or("workspaceId is required")?;
            let ids = parse_id_list(params.get("orderedIds").ok_or("orderedIds is required")?)?;
            handle_result!(store.widget_reorder(id, &ids, expected(params)))
        }
        "workspace_background_save" => {
            let id = params
                .get("workspaceId")
                .and_then(Value::as_str)
                .ok_or("workspaceId is required")?;
            let background = params
                .get("background")
                .cloned()
                .ok_or("background is required")?;
            handle_result!(store.background_save(id, &background, expected(params)))
        }
        "workspace_instantiate_template" => {
            let template = params
                .get("template")
                .and_then(Value::as_str)
                .ok_or("template is required")?;
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .ok_or("name is required")?;
            handle_result!(store.instantiate_template(template, name))
        }
        "workspace_template_list" => handle_result!(store.template_list()),
        "workspace_template_save" => {
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .ok_or("name is required")?;
            let workspace_id = params
                .get("workspaceId")
                .and_then(Value::as_str)
                .ok_or("workspaceId is required")?;
            handle_result!(store.template_save(name, workspace_id))
        }
        "workspace_template_delete" => {
            let template_id = params
                .get("templateId")
                .and_then(Value::as_str)
                .ok_or("templateId is required")?;
            handle_result!(store.template_delete(template_id))
        }
        "workspace_tabliss_preview" => {
            let tabliss = params
                .get("tabliss")
                .cloned()
                .ok_or("tabliss is required")?;
            handle_result!(store.tabliss_preview(&tabliss))
        }
        "workspace_export_tabliss" => {
            let id = params
                .get("workspaceId")
                .and_then(Value::as_str)
                .ok_or("workspaceId is required")?;
            handle_result!(store.export_tabliss(id))
        }
        "workspace_save_from_tabliss" => {
            let id = params
                .get("workspaceId")
                .and_then(Value::as_str)
                .ok_or("workspaceId is required")?;
            let tabliss = params
                .get("tabliss")
                .cloned()
                .ok_or("tabliss is required")?;
            handle_result!(store.save_from_tabliss(id, &tabliss, expected(params)))
        }
        "workspace_reset" => {
            let id = params
                .get("workspaceId")
                .and_then(Value::as_str)
                .ok_or("workspaceId is required")?;
            let template = params
                .get("template")
                .and_then(Value::as_str)
                .ok_or("template is required")?;
            handle_result!(store.reset(id, template, expected(params)))
        }
        "settings_get" => {
            let keys = params
                .get("keys")
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            store.settings_get(&keys).map_err(|e| e.to_string())
        }
        "settings_set" => {
            let entries = params
                .get("entries")
                .cloned()
                .ok_or("entries is required")?;
            store.settings_set(&entries).map_err(|e| e.to_string())
        }
        other => Err(format!("unsupported workspace method: {other}")),
    }
}

fn parse_id_list(value: &Value) -> Result<Vec<String>, String> {
    value
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .ok_or_else(|| "expected array of ids".to_string())
}
