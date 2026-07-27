//! Skill metadata CRUD + scan merge + import (ADR-0016).
//!
//! The skill body stays on disk (dir + SKILL.md) to preserve interop with
//! external CLI engines; this table owns metadata, category tags and the
//! enable/trust switches. Row ids reuse the runtime discovery ids
//! (`user:<name>` / `project:<name>`) so run-level selection and the runtime
//! `skill.list` surface speak the same language.

use super::store;
use crate::skill_store::{SkillScope, SkillStore};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn hash_body(body: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(body.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn row_to_json(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let tags: String = row.get("tags_json")?;
    let engine_targets: String = row.get("engine_targets_json")?;
    Ok(json!({
        "id": row.get::<_, String>("id")?,
        "name": row.get::<_, String>("name")?,
        "description": row.get::<_, String>("description")?,
        "scope": row.get::<_, String>("scope")?,
        "projectId": row.get::<_, Option<String>>("project_id")?,
        "dirPath": row.get::<_, String>("dir_path")?,
        "contentHash": row.get::<_, Option<String>>("content_hash")?,
        "category": row.get::<_, Option<String>>("category")?,
        "tags": serde_json::from_str::<Value>(&tags).unwrap_or_else(|_| json!([])),
        "enabled": row.get::<_, i64>("enabled")? != 0,
        "trusted": row.get::<_, i64>("trusted")? != 0,
        "source": row.get::<_, String>("source")?,
        "sourceRef": row.get::<_, Option<String>>("source_ref")?,
        "engineTargets": serde_json::from_str::<Value>(&engine_targets)
            .unwrap_or_else(|_| json!(["native"])),
        "createdAt": row.get::<_, String>("created_at")?,
        "updatedAt": row.get::<_, String>("updated_at")?,
    }))
}

const SELECT_COLS: &str = "id, name, description, scope, project_id, dir_path, content_hash, \
     category, tags_json, enabled, trusted, source, source_ref, engine_targets_json, \
     created_at, updated_at";

pub fn list(params_value: &Value) -> Result<Value, String> {
    let data = store()?;
    let conn = data.conn()?;
    let category = params_value.get("category").and_then(Value::as_str);
    let scope = params_value.get("scope").and_then(Value::as_str);
    let query = params_value.get("query").and_then(Value::as_str);
    let enabled_only = params_value
        .get("enabledOnly")
        .or_else(|| params_value.get("enabled_only"))
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let mut sql = format!("SELECT {SELECT_COLS} FROM capability_skill WHERE 1=1");
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(category) = category.filter(|s| !s.is_empty()) {
        sql.push_str(" AND category = ?");
        args.push(Box::new(category.to_string()));
    }
    if let Some(scope) = scope.filter(|s| !s.is_empty()) {
        sql.push_str(" AND scope = ?");
        args.push(Box::new(scope.to_string()));
    }
    if let Some(query) = query.filter(|s| !s.is_empty()) {
        sql.push_str(" AND (name LIKE ? OR description LIKE ?)");
        let like = format!("%{query}%");
        args.push(Box::new(like.clone()));
        args.push(Box::new(like));
    }
    if enabled_only {
        sql.push_str(" AND enabled = 1");
    }
    sql.push_str(" ORDER BY scope ASC, name ASC");

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(
            rusqlite::params_from_iter(args.iter().map(|a| a.as_ref())),
            row_to_json,
        )
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(json!({ "skills": rows }))
}

pub fn get(params_value: &Value) -> Result<Value, String> {
    let id = required_str(params_value, "id")?;
    let data = store()?;
    let conn = data.conn()?;
    let skill = conn
        .query_row(
            &format!("SELECT {SELECT_COLS} FROM capability_skill WHERE id = ?1"),
            params![id],
            row_to_json,
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("skill not found: {id}"))?;
    // Body preview from disk (metadata row never stores the body).
    let dir_path = skill
        .get("dirPath")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let body_preview = read_skill_md(Path::new(&dir_path))
        .map(|body| body.chars().take(2000).collect::<String>())
        .unwrap_or_default();
    let mut skill = skill;
    skill["bodyPreview"] = json!(body_preview);
    Ok(json!({ "skill": skill }))
}

pub fn update(params_value: &Value) -> Result<Value, String> {
    let id = required_str(params_value, "id")?;
    let data = store()?;
    let conn = data.conn()?;

    let mut sets: Vec<String> = Vec::new();
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(category) = params_value.get("category") {
        sets.push("category = ?".into());
        match category.as_str() {
            Some(s) if !s.trim().is_empty() => args.push(Box::new(s.trim().to_string())),
            _ => args.push(Box::new(None::<String>)),
        }
    }
    if let Some(tags) = params_value.get("tags").and_then(Value::as_array) {
        let tags: Vec<String> = tags
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        sets.push("tags_json = ?".into());
        args.push(Box::new(
            serde_json::to_string(&tags).unwrap_or_else(|_| "[]".into()),
        ));
    }
    if let Some(enabled) = params_value.get("enabled").and_then(Value::as_bool) {
        sets.push("enabled = ?".into());
        args.push(Box::new(enabled as i64));
    }
    if let Some(trusted) = params_value.get("trusted").and_then(Value::as_bool) {
        sets.push("trusted = ?".into());
        args.push(Box::new(trusted as i64));
    }
    if let Some(targets) = params_value
        .get("engineTargets")
        .or_else(|| params_value.get("engine_targets"))
        .and_then(Value::as_array)
    {
        let targets: Vec<String> = targets
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        for t in &targets {
            if !matches!(t.as_str(), "native" | "claude_cli" | "codex_cli") {
                return Err(format!("invalid engine target: {t}"));
            }
        }
        sets.push("engine_targets_json = ?".into());
        args.push(Box::new(
            serde_json::to_string(&targets).unwrap_or_else(|_| "[\"native\"]".into()),
        ));
    }
    if sets.is_empty() {
        return Err("no updatable fields provided".into());
    }
    sets.push("updated_at = ?".into());
    args.push(Box::new(now_iso()));
    args.push(Box::new(id.to_string()));
    let sql = format!(
        "UPDATE capability_skill SET {} WHERE id = ?",
        sets.join(", ")
    );
    let changed = conn
        .execute(
            &sql,
            rusqlite::params_from_iter(args.iter().map(|a| a.as_ref())),
        )
        .map_err(|e| e.to_string())?;
    if changed == 0 {
        return Err(format!("skill not found: {id}"));
    }
    drop(conn);
    get(&json!({ "id": id }))
}

/// Delete a skill metadata row. `mode: "unregister"` (default) keeps files on
/// disk; `mode: "remove_dir"` also deletes the skill directory — only allowed
/// for imported skills whose directory is inside a recognised skills root.
pub fn delete(params_value: &Value) -> Result<Value, String> {
    let id = required_str(params_value, "id")?;
    let mode = params_value
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("unregister");
    let data = store()?;
    let conn = data.conn()?;
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT dir_path, source FROM capability_skill WHERE id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some((dir_path, source)) = row else {
        return Err(format!("skill not found: {id}"));
    };
    let mut removed_dir = false;
    if mode == "remove_dir" {
        if source == "scan" {
            return Err("remove_dir is only allowed for imported skills; unregister scan-discovered skills instead".into());
        }
        let path = PathBuf::from(&dir_path);
        if !is_inside_skills_root(&path) {
            return Err(format!(
                "refusing to delete directory outside a skills root: {dir_path}"
            ));
        }
        if path.exists() {
            std::fs::remove_dir_all(&path).map_err(|e| e.to_string())?;
            removed_dir = true;
        }
    }
    conn.execute("DELETE FROM capability_skill WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(json!({ "deleted": id, "removedDir": removed_dir }))
}

/// Re-run the filesystem scanner and merge results into the metadata table.
/// DB rows own the enable/trust/category switches; the scanner refreshes
/// paths, descriptions and drift hashes.
pub fn rescan(params_value: &Value) -> Result<Value, String> {
    let project_root = params_value
        .get("projectRoot")
        .or_else(|| params_value.get("project_root"))
        .and_then(Value::as_str)
        .map(PathBuf::from);

    let scanner = SkillStore::new();
    scanner.discover_for_project(project_root.as_deref());
    let discovered = scanner.list();

    let data = store()?;
    let conn = data.conn()?;
    let now = now_iso();
    let mut new_count = 0u32;
    let mut updated_count = 0u32;

    for record in &discovered {
        // dir path = parent of SKILL.md when the skill is a directory.
        let md_path = PathBuf::from(&record.path);
        let dir_path = md_path
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| record.path.clone());
        let body = std::fs::read_to_string(&md_path).unwrap_or_default();
        let content_hash = hash_body(&body);
        let scope = match record.scope {
            SkillScope::Project => "project",
            SkillScope::User => "user",
        };
        let existing: Option<String> = conn
            .query_row(
                "SELECT content_hash FROM capability_skill WHERE id = ?1",
                params![record.id],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .flatten();
        match existing {
            None => {
                conn.execute(
                    "INSERT INTO capability_skill
                        (id, name, description, scope, dir_path, content_hash,
                         enabled, trusted, source, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, 'scan', ?8, ?8)
                     ON CONFLICT(id) DO NOTHING",
                    params![
                        record.id,
                        record.name,
                        record.description,
                        scope,
                        dir_path,
                        content_hash,
                        // Locally discovered skills are the user's own files.
                        record.trusted as i64,
                        now,
                    ],
                )
                .map_err(|e| e.to_string())?;
                new_count += 1;
            }
            Some(previous_hash) => {
                if previous_hash != content_hash {
                    updated_count += 1;
                }
                conn.execute(
                    "UPDATE capability_skill
                        SET description = ?2, dir_path = ?3, content_hash = ?4, updated_at = ?5
                      WHERE id = ?1",
                    params![record.id, record.description, dir_path, content_hash, now],
                )
                .map_err(|e| e.to_string())?;
            }
        }
    }

    // Scan-sourced rows whose directory disappeared: report, never auto-delete.
    let mut missing = Vec::new();
    {
        let mut stmt = conn
            .prepare("SELECT id, dir_path FROM capability_skill WHERE source = 'scan'")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(|e| e.to_string())?;
        for row in rows.flatten() {
            let (id, dir_path) = row;
            if !Path::new(&dir_path).exists() {
                missing.push(id);
            }
        }
    }

    Ok(json!({
        "scanned": discovered.len(),
        "new": new_count,
        "changed": updated_count,
        "missing": missing,
    }))
}

/// Import a skill from a local directory or zip archive into
/// `~/.natives/skills/<name>`. Imported skills default to untrusted; the user
/// must promote them explicitly. Zip extraction rejects path traversal.
pub fn import(params_value: &Value) -> Result<Value, String> {
    let source_kind = params_value
        .get("source")
        .and_then(Value::as_str)
        .ok_or("source required: 'zip' | 'dir'")?;
    let source_path = PathBuf::from(required_str(params_value, "path")?);
    if !source_path.exists() {
        return Err(format!("source path not found: {}", source_path.display()));
    }
    let name = params_value
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            source_path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
        })
        .ok_or("cannot derive skill name")?;
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err("invalid skill name".into());
    }

    let home = std::env::var("HOME").map_err(|_| "HOME not set".to_string())?;
    let dest = PathBuf::from(home)
        .join(".natives")
        .join("skills")
        .join(&name);
    if dest.exists() {
        return Err(format!("skill already exists: {}", dest.display()));
    }
    std::fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;

    let source_tag = match source_kind {
        "dir" => {
            copy_dir_recursive(&source_path, &dest)?;
            "import_dir"
        }
        "zip" => {
            extract_zip(&source_path, &dest)?;
            "import_zip"
        }
        other => return Err(format!("unsupported import source: {other}")),
    };

    let skill_md = ["SKILL.md", "skill.md"]
        .iter()
        .map(|n| dest.join(n))
        .find(|p| p.exists());
    let Some(skill_md) = skill_md else {
        let _ = std::fs::remove_dir_all(&dest);
        return Err("import rejected: no SKILL.md at archive root".into());
    };
    let body = std::fs::read_to_string(&skill_md).map_err(|e| e.to_string())?;
    let description = body
        .lines()
        .find(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .unwrap_or("")
        .trim()
        .chars()
        .take(160)
        .collect::<String>();

    // Optional interop symlink for external CLI engines.
    let link_claude = params_value
        .get("linkClaudeDir")
        .or_else(|| params_value.get("link_claude_dir"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if link_claude {
        let claude_dir = PathBuf::from(std::env::var("HOME").unwrap_or_default())
            .join(".claude")
            .join("skills");
        let _ = std::fs::create_dir_all(&claude_dir);
        #[cfg(unix)]
        let _ = std::os::unix::fs::symlink(&dest, claude_dir.join(&name));
    }

    let id = format!("user:{name}");
    let category = params_value.get("category").and_then(Value::as_str);
    let tags: Vec<String> = params_value
        .get("tags")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let now = now_iso();
    let data = store()?;
    let conn = data.conn()?;
    conn.execute(
        "INSERT INTO capability_skill
            (id, name, description, scope, dir_path, content_hash, category, tags_json,
             enabled, trusted, source, source_ref, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'user', ?4, ?5, ?6, ?7, 1, 0, ?8, ?9, ?10, ?10)",
        params![
            id,
            name,
            description,
            dest.display().to_string(),
            hash_body(&body),
            category,
            serde_json::to_string(&tags).unwrap_or_else(|_| "[]".into()),
            source_tag,
            source_path.display().to_string(),
            now,
        ],
    )
    .map_err(|e| {
        let _ = std::fs::remove_dir_all(&dest);
        e.to_string()
    })?;
    drop(conn);
    get(&json!({ "id": id }))
}

/// Selection ids that exist, are enabled and trusted → concatenated prompt.
/// Any missing / disabled / untrusted id fails the whole selection (fail-closed).
pub fn prompt_for_selection(ids: &[String]) -> Result<String, Vec<String>> {
    if ids.is_empty() {
        return Ok(String::new());
    }
    let data = match store() {
        Ok(s) => s,
        Err(_) => return Err(ids.to_vec()),
    };
    let conn = match data.conn() {
        Ok(c) => c,
        Err(_) => return Err(ids.to_vec()),
    };
    let mut missing = Vec::new();
    let mut parts = Vec::new();
    for id in ids {
        // Bare names (file-imported profiles) fall back to project: then user:
        // scope — project skills shadow user skills, same as the scanner.
        let lookup = |candidate: &str| -> Option<(String, String, i64, i64)> {
            conn.query_row(
                "SELECT name, dir_path, enabled, trusted FROM capability_skill WHERE id = ?1",
                params![candidate],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .unwrap_or(None)
        };
        let row = if id.contains(':') {
            lookup(id)
        } else {
            lookup(&format!("project:{id}")).or_else(|| lookup(&format!("user:{id}")))
        };
        match row {
            Some((name, dir_path, enabled, trusted)) if enabled != 0 && trusted != 0 => {
                match read_skill_md(Path::new(&dir_path)) {
                    Some(body) => parts.push(format!("### Skill: {name}\n{body}")),
                    None => missing.push(id.clone()),
                }
            }
            _ => missing.push(id.clone()),
        }
    }
    if missing.is_empty() {
        Ok(parts.join("\n\n"))
    } else {
        Err(missing)
    }
}

fn read_skill_md(dir: &Path) -> Option<String> {
    for name in ["SKILL.md", "skill.md"] {
        let p = dir.join(name);
        if let Ok(body) = std::fs::read_to_string(&p) {
            return Some(body);
        }
    }
    // Bare .md file skills store the file itself as dir_path parent fallback.
    if dir.extension().map(|e| e == "md").unwrap_or(false) {
        return std::fs::read_to_string(dir).ok();
    }
    None
}

fn is_inside_skills_root(path: &Path) -> bool {
    path.components().any(|c| c.as_os_str() == "skills")
}

fn copy_dir_recursive(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(from).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let ty = entry.file_type().map_err(|e| e.to_string())?;
        let dest = to.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &dest)?;
        } else if ty.is_file() {
            std::fs::copy(entry.path(), &dest).map_err(|e| e.to_string())?;
        }
        // Symlinks inside imports are intentionally skipped (escape hatch risk).
    }
    Ok(())
}

/// Minimal zip extraction with zip-slip protection. Uses the system `unzip`
/// via std process to avoid a new crate dependency, then verifies no entry
/// escaped the destination.
fn extract_zip(archive: &Path, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    // Refuse entries with `..` before extraction (zip-slip).
    let listing = std::process::Command::new("unzip")
        .arg("-Z1")
        .arg(archive)
        .output()
        .map_err(|e| format!("unzip unavailable: {e}"))?;
    if !listing.status.success() {
        return Err("failed to read zip listing".into());
    }
    let names = String::from_utf8_lossy(&listing.stdout);
    for name in names.lines() {
        let p = Path::new(name);
        if p.is_absolute()
            || p.components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(format!("zip-slip rejected: {name}"));
        }
    }
    let status = std::process::Command::new("unzip")
        .arg("-q")
        .arg(archive)
        .arg("-d")
        .arg(dest)
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        let _ = std::fs::remove_dir_all(dest);
        return Err("unzip failed".into());
    }
    // If the archive wrapped everything in a single top-level dir, flatten it.
    let entries: Vec<_> = std::fs::read_dir(dest)
        .map_err(|e| e.to_string())?
        .flatten()
        .collect();
    if entries.len() == 1 && entries[0].path().is_dir() {
        let inner = entries[0].path();
        let has_md = ["SKILL.md", "skill.md"]
            .iter()
            .any(|n| inner.join(n).exists());
        if has_md {
            for entry in std::fs::read_dir(&inner)
                .map_err(|e| e.to_string())?
                .flatten()
            {
                let target = dest.join(entry.file_name());
                std::fs::rename(entry.path(), target).map_err(|e| e.to_string())?;
            }
            let _ = std::fs::remove_dir(&inner);
        }
    }
    Ok(())
}

fn required_str<'a>(params: &'a Value, key: &str) -> Result<&'a str, String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("{key} required"))
}
