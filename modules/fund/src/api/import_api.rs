//! HTTP API 业务请求处理（拆分自 api.rs，仅移动，无逻辑变更）。

use super::{api_error};
use crate::import;
use crate::storage::Store;

pub(crate) fn api_import_preview(body: &str) -> Result<String, (u16, String)> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| api_error(400, "IMPORT_INVALID", &e.to_string()))?;
    let template = value
        .get("templateVersion")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| api_error(400, "IMPORT_INVALID", "templateVersion 缺失"))?;
    let csv = value
        .get("csv")
        .and_then(|v| v.as_str())
        .ok_or_else(|| api_error(400, "IMPORT_INVALID", "csv 缺失"))?;
    if csv.len() > 5 * 1024 * 1024 {
        return Err(api_error(400, "IMPORT_INVALID", "文件超过 5MiB"));
    }
    let preview =
        import::preview(template, csv).map_err(|e| api_error(400, e.code(), &e.message()))?;
    let json =
        serde_json::to_string(&preview).map_err(|e| api_error(500, "IMPORT_DB", &e.to_string()))?;
    Ok(json)
}

pub(crate) fn api_import_commit(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| api_error(400, "IMPORT_INVALID", &e.to_string()))?;
    let template = value
        .get("templateVersion")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| api_error(400, "IMPORT_INVALID", "templateVersion 缺失"))?;
    let csv = value
        .get("csv")
        .and_then(|v| v.as_str())
        .ok_or_else(|| api_error(400, "IMPORT_INVALID", "csv 缺失"))?
        .to_string();
    let source_id = value
        .get("sourceId")
        .and_then(|v| v.as_str())
        .unwrap_or("manual-upload")
        .to_string();
    if csv.len() > 5 * 1024 * 1024 {
        return Err(api_error(400, "IMPORT_INVALID", "文件超过 5MiB"));
    }
    let preview =
        import::preview(template, &csv).map_err(|e| api_error(400, e.code(), &e.message()))?;
    let file_hash = app_runtime_core::sha256_hex(csv.as_bytes());
    let params_hash = app_runtime_core::sha256_hex(template.to_string().as_bytes());
    let receipt = import::commit(
        store,
        &source_id,
        template,
        &file_hash,
        &params_hash,
        &preview,
    )
    .map_err(|e| api_error(409, e.code(), &e.message()))?;
    let json =
        serde_json::to_string(&receipt).map_err(|e| api_error(500, "IMPORT_DB", &e.to_string()))?;
    Ok(json)
}
