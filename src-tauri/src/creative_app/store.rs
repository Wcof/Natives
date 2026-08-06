//! SQLite persistence for external creative apps + encrypted env.

use super::model::{
    CreativeAppRuntime, CreativeAppState, ExternalCreativeAppRecord, RuntimeConfig,
};
use crate::env_manager;
use crate::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

const GITHUB_TOKEN_SETTING: &str = "creative_app_github_token_encrypted";

pub fn insert_app(conn: &Connection, rec: &ExternalCreativeAppRecord) -> Result<()> {
    conn.execute(
        "INSERT INTO external_creative_apps (
            id, title, description, icon, version, owner, repo, repository_url,
            release_tag, release_id, runtime, state, open_url, health_url, host_port,
            runtime_config_json, last_error, created_at, updated_at
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
        params![
            rec.id,
            rec.title,
            rec.description,
            rec.icon,
            rec.version,
            rec.owner,
            rec.repo,
            rec.repository_url,
            rec.release_tag,
            rec.release_id,
            runtime_to_db(rec.runtime),
            rec.state.as_str(),
            rec.open_url,
            rec.health_url,
            rec.host_port.map(|p| p as i64),
            rec.runtime_config_json,
            rec.last_error,
            rec.created_at,
            rec.updated_at,
        ],
    )
    .map_err(Error::Database)?;
    Ok(())
}

pub fn update_app(conn: &Connection, rec: &ExternalCreativeAppRecord) -> Result<()> {
    conn.execute(
        "UPDATE external_creative_apps SET
            title=?2, description=?3, icon=?4, version=?5, owner=?6, repo=?7,
            repository_url=?8, release_tag=?9, release_id=?10, runtime=?11, state=?12,
            open_url=?13, health_url=?14, host_port=?15, runtime_config_json=?16,
            last_error=?17, updated_at=?18
         WHERE id=?1",
        params![
            rec.id,
            rec.title,
            rec.description,
            rec.icon,
            rec.version,
            rec.owner,
            rec.repo,
            rec.repository_url,
            rec.release_tag,
            rec.release_id,
            runtime_to_db(rec.runtime),
            rec.state.as_str(),
            rec.open_url,
            rec.health_url,
            rec.host_port.map(|p| p as i64),
            rec.runtime_config_json,
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
    updated_at: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE external_creative_apps SET state=?2, last_error=?3, updated_at=?4 WHERE id=?1",
        params![id, state.as_str(), last_error, updated_at],
    )
    .map_err(Error::Database)?;
    Ok(())
}

pub fn delete_app(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM external_creative_apps WHERE id = ?1",
        params![id],
    )
    .map_err(Error::Database)?;
    Ok(())
}

pub fn get_app(conn: &Connection, id: &str) -> Result<Option<ExternalCreativeAppRecord>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, title, description, icon, version, owner, repo, repository_url,
                    release_tag, release_id, runtime, state, open_url, health_url, host_port,
                    runtime_config_json, last_error, created_at, updated_at
             FROM external_creative_apps WHERE id = ?1",
        )
        .map_err(Error::Database)?;
    let row = stmt
        .query_row(params![id], map_row)
        .optional()
        .map_err(Error::Database)?;
    Ok(row)
}

pub fn list_apps(conn: &Connection) -> Result<Vec<ExternalCreativeAppRecord>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, title, description, icon, version, owner, repo, repository_url,
                    release_tag, release_id, runtime, state, open_url, health_url, host_port,
                    runtime_config_json, last_error, created_at, updated_at
             FROM external_creative_apps ORDER BY updated_at DESC",
        )
        .map_err(Error::Database)?;
    let rows = stmt.query_map([], map_row).map_err(Error::Database)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(Error::Database)?);
    }
    Ok(out)
}

pub fn list_transient(conn: &Connection) -> Result<Vec<ExternalCreativeAppRecord>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, title, description, icon, version, owner, repo, repository_url,
                    release_tag, release_id, runtime, state, open_url, health_url, host_port,
                    runtime_config_json, last_error, created_at, updated_at
             FROM external_creative_apps
             WHERE state IN ('installing','starting','stopping','deleting')",
        )
        .map_err(Error::Database)?;
    let rows = stmt.query_map([], map_row).map_err(Error::Database)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(Error::Database)?);
    }
    Ok(out)
}

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ExternalCreativeAppRecord> {
    let runtime_s: String = row.get(10)?;
    let state_s: String = row.get(11)?;
    let host_port: Option<i64> = row.get(14)?;
    Ok(ExternalCreativeAppRecord {
        id: row.get(0)?,
        title: row.get(1)?,
        description: row.get(2)?,
        icon: row.get(3)?,
        version: row.get(4)?,
        owner: row.get(5)?,
        repo: row.get(6)?,
        repository_url: row.get(7)?,
        release_tag: row.get(8)?,
        release_id: row.get(9)?,
        runtime: runtime_from_db(&runtime_s).unwrap_or(CreativeAppRuntime::DockerCompose),
        state: CreativeAppState::parse(&state_s).unwrap_or(CreativeAppState::InstallFailed),
        open_url: row.get(12)?,
        health_url: row.get(13)?,
        host_port: host_port.map(|p| p as u16),
        runtime_config_json: row.get(15)?,
        last_error: row.get(16)?,
        created_at: row.get(17)?,
        updated_at: row.get(18)?,
    })
}

pub fn runtime_to_db(r: CreativeAppRuntime) -> &'static str {
    match r {
        CreativeAppRuntime::WorkshopStatic => "workshop_static",
        CreativeAppRuntime::DockerCompose => "docker_compose",
        CreativeAppRuntime::DockerRun => "docker_run",
        CreativeAppRuntime::LocalStatic => "local_static",
        CreativeAppRuntime::NodeDevServer => "node_dev_server",
    }
}

pub fn runtime_from_db(s: &str) -> Option<CreativeAppRuntime> {
    match s {
        "workshop_static" => Some(CreativeAppRuntime::WorkshopStatic),
        "docker_compose" => Some(CreativeAppRuntime::DockerCompose),
        "docker_run" => Some(CreativeAppRuntime::DockerRun),
        "local_static" => Some(CreativeAppRuntime::LocalStatic),
        "node_dev_server" => Some(CreativeAppRuntime::NodeDevServer),
        _ => None,
    }
}

pub fn parse_runtime_config(json: &str) -> Result<RuntimeConfig> {
    RuntimeConfig::from_json(json).map_err(|e| Error::InvalidInput(format!("runtime_config: {e}")))
}

// ── Encrypted env ──────────────────────────────────────────────

pub fn set_env(conn: &Connection, app_id: &str, key: &str, plaintext: &str) -> Result<()> {
    let enc_key = env_manager::get_encryption_key(conn)?;
    let encrypted = env_manager::encrypt(plaintext, &enc_key)?;
    conn.execute(
        "INSERT INTO creative_app_env (app_id, key, value_encrypted)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(app_id, key) DO UPDATE SET value_encrypted = excluded.value_encrypted",
        params![app_id, key, encrypted],
    )
    .map_err(Error::Database)?;
    Ok(())
}

pub fn get_env_map(conn: &Connection, app_id: &str) -> Result<Vec<(String, String)>> {
    let enc_key = env_manager::get_encryption_key(conn)?;
    let mut stmt = conn
        .prepare("SELECT key, value_encrypted FROM creative_app_env WHERE app_id = ?1")
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
        .prepare("SELECT key FROM creative_app_env WHERE app_id = ?1 ORDER BY key")
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

// ── GitHub token (settings, AES-GCM) ───────────────────────────

pub fn set_github_token(conn: &Connection, plaintext: &str) -> Result<()> {
    let enc_key = env_manager::get_encryption_key(conn)?;
    let encrypted = env_manager::encrypt(plaintext, &enc_key)?;
    crate::db::set_setting(conn, GITHUB_TOKEN_SETTING, &encrypted)?;
    Ok(())
}

pub fn clear_github_token(conn: &Connection) -> Result<()> {
    crate::db::delete_setting(conn, GITHUB_TOKEN_SETTING)?;
    Ok(())
}

pub fn get_github_token_plaintext(conn: &Connection) -> Result<Option<String>> {
    match crate::db::get_setting(conn, GITHUB_TOKEN_SETTING)? {
        Some(enc) if !enc.is_empty() => {
            let enc_key = env_manager::get_encryption_key(conn)?;
            let plain = env_manager::decrypt(&enc, &enc_key)?;
            Ok(Some(plain))
        }
        _ => Ok(None),
    }
}

pub fn github_token_status(conn: &Connection) -> Result<super::model::GithubTokenStatus> {
    match get_github_token_plaintext(conn)? {
        Some(t) if !t.is_empty() => Ok(super::model::GithubTokenStatus {
            configured: true,
            masked: Some(mask_token(&t)),
        }),
        _ => Ok(super::model::GithubTokenStatus {
            configured: false,
            masked: None,
        }),
    }
}

pub fn mask_token(token: &str) -> String {
    let t = token.trim();
    if t.len() <= 8 {
        return "••••".to_string();
    }
    let head: String = t.chars().take(4).collect();
    let tail: String = t
        .chars()
        .rev()
        .take(4)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("{head}…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{apply_migrations, create_tables};
    use std::sync::{Mutex, OnceLock};

    fn env_key_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    fn mem() -> Connection {
        // Clear process-global env key cache so in-memory DBs don't reuse another test's key.
        crate::env_manager::reset_env_key_cache_for_tests();
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        env_manager::init_env_encryption_key(&conn).unwrap();
        conn
    }

    #[test]
    fn crud_and_cascade_env() {
        let _lock = env_key_lock();
        let conn = mem();
        let now = chrono::Utc::now().to_rfc3339();
        let rec = ExternalCreativeAppRecord {
            id: "app1".into(),
            title: "T".into(),
            description: None,
            icon: None,
            version: "1.0.0".into(),
            owner: "o".into(),
            repo: "r".into(),
            repository_url: "https://github.com/o/r".into(),
            release_tag: "v1".into(),
            release_id: Some(1),
            runtime: CreativeAppRuntime::DockerRun,
            state: CreativeAppState::Installing,
            open_url: None,
            health_url: None,
            host_port: Some(8080),
            runtime_config_json: RuntimeConfig::DockerRun {
                container_name: "natives-ca-app1".into(),
                image: "nginx:alpine".into(),
                container_port: 80,
                host_port: 8080,
                open_path: "/".into(),
                health_path: None,
                env_keys: vec!["K".into()],
            }
            .to_json()
            .unwrap(),
            last_error: None,
            created_at: now.clone(),
            updated_at: now,
        };
        insert_app(&conn, &rec).unwrap();
        set_env(&conn, "app1", "K", "secret-value").unwrap();
        let map = get_env_map(&conn, "app1").unwrap();
        assert_eq!(map, vec![("K".into(), "secret-value".into())]);
        delete_app(&conn, "app1").unwrap();
        assert!(get_app(&conn, "app1").unwrap().is_none());
        // CASCADE should remove env
        assert!(list_env_keys(&conn, "app1").unwrap().is_empty());
    }

    #[test]
    fn token_mask_and_roundtrip() {
        let _lock = env_key_lock();
        let conn = mem();
        // Serialize against other env tests that share process-global key cache.
        set_github_token(&conn, "ghp_abcdefghijklmnop").unwrap();
        // Force cache to this connection's key before status/decrypt.
        crate::env_manager::reset_env_key_cache_for_tests();
        let key = env_manager::init_env_encryption_key(&conn).unwrap();
        assert_eq!(key.len(), 64);
        let st = github_token_status(&conn).unwrap();
        assert!(st.configured);
        assert!(st.masked.as_ref().unwrap().contains('…'));
        assert!(!st.masked.as_ref().unwrap().contains("ghp_abcdefghijklmnop"));
        let plain = get_github_token_plaintext(&conn).unwrap().unwrap();
        assert_eq!(plain, "ghp_abcdefghijklmnop");
        clear_github_token(&conn).unwrap();
        assert!(!github_token_status(&conn).unwrap().configured);
    }

    #[test]
    fn mask_token_shape() {
        assert_eq!(mask_token("short"), "••••");
        let m = mask_token("ghp_abcdefghijklmnop");
        assert!(m.starts_with("ghp_"));
        assert!(m.contains('…'));
    }

    #[test]
    fn modules_survive_v8() {
        let conn = mem();
        conn.execute(
            "INSERT INTO modules (id, name, version, entry, type, enabled, state) VALUES ('m1','M','1','index.html','web',1,'installed')",
            [],
        )
        .unwrap();
        // re-apply migrations (idempotent)
        apply_migrations(&conn).unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM modules WHERE id='m1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(n, 1);
        let ver: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key='_schema_version'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            ver,
            crate::db::SCHEMA_VERSION,
            "schema version must reflect the current migration head"
        );
        let local: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='local_creative_apps'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(local, 1);
    }
}
