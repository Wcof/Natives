//! 终端路径定位链命令壳（W11）——核心逻辑见 crate::locate

use crate::{locate, Error, Result};

/// 终端划线前批量验真：stat 得到才配下划线
#[tauri::command]
pub fn fs_verify_paths(candidates: Vec<String>) -> Result<Vec<locate::VerifyPathResult>> {
    Ok(locate::verify_paths(&candidates))
}

/// 四级兜底定位：直接 stat → 空格扩展 → 多根搜索 → Spotlight。
/// 多根 walk + mdfind 可能耗秒级，走 spawn_blocking 避免阻塞 IPC。
#[tauri::command]
pub async fn fs_locate(
    query: String,
    cwd: Option<String>,
    roots: Option<Vec<String>>,
) -> Result<locate::LocateResult> {
    tokio::task::spawn_blocking(move || {
        locate::locate(&query, cwd.as_deref(), &roots.unwrap_or_default())
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))?
}
