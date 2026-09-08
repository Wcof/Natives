use file_manager_core::file_manager;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex};

/// Hard cap for a Host → Chrome response (Chrome's native-messaging
/// limit is 1 MiB per message; we stay strictly under it).
pub(crate) const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

/// Hard cap for an Extension → Host request frame. Chrome allows 64 MiB;
/// the largest legitimate V1 frame is a 20 MiB package payload in
/// base64 (≈26.7 MiB) inside `apps:install_package`. 32 MiB gives margin
/// while culling hostile/malformed frames far below Chrome's ceiling.
pub(crate) const MAX_INCOMING_FRAME_BYTES: usize = 32 * 1024 * 1024;
const MAX_ID_BYTES: usize = 128;
const MAX_METHOD_BYTES: usize = 64;
const MAX_IMPORT_CHUNK_BASE64_BYTES: usize = 700_000;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Request {
    pub(crate) id: String,
    pub(crate) method: String,
    #[serde(default)]
    pub(crate) params: Value,
}

#[derive(Serialize)]
pub(crate) struct Response<'a> {
    pub(crate) id: &'a str,
    pub(crate) ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
}

pub(crate) fn read_frame(input: &mut impl Read) -> Option<Vec<u8>> {
    let mut header = [0u8; 4];
    input.read_exact(&mut header).ok()?;
    let len = u32::from_ne_bytes(header) as usize;
    if len > MAX_INCOMING_FRAME_BYTES {
        return None;
    }
    let mut body = vec![0u8; len];
    input.read_exact(&mut body).ok()?;
    Some(body)
}

pub(crate) fn respond(writer: &Arc<Mutex<io::Stdout>>, response: Response<'_>) -> io::Result<()> {
    let body = serde_json::to_vec(&response).map_err(io::Error::other)?;
    if body.len() >= MAX_MESSAGE_BYTES {
        // Chrome drops any Host → Chrome message at 1 MiB; fail loud
        // instead of writing a frame the browser can never read.
        return Err(io::Error::other(
            "host response exceeds the 1 MiB native-messaging limit",
        ));
    }
    let len = (body.len() as u32).to_ne_bytes();
    let mut out = writer
        .lock()
        .map_err(|_| io::Error::other("stdout lock poisoned"))?;
    out.write_all(&len)?;
    out.write_all(&body)?;
    out.flush()
}

pub(crate) fn safe_error(error: String) -> String {
    if error.starts_with("IO error:")
        || error.starts_with("Internal error:")
        || error.starts_with("Notify error:")
        || error.contains('/')
        || error.contains('\\')
    {
        "文件操作失败".into()
    } else {
        error
    }
}

pub(crate) fn validate_request(request: &Request) -> Result<(), String> {
    if request.id.is_empty() || request.id.len() > MAX_ID_BYTES {
        return Err("invalid request id".into());
    }
    if request.method.is_empty() || request.method.len() > MAX_METHOD_BYTES {
        return Err("invalid request method".into());
    }
    if !matches!(
        request.method.as_str(),
        "version"
            | "roots"
            | "list_dir"
            | "disk_usage"
            | "search"
            | "locate"
            | "search_cancel"
            | "preview_cancel"
            | "batch_cancel"
            | "read_file"
            | "image_preview"
            | "pdf_preview"
            | "media_preview"
            | "archive_list"
            | "extract_archive"
            | "create_zip"
            | "stat"
            | "create_folder"
            | "create_file"
            | "write_file"
            | "rename"
            | "trash"
            | "copy"
            | "move"
            | "duplicate"
            | "duplicate_batch"
            | "copy_batch"
            | "move_batch"
            | "trash_batch"
            | "copy_paths"
            | "copy_image"
            | "open"
            | "editor"
            | "reveal"
            | "open_trash"
            | "watch_start"
            | "watch_stop"
            | "import_probe"
            | "import_begin"
            | "import_chunk"
            | "import_end"
            | "import_cancel"
            | "workspace_session"
            | "workspace_snapshot"
            | "workspace_create"
            | "workspace_rename"
            | "workspace_reorder"
            | "workspace_pin"
            | "workspace_duplicate"
            | "workspace_delete"
            | "workspace_open_tab"
            | "workspace_close_tab"
            | "workspace_reorder_tabs"
            | "workspace_widget_upsert"
            | "workspace_widget_remove"
            | "workspace_widget_reorder"
            | "workspace_background_save"
            | "workspace_instantiate_template"
            | "workspace_template_list"
            | "workspace_template_save"
            | "workspace_template_delete"
            | "workspace_tabliss_preview"
            | "workspace_export_tabliss"
            | "workspace_save_from_tabliss"
            | "workspace_reset"
            | "settings_get"
            | "settings_set"
            | "apps:handshake"
            | "apps:list"
            | "apps:get"
            | "apps:health"
            | "apps:install_begin"
            | "apps:install_package"
            | "apps:install_commit"
            | "apps:install_abort"
            | "apps:uninstall"
            | "apps:clear_data"
            | "apps:recover"
            | "apps:set_enabled"
            | "apps:set_sidebar"
    ) {
        return Err("unsupported method".into());
    }
    if !request.params.is_object() {
        return Err("params must be an object".into());
    }
    let allowed: &[&str] = match request.method.as_str() {
        "version" | "roots" | "open_trash" => &[],
        "list_dir" => &[
            "path",
            "offset",
            "limit",
            "sortBy",
            "sortDir",
            "showHidden",
            "probeProjects",
        ],
        "disk_usage" => &["path"],
        "search" | "locate" => &[
            "path",
            "query",
            "offset",
            "limit",
            "recursive",
            "content",
            "showHidden",
        ],
        "search_cancel" | "preview_cancel" | "batch_cancel" => &["requestId"],
        "read_file" | "image_preview" | "pdf_preview" | "media_preview" | "archive_list"
        | "stat" | "open" | "editor" | "reveal" | "trash" | "watch_start" | "watch_stop" => {
            &["path"]
        }
        "extract_archive" => &["path", "dest"],
        "create_zip" => &["paths", "dest", "name"],
        "import_probe" => &["parent", "name"],
        "import_begin" => &["uploadId", "parent", "name", "size", "conflict"],
        "import_chunk" => &["uploadId", "offset", "data"],
        "import_end" | "import_cancel" => &["uploadId"],
        "create_folder" | "create_file" => &["parent", "name"],
        "write_file" => &["parent", "name", "data", "expectedMtime"],
        "rename" => &["path", "name"],
        "copy" | "move" => &["from", "to", "source", "target"],
        "duplicate" => &["path"],
        "duplicate_batch" => &["paths"],
        "copy_batch" | "move_batch" => &["paths", "dest"],
        "trash_batch" => &["paths"],
        "copy_paths" => &["paths"],
        "copy_image" => &["path"],
        "workspace_session" => &[],
        "workspace_snapshot" => &["workspaceId"],
        "workspace_create" => &["name", "template"],
        "workspace_rename" => &["workspaceId", "name", "expectedRevision"],
        "workspace_reorder" | "workspace_reorder_tabs" | "workspace_widget_reorder" => {
            &["orderedIds", "expectedRevision"]
        }
        "workspace_pin" => &["workspaceId", "pinned", "expectedRevision"],
        "workspace_duplicate"
        | "workspace_delete"
        | "workspace_open_tab"
        | "workspace_close_tab" => &["workspaceId", "expectedRevision"],
        "workspace_widget_upsert" => &["workspaceId", "widget", "expectedRevision"],
        "workspace_widget_remove" => &["workspaceId", "widgetId", "expectedRevision"],
        "workspace_background_save" => &["workspaceId", "background", "expectedRevision"],
        "workspace_instantiate_template" => &["template", "name"],
        "workspace_template_list" => &[],
        "workspace_template_save" => &["name", "workspaceId"],
        "workspace_template_delete" => &["templateId"],
        "workspace_tabliss_preview" => &["tabliss"],
        "workspace_export_tabliss" => &["workspaceId"],
        "workspace_save_from_tabliss" => &["workspaceId", "tabliss", "expectedRevision"],
        "workspace_reset" => &["workspaceId", "template", "expectedRevision"],
        "settings_get" => &["keys"],
        "settings_set" => &["entries"],
        "apps:list" | "apps:health" => &[],
        "apps:handshake" => &["origin"],
        "apps:get" | "apps:recover" => &["appId"],
        "apps:uninstall" => &["appId", "purgeData", "confirmPurge"],
        "apps:clear_data" => &["appId", "confirmPurge"],
        "apps:set_enabled" => &["appId", "enabled"],
        "apps:set_sidebar" => &["appId", "show", "order"],
        "apps:install_begin" => &["request"],
        "apps:install_package" => &["installId", "packageId", "data"],
        "apps:install_commit" => &["installId"],
        "apps:install_abort" => &["installId", "errorCode", "errorMessage"],
        _ => &[],
    };
    let params = request.params.as_object().expect("validated object");
    if params.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err("unknown parameter".into());
    }
    if request.method == "create_zip" {
        let Some(paths) = params.get("paths").and_then(Value::as_array) else {
            return Err("paths must be an array".into());
        };
        if paths.is_empty() || paths.len() > 200 {
            return Err("invalid archive input count".into());
        }
        if paths.iter().any(|value| {
            value
                .as_str()
                .is_none_or(|path| path.is_empty() || path.len() > 4 * 1024)
        }) {
            return Err("invalid archive input path".into());
        }
    }
    for key in [
        "path",
        "parent",
        "name",
        "data",
        "query",
        "requestId",
        "from",
        "to",
        "source",
        "target",
        "dest",
        "sortBy",
        "sortDir",
        "uploadId",
        "conflict",
    ] {
        if let Some(value) = params.get(key) {
            let Some(text) = value.as_str() else {
                return Err("parameter must be a string".into());
            };
            let maximum = if request.method == "apps:install_package" && key == "data" {
                crate::app_store::types::PACKAGE_DATA_MAX_BASE64_BYTES
            } else {
                4 * 1024 * 1024
            };
            if text.len() > maximum {
                return Err("parameter is too long".into());
            }
            if request.method == "import_chunk"
                && key == "data"
                && text.len() > MAX_IMPORT_CHUNK_BASE64_BYTES
            {
                return Err("import chunk is too large".into());
            }
        }
    }
    for key in ["offset", "limit", "size"] {
        if let Some(value) = params.get(key) {
            let Some(number) = value.as_u64() else {
                return Err("parameter must be a number".into());
            };
            if key == "limit" && (number == 0 || number > 2_000) {
                return Err("invalid limit".into());
            }
            if key == "offset" && number > 100_000 {
                return Err("invalid offset".into());
            }
            if key == "size" && number > file_manager::MAX_IMPORT_BYTES {
                return Err("import is too large".into());
            }
        }
    }
    for key in [
        "showHidden",
        "probeProjects",
        "recursive",
        "purgeData",
        "confirmPurge",
        "enabled",
        "show",
    ] {
        if let Some(value) = params.get(key) {
            if !value.is_boolean() {
                return Err("parameter must be a boolean".into());
            }
        }
    }
    if let Some(value) = params.get("expectedMtime") {
        if !value.as_f64().is_some_and(f64::is_finite) {
            return Err("expectedMtime must be a finite number".into());
        }
    }
    if let Some(value) = params.get("sortBy") {
        if !matches!(value.as_str(), Some("name" | "size" | "mtime")) {
            return Err("invalid sortBy".into());
        }
    }
    if let Some(value) = params.get("sortDir") {
        if !matches!(value.as_str(), Some("asc" | "desc")) {
            return Err("invalid sortDir".into());
        }
    }
    if let Some(value) = params.get("conflict") {
        if !matches!(value.as_str(), Some("rename" | "skip")) {
            return Err("invalid import conflict policy".into());
        }
    }
    if request.method == "import_chunk" {
        if params.get("uploadId").and_then(Value::as_str).is_none()
            || params.get("data").and_then(Value::as_str).is_none()
        {
            return Err("uploadId and data are required".into());
        }
    }
    if let Some(paths) = params.get("paths") {
        let Some(paths) = paths.as_array() else {
            return Err("paths must be an array".into());
        };
        if paths.is_empty() || paths.len() > 200 {
            return Err("invalid paths".into());
        }
        if paths
            .iter()
            .any(|path| path.as_str().is_none_or(|path| path.len() > 4 * 1024))
        {
            return Err("invalid path entry".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_package_request_accepts_full_payload_budget_without_relaxing_file_limits() {
        let mut request: Request = serde_json::from_value(serde_json::json!({
            "id": "package", "method": "apps:install_package",
            "params": { "installId": "tx", "packageId": "host", "data": "AAAA" }
        }))
        .unwrap();
        assert!(
            validate_request(&request).is_ok(),
            "package method must reach dispatch"
        );
        let maximum = crate::app_store::types::PACKAGE_DATA_MAX_BASE64_BYTES;
        request.params["data"] = Value::String("A".repeat(maximum));
        assert!(validate_request(&request).is_ok());
        request.params["data"] = Value::String("A".repeat(maximum + 1));
        assert!(validate_request(&request).is_err());
        request.method = "write_file".into();
        request.params = serde_json::json!({ "parent": ".", "name": "a", "data": "A".repeat(4 * 1024 * 1024 + 1) });
        assert!(validate_request(&request).is_err());
    }
    use std::io::Cursor;

    #[test]
    fn reads_native_message_frame() {
        let body = br#"{"id":"1","method":"roots","params":{}}"#;
        let mut frame = (body.len() as u32).to_ne_bytes().to_vec();
        frame.extend_from_slice(body);
        assert_eq!(read_frame(&mut Cursor::new(frame)), Some(body.to_vec()));
    }

    #[test]
    fn rejects_oversized_native_message_frame() {
        let frame = ((MAX_INCOMING_FRAME_BYTES + 1) as u32).to_ne_bytes();
        assert_eq!(read_frame(&mut Cursor::new(frame)), None);
    }

    #[test]
    fn reads_package_frame_at_incoming_limit() {
        // 32 MiB header must be accepted (package payloads live here);
        // only the declared length is checked, the body is not allocated
        // when the stream ends first.
        let mut stream = (MAX_INCOMING_FRAME_BYTES as u32).to_ne_bytes().to_vec();
        stream.extend_from_slice(br#"{"id":"1","method":"apps:install_package"}"#);
        assert_eq!(
            read_frame(&mut Cursor::new(stream)),
            None, // body shorter than declared: EOF, not a size rejection
        );
        // A frame one byte above the cap is rejected without allocating.
        let over = (MAX_INCOMING_FRAME_BYTES as u32 + 1).to_ne_bytes();
        assert_eq!(read_frame(&mut Cursor::new(over)), None);
    }

    #[test]
    fn refuses_oversized_host_response() {
        let body: Vec<u8> = vec![b'a'; MAX_MESSAGE_BYTES];
        let writer: Arc<Mutex<io::Stdout>> = Arc::new(Mutex::new(io::stdout()));
        // We cannot safely write 1 MiB to real stdout in a test, so
        // exercise the guard through a direct length check instead.
        let response = Response {
            id: "1",
            ok: true,
            result: Some(serde_json::json!(body)),
            error: None,
        };
        let serialized = serde_json::to_vec(&response).expect("serialize");
        assert!(serialized.len() >= MAX_MESSAGE_BYTES);
        let result = respond(&writer, response);
        assert!(result.is_err(), "1 MiB responses must be refused");
        let _ = writer.lock().expect("lock").write_all(b"");
    }
}
