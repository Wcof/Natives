//! jobs/store.rs — Job 持久化（复用并扩展 assistant.db 的 scheduled_tasks / task_runs）
//!
//! 契约第 1、2 节：表名不改，按既有机制（PRAGMA table_info 探测 + 条件
//! ALTER TABLE ADD COLUMN）补齐缺失列，禁止 DROP/rebuild。
//! 本文件是两张表 DDL 的单一来源：db.rs init_assistant_db 调用 ensure_schema。

use crate::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

/// JobDefinition — scheduled_tasks 一行的完整视图（契约第 1 节）。
#[derive(Debug, Clone)]
pub struct JobDefinition {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub description: Option<String>,
    /// 业务必填（引擎硬性要求，禁止 cwd 兜底）；老行可空，派发时校验。
    pub project_path: Option<String>,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub key_id: Option<String>,
    /// 接缝 B：只存不校验。
    pub agent_profile_id: Option<String>,
    /// JSON 数组字符串，默认 `[]`；只存不校验。
    pub capability_refs: String,
    /// readonly | ask | full_access，默认 readonly。
    pub permission_profile: String,
    pub max_steps: Option<i64>,
    pub effort: Option<String>,
    pub runtime_id: Option<String>,
    pub schedule_type: String,
    pub schedule_value: String,
    pub next_run: String,
    pub last_status: Option<String>,
    pub last_run_at: Option<String>,
    pub consecutive_errors: i64,
    pub enabled: bool,
    pub created_at: String,
    pub expires_at: Option<String>,
}

/// JobRun — task_runs 一行（契约第 2 节）。JSON snake_case，直接序列化给前端。
#[derive(Debug, Clone, Serialize)]
pub struct JobRunRecord {
    pub id: String,
    /// DB 列名 task_id；对外统一为 job_id。
    pub job_id: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    /// pending → dispatched → running → succeeded | failed | cancelled；
    /// 旁路终态 skipped / dispatch_error。
    pub status: Option<String>,
    pub result_summary: Option<String>,
    pub error: Option<String>,
    /// 引擎侧 run id，未派发为 NULL。
    pub run_id: Option<String>,
    pub conversation_id: Option<String>,
    /// schedule | manual。
    pub trigger: Option<String>,
    pub error_code: Option<String>,
    pub detail: Option<String>,
}

const JOB_COLUMNS: &str = "id, name, prompt, description, project_path, provider_id, model_id, \
     key_id, agent_profile_id, capability_refs, permission_profile, max_steps, effort, \
     runtime_id, schedule_type, schedule_value, next_run, last_status, last_run_at, \
     consecutive_errors, enabled, created_at, expires_at";

const RUN_COLUMNS: &str =
    "id, task_id, started_at, finished_at, status, result_summary, error, run_id, \
     conversation_id, \"trigger\", error_code, detail";

/// 建表 + 条件补列（幂等）。db.rs init_assistant_db 与单测共同入口。
pub fn ensure_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS scheduled_tasks (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            prompt TEXT NOT NULL,
            runtime_override TEXT,
            schedule_type TEXT NOT NULL,
            schedule_value TEXT NOT NULL,
            next_run TEXT NOT NULL,
            last_status TEXT,
            consecutive_errors INTEGER NOT NULL DEFAULT 0,
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL,
            expires_at TEXT
        );
        CREATE TABLE IF NOT EXISTS task_runs (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL REFERENCES scheduled_tasks(id) ON DELETE CASCADE,
            started_at TEXT NOT NULL,
            finished_at TEXT,
            status TEXT,
            result_summary TEXT,
            error TEXT
        );
        ",
    )
    .map_err(Error::Database)?;

    // scheduled_tasks 补列（契约第 1 节）
    let job_cols = table_columns(conn, "scheduled_tasks")?;
    const JOB_ADD: &[(&str, &str)] = &[
        ("description", "TEXT"),
        ("project_path", "TEXT"),
        ("provider_id", "TEXT"),
        ("model_id", "TEXT"),
        ("key_id", "TEXT"),
        ("agent_profile_id", "TEXT"),
        ("capability_refs", "TEXT NOT NULL DEFAULT '[]'"),
        ("permission_profile", "TEXT NOT NULL DEFAULT 'readonly'"),
        ("max_steps", "INTEGER"),
        ("effort", "TEXT"),
        ("runtime_id", "TEXT"),
        ("last_run_at", "TEXT"),
    ];
    for (name, ty) in JOB_ADD {
        if !job_cols.iter().any(|c| c == name) {
            conn.execute(
                &format!("ALTER TABLE scheduled_tasks ADD COLUMN \"{name}\" {ty}"),
                [],
            )
            .map_err(Error::Database)?;
        }
    }

    // task_runs 补列（契约第 2 节）。trigger 是 SQLite 关键字，统一加引号。
    let run_cols = table_columns(conn, "task_runs")?;
    const RUN_ADD: &[(&str, &str)] = &[
        ("run_id", "TEXT"),
        ("conversation_id", "TEXT"),
        ("trigger", "TEXT"),
        ("error_code", "TEXT"),
        ("detail", "TEXT"),
    ];
    for (name, ty) in RUN_ADD {
        if !run_cols.iter().any(|c| c == name) {
            conn.execute(
                &format!("ALTER TABLE task_runs ADD COLUMN \"{name}\" {ty}"),
                [],
            )
            .map_err(Error::Database)?;
        }
    }

    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_task_runs_task ON task_runs(task_id, started_at);",
    )
    .map_err(Error::Database)?;
    Ok(())
}

fn table_columns(conn: &Connection, table: &str) -> Result<Vec<String>> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(Error::Database)?;
    let cols = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(Error::Database)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(cols)
}

fn job_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<JobDefinition> {
    Ok(JobDefinition {
        id: row.get(0)?,
        name: row.get(1)?,
        prompt: row.get(2)?,
        description: row.get(3)?,
        project_path: row.get(4)?,
        provider_id: row.get(5)?,
        model_id: row.get(6)?,
        key_id: row.get(7)?,
        agent_profile_id: row.get(8)?,
        capability_refs: row
            .get::<_, Option<String>>(9)?
            .unwrap_or_else(|| "[]".to_string()),
        permission_profile: row
            .get::<_, Option<String>>(10)?
            .unwrap_or_else(|| "readonly".to_string()),
        max_steps: row.get(11)?,
        effort: row.get(12)?,
        runtime_id: row.get(13)?,
        schedule_type: row.get(14)?,
        schedule_value: row.get(15)?,
        next_run: row.get(16)?,
        last_status: row.get(17)?,
        last_run_at: row.get(18)?,
        consecutive_errors: row.get(19)?,
        enabled: row.get::<_, i64>(20)? != 0,
        created_at: row.get(21)?,
        expires_at: row.get(22)?,
    })
}

fn run_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<JobRunRecord> {
    Ok(JobRunRecord {
        id: row.get(0)?,
        job_id: row.get(1)?,
        started_at: row.get(2)?,
        finished_at: row.get(3)?,
        status: row.get(4)?,
        result_summary: row.get(5)?,
        error: row.get(6)?,
        run_id: row.get(7)?,
        conversation_id: row.get(8)?,
        trigger: row.get(9)?,
        error_code: row.get(10)?,
        detail: row.get(11)?,
    })
}

pub fn insert_job(conn: &Connection, job: &JobDefinition) -> Result<()> {
    conn.execute(
        "INSERT INTO scheduled_tasks (id, name, prompt, description, project_path, provider_id, \
         model_id, key_id, agent_profile_id, capability_refs, permission_profile, max_steps, \
         effort, runtime_id, schedule_type, schedule_value, next_run, last_status, last_run_at, \
         consecutive_errors, enabled, created_at, expires_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23)",
        params![
            job.id,
            job.name,
            job.prompt,
            job.description,
            job.project_path,
            job.provider_id,
            job.model_id,
            job.key_id,
            job.agent_profile_id,
            job.capability_refs,
            job.permission_profile,
            job.max_steps,
            job.effort,
            job.runtime_id,
            job.schedule_type,
            job.schedule_value,
            job.next_run,
            job.last_status,
            job.last_run_at,
            job.consecutive_errors,
            job.enabled as i64,
            job.created_at,
            job.expires_at,
        ],
    )
    .map_err(Error::Database)?;
    Ok(())
}

pub fn get_job(conn: &Connection, id: &str) -> Result<Option<JobDefinition>> {
    conn.query_row(
        &format!("SELECT {JOB_COLUMNS} FROM scheduled_tasks WHERE id = ?1"),
        params![id],
        job_from_row,
    )
    .optional()
    .map_err(Error::Database)
}

pub fn list_jobs(conn: &Connection) -> Result<Vec<JobDefinition>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {JOB_COLUMNS} FROM scheduled_tasks ORDER BY created_at DESC"
        ))
        .map_err(Error::Database)?;
    let jobs = stmt
        .query_map([], job_from_row)
        .map_err(Error::Database)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Error::Database)?;
    Ok(jobs)
}

/// 整行更新（id 定位）。返回是否命中。
pub fn update_job(conn: &Connection, job: &JobDefinition) -> Result<bool> {
    let n = conn
        .execute(
            "UPDATE scheduled_tasks SET name=?2, prompt=?3, description=?4, project_path=?5, \
             provider_id=?6, model_id=?7, key_id=?8, agent_profile_id=?9, capability_refs=?10, \
             permission_profile=?11, max_steps=?12, effort=?13, runtime_id=?14, schedule_type=?15, \
             schedule_value=?16, next_run=?17, last_status=?18, last_run_at=?19, \
             consecutive_errors=?20, enabled=?21, expires_at=?22 WHERE id=?1",
            params![
                job.id,
                job.name,
                job.prompt,
                job.description,
                job.project_path,
                job.provider_id,
                job.model_id,
                job.key_id,
                job.agent_profile_id,
                job.capability_refs,
                job.permission_profile,
                job.max_steps,
                job.effort,
                job.runtime_id,
                job.schedule_type,
                job.schedule_value,
                job.next_run,
                job.last_status,
                job.last_run_at,
                job.consecutive_errors,
                job.enabled as i64,
                job.expires_at,
            ],
        )
        .map_err(Error::Database)?;
    Ok(n > 0)
}

pub fn delete_job(conn: &Connection, id: &str) -> Result<bool> {
    let n = conn
        .execute("DELETE FROM scheduled_tasks WHERE id = ?1", params![id])
        .map_err(Error::Database)?;
    Ok(n > 0)
}

/// 设置 enabled；重新启用时可同时刷新 next_run。返回是否命中。
pub fn set_enabled(
    conn: &Connection,
    id: &str,
    enabled: bool,
    next_run: Option<&str>,
) -> Result<bool> {
    let n = match next_run {
        Some(next) => conn
            .execute(
                "UPDATE scheduled_tasks SET enabled=?2, next_run=?3 WHERE id=?1",
                params![id, enabled as i64, next],
            )
            .map_err(Error::Database)?,
        None => conn
            .execute(
                "UPDATE scheduled_tasks SET enabled=?2 WHERE id=?1",
                params![id, enabled as i64],
            )
            .map_err(Error::Database)?,
    };
    Ok(n > 0)
}

/// 未接线推迟：更新 last_status 并推进 next_run（不写 run 行，契约第 4 节）。
pub fn mark_deferred(conn: &Connection, id: &str, last_status: &str, next_run: &str) -> Result<()> {
    conn.execute(
        "UPDATE scheduled_tasks SET last_status=?2, next_run=?3 WHERE id=?1",
        params![id, last_status, next_run],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// 停用并记录终态（once 已消费 / 表达式不可满足 / 过期）。
pub fn disable_with_status(
    conn: &Connection,
    id: &str,
    last_status: &str,
    last_run_at: Option<&str>,
) -> Result<()> {
    match last_run_at {
        Some(at) => conn
            .execute(
                "UPDATE scheduled_tasks SET enabled=0, last_status=?2, last_run_at=?3 WHERE id=?1",
                params![id, last_status, at],
            )
            .map_err(Error::Database)?,
        None => conn
            .execute(
                "UPDATE scheduled_tasks SET enabled=0, last_status=?2 WHERE id=?1",
                params![id, last_status],
            )
            .map_err(Error::Database)?,
    };
    Ok(())
}

/// 派发尝试后的状态推进：next_run=None 表示无下次（once），任务停用。
pub fn mark_dispatch_attempt(
    conn: &Connection,
    id: &str,
    last_status: &str,
    last_run_at: &str,
    next_run: Option<&str>,
) -> Result<()> {
    match next_run {
        Some(next) => conn
            .execute(
                "UPDATE scheduled_tasks SET last_status=?2, last_run_at=?3, next_run=?4 WHERE id=?1",
                params![id, last_status, last_run_at, next],
            )
            .map_err(Error::Database)?,
        None => conn
            .execute(
                "UPDATE scheduled_tasks SET enabled=0, last_status=?2, last_run_at=?3 WHERE id=?1",
                params![id, last_status, last_run_at],
            )
            .map_err(Error::Database)?,
    };
    Ok(())
}

/// 过期任务（expires_at < now）统一置 enabled=0、last_status='expired'。返回处理条数。
pub fn expire_overdue(conn: &Connection, now_iso: &str) -> Result<usize> {
    conn.execute(
        "UPDATE scheduled_tasks SET enabled=0, last_status='expired' \
         WHERE enabled=1 AND expires_at IS NOT NULL AND datetime(expires_at) < datetime(?1)",
        params![now_iso],
    )
    .map_err(Error::Database)
}

/// 到期任务：enabled 且 next_run<=now 且未过期。datetime() 归一化兼容
/// 旧 to_rfc3339()（+00:00）与新 Z 后缀两种写法。
pub fn list_due(conn: &Connection, now_iso: &str) -> Result<Vec<JobDefinition>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {JOB_COLUMNS} FROM scheduled_tasks \
             WHERE enabled=1 AND datetime(next_run) <= datetime(?1) \
             AND (expires_at IS NULL OR datetime(expires_at) >= datetime(?1)) \
             ORDER BY datetime(next_run) ASC"
        ))
        .map_err(Error::Database)?;
    let jobs = stmt
        .query_map(params![now_iso], job_from_row)
        .map_err(Error::Database)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Error::Database)?;
    Ok(jobs)
}

pub fn insert_run(conn: &Connection, run: &JobRunRecord) -> Result<()> {
    conn.execute(
        "INSERT INTO task_runs (id, task_id, started_at, finished_at, status, result_summary, \
         error, run_id, conversation_id, \"trigger\", error_code, detail) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        params![
            run.id,
            run.job_id,
            run.started_at,
            run.finished_at,
            run.status,
            run.result_summary,
            run.error,
            run.run_id,
            run.conversation_id,
            run.trigger,
            run.error_code,
            run.detail,
        ],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// 派发结果落到 run 行（wired 链路：pending → dispatched | dispatch_error）。
#[allow(clippy::too_many_arguments)]
pub fn update_run_result(
    conn: &Connection,
    row_id: &str,
    status: &str,
    run_id: Option<&str>,
    conversation_id: Option<&str>,
    error_code: Option<&str>,
    detail: Option<&str>,
    finished_at: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE task_runs SET status=?2, run_id=?3, conversation_id=?4, error_code=?5, \
         detail=?6, finished_at=?7 WHERE id=?1",
        params![
            row_id,
            status,
            run_id,
            conversation_id,
            error_code,
            detail,
            finished_at
        ],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// 运行历史分页。job_id=None 时跨任务列出。返回 (rows, total)。
pub fn list_runs(
    conn: &Connection,
    job_id: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<(Vec<JobRunRecord>, i64)> {
    let (total, rows) = match job_id {
        Some(jid) => {
            let total: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM task_runs WHERE task_id=?1",
                    params![jid],
                    |r| r.get(0),
                )
                .map_err(Error::Database)?;
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {RUN_COLUMNS} FROM task_runs WHERE task_id=?1 \
                     ORDER BY datetime(started_at) DESC, id DESC LIMIT ?2 OFFSET ?3"
                ))
                .map_err(Error::Database)?;
            let rows = stmt
                .query_map(params![jid, limit, offset], run_from_row)
                .map_err(Error::Database)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(Error::Database)?;
            (total, rows)
        }
        None => {
            let total: i64 = conn
                .query_row("SELECT COUNT(*) FROM task_runs", [], |r| r.get(0))
                .map_err(Error::Database)?;
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {RUN_COLUMNS} FROM task_runs \
                     ORDER BY datetime(started_at) DESC, id DESC LIMIT ?1 OFFSET ?2"
                ))
                .map_err(Error::Database)?;
            let rows = stmt
                .query_map(params![limit, offset], run_from_row)
                .map_err(Error::Database)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(Error::Database)?;
            (total, rows)
        }
    };
    Ok((rows, total))
}
