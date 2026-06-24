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
    // MVP: 返回空列表
    Ok(vec![])
}

#[tauri::command]
pub async fn scheduler_create_task(input: CreateTaskInput) -> Result<ScheduledTask> {
    let task = ScheduledTask {
        id: format!("task_{}", chrono::Utc::now().timestamp_millis()),
        name: input.name,
        prompt: input.prompt,
        runtime_override: input.runtime_override,
        schedule_type: input.schedule_type,
        schedule_value: input.schedule_value,
        next_run: chrono::Utc::now().to_rfc3339(),
        last_status: None,
        consecutive_errors: 0,
        enabled: true,
        created_at: chrono::Utc::now().to_rfc3339(),
        expires_at: None,
    };
    Ok(task)
}

#[tauri::command]
pub async fn scheduler_update_task(_id: String, _name: Option<String>, _prompt: Option<String>, _enabled: Option<bool>) -> Result<()> {
    Ok(())
}

#[tauri::command]
pub async fn scheduler_delete_task(_id: String) -> Result<()> {
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
