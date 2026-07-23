//! Filesystem paths for creative apps under ~/.natives/creative-apps.

use std::path::{Path, PathBuf};

pub const CREATIVE_APPS_DIRNAME: &str = "creative-apps";
pub const RELEASE_DIRNAME: &str = "release";
pub const RUNTIME_DIRNAME: &str = "runtime";

/// Max size of a single release asset (100 MiB).
pub const MAX_ASSET_BYTES: u64 = 100 * 1024 * 1024;
/// Max total size of extracted/downloaded assets for one app (200 MiB).
pub const MAX_TOTAL_BYTES: u64 = 200 * 1024 * 1024;

pub fn natives_home() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".natives")
}

pub fn creative_apps_root() -> PathBuf {
    natives_home().join(CREATIVE_APPS_DIRNAME)
}

pub fn app_dir(app_id: &str) -> PathBuf {
    creative_apps_root().join(app_id)
}

pub fn release_dir(app_id: &str) -> PathBuf {
    app_dir(app_id).join(RELEASE_DIRNAME)
}

pub fn runtime_dir(app_id: &str) -> PathBuf {
    app_dir(app_id).join(RUNTIME_DIRNAME)
}

pub fn compose_project_name(app_id: &str) -> String {
    // Docker Compose project names: lowercase alphanumeric + hyphens.
    let safe: String = app_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    format!("natives-{safe}")
}

pub fn run_container_name(app_id: &str) -> String {
    let safe: String = app_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    format!("natives-ca-{safe}")
}

pub fn resource_label(app_id: &str) -> String {
    format!("ai.natives.creative-app.id={app_id}")
}

pub fn resource_label_key() -> &'static str {
    "ai.natives.creative-app.id"
}

/// Atomically write bytes: temp file → fsync → rename.
pub fn atomic_write(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!(
        "tmp.{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Ensure path stays under base (no traversal).
pub fn ensure_under(base: &Path, candidate: &Path) -> Result<PathBuf, String> {
    let base_c = base
        .canonicalize()
        .map_err(|e| format!("base canonicalize: {e}"))?;
    // candidate may not exist yet — canonicalize parent + join name when needed
    let cand = if candidate.exists() {
        candidate
            .canonicalize()
            .map_err(|e| format!("candidate canonicalize: {e}"))?
    } else {
        let parent = candidate
            .parent()
            .ok_or_else(|| "candidate has no parent".to_string())?;
        let name = candidate
            .file_name()
            .ok_or_else(|| "candidate has no file name".to_string())?;
        if parent.as_os_str().is_empty() || parent == Path::new("") {
            base_c.join(name)
        } else {
            let parent_c = if parent.exists() {
                parent
                    .canonicalize()
                    .map_err(|e| format!("parent canonicalize: {e}"))?
            } else {
                // relative to base
                base_c.join(parent)
            };
            parent_c.join(name)
        }
    };
    if !cand.starts_with(&base_c) {
        return Err(format!(
            "path escapes base: {} not under {}",
            cand.display(),
            base_c.display()
        ));
    }
    Ok(cand)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_and_container_names() {
        assert_eq!(compose_project_name("AbC_1"), "natives-abc_1");
        assert_eq!(run_container_name("x y"), "natives-ca-x-y");
    }

    #[test]
    fn atomic_write_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "natives-ca-path-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("a.txt");
        atomic_write(&p, b"hello").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "hello");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
