//! SQLite persistence for local creative apps + encrypted env.
//!
//! Does not touch project source directories. Delete only removes Natives records
//! and app logs (log cleanup is phase 2).

use super::path::{device_id, device_name};
use crate::creative_app::model::*;
use crate::env_manager;
use crate::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

pub type LocalStoreError = Error;

pub fn insert_app(conn: &Connection, rec: &LocalCreativeAppRecord) -> Result<()> {
    conn.execute(
        "INSERT INTO local_creative_apps (
            id, title, description, icon, canonical_project_root, device_id, device_name,
            project_kind, launch_mode, launch_plan_json, plan_fingerprint, state,
            status_detail_json, open_url, current_port, process_identity_json,
            auto_open, startup_timeout_ms, last_started_at, last_exit_reason, last_error,
            created_at, updated_at
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23)",
        params![
            rec.id,
            rec.title,
            rec.description,
            rec.icon,
            rec.canonical_project_root,
            rec.device_id,
            rec.device_name,
            rec.project_kind.as_str(),
            rec.launch_mode.as_str(),
            rec.launch_plan_json,
            rec.plan_fingerprint,
            rec.state.as_str(),
            rec.status_detail_json,
            rec.open_url,
            rec.current_port.map(|p| p as i64),
            rec.process_identity_json,
            if rec.auto_open { 1 } else { 0 },
            rec.startup_timeout_ms as i64,
            rec.last_started_at,
            rec.last_exit_reason,
            rec.last_error,
            rec.created_at,
            rec.updated_at,
        ],
    )
    .map_err(Error::Database)?;
    Ok(())
}

pub fn update_app(conn: &Connection, rec: &LocalCreativeAppRecord) -> Result<()> {
    conn.execute(
        "UPDATE local_creative_apps SET
            title=?2, description=?3, icon=?4, canonical_project_root=?5,
            device_id=?6, device_name=?7, project_kind=?8, launch_mode=?9,
            launch_plan_json=?10, plan_fingerprint=?11, state=?12,
            status_detail_json=?13, open_url=?14, current_port=?15,
            process_identity_json=?16, auto_open=?17, startup_timeout_ms=?18,
            last_started_at=?19, last_exit_reason=?20, last_error=?21, updated_at=?22
         WHERE id=?1",
        params![
            rec.id,
            rec.title,
            rec.description,
            rec.icon,
            rec.canonical_project_root,
            rec.device_id,
            rec.device_name,
            rec.project_kind.as_str(),
            rec.launch_mode.as_str(),
            rec.launch_plan_json,
            rec.plan_fingerprint,
            rec.state.as_str(),
            rec.status_detail_json,
            rec.open_url,
            rec.current_port.map(|p| p as i64),
            rec.process_identity_json,
            if rec.auto_open { 1 } else { 0 },
            rec.startup_timeout_ms as i64,
            rec.last_started_at,
            rec.last_exit_reason,
            rec.last_error,
            rec.updated_at,
        ],
    )
    .map_err(Error::Database)?;
    Ok(())
}

pub fn set_state(
    conn: &Connection,
    id: &str,
    state: CreativeAppState,
    last_error: Option<&str>,
    status_detail_json: Option<&str>,
    updated_at: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE local_creative_apps SET state=?2, last_error=?3, status_detail_json=?4, updated_at=?5 WHERE id=?1",
        params![id, state.as_str(), last_error, status_detail_json, updated_at],
    )
    .map_err(Error::Database)?;
    Ok(())
}

pub fn delete_app(conn: &Connection, id: &str) -> Result<()> {
    // CASCADE removes local_creative_env
    conn.execute("DELETE FROM local_creative_apps WHERE id = ?1", params![id])
        .map_err(Error::Database)?;
    Ok(())
}

pub fn get_app(conn: &Connection, id: &str) -> Result<Option<LocalCreativeAppRecord>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, title, description, icon, canonical_project_root, device_id, device_name,
                    project_kind, launch_mode, launch_plan_json, plan_fingerprint, state,
                    status_detail_json, open_url, current_port, process_identity_json,
                    auto_open, startup_timeout_ms, last_started_at, last_exit_reason, last_error,
                    created_at, updated_at
             FROM local_creative_apps WHERE id = ?1",
        )
        .map_err(Error::Database)?;
    let row = stmt
        .query_row(params![id], map_row)
        .optional()
        .map_err(Error::Database)?;
    Ok(row)
}

pub fn get_app_by_root(
    conn: &Connection,
    canonical_root: &str,
) -> Result<Option<LocalCreativeAppRecord>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, title, description, icon, canonical_project_root, device_id, device_name,
                    project_kind, launch_mode, launch_plan_json, plan_fingerprint, state,
                    status_detail_json, open_url, current_port, process_identity_json,
                    auto_open, startup_timeout_ms, last_started_at, last_exit_reason, last_error,
                    created_at, updated_at
             FROM local_creative_apps WHERE canonical_project_root = ?1",
        )
        .map_err(Error::Database)?;
    let row = stmt
        .query_row(params![canonical_root], map_row)
        .optional()
        .map_err(Error::Database)?;
    Ok(row)
}

pub fn list_apps(conn: &Connection) -> Result<Vec<LocalCreativeAppRecord>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, title, description, icon, canonical_project_root, device_id, device_name,
                    project_kind, launch_mode, launch_plan_json, plan_fingerprint, state,
                    status_detail_json, open_url, current_port, process_identity_json,
                    auto_open, startup_timeout_ms, last_started_at, last_exit_reason, last_error,
                    created_at, updated_at
             FROM local_creative_apps ORDER BY updated_at DESC",
        )
        .map_err(Error::Database)?;
    let rows = stmt.query_map([], map_row).map_err(Error::Database)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(Error::Database)?);
    }
    Ok(out)
}

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<LocalCreativeAppRecord> {
    let kind_s: String = row.get(7)?;
    let mode_s: String = row.get(8)?;
    let state_s: String = row.get(11)?;
    let port: Option<i64> = row.get(14)?;
    let auto_open_i: i64 = row.get(16)?;
    let timeout: i64 = row.get(17)?;
    Ok(LocalCreativeAppRecord {
        id: row.get(0)?,
        title: row.get(1)?,
        description: row.get(2)?,
        icon: row.get(3)?,
        canonical_project_root: row.get(4)?,
        device_id: row.get(5)?,
        device_name: row.get(6)?,
        project_kind: LocalProjectKind::parse(&kind_s).unwrap_or(LocalProjectKind::Unknown),
        launch_mode: LaunchMode::parse(&mode_s).unwrap_or(LaunchMode::Smart),
        launch_plan_json: row.get(9)?,
        plan_fingerprint: row.get(10)?,
        state: CreativeAppState::parse(&state_s).unwrap_or(CreativeAppState::InstalledStopped),
        status_detail_json: row.get(12)?,
        open_url: row.get(13)?,
        current_port: port.map(|p| p as u16),
        process_identity_json: row.get(15)?,
        auto_open: auto_open_i != 0,
        startup_timeout_ms: timeout.clamp(0, u32::MAX as i64) as u32,
        last_started_at: row.get(18)?,
        last_exit_reason: row.get(19)?,
        last_error: row.get(20)?,
        created_at: row.get(21)?,
        updated_at: row.get(22)?,
    })
}

// ── Encrypted env (independent of GitHub creative_app_env) ─────────

pub fn set_env(conn: &Connection, app_id: &str, key: &str, plaintext: &str) -> Result<()> {
    let enc_key = env_manager::get_encryption_key(conn)?;
    let encrypted = env_manager::encrypt(plaintext, &enc_key)?;
    conn.execute(
        "INSERT INTO local_creative_env (app_id, key, value_encrypted)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(app_id, key) DO UPDATE SET value_encrypted = excluded.value_encrypted",
        params![app_id, key, encrypted],
    )
    .map_err(Error::Database)?;
    Ok(())
}

pub fn replace_env(conn: &Connection, app_id: &str, pairs: &[(String, String)]) -> Result<()> {
    // Prefer upsert semantics: do not wipe keys that are not mentioned.
    // For full replace callers still pass the complete list.
    let existing = list_env_keys(conn, app_id)?;
    let new_keys: std::collections::HashSet<&str> =
        pairs.iter().map(|(k, _)| k.as_str()).collect();
    for old in existing {
        if !new_keys.contains(old.as_str()) {
            remove_env_key(conn, app_id, &old)?;
        }
    }
    for (k, v) in pairs {
        set_env(conn, app_id, k, v)?;
    }
    Ok(())
}

pub fn upsert_env(conn: &Connection, app_id: &str, pairs: &[(String, String)]) -> Result<()> {
    for (k, v) in pairs {
        set_env(conn, app_id, k, v)?;
    }
    Ok(())
}

pub fn remove_env_key(conn: &Connection, app_id: &str, key: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM local_creative_env WHERE app_id = ?1 AND key = ?2",
        params![app_id, key],
    )
    .map_err(Error::Database)?;
    Ok(())
}

pub fn remove_env_keys(conn: &Connection, app_id: &str, keys: &[String]) -> Result<()> {
    for k in keys {
        remove_env_key(conn, app_id, k)?;
    }
    Ok(())
}

/// Non-empty env values for log redaction (never log these).
pub fn secret_values(conn: &Connection, app_id: &str) -> Vec<String> {
    get_env_map(conn, app_id)
        .unwrap_or_default()
        .into_iter()
        .map(|(_, v)| v)
        .filter(|v| v.trim().len() >= 4)
        .collect()
}

pub fn get_env_map(conn: &Connection, app_id: &str) -> Result<Vec<(String, String)>> {
    let enc_key = env_manager::get_encryption_key(conn)?;
    let mut stmt = conn
        .prepare("SELECT key, value_encrypted FROM local_creative_env WHERE app_id = ?1")
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map(params![app_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(Error::Database)?;
    let mut out = Vec::new();
    for r in rows {
        let (k, enc) = r.map_err(Error::Database)?;
        let plain = env_manager::decrypt(&enc, &enc_key)?;
        out.push((k, plain));
    }
    Ok(out)
}

pub fn list_env_keys(conn: &Connection, app_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn
        .prepare("SELECT key FROM local_creative_env WHERE app_id = ?1 ORDER BY key")
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map(params![app_id], |row| row.get::<_, String>(0))
        .map_err(Error::Database)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(Error::Database)?);
    }
    Ok(out)
}

pub fn summary_from_local(rec: &LocalCreativeAppRecord) -> CreativeAppSummary {
    let plan = LaunchPlan::from_json(&rec.launch_plan_json).ok();
    let runtime = plan
        .as_ref()
        .map(|p| p.creative_runtime())
        .unwrap_or(CreativeAppRuntime::LocalStatic);
    let package_manager = plan.as_ref().and_then(|p| match p.program {
        LaunchProgram::Npm => Some(PackageManager::Npm),
        LaunchProgram::Pnpm => Some(PackageManager::Pnpm),
        LaunchProgram::Yarn => Some(PackageManager::Yarn),
        _ => None,
    });
    let status_detail = rec
        .status_detail_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok());

    CreativeAppSummary {
        id: rec.id.clone(),
        source: CreativeAppSource::LocalProject,
        runtime,
        title: rec.title.clone(),
        description: rec.description.clone(),
        icon: rec.icon.clone(),
        version: "local".into(),
        state: rec.state,
        open_url: rec.open_url.clone(),
        repository_url: None,
        last_error: rec.last_error.clone(),
        status_detail,
        local_project: Some(LocalProjectSummary {
            project_root: rec.canonical_project_root.clone(),
            project_kind: rec.project_kind,
            launch_mode: rec.launch_mode,
            package_manager,
            device_id: rec.device_id.clone(),
            device_name: rec.device_name.clone(),
        }),
        actions: CreativeAppActions::for_state(CreativeAppSource::LocalProject, rec.state),
    }
}

/// Helper for create flow: stamp device fields.
pub fn current_device() -> (String, String) {
    (device_id(), device_name())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{apply_migrations, create_tables};
    use crate::env_manager;

    fn mem() -> Connection {
        env_manager::reset_env_key_cache_for_tests();
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        env_manager::init_env_encryption_key(&conn).unwrap();
        conn
    }

    fn sample_rec(root: &str) -> LocalCreativeAppRecord {
        let plan = LaunchPlan {
            schema_version: 1,
            source: LaunchPlanSource::Rule,
            project_kind: LocalProjectKind::Html,
            runtime: LocalLaunchRuntime::StaticHttp,
            program: LaunchProgram::Internal,
            cwd_relative: ".".into(),
            script: None,
            entry_file: Some("index.html".into()),
            script_runner: None,
            args: vec![],
            environment_keys: vec![],
            port: LaunchPort {
                mode: LaunchPortMode::Auto,
                value: None,
            },
            open_path: "/".into(),
            health_path: "/".into(),
            startup_timeout_ms: 60_000,
            auto_open: true,
            confidence: Some(1.0),
            reason: "test".into(),
        };
        let now = chrono::Utc::now().to_rfc3339();
        LocalCreativeAppRecord {
            id: "loc1".into(),
            title: "Local HTML".into(),
            description: Some("desc".into()),
            icon: None,
            canonical_project_root: root.into(),
            device_id: "host:test".into(),
            device_name: "test".into(),
            project_kind: LocalProjectKind::Html,
            launch_mode: LaunchMode::Smart,
            launch_plan_json: plan.to_json().unwrap(),
            plan_fingerprint: "abc".into(),
            state: CreativeAppState::InstalledStopped,
            status_detail_json: None,
            open_url: None,
            current_port: None,
            process_identity_json: None,
            auto_open: true,
            startup_timeout_ms: 60_000,
            last_started_at: None,
            last_exit_reason: None,
            last_error: None,
            created_at: now.clone(),
            updated_at: now,
        }
    }

    #[test]
    fn crud_and_unique_root() {
        let conn = mem();
        let rec = sample_rec("/tmp/proj-a");
        insert_app(&conn, &rec).unwrap();
        let got = get_app(&conn, "loc1").unwrap().unwrap();
        assert_eq!(got.title, "Local HTML");
        assert_eq!(
            get_app_by_root(&conn, "/tmp/proj-a").unwrap().unwrap().id,
            "loc1"
        );
        set_env(&conn, "loc1", "API_URL", "http://x").unwrap();
        assert_eq!(list_env_keys(&conn, "loc1").unwrap(), vec!["API_URL".to_string()]);
        let map = get_env_map(&conn, "loc1").unwrap();
        assert_eq!(map, vec![("API_URL".into(), "http://x".into())]);

        let s = summary_from_local(&got);
        assert_eq!(s.source, CreativeAppSource::LocalProject);
        assert_eq!(s.runtime, CreativeAppRuntime::LocalStatic);
        assert!(s.local_project.is_some());

        delete_app(&conn, "loc1").unwrap();
        assert!(get_app(&conn, "loc1").unwrap().is_none());
        assert!(list_env_keys(&conn, "loc1").unwrap().is_empty());
    }

    #[test]
    fn list_includes_inserted() {
        let conn = mem();
        insert_app(&conn, &sample_rec("/tmp/a")).unwrap();
        let mut r2 = sample_rec("/tmp/b");
        r2.id = "loc2".into();
        insert_app(&conn, &r2).unwrap();
        assert_eq!(list_apps(&conn).unwrap().len(), 2);
    }
}
