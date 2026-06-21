use crate::{Error, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Module manifest — mirrors Natives Zod schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub entry: String,
    #[serde(rename = "type", default = "default_type")]
    pub module_type: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(rename = "minNativesVersion", default)]
    pub min_natives_version: Option<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
}

fn default_type() -> String {
    "web".to_string()
}

#[derive(Debug, Serialize)]
pub struct ScanResult {
    pub module_id: String,
    pub manifest: Option<Manifest>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct InstalledModule {
    pub id: String,
    pub name: String,
    pub version: String,
    pub enabled: i32,
    pub state: String,
}

/// Validate a manifest JSON value
pub fn validate_manifest(data: &serde_json::Value) -> std::result::Result<Manifest, String> {
    serde_json::from_value::<Manifest>(data.clone())
        .map_err(|e| format!("invalid manifest: {e}"))
}

/// Read manifest from a directory
fn read_manifest_from_dir(dir: &Path) -> std::result::Result<Manifest, String> {
    let manifest_path = dir.join("manifest.json");
    if !manifest_path.exists() {
        return Err("Missing manifest.json".to_string());
    }
    let content = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("failed to read manifest.json: {e}"))?;
    let data: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("invalid JSON: {e}"))?;
    validate_manifest(&data)
}

/// Scan all modules in the modules directory
pub fn scan_modules(modules_dir: &Path) -> Vec<ScanResult> {
    let mut results = Vec::new();
    if !modules_dir.exists() {
        return results;
    }
    let entries = match std::fs::read_dir(modules_dir) {
        Ok(e) => e,
        Err(_) => return results,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let module_id = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        match read_manifest_from_dir(&path) {
            Ok(manifest) => results.push(ScanResult {
                module_id,
                manifest: Some(manifest),
                error: None,
            }),
            Err(e) => results.push(ScanResult {
                module_id,
                manifest: None,
                error: Some(e),
            }),
        }
    }
    results
}

/// Sync scanned modules to the database (upsert modules, permissions, order)
pub fn sync_modules_to_db(conn: &Connection, modules_dir: &Path) -> Result<()> {
    let scan_results = scan_modules(modules_dir);
    let now = chrono::Utc::now().to_rfc3339();

    let tx = conn.unchecked_transaction().map_err(Error::Database)?;

    // Collect IDs from disk
    let disk_ids: std::collections::HashSet<String> = scan_results
        .iter()
        .filter(|r| r.manifest.is_some())
        .map(|r| r.module_id.clone())
        .collect();

    for result in &scan_results {
        if let Some(manifest) = &result.manifest {
            // UPSERT module
            tx.execute(
                "INSERT INTO modules (id, name, version, entry, type, description, author, icon, min_natives_version, enabled, state, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1, 'installed', ?10, ?10)
                 ON CONFLICT(id) DO UPDATE SET name=excluded.name, version=excluded.version, entry=excluded.entry, type=excluded.type, description=excluded.description, author=excluded.author, icon=excluded.icon, min_natives_version=excluded.min_natives_version, updated_at=excluded.updated_at",
                rusqlite::params![
                    manifest.id, manifest.name, manifest.version, manifest.entry,
                    manifest.module_type, manifest.description, manifest.author, manifest.icon,
                    manifest.min_natives_version, now
                ],
            ).map_err(Error::Database)?;

            // Sync permissions: delete old, insert new (all granted=0)
            tx.execute(
                "DELETE FROM module_permissions WHERE module_id = ?1",
                rusqlite::params![manifest.id],
            )
            .map_err(Error::Database)?;
            for perm in &manifest.permissions {
                tx.execute(
                    "INSERT INTO module_permissions (module_id, permission, granted) VALUES (?1, ?2, 0)",
                    rusqlite::params![manifest.id, perm],
                ).map_err(Error::Database)?;
            }

            // Ensure module_order entry
            tx.execute(
                "INSERT OR IGNORE INTO module_order (module_id, sort_order) VALUES (?1, (SELECT COALESCE(MAX(sort_order), 0) + 1 FROM module_order))",
                rusqlite::params![manifest.id],
            ).map_err(Error::Database)?;
        }
    }

    // Purge DB rows for modules no longer on disk
    let existing: Vec<String> = {
        let mut stmt = tx
            .prepare("SELECT id FROM modules")
            .map_err(Error::Database)?;
        let mut rows = stmt.query([]).map_err(Error::Database)?;
        let mut ids = Vec::new();
        while let Some(row) = rows.next().map_err(Error::Database)? {
            ids.push(row.get::<_, String>(0).map_err(Error::Database)?);
        }
        ids
    };
    for id in existing {
        if !disk_ids.contains(&id) {
            tx.execute(
                "DELETE FROM module_permissions WHERE module_id = ?1",
                rusqlite::params![id],
            )
            .map_err(Error::Database)?;
            tx.execute(
                "DELETE FROM module_order WHERE module_id = ?1",
                rusqlite::params![id],
            )
            .map_err(Error::Database)?;
            tx.execute("DELETE FROM modules WHERE id = ?1", rusqlite::params![id])
                .map_err(Error::Database)?;
        }
    }

    tx.commit().map_err(Error::Database)?;
    Ok(())
}

fn get_actual_manifest_dir(extract_dir: &Path) -> std::path::PathBuf {
    if let Ok(entries) = std::fs::read_dir(extract_dir) {
        let mut subdirs = Vec::new();
        let mut file_count = 0;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "__MACOSX" {
                continue;
            }
            if entry.path().is_dir() {
                subdirs.push(entry.path());
            } else {
                file_count += 1;
            }
        }
        if subdirs.len() == 1 && file_count == 0 {
            return subdirs[0].clone();
        }
    }
    extract_dir.to_path_buf()
}

/// Install a module from a directory or zip file
pub fn install_module(conn: &Connection, modules_dir: &Path, source: &str) -> Result<String> {
    std::fs::create_dir_all(modules_dir).map_err(Error::Io)?;

    let source_path = Path::new(source);
    let manifest = if source_path.is_dir() {
        read_manifest_from_dir(source_path).map_err(|e| Error::InvalidInput(e))?
    } else if source.ends_with(".zip") {
        // Extract zip to temp dir
        let temp_dir = modules_dir.join(format!(
            "__extract_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        ));
        extract_zip(source_path, &temp_dir)?;
        
        // Find manifest in extracted content
        let actual_dir = get_actual_manifest_dir(&temp_dir);
        let manifest = read_manifest_from_dir(&actual_dir).map_err(|e| Error::InvalidInput(e))?;
        
        // Move to final location
        let dest = modules_dir.join(&manifest.id);
        if dest.exists() {
            std::fs::remove_dir_all(&dest).map_err(Error::Io)?;
        }
        std::fs::rename(&actual_dir, &dest).map_err(Error::Io)?;
        
        // Clean up temp_dir if it was nested
        if actual_dir != temp_dir {
            let _ = std::fs::remove_dir_all(&temp_dir);
        }
        
        manifest
    } else {
        return Err(Error::InvalidInput(
            "source must be a directory or .zip file".into(),
        ));
    };

    let module_id = manifest.id.clone();
    let dest = modules_dir.join(&module_id);

    // If directory exists but wasn't just installed (rename succeeded above), copy
    if !dest.exists() && source_path.is_dir() {
        copy_dir_recursive(source_path, &dest)?;
    }

    // DB registration
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO modules (id, name, version, entry, type, description, author, icon, min_natives_version, enabled, state, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1, 'installed', ?10, ?10)
         ON CONFLICT(id) DO UPDATE SET name=excluded.name, version=excluded.version, entry=excluded.entry, type=excluded.type, description=excluded.description, author=excluded.author, icon=excluded.icon, min_natives_version=excluded.min_natives_version, state='installed', updated_at=excluded.updated_at",
        rusqlite::params![
            manifest.id, manifest.name, manifest.version, manifest.entry,
            manifest.module_type, manifest.description, manifest.author, manifest.icon,
            manifest.min_natives_version, now
        ],
    ).map_err(Error::Database)?;

    // Sync permissions (all granted=0 initially)
    conn.execute(
        "DELETE FROM module_permissions WHERE module_id = ?1",
        rusqlite::params![manifest.id],
    )
    .map_err(Error::Database)?;
    for perm in &manifest.permissions {
        conn.execute(
            "INSERT INTO module_permissions (module_id, permission, granted) VALUES (?1, ?2, 0)",
            rusqlite::params![manifest.id, perm],
        )
        .map_err(Error::Database)?;
    }

    // Ensure module_order
    conn.execute(
        "INSERT OR IGNORE INTO module_order (module_id, sort_order) VALUES (?1, (SELECT COALESCE(MAX(sort_order), 0) + 1 FROM module_order))",
        rusqlite::params![manifest.id],
    ).map_err(Error::Database)?;

    Ok(module_id)
}

/// Uninstall a module (remove from disk + DB)
/// Update a module from a new source (directory or .zip).
/// Flow: read new manifest → backup old dir → replace files → re-sync DB (permissions etc.)
/// On failure, rolls back from backup.
pub fn update_module(
    conn: &Connection,
    modules_dir: &Path,
    module_id: &str,
    source: &str,
) -> Result<String> {
    let module_dir = modules_dir.join(module_id);
    if !module_dir.exists() {
        return Err(Error::InvalidInput(format!(
            "module '{}' is not installed",
            module_id
        )));
    }

    // 1. Read new manifest from source
    let source_path = Path::new(source);
    let (manifest, extracted_temp) = if source_path.is_dir() {
        let m = read_manifest_from_dir(source_path).map_err(|e| Error::InvalidInput(e))?;
        (m, None)
    } else if source.ends_with(".zip") {
        let temp_dir = modules_dir.join(format!(
            "__update_extract_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        ));
        extract_zip(source_path, &temp_dir)?;
        let actual_dir = get_actual_manifest_dir(&temp_dir);
        let m = read_manifest_from_dir(&actual_dir).map_err(|e| Error::InvalidInput(e))?;
        (m, Some((temp_dir, actual_dir)))
    } else {
        return Err(Error::InvalidInput(
            "source must be a directory or .zip file".into(),
        ));
    };

    // Verify manifest id matches
    if manifest.id != module_id {
        // Clean up temp extraction if any
        if let Some((temp, _)) = &extracted_temp {
            let _ = std::fs::remove_dir_all(temp);
        }
        return Err(Error::InvalidInput(format!(
            "manifest id '{}' does not match module_id '{}'",
            manifest.id, module_id
        )));
    }

    // 2. Backup existing module directory (atomic: rename to .bak)
    let backup_dir = modules_dir.join(format!("{}.bak", module_id));
    if backup_dir.exists() {
        let _ = std::fs::remove_dir_all(&backup_dir);
    }
    std::fs::rename(&module_dir, &backup_dir).map_err(Error::Io)?;

    // 3. Replace files
    let replace_result: Result<()> = if let Some((temp, actual_dir)) = &extracted_temp {
        // Move extracted content to module dir
        std::fs::rename(actual_dir, &module_dir).map_err(Error::Io)?;
        // Clean up temp_dir if it was nested
        if *actual_dir != *temp {
            let _ = std::fs::remove_dir_all(temp);
        }
        Ok(())
    } else {
        // Copy from source directory
        copy_dir_recursive(source_path, &module_dir)
    };

    if let Err(e) = replace_result {
        // Rollback: restore from backup
        let _ = std::fs::rename(&backup_dir, &module_dir);
        return Err(e);
    }

    // 4. Re-sync DB: update manifest fields + permissions
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE modules SET name=?1, version=?2, entry=?3, type=?4, description=?5, author=?6, icon=?7, min_natives_version=?8, state='installed', updated_at=?9 WHERE id=?10",
        rusqlite::params![
            manifest.name, manifest.version, manifest.entry,
            manifest.module_type, manifest.description, manifest.author, manifest.icon,
            manifest.min_natives_version, now, manifest.id
        ],
    ).map_err(Error::Database)?;

    // Re-sync permissions: delete old, insert new (preserve granted status for matching perms)
    let existing_granted: std::collections::HashMap<String, bool> = {
        let mut map = std::collections::HashMap::new();
        let mut stmt = conn.prepare("SELECT permission, granted FROM module_permissions WHERE module_id = ?1").map_err(Error::Database)?;
        let rows = stmt.query_map(rusqlite::params![manifest.id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?))
        }).map_err(Error::Database)?;
        for row in rows {
            if let Ok((perm, granted)) = row {
                map.insert(perm, granted);
            }
        }
        map
    };

    conn.execute(
        "DELETE FROM module_permissions WHERE module_id = ?1",
        rusqlite::params![manifest.id],
    ).map_err(Error::Database)?;

    for perm in &manifest.permissions {
        // Preserve previously granted status; new perms default to not granted
        let granted = existing_granted.get(perm).copied().unwrap_or(false);
        conn.execute(
            "INSERT INTO module_permissions (module_id, permission, granted) VALUES (?1, ?2, ?3)",
            rusqlite::params![manifest.id, perm, granted],
        ).map_err(Error::Database)?;
    }

    // 5. Remove backup on success
    let _ = std::fs::remove_dir_all(&backup_dir);

    Ok(manifest.id.clone())
}

pub fn uninstall_module(conn: &Connection, modules_dir: &Path, module_id: &str) -> Result<()> {
    let module_dir = modules_dir.join(module_id);
    if module_dir.exists() {
        std::fs::remove_dir_all(&module_dir).map_err(Error::Io)?;
    }

    // Delete from DB in correct order (foreign keys)
    conn.execute(
        "DELETE FROM module_permissions WHERE module_id = ?1",
        rusqlite::params![module_id],
    )
    .map_err(Error::Database)?;
    conn.execute(
        "DELETE FROM module_order WHERE module_id = ?1",
        rusqlite::params![module_id],
    )
    .map_err(Error::Database)?;
    conn.execute(
        "DELETE FROM module_data WHERE module_id = ?1",
        rusqlite::params![module_id],
    )
    .map_err(Error::Database)?;
    conn.execute(
        "DELETE FROM modules WHERE id = ?1",
        rusqlite::params![module_id],
    )
    .map_err(Error::Database)?;

    Ok(())
}

/// Enable a module
pub fn enable_module(conn: &Connection, module_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE modules SET enabled = 1 WHERE id = ?1",
        rusqlite::params![module_id],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Disable a module
pub fn disable_module(conn: &Connection, module_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE modules SET enabled = 0 WHERE id = ?1",
        rusqlite::params![module_id],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// List all installed modules
pub fn list_modules(conn: &Connection) -> Result<Vec<InstalledModule>> {
    let mut stmt = conn
        .prepare("SELECT id, name, version, enabled, state FROM modules ORDER BY id")
        .map_err(Error::Database)?;
    let mut results = Vec::new();
    let mut rows = stmt.query([]).map_err(Error::Database)?;
    while let Some(row) = rows.next().map_err(Error::Database)? {
        results.push(InstalledModule {
            id: row.get(0).map_err(Error::Database)?,
            name: row.get(1).map_err(Error::Database)?,
            version: row.get(2).map_err(Error::Database)?,
            enabled: row.get(3).map_err(Error::Database)?,
            state: row.get(4).map_err(Error::Database)?,
        });
    }
    Ok(results)
}

/// Read manifest from source (directory or zip)
pub fn read_manifest_from_source(
    modules_dir: &Path,
    source: &str,
) -> std::result::Result<Manifest, String> {
    let source_path = Path::new(source);
    if source_path.is_dir() {
        read_manifest_from_dir(source_path)
    } else if source.ends_with(".zip") {
        let temp_dir = modules_dir.join(format!(
            "__read_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        ));
        extract_zip(source_path, &temp_dir).map_err(|e| e.to_string())?;
        let actual_dir = get_actual_manifest_dir(&temp_dir);
        let manifest = read_manifest_from_dir(&actual_dir);
        let _ = std::fs::remove_dir_all(&temp_dir);
        manifest
    } else {
        Err("source must be a directory or .zip file".into())
    }
}

// ── Helpers ──

fn extract_zip(zip_path: &Path, dest: &Path) -> Result<()> {
    let file = std::fs::File::open(zip_path).map_err(Error::Io)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| Error::Internal(e.to_string()))?;

    std::fs::create_dir_all(dest).map_err(Error::Io)?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| Error::Internal(e.to_string()))?;
        let entry_name = entry.name().to_string();

        // Zip Slip protection
        if entry_name.contains("..") || entry_name.starts_with('/') {
            return Err(Error::InvalidInput(format!(
                "unsafe path in zip: {entry_name}"
            )));
        }

        let outpath = dest.join(&entry_name);
        if entry.is_dir() {
            std::fs::create_dir_all(&outpath).map_err(Error::Io)?;
        } else {
            if let Some(parent) = outpath.parent() {
                std::fs::create_dir_all(parent).map_err(Error::Io)?;
            }
            let mut outfile = std::fs::File::create(&outpath).map_err(Error::Io)?;
            std::io::copy(&mut entry, &mut outfile).map_err(Error::Io)?;
        }
    }
    Ok(())
}

/// Write an AI-generated module to disk atomically, then hot-sync into DB.
///
/// This is the core of the "AI App Engine": the AI brain generates HTML/JS/Tailwind
/// code, and this function writes it to the modules directory with full atomic
/// guarantees (temp file → fsync → rename), then calls `sync_modules_to_db` to
/// bring it online immediately without restart.
pub fn write_generated_module(
    conn: &Connection,
    modules_dir: &Path,
    module_id: &str,
    name: &str,
    html_content: &str,
    permissions: &[String],
) -> Result<()> {
    let app_dir = modules_dir.join(module_id);
    std::fs::create_dir_all(&app_dir).map_err(Error::Io)?;

    // Build manifest matching existing Manifest struct
    let manifest = Manifest {
        id: module_id.to_string(),
        name: name.to_string(),
        version: "0.1.0".to_string(),
        entry: "index.html".to_string(),
        module_type: "web".to_string(),
        description: Some(format!("AI-generated module: {name}")),
        author: Some("ai-engine".to_string()),
        icon: None,
        min_natives_version: None,
        permissions: permissions.to_vec(),
    };
    let manifest_json = serde_json::to_string_pretty(&manifest).map_err(Error::Json)?;

    // Atomic write helper: write to temp → fsync → rename
    atomic_write(&app_dir.join("manifest.json"), &manifest_json)?;
    atomic_write(&app_dir.join("index.html"), html_content)?;

    // Hot-sync: re-scan all modules and UPSERT into SQLite
    // This reuses the existing transactional sync logic (UPSERT + permission
    // refresh + module_order append + orphan cleanup) — zero duplication.
    sync_modules_to_db(conn, modules_dir)?;

    Ok(())
}

/// Atomic file write: write to a temp file in the same directory, fsync, then
/// rename over the target. On POSIX, rename is atomic, so a crash at any point
/// leaves either the old or the new file intact — never a partial write.
fn atomic_write(path: &Path, content: &str) -> Result<()> {
    use std::io::Write;
    let dir = path.parent().ok_or_else(|| Error::Internal("no parent dir".into()))?;
    let tmp_path = dir.join(format!(
        ".{}.tmp",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
    ));
    {
        let mut f = std::fs::File::create(&tmp_path).map_err(Error::Io)?;
        f.write_all(content.as_bytes()).map_err(Error::Io)?;
        f.sync_all().map_err(Error::Io)?;
    }
    std::fs::rename(&tmp_path, path).map_err(Error::Io)?;
    Ok(())
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest).map_err(Error::Io)?;
    for entry in std::fs::read_dir(src).map_err(Error::Io)? {
        let entry = entry.map_err(Error::Io)?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else {
            std::fs::copy(&src_path, &dest_path).map_err(Error::Io)?;
        }
    }
    Ok(())
}
