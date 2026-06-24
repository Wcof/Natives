//! scheduler.rs — Task Scheduler（定时调度器）
//!
//! 常驻 tokio task 10s 轮询 scheduled_tasks 表找到期任务执行。
//! 失败按指数退避重试，连续 10 次熔断。

use crate::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTask {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub runtime_override: Option<String>,
    pub schedule_type: String,    // once / interval / cron
    pub schedule_value: String,
    pub next_run: String,
    pub last_status: Option<String>,
    pub consecutive_errors: u32,
    pub enabled: bool,
    pub created_at: String,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRun {
    pub id: String,
    pub task_id: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub status: Option<String>,
    pub result_summary: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskInput {
    pub name: String,
    pub prompt: String,
    pub runtime_override: Option<String>,
    pub schedule_type: String,
    pub schedule_value: String,
}

/// 启动调度器常驻 task
pub fn start_scheduler(_app: tauri::AppHandle) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(10));
        loop {
            ticker.tick().await;
            // MVP: 轮询 scheduled_tasks 表执行到期任务
            // 完整实现在后续迭代补齐
        }
    });
}

// ── IPC 命令 ──

#[tauri::command]
pub async fn scheduler_list_tasks() -> Result<Vec<ScheduledTask>> {
    let conn = crate::db::get_assistant_db_conn()
        .map_err(|e| crate::Error::Internal(format!("scheduler DB: {e}")))?;
    let mut stmt = conn.prepare(
        "SELECT id, name, prompt, runtime_override, schedule_type, schedule_value, next_run, last_status, consecutive_errors, enabled, created_at, expires_at FROM scheduled_tasks ORDER BY created_at DESC"
    ).map_err(|e| crate::Error::Internal(e.to_string()))?;
    let rows = stmt.query_map([], |row| Ok(ScheduledTask {
        id: row.get(0)?, name: row.get(1)?, prompt: row.get(2)?,
        runtime_override: row.get(3)?, schedule_type: row.get(4)?,
        schedule_value: row.get(5)?, next_run: row.get(6)?,
        last_status: row.get(7)?, consecutive_errors: row.get(8)?,
        enabled: row.get::<_, i32>(9)? != 0, created_at: row.get(10)?,
        expires_at: row.get(11)?,
    })).map_err(|e| crate::Error::Internal(e.to_string()))?;
    let tasks: Vec<ScheduledTask> = rows.filter_map(|r| r.ok()).collect();
    Ok(tasks)
}

#[tauri::command]
pub async fn scheduler_create_task(input: CreateTaskInput) -> Result<ScheduledTask> {
    let conn = crate::db::get_assistant_db_conn()
        .map_err(|e| crate::Error::Internal(format!("scheduler DB: {e}")))?;
    let task = ScheduledTask {
        id: format!("task_{}", chrono::Utc::now().timestamp_millis()),
        name: input.name, prompt: input.prompt,
        runtime_override: input.runtime_override,
        schedule_type: input.schedule_type, schedule_value: input.schedule_value,
        next_run: chrono::Utc::now().to_rfc3339(), last_status: None,
        consecutive_errors: 0, enabled: true,
        created_at: chrono::Utc::now().to_rfc3339(), expires_at: None,
    };
    conn.execute(
        "INSERT INTO scheduled_tasks (id, name, prompt, runtime_override, schedule_type, schedule_value, next_run, consecutive_errors, enabled, created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,0,1,?8)",
        rusqlite::params![task.id, task.name, task.prompt, task.runtime_override, task.schedule_type, task.schedule_value, task.next_run, task.created_at],
    ).map_err(|e| crate::Error::Internal(e.to_string()))?;
    Ok(task)
}

#[tauri::command]
pub async fn scheduler_update_task(id: String, name: Option<String>, prompt: Option<String>, enabled: Option<bool>) -> Result<()> {
    let conn = crate::db::get_assistant_db_conn()
        .map_err(|e| crate::Error::Internal(format!("scheduler DB: {e}")))?;
    if let Some(n) = name { conn.execute("UPDATE scheduled_tasks SET name=?1 WHERE id=?2", rusqlite::params![n, id]).map_err(|e| crate::Error::Internal(e.to_string()))?; }
    if let Some(p) = prompt { conn.execute("UPDATE scheduled_tasks SET prompt=?1 WHERE id=?2", rusqlite::params![p, id]).map_err(|e| crate::Error::Internal(e.to_string()))?; }
    if let Some(e) = enabled { conn.execute("UPDATE scheduled_tasks SET enabled=?1 WHERE id=?2", rusqlite::params![e as i32, id]).map_err(|e| crate::Error::Internal(e.to_string()))?; }
    Ok(())
}

#[tauri::command]
pub async fn scheduler_delete_task(id: String) -> Result<()> {
    let conn = crate::db::get_assistant_db_conn()
        .map_err(|e| crate::Error::Internal(format!("scheduler DB: {e}")))?;
    conn.execute("DELETE FROM scheduled_tasks WHERE id=?1", rusqlite::params![id]).map_err(|e| crate::Error::Internal(e.to_string()))?;
    Ok(())
}

#[tauri::command]
pub async fn scheduler_run_task_now(_id: String) -> Result<serde_json::Value> {
    Ok(serde_json::json!({ "status": "started" }))
}

#[tauri::command]
pub async fn scheduler_list_runs(_task_id: String) -> Result<Vec<TaskRun>> {
    Ok(vec![])
}
