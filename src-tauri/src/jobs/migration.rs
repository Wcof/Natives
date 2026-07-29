//! One-time import of the retired Daemon scheduler JSON into Host-owned jobs.

use super::store::JobDefinition;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum LegacyScheduleKind {
    Interval { every_secs: u64 },
    OneShot { at: DateTime<Utc> },
    Cron { expr: String },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct LegacySchedulerJob {
    id: String,
    name: String,
    enabled: bool,
    schedule: LegacyScheduleKind,
    project_path: Option<String>,
    provider_id: String,
    key_id: Option<String>,
    model_id: String,
    permission_profile: String,
    prompt: String,
    created_at: DateTime<Utc>,
    last_run_at: Option<DateTime<Utc>>,
    last_status: Option<String>,
}

fn map_legacy_job(legacy: LegacySchedulerJob, now: DateTime<Utc>) -> Result<JobDefinition, String> {
    let last_run_at = legacy.last_run_at;
    let (schedule_type, schedule_value, next_run) = match legacy.schedule {
        LegacyScheduleKind::OneShot { at } => {
            let at = super::schedule::format_utc(&at);
            ("once".to_string(), at.clone(), at)
        }
        LegacyScheduleKind::Interval { every_secs } => {
            let schedule_value = every_secs.to_string();
            super::schedule::parse("interval", &schedule_value)?;
            let next = match last_run_at {
                Some(last) => {
                    let seconds = i64::try_from(every_secs)
                        .map_err(|_| "legacy interval exceeds supported range".to_string())?;
                    last.checked_add_signed(chrono::Duration::seconds(seconds))
                        .ok_or_else(|| "legacy interval next_run overflow".to_string())?
                }
                None => now,
            };
            (
                "interval".to_string(),
                schedule_value,
                super::schedule::format_utc(&next),
            )
        }
        LegacyScheduleKind::Cron { expr } => {
            let spec = super::schedule::parse("cron", &expr)?;
            let next = super::schedule::next_run(&spec, now)
                .ok_or_else(|| "legacy cron schedule has no next run".to_string())?;
            ("cron".to_string(), expr, super::schedule::format_utc(&next))
        }
    };
    Ok(JobDefinition {
        id: legacy.id,
        name: legacy.name,
        prompt: legacy.prompt,
        description: None,
        project_path: legacy.project_path,
        provider_id: Some(legacy.provider_id),
        model_id: Some(legacy.model_id),
        key_id: legacy.key_id,
        agent_profile_id: None,
        capability_refs: r#"{"skills":[],"mcp_servers":[]}"#.to_string(),
        permission_profile: legacy.permission_profile,
        max_steps: Some(30),
        effort: None,
        runtime_id: Some("native".to_string()),
        schedule_type,
        schedule_value,
        next_run,
        last_status: legacy.last_status,
        last_run_at: last_run_at.as_ref().map(super::schedule::format_utc),
        consecutive_errors: 0,
        enabled: legacy.enabled,
        created_at: super::schedule::format_utc(&legacy.created_at),
        expires_at: None,
    })
}

fn jobs_equivalent(existing: &JobDefinition, mapped: &JobDefinition) -> bool {
    existing.id == mapped.id
        && existing.name == mapped.name
        && existing.prompt == mapped.prompt
        && existing.description == mapped.description
        && existing.project_path == mapped.project_path
        && existing.provider_id == mapped.provider_id
        && existing.model_id == mapped.model_id
        && existing.key_id == mapped.key_id
        && existing.agent_profile_id == mapped.agent_profile_id
        && existing.capability_refs == mapped.capability_refs
        && existing.permission_profile == mapped.permission_profile
        && existing.max_steps == mapped.max_steps
        && existing.effort == mapped.effort
        && existing.runtime_id == mapped.runtime_id
        && existing.schedule_type == mapped.schedule_type
        && existing.schedule_value == mapped.schedule_value
        && existing.last_status == mapped.last_status
        && existing.last_run_at == mapped.last_run_at
        && existing.enabled == mapped.enabled
        && existing.created_at == mapped.created_at
        && existing.expires_at == mapped.expires_at
}

fn import_legacy_jobs(
    conn: &mut rusqlite::Connection,
    jobs: &HashMap<String, LegacySchedulerJob>,
    now: DateTime<Utc>,
) -> Result<usize, String> {
    let tx = conn.transaction().map_err(|error| error.to_string())?;
    let mut ids: Vec<&String> = jobs.keys().collect();
    ids.sort();
    let mut imported = 0;
    for source_id in ids {
        let legacy = jobs
            .get(source_id)
            .cloned()
            .ok_or_else(|| format!("legacy scheduler job disappeared: {source_id}"))?;
        if legacy.id != *source_id {
            return Err(format!(
                "JOB_MIGRATION_CONFLICT: map key '{source_id}' does not match job id '{}'",
                legacy.id
            ));
        }
        let mapped = map_legacy_job(legacy, now)
            .map_err(|error| format!("JOB_MIGRATION_INVALID_SCHEDULE: {source_id}: {error}"))?;
        if let Some(existing) =
            super::store::get_job(&tx, source_id).map_err(|error| error.to_string())?
        {
            if jobs_equivalent(&existing, &mapped) {
                continue;
            }
            return Err(format!(
                "JOB_MIGRATION_CONFLICT: Host job '{source_id}' differs from legacy scheduler data"
            ));
        }
        super::store::insert_job(&tx, &mapped).map_err(|error| error.to_string())?;
        imported += 1;
    }
    tx.commit().map_err(|error| error.to_string())?;
    Ok(imported)
}

#[derive(Debug, Clone)]
pub struct MigrationReport {
    pub source_found: bool,
    pub already_migrated: bool,
    pub imported: usize,
    pub backup_path: Option<PathBuf>,
    pub marker_path: PathBuf,
}

fn migration_paths(source: &Path) -> (PathBuf, PathBuf) {
    let file_name = source
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("jobs.json");
    (
        source.with_file_name(format!("{file_name}.host-migration-v1.bak")),
        source.with_file_name(format!("{file_name}.host-migration-v1.complete")),
    )
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn validate_completion_marker(
    marker_path: &Path,
    backup_path: &Path,
    source_path: &Path,
) -> Result<(), String> {
    let raw = std::fs::read(marker_path)
        .map_err(|error| format!("JOB_MIGRATION_INVALID_MARKER: {error}"))?;
    let marker: serde_json::Value = serde_json::from_slice(&raw)
        .map_err(|error| format!("JOB_MIGRATION_INVALID_MARKER: {error}"))?;
    if marker.get("version").and_then(serde_json::Value::as_u64) != Some(1) {
        return Err("JOB_MIGRATION_INVALID_MARKER: unsupported or missing version".to_string());
    }
    if !backup_path.exists() {
        return Err(
            "JOB_MIGRATION_INVALID_MARKER: completion marker exists without backup".to_string(),
        );
    }
    let backup = std::fs::read(backup_path)
        .map_err(|error| format!("JOB_MIGRATION_INVALID_MARKER: {error}"))?;
    if let Some(expected) = marker
        .get("source_sha256")
        .and_then(serde_json::Value::as_str)
    {
        if sha256_hex(&backup) != expected {
            return Err(
                "JOB_MIGRATION_INVALID_MARKER: backup digest differs from marker".to_string(),
            );
        }
    } else if source_path.exists() {
        let source = std::fs::read(source_path)
            .map_err(|error| format!("JOB_MIGRATION_INVALID_MARKER: {error}"))?;
        if source != backup {
            return Err(
                "JOB_MIGRATION_INVALID_MARKER: backup differs from preserved source".to_string(),
            );
        }
    } else {
        return Err(
            "JOB_MIGRATION_INVALID_MARKER: marker has no digest and source is missing".to_string(),
        );
    }
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8], readonly: bool, code: &str) -> Result<(), String> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("migration-artifact");
    let temp_path = path.with_file_name(format!(".{file_name}.{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        std::fs::write(&temp_path, bytes).map_err(|error| format!("{code}: {error}"))?;
        if readonly {
            let mut permissions = std::fs::metadata(&temp_path)
                .map_err(|error| format!("{code}: {error}"))?
                .permissions();
            permissions.set_readonly(true);
            std::fs::set_permissions(&temp_path, permissions)
                .map_err(|error| format!("{code}: {error}"))?;
        }
        std::fs::rename(&temp_path, path).map_err(|error| format!("{code}: {error}"))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    result
}

pub fn migrate_legacy_scheduler_jobs(
    conn: &mut rusqlite::Connection,
    source: &Path,
    now: DateTime<Utc>,
) -> Result<MigrationReport, String> {
    let (backup_path, marker_path) = migration_paths(source);
    if marker_path.exists() {
        validate_completion_marker(&marker_path, &backup_path, source)?;
        return Ok(MigrationReport {
            source_found: source.exists(),
            already_migrated: true,
            imported: 0,
            backup_path: backup_path.exists().then_some(backup_path),
            marker_path,
        });
    }
    if !source.exists() {
        return Ok(MigrationReport {
            source_found: false,
            already_migrated: false,
            imported: 0,
            backup_path: None,
            marker_path,
        });
    }

    let raw =
        std::fs::read(source).map_err(|error| format!("JOB_MIGRATION_READ_FAILED: {error}"))?;
    let jobs: HashMap<String, LegacySchedulerJob> = serde_json::from_slice(&raw)
        .map_err(|error| format!("JOB_MIGRATION_INVALID_JSON: {error}"))?;
    let imported = import_legacy_jobs(conn, &jobs, now)?;

    if backup_path.exists() {
        let existing = std::fs::read(&backup_path)
            .map_err(|error| format!("JOB_MIGRATION_BACKUP_FAILED: {error}"))?;
        if existing != raw {
            return Err(
                "JOB_MIGRATION_BACKUP_CONFLICT: existing backup differs from source".to_string(),
            );
        }
    } else {
        write_atomic(&backup_path, &raw, true, "JOB_MIGRATION_BACKUP_FAILED")?;
    }
    let mut permissions = std::fs::metadata(&backup_path)
        .map_err(|error| format!("JOB_MIGRATION_BACKUP_FAILED: {error}"))?
        .permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&backup_path, permissions)
        .map_err(|error| format!("JOB_MIGRATION_BACKUP_FAILED: {error}"))?;

    let marker = serde_json::json!({
        "version": 1,
        "completed_at": super::schedule::format_utc(&now),
        "imported": imported,
        "source": source,
        "backup": backup_path,
        "source_sha256": sha256_hex(&raw),
    });
    let marker_bytes = serde_json::to_vec_pretty(&marker)
        .map_err(|error| format!("JOB_MIGRATION_MARKER_FAILED: {error}"))?;
    write_atomic(
        &marker_path,
        &marker_bytes,
        false,
        "JOB_MIGRATION_MARKER_FAILED",
    )?;

    Ok(MigrationReport {
        source_found: true,
        already_migrated: false,
        imported,
        backup_path: Some(backup_path),
        marker_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, s)
            .single()
            .expect("valid utc")
    }

    fn legacy_job(schedule: LegacyScheduleKind) -> LegacySchedulerJob {
        LegacySchedulerJob {
            id: "legacy-job".to_string(),
            name: "Legacy job".to_string(),
            enabled: true,
            schedule,
            project_path: Some("/tmp".to_string()),
            provider_id: "provider-a".to_string(),
            key_id: Some("key-a".to_string()),
            model_id: "model-a".to_string(),
            permission_profile: "ask".to_string(),
            prompt: "run legacy work".to_string(),
            created_at: utc(2026, 7, 1, 0, 0, 0),
            last_run_at: None,
            last_status: Some("ready".to_string()),
        }
    }

    #[test]
    fn one_shot_mapping_preserves_execution_fields_and_identity() {
        let at = utc(2026, 8, 1, 0, 0, 0);
        let mapped = map_legacy_job(
            legacy_job(LegacyScheduleKind::OneShot { at }),
            utc(2026, 7, 29, 0, 0, 0),
        )
        .expect("map");

        assert_eq!(mapped.id, "legacy-job");
        assert_eq!(mapped.prompt, "run legacy work");
        assert_eq!(mapped.provider_id.as_deref(), Some("provider-a"));
        assert_eq!(mapped.model_id.as_deref(), Some("model-a"));
        assert_eq!(mapped.key_id.as_deref(), Some("key-a"));
        assert_eq!(mapped.permission_profile, "ask");
        assert_eq!(mapped.schedule_type, "once");
        assert_eq!(mapped.schedule_value, "2026-08-01T00:00:00Z");
        assert_eq!(mapped.next_run, "2026-08-01T00:00:00Z");
        assert!(mapped.enabled);
        assert_eq!(mapped.last_status.as_deref(), Some("ready"));
    }

    #[test]
    fn interval_mapping_continues_from_the_last_real_run_time() {
        let mut legacy = legacy_job(LegacyScheduleKind::Interval { every_secs: 3600 });
        legacy.last_run_at = Some(utc(2026, 7, 29, 1, 30, 0));

        let mapped = map_legacy_job(legacy, utc(2026, 7, 29, 2, 0, 0)).expect("map");

        assert_eq!(mapped.schedule_type, "interval");
        assert_eq!(mapped.schedule_value, "3600");
        assert_eq!(mapped.next_run, "2026-07-29T02:30:00Z");
        assert_eq!(mapped.last_run_at.as_deref(), Some("2026-07-29T01:30:00Z"));
    }

    #[test]
    fn cron_mapping_uses_the_host_scheduler_for_next_run() {
        let mapped = map_legacy_job(
            legacy_job(LegacyScheduleKind::Cron {
                expr: "*/15 * * * *".to_string(),
            }),
            utc(2026, 7, 29, 2, 7, 0),
        )
        .expect("map");

        assert_eq!(mapped.schedule_type, "cron");
        assert_eq!(mapped.schedule_value, "*/15 * * * *");
        assert_eq!(mapped.next_run, "2026-07-29T02:15:00Z");
    }

    #[test]
    fn transactional_import_is_idempotent_by_legacy_job_id() {
        let mut conn = rusqlite::Connection::open_in_memory().expect("db");
        super::super::store::ensure_schema(&conn).expect("schema");
        let legacy = legacy_job(LegacyScheduleKind::Interval { every_secs: 3600 });
        let jobs = HashMap::from([(legacy.id.clone(), legacy)]);
        let now = utc(2026, 7, 29, 2, 0, 0);

        assert_eq!(
            import_legacy_jobs(&mut conn, &jobs, now).expect("first import"),
            1
        );
        assert_eq!(
            import_legacy_jobs(&mut conn, &jobs, now).expect("repeat import"),
            0
        );
        assert!(super::super::store::get_job(&conn, "legacy-job")
            .expect("get")
            .is_some());
    }

    #[test]
    fn conflicting_id_rolls_back_every_job_in_the_import_batch() {
        let mut conn = rusqlite::Connection::open_in_memory().expect("db");
        super::super::store::ensure_schema(&conn).expect("schema");
        let now = utc(2026, 7, 29, 2, 0, 0);

        let mut source_a = legacy_job(LegacyScheduleKind::Interval { every_secs: 3600 });
        source_a.id = "a".to_string();
        let mut source_b = legacy_job(LegacyScheduleKind::Interval { every_secs: 3600 });
        source_b.id = "b".to_string();
        let jobs = HashMap::from([
            (source_a.id.clone(), source_a),
            (source_b.id.clone(), source_b.clone()),
        ]);

        let mut existing_b = map_legacy_job(source_b, now).expect("map existing");
        existing_b.name = "Different Host job".to_string();
        super::super::store::insert_job(&conn, &existing_b).expect("insert conflict");

        let error = import_legacy_jobs(&mut conn, &jobs, now).expect_err("conflict");

        assert!(error.contains("JOB_MIGRATION_CONFLICT"));
        assert!(
            super::super::store::get_job(&conn, "a")
                .expect("get a")
                .is_none(),
            "job inserted before the conflict must be rolled back"
        );
        assert_eq!(
            super::super::store::get_job(&conn, "b")
                .expect("get b")
                .expect("existing b")
                .name,
            "Different Host job"
        );
    }

    #[test]
    fn file_migration_keeps_read_only_backup_and_writes_completion_marker() {
        let temp = tempfile::tempdir().expect("temp");
        let scheduler_dir = temp.path().join("scheduler");
        std::fs::create_dir_all(&scheduler_dir).expect("scheduler dir");
        let source = scheduler_dir.join("jobs.json");
        let legacy = legacy_job(LegacyScheduleKind::Interval { every_secs: 3600 });
        let jobs = HashMap::from([(legacy.id.clone(), legacy)]);
        std::fs::write(
            &source,
            serde_json::to_vec_pretty(&jobs).expect("serialize jobs"),
        )
        .expect("write source");
        let mut conn = rusqlite::Connection::open_in_memory().expect("db");
        super::super::store::ensure_schema(&conn).expect("schema");

        let report = migrate_legacy_scheduler_jobs(&mut conn, &source, utc(2026, 7, 29, 2, 0, 0))
            .expect("migrate");

        assert_eq!(report.imported, 1);
        assert!(source.exists(), "source JSON remains untouched");
        assert!(report
            .backup_path
            .as_ref()
            .is_some_and(|path| path.exists()));
        assert!(report.marker_path.exists());
        assert!(report
            .backup_path
            .as_ref()
            .expect("backup")
            .metadata()
            .expect("metadata")
            .permissions()
            .readonly());

        let repeated = migrate_legacy_scheduler_jobs(&mut conn, &source, utc(2026, 7, 29, 3, 0, 0))
            .expect("repeat");
        assert!(repeated.already_migrated);
        assert_eq!(repeated.imported, 0);
    }

    #[test]
    fn corrupt_completion_marker_never_skips_migration_silently() {
        let temp = tempfile::tempdir().expect("temp");
        let source = temp.path().join("jobs.json");
        let (_, marker) = migration_paths(&source);
        std::fs::write(&marker, b"{incomplete").expect("marker");
        let mut conn = rusqlite::Connection::open_in_memory().expect("db");
        super::super::store::ensure_schema(&conn).expect("schema");

        let error = migrate_legacy_scheduler_jobs(&mut conn, &source, utc(2026, 7, 29, 2, 0, 0))
            .expect_err("invalid marker must fail closed");

        assert!(error.contains("JOB_MIGRATION_INVALID_MARKER"));
    }

    #[test]
    fn preexisting_backup_must_match_the_legacy_source_before_marking_complete() {
        let temp = tempfile::tempdir().expect("temp");
        let source = temp.path().join("jobs.json");
        let (backup, marker) = migration_paths(&source);
        std::fs::write(&source, b"{}").expect("source");
        std::fs::write(&backup, b"{different}").expect("backup");
        let mut conn = rusqlite::Connection::open_in_memory().expect("db");
        super::super::store::ensure_schema(&conn).expect("schema");

        let error = migrate_legacy_scheduler_jobs(&mut conn, &source, utc(2026, 7, 29, 2, 0, 0))
            .expect_err("mismatched backup must fail closed");

        assert!(error.contains("JOB_MIGRATION_BACKUP_CONFLICT"));
        assert!(!marker.exists());
    }

    #[test]
    fn completion_marker_does_not_hide_a_tampered_backup() {
        let temp = tempfile::tempdir().expect("temp");
        let source = temp.path().join("jobs.json");
        let (backup, marker) = migration_paths(&source);
        std::fs::write(&source, b"{}").expect("source");
        std::fs::write(&backup, b"{tampered}").expect("backup");
        std::fs::write(&marker, br#"{"version":1}"#).expect("marker");
        let mut conn = rusqlite::Connection::open_in_memory().expect("db");
        super::super::store::ensure_schema(&conn).expect("schema");

        let error = migrate_legacy_scheduler_jobs(&mut conn, &source, utc(2026, 7, 29, 2, 0, 0))
            .expect_err("a completion marker must not bypass backup verification");

        assert!(error.contains("JOB_MIGRATION_INVALID_MARKER"));
    }
}
