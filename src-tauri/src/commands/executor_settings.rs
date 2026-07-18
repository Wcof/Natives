// 执行引擎设置持久化（PRD 3.4 + CONTEXT「执行边界」）
//
// 复用 natives.db 的 KV `settings` 表，键 `executor:settings`，值 JSON：
//   { "enabledTools": { "read_file": true, ... }, "maxSelfHeal": 3 }
//
// Daemon-native run start reads this configuration, overriding catalog defaults
// and the hard-coded self-heal limit so Settings → Execution Engine takes
// effect without depending on the retired executor run path.

use crate::{db, emit_db_state_changed, Error, Result};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::AppState;

pub const EXECUTOR_KEY: &str = "executor:settings";

/// 持久化的执行引擎配置。前端 Settings ↔ Daemon-native run start 共用。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExecutorSettings {
    /// 工具名 → 是否启用。缺省键视为关闭。
    #[serde(default)]
    pub enabled_tools: std::collections::HashMap<String, bool>,
    /// 自愈上限（PRD 3.4：≤3 次自愈，第 4 次熔断）。
    #[serde(default = "default_max_self_heal")]
    pub max_self_heal: u32,
    /// 步数上限（Q17 S1：默认 50，可在设置页配置）。
    #[serde(default)]
    pub max_steps: Option<u32>,
}

fn default_max_self_heal() -> u32 {
    3
}

/// 读 DB 覆盖默认值。未配置时返回 catalog 默认工具 + 3。
pub fn load_executor_settings() -> ExecutorSettings {
    let defaults = ExecutorSettings {
        enabled_tools: crate::executor_catalog::default_enabled_tools(),
        max_self_heal: 3,
        max_steps: None,
    };
    // Runtime settings are application configuration, so they live with the
    // provider/runtime registry in natives.db. Assistant DB is run data only.
    let Ok(pool_conn) = db::get_main_conn() else {
        return defaults;
    };
    let conn: &rusqlite::Connection = &*pool_conn;
    match db::get_setting(conn, EXECUTOR_KEY) {
        Ok(Some(json)) => serde_json::from_str::<ExecutorSettings>(&json).unwrap_or(defaults),
        _ => defaults,
    }
}

#[tauri::command]
pub fn executor_get_settings(state: State<'_, AppState>) -> Result<ExecutorSettings> {
    let pool_conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;
    let defaults = ExecutorSettings {
        enabled_tools: crate::executor_catalog::default_enabled_tools(),
        max_self_heal: 3,
        max_steps: None,
    };
    Ok(db::get_setting(conn, EXECUTOR_KEY)?
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(defaults))
}

#[tauri::command]
pub fn executor_save_settings(
    settings: ExecutorSettings,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<()> {
    let pool_conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;
    let json = serde_json::to_string(&settings).map_err(|e| Error::Internal(e.to_string()))?;
    db::set_setting(conn, EXECUTOR_KEY, &json)?;
    emit_db_state_changed(
        &app_handle,
        "executor",
        serde_json::json!({ "settings": settings }),
    );
    Ok(())
}
