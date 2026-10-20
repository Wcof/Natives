//! 设置 / 设备 / 同步 / 采集 / 托盘状态相关 API 处理器。

use super::stats::{format_compact_tokens, known_cost};
use super::{api_error, json_str};
use crate::model_host::ModelUsageQueryClient;
use crate::storage::Store;

pub(super) fn api_sync_push(_store: &Store) -> Result<String, (u16, String)> {
    Ok("{\"ok\":true,\"devices\":[]}".into())
}

pub(super) fn api_sync_pull(_store: &Store, _body: &str) -> Result<String, (u16, String)> {
    Ok("{\"ok\":true}".into())
}

pub(super) fn api_devices(store: &Store) -> Result<String, (u16, String)> {
    store.with_read(|conn| {
        let mut stmt = conn
            .prepare("SELECT device_id, device_name, last_synced_at, status FROM sync_devices ORDER BY last_synced_at DESC")
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                let name: String = row.get(1)?;
                let last: String = row.get(2)?;
                let status: String = row.get(3)?;

                Ok(format!(
                    "{{\"deviceId\":{},\"deviceName\":{},\"lastSyncedAt\":{},\"status\":{}}}",
                    json_str(&id), json_str(&name), json_str(&last), json_str(&status)
                ))
            })
            .map_err(|e| e.to_string())?;

        let mut items = Vec::new();
        for r in rows {
            items.push(r.map_err(|e| e.to_string())?);
        }
        Ok(format!("{{\"devices\":[{}]}}", items.join(",")))
    }).map_err(|e| api_error(500, "DB_ERROR", &e))
}

pub(super) fn api_get_settings(store: &Store) -> Result<String, (u16, String)> {
    store
        .with_read(|conn| {
            let mut stmt = conn
                .prepare("SELECT key, value_json FROM settings")
                .map_err(|e| e.to_string())?;

            let rows = stmt
                .query_map([], |row| {
                    let k: String = row.get(0)?;
                    let v: String = row.get(1)?;
                    Ok(format!("{}:{}", json_str(&k), v))
                })
                .map_err(|e| e.to_string())?;

            let mut entries = Vec::new();
            for r in rows {
                entries.push(r.map_err(|e| e.to_string())?);
            }
            Ok(format!("{{{}}}", entries.join(",")))
        })
        .map_err(|e| api_error(500, "DB_ERROR", &e))
}

pub(super) fn api_save_settings(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let parsed: serde_json::Value =
        serde_json::from_str(body).map_err(|e| api_error(400, "INVALID_JSON", &e.to_string()))?;

    let now = chrono_now();
    store.with_write(|conn| {
        if let Some(obj) = parsed.as_object() {
            let mut stmt = conn
                .prepare("INSERT INTO settings (key, value_json, updated_at) VALUES (?1, ?2, ?3) ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json, updated_at = excluded.updated_at")
                .map_err(|e| e.to_string())?;
            for (k, v) in obj {
                stmt.execute([k, &v.to_string(), &now])
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok("{\"ok\":true}".into())
    }).map_err(|e| api_error(500, "DB_ERROR", &e))
}

pub(super) fn api_collect(
    _client: &ModelUsageQueryClient,
    _body: &str,
) -> Result<String, (u16, String)> {
    // 依据架构整改 Wave 2 / 审查说明：
    // 本地采集器已收口至 Model Host，不再支持虚假空刷新。
    Err(api_error(
        400,
        "UNSUPPORTED",
        "本地采集器已收口至 Model Host，不再支持手动触发全量扫描；额度刷新请调用 /api/limits/refresh 或刷新页面",
    ))
}

pub(super) fn api_tray_state(client: &ModelUsageQueryClient) -> Result<String, (u16, String)> {
    let overview = client
        .get_overview(Some("range=today"))
        .map_err(|e| api_error(503, "MODEL_HOST_UNAVAILABLE", &e.to_string()))?;
    let tokens = overview.metrics.total_tokens;
    let cost = overview.metrics.estimated_cost_usd;
    let formatted_tokens = format_compact_tokens(tokens);
    let formatted_cost = if overview.metrics.priced == Some(true) {
        format!("${:.2}", cost)
    } else {
        "--".into()
    };
    let display_text = format!("{} · {}", formatted_tokens, formatted_cost);

    Ok(format!(
        "{{\"display\":{},\"displayText\":{},\"tokens\":{},\"cost\":{},\"formattedTokens\":{},\"formattedCost\":{}}}",
        json_str(&display_text),
        json_str(&display_text),
        tokens,
        known_cost(cost, overview.metrics.priced),
        json_str(&formatted_tokens),
        json_str(&formatted_cost)
    ))
}

fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", dur.as_secs())
}
