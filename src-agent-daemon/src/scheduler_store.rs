//! Persistent scheduler store + due-tick runner for Agent Daemon.
//!
//! Jobs are stored as JSON under `NATIVES_RUNTIME_DIR/scheduler/jobs.json`.
//! Interval and one-shot schedules are executed by [`SchedulerRunner`]; cron
//! expressions are persisted and listed, and fire on a coarse minute tick when
//! the expression is a simple `*/N * * * *` form (full cron later).

use chrono::{DateTime, Datelike, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleKind {
    Interval { every_secs: u64 },
    OneShot { at: DateTime<Utc> },
    Cron { expr: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerJob {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub schedule: ScheduleKind,
    pub project_path: Option<String>,
    pub provider_id: String,
    pub key_id: Option<String>,
    pub model_id: String,
    pub permission_profile: String,
    pub prompt: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_run_at: Option<DateTime<Utc>>,
    pub last_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSchedulerJob {
    pub name: String,
    pub schedule: ScheduleKind,
    pub project_path: Option<String>,
    pub provider_id: String,
    pub key_id: Option<String>,
    pub model_id: String,
    pub permission_profile: Option<String>,
    pub prompt: String,
    pub enabled: Option<bool>,
}

#[derive(Default)]
pub struct SchedulerStore {
    jobs: Mutex<HashMap<String, SchedulerJob>>,
    path: PathBuf,
}

impl SchedulerStore {
    pub fn open_default() -> Self {
        let dir = std::env::var("NATIVES_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .map(|h| PathBuf::from(h).join(".natives").join("runtime"))
                    .unwrap_or_else(|| std::env::temp_dir().join("natives-runtime"))
            })
            .join("scheduler");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("jobs.json");
        let mut store = Self {
            jobs: Mutex::new(HashMap::new()),
            path,
        };
        store.load();
        store
    }

    fn load(&mut self) {
        let Ok(raw) = std::fs::read_to_string(&self.path) else {
            return;
        };
        if let Ok(map) = serde_json::from_str::<HashMap<String, SchedulerJob>>(&raw) {
            if let Ok(mut jobs) = self.jobs.lock() {
                *jobs = map;
            }
        }
    }

    fn persist(&self) -> Result<(), String> {
        let jobs = self.jobs.lock().map_err(|e| e.to_string())?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let raw = serde_json::to_string_pretty(&*jobs).map_err(|e| e.to_string())?;
        std::fs::write(&self.path, raw).map_err(|e| e.to_string())
    }

    pub fn list(&self) -> Vec<SchedulerJob> {
        self.jobs
            .lock()
            .map(|g| g.values().cloned().collect())
            .unwrap_or_default()
    }

    pub fn create(&self, req: CreateSchedulerJob) -> Result<SchedulerJob, String> {
        if req.name.trim().is_empty() {
            return Err("name required".into());
        }
        if req.provider_id.trim().is_empty() || req.model_id.trim().is_empty() {
            return Err("provider_id and model_id required".into());
        }
        let now = Utc::now();
        let job = SchedulerJob {
            id: Uuid::new_v4().to_string(),
            name: req.name,
            enabled: req.enabled.unwrap_or(true),
            schedule: req.schedule,
            project_path: req.project_path,
            provider_id: req.provider_id,
            key_id: req.key_id,
            model_id: req.model_id,
            permission_profile: req
                .permission_profile
                .unwrap_or_else(|| "ask".into()),
            prompt: req.prompt,
            created_at: now,
            updated_at: now,
            last_run_at: None,
            last_status: None,
        };
        {
            let mut jobs = self.jobs.lock().map_err(|e| e.to_string())?;
            jobs.insert(job.id.clone(), job.clone());
        }
        self.persist()?;
        Ok(job)
    }

    pub fn update(&self, id: &str, patch: serde_json::Value) -> Result<SchedulerJob, String> {
        let mut jobs = self.jobs.lock().map_err(|e| e.to_string())?;
        let job = jobs
            .get_mut(id)
            .ok_or_else(|| format!("scheduler job not found: {id}"))?;
        if let Some(name) = patch.get("name").and_then(|v| v.as_str()) {
            job.name = name.to_string();
        }
        if let Some(enabled) = patch.get("enabled").and_then(|v| v.as_bool()) {
            job.enabled = enabled;
        }
        if let Some(prompt) = patch.get("prompt").and_then(|v| v.as_str()) {
            job.prompt = prompt.to_string();
        }
        if let Some(pp) = patch.get("permission_profile").and_then(|v| v.as_str()) {
            job.permission_profile = pp.to_string();
        }
        job.updated_at = Utc::now();
        let out = job.clone();
        drop(jobs);
        self.persist()?;
        Ok(out)
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        {
            let mut jobs = self.jobs.lock().map_err(|e| e.to_string())?;
            if jobs.remove(id).is_none() {
                return Err(format!("scheduler job not found: {id}"));
            }
        }
        self.persist()
    }

    /// Jobs that should fire at `now` (enabled + schedule due).
    pub fn due_jobs(&self, now: DateTime<Utc>) -> Vec<SchedulerJob> {
        self.list()
            .into_iter()
            .filter(|j| j.enabled && is_due(j, now))
            .collect()
    }

    /// Mark a job as fired (updates last_run_at / last_status and persists).
    pub fn mark_fired(&self, id: &str, status: &str) -> Result<SchedulerJob, String> {
        let mut jobs = self.jobs.lock().map_err(|e| e.to_string())?;
        let job = jobs
            .get_mut(id)
            .ok_or_else(|| format!("scheduler job not found: {id}"))?;
        job.last_run_at = Some(Utc::now());
        job.last_status = Some(status.to_string());
        job.updated_at = Utc::now();
        // One-shot: disable after fire.
        if matches!(job.schedule, ScheduleKind::OneShot { .. }) {
            job.enabled = false;
        }
        let out = job.clone();
        drop(jobs);
        self.persist()?;
        Ok(out)
    }

    /// History-friendly list of last statuses (for RPC / UI).
    pub fn history(&self, limit: usize) -> Vec<serde_json::Value> {
        let mut jobs = self.list();
        jobs.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        jobs.into_iter()
            .filter(|j| j.last_run_at.is_some())
            .take(limit.max(1))
            .map(|j| {
                serde_json::json!({
                    "job_id": j.id,
                    "name": j.name,
                    "last_run_at": j.last_run_at,
                    "last_status": j.last_status,
                    "enabled": j.enabled,
                })
            })
            .collect()
    }
}

fn is_due(job: &SchedulerJob, now: DateTime<Utc>) -> bool {
    match &job.schedule {
        ScheduleKind::OneShot { at } => {
            now >= *at
                && job
                    .last_run_at
                    .map(|t| t < *at)
                    .unwrap_or(true)
        }
        ScheduleKind::Interval { every_secs } => {
            let every = (*every_secs).max(1);
            match job.last_run_at {
                None => true,
                Some(last) => (now - last).num_seconds() as u64 >= every,
            }
        }
        ScheduleKind::Cron { expr } => {
            if now.second() > 5 {
                // Coarse tick: only evaluate near the top of a minute.
                return false;
            }
            if !cron_matches(expr, now) {
                return false;
            }
            match job.last_run_at {
                None => true,
                // Avoid double-fire within the same minute.
                Some(last) => (now - last).num_seconds() >= 55,
            }
        }
    }
}

/// 5-field cron: `minute hour day-of-month month day-of-week`
/// Supports: `*`, `N`, `*/N`, `A-B`, `A,B,C` for each field.
fn cron_matches(expr: &str, now: DateTime<Utc>) -> bool {
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() < 5 {
        return false;
    }
    let minute = now.minute() as u32;
    let hour = now.hour() as u32;
    let day = now.day();
    let month = now.month();
    // chrono: Sunday=0 ... Saturday=6; many crons use 0/7=Sunday
    let dow = now.weekday().num_days_from_sunday();
    field_matches(parts[0], minute, 0, 59)
        && field_matches(parts[1], hour, 0, 23)
        && field_matches(parts[2], day, 1, 31)
        && field_matches(parts[3], month, 1, 12)
        && field_matches(parts[4], dow, 0, 7)
}

fn field_matches(field: &str, value: u32, min: u32, max: u32) -> bool {
    if field == "*" {
        return true;
    }
    // lists
    if field.contains(',') {
        return field.split(',').any(|f| field_matches(f.trim(), value, min, max));
    }
    // step
    if let Some((base, step_s)) = field.split_once('/') {
        let step: u32 = match step_s.parse() {
            Ok(s) if s > 0 => s,
            _ => return false,
        };
        if base == "*" {
            return value >= min && (value - min) % step == 0;
        }
        if let Some((a, b)) = base.split_once('-') {
            let a: u32 = match a.parse() {
                Ok(v) => v,
                Err(_) => return false,
            };
            let b: u32 = match b.parse() {
                Ok(v) => v,
                Err(_) => return false,
            };
            return value >= a && value <= b && (value - a) % step == 0;
        }
        let start: u32 = match base.parse() {
            Ok(v) => v,
            Err(_) => return false,
        };
        return value >= start && (value - start) % step == 0;
    }
    // range
    if let Some((a, b)) = field.split_once('-') {
        let a: u32 = match a.parse() {
            Ok(v) => v,
            Err(_) => return false,
        };
        let b: u32 = match b.parse() {
            Ok(v) => v,
            Err(_) => return false,
        };
        return value >= a && value <= b && a >= min && b <= max;
    }
    // exact; allow 7 as Sunday alias for DOW
    match field.parse::<u32>() {
        Ok(n) if n == value => true,
        Ok(7) if max == 7 && value == 0 => true,
        _ => false,
    }
}

#[cfg(test)]
fn parse_every_n_minutes(expr: &str) -> Option<u64> {
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() < 5 {
        return None;
    }
    let min = parts[0];
    if let Some(rest) = min.strip_prefix("*/") {
        return rest.parse().ok().filter(|n| *n > 0);
    }
    None
}

/// Background runner: on each tick, fire due jobs via RunManager.
pub struct SchedulerRunner {
    stop: AtomicBool,
}

impl Default for SchedulerRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl SchedulerRunner {
    pub fn new() -> Self {
        Self {
            stop: AtomicBool::new(false),
        }
    }

    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    /// Process due jobs once (testable without sleep loop).
    pub fn tick_once(&self) -> Result<Vec<String>, String> {
        let now = Utc::now();
        let due = global_scheduler().due_jobs(now);
        let mut fired = Vec::new();
        for job in due {
            match fire_job(&job) {
                Ok(run_id) => {
                    let _ = global_scheduler().mark_fired(&job.id, &format!("started:{run_id}"));
                    fired.push(run_id);
                }
                Err(e) => {
                    let _ = global_scheduler().mark_fired(&job.id, &format!("failed:{e}"));
                }
            }
        }
        Ok(fired)
    }

    /// Spawn a tokio task that ticks every `interval_secs` until stop.
    pub fn spawn_loop(self: &std::sync::Arc<Self>, interval_secs: u64) {
        let this = std::sync::Arc::clone(self);
        tokio::spawn(async move {
            let mut ticker =
                tokio::time::interval(std::time::Duration::from_secs(interval_secs.max(1)));
            loop {
                ticker.tick().await;
                if this.stop.load(Ordering::SeqCst) {
                    break;
                }
                let _ = this.tick_once();
            }
        });
    }
}

fn fire_job(job: &SchedulerJob) -> Result<String, String> {
    use assistant_protocol::v2::{CreateRunRequest, StartRunRequest};
    use crate::run_manager::{global_run_manager, RunManager};

    if job.project_path.as_ref().map(|s| s.trim().is_empty()).unwrap_or(true) {
        return Err("scheduler job missing project_path".into());
    }
    let conv = format!("sched-{}", job.id);
    let run = global_run_manager().create_run(CreateRunRequest {
            capability_selection: None,
        conversation_id: conv,
        provider_id: job.provider_id.clone(),
        model_id: job.model_id.clone(),
        key_id: job.key_id.clone(),
        agent_profile_id: None,
        permission_profile: Some(job.permission_profile.clone()),
        content: Some(job.prompt.clone()),
        attachments: None,
        max_steps: Some(30),
        parent_run_id: None,
        project_path: job.project_path.clone(),
        idempotency_key: Some(format!(
            "sched-{}-{}",
            job.id,
            Utc::now().timestamp()
        )),
                effort: None,
            runtime_id: None,
        })?;
    let started = RunManager::start_detached_global(StartRunRequest {
            agent_profile_id: None,
            capability_selection: None,
        run_id: Some(run.id.clone()),
        conversation_id: Some(run.conversation_id.clone()),
        provider_id: Some(run.provider_id.clone()),
        model_id: Some(run.model_id.clone()),
        key_id: run.key_id.clone(),
        content: Some(job.prompt.clone()),
        attachments: None,
        trigger_message_id: None,
        permission_profile: Some(run.permission_profile.clone()),
        max_steps: Some(run.max_steps),
        project_path: job.project_path.clone(),
        idempotency_key: None,
                effort: None,
            runtime_id: None,
        })?;
    Ok(started.id)
}

static GLOBAL_SCHEDULER: std::sync::OnceLock<SchedulerStore> = std::sync::OnceLock::new();
static GLOBAL_RUNNER: std::sync::OnceLock<std::sync::Arc<SchedulerRunner>> =
    std::sync::OnceLock::new();

pub fn global_scheduler() -> &'static SchedulerStore {
    GLOBAL_SCHEDULER.get_or_init(SchedulerStore::open_default)
}

/// Start process-wide scheduler runner (idempotent).
pub fn ensure_scheduler_runner() -> std::sync::Arc<SchedulerRunner> {
    GLOBAL_RUNNER
        .get_or_init(|| {
            let runner = std::sync::Arc::new(SchedulerRunner::new());
            runner.spawn_loop(15);
            runner
        })
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_list_delete_round_trip() {
        let dir = std::env::temp_dir().join(format!(
            "natives-sched-{}",
            Uuid::new_v4()
        ));
        let _ = std::fs::create_dir_all(&dir);
        std::env::set_var("NATIVES_RUNTIME_DIR", &dir);
        let store = SchedulerStore {
            jobs: Mutex::new(HashMap::new()),
            path: dir.join("scheduler").join("jobs.json"),
        };
        let job = store
            .create(CreateSchedulerJob {
                name: "daily".into(),
                schedule: ScheduleKind::Interval { every_secs: 3600 },
                project_path: Some("/tmp/p".into()),
                provider_id: "openai".into(),
                key_id: Some("k1".into()),
                model_id: "gpt-4o".into(),
                permission_profile: Some("ask".into()),
                prompt: "ping".into(),
                enabled: Some(true),
            })
            .unwrap();
        assert_eq!(store.list().len(), 1);
        store.delete(&job.id).unwrap();
        assert!(store.list().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn interval_due_and_mark_fired() {
        let dir = std::env::temp_dir().join(format!("natives-sched-due-{}", Uuid::new_v4()));
        let store = SchedulerStore {
            jobs: Mutex::new(HashMap::new()),
            path: dir.join("jobs.json"),
        };
        let job = store
            .create(CreateSchedulerJob {
                name: "tick".into(),
                schedule: ScheduleKind::Interval { every_secs: 60 },
                project_path: Some("/tmp/p".into()),
                provider_id: "openai".into(),
                key_id: None,
                model_id: "m".into(),
                permission_profile: None,
                prompt: "hi".into(),
                enabled: Some(true),
            })
            .unwrap();
        let due = store.due_jobs(Utc::now());
        assert!(due.iter().any(|j| j.id == job.id));
        store.mark_fired(&job.id, "started:r1").unwrap();
        let after = store.due_jobs(Utc::now());
        assert!(!after.iter().any(|j| j.id == job.id));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn oneshot_disables_after_fire() {
        let dir = std::env::temp_dir().join(format!("natives-sched-os-{}", Uuid::new_v4()));
        let store = SchedulerStore {
            jobs: Mutex::new(HashMap::new()),
            path: dir.join("jobs.json"),
        };
        let at = Utc::now() - chrono::Duration::seconds(5);
        let job = store
            .create(CreateSchedulerJob {
                name: "once".into(),
                schedule: ScheduleKind::OneShot { at },
                project_path: Some("/tmp/p".into()),
                provider_id: "openai".into(),
                key_id: None,
                model_id: "m".into(),
                permission_profile: None,
                prompt: "once".into(),
                enabled: Some(true),
            })
            .unwrap();
        assert!(store.due_jobs(Utc::now()).iter().any(|j| j.id == job.id));
        let updated = store.mark_fired(&job.id, "ok").unwrap();
        assert!(!updated.enabled);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_cron_every_n() {
        assert_eq!(parse_every_n_minutes("*/5 * * * *"), Some(5));
        assert_eq!(parse_every_n_minutes("0 0 * * *"), None);
    }

    #[test]
    fn cron_field_matcher_supports_lists_ranges_steps() {
        // 2026-07-17 10:05:00 UTC → weekday Friday=5
        let now = chrono::DateTime::parse_from_rfc3339("2026-07-17T10:05:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(cron_matches("*/5 * * * *", now));
        assert!(cron_matches("5 10 * * *", now));
        assert!(cron_matches("0-10 10 * * 5", now));
        assert!(cron_matches("5,15,25 10 * * *", now));
        assert!(!cron_matches("0 10 * * *", now));
        assert!(!cron_matches("5 11 * * *", now));
    }
}
