use crate::{agent, Error, Result};
use std::path::Path;

/// 启用 skill：从 `_disabled/name` 移回父目录 `name`
/// 处理符号链接：解析绝对目标后删旧链建新链
#[tauri::command]
pub fn skills_enable(path: String) -> Result<()> {
    if !agent::validate_skill_dir(&path).unwrap_or(true) {
        return Err(Error::Internal("不在已扫描的 skills 清单里".into()));
    }

    let skill_path = Path::new(&path);
    let skill_name = match skill_path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n.to_string(),
        None => return Err(Error::Internal("无效的 skill 路径".into())),
    };

    let parent = match skill_path.parent() {
        Some(p) => p,
        None => return Err(Error::Internal("无效路径".into())),
    };

    let parent_name = parent.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if parent_name != "_disabled" {
        agent::invalidate_skills_cache();
        return Ok(());
    }

    let grandparent = match parent.parent() {
        Some(p) => p,
        None => return Err(Error::Internal("无效路径".into())),
    };
    let dest = grandparent.join(&skill_name);

    if dest.exists() {
        return Err(Error::Internal("目标位置已有同名目录".into()));
    }

    let meta = std::fs::symlink_metadata(skill_path).map_err(Error::Io)?;
    if meta.file_type().is_symlink() {
        let target = std::fs::canonicalize(skill_path).map_err(Error::Io)?;
        std::fs::remove_file(skill_path).map_err(Error::Io)?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &dest).map_err(Error::Io)?;
    } else {
        std::fs::rename(skill_path, &dest).map_err(Error::Io)?;
    }

    agent::invalidate_skills_cache();
    Ok(())
}

/// 禁用 skill：从 `name` 移入 `_disabled/name`
/// 处理符号链接：解析绝对目标后删旧链建新链
#[tauri::command]
pub fn skills_disable(path: String) -> Result<()> {
    if !agent::validate_skill_dir(&path).unwrap_or(true) {
        return Err(Error::Internal("不在已扫描的 skills 清单里".into()));
    }

    let skill_path = Path::new(&path);
    let skill_name = match skill_path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n.to_string(),
        None => return Err(Error::Internal("无效的 skill 路径".into())),
    };

    let parent = match skill_path.parent() {
        Some(p) => p,
        None => return Err(Error::Internal("无效路径".into())),
    };

    let disabled_dir = parent.join("_disabled");
    let dest = disabled_dir.join(&skill_name);

    std::fs::create_dir_all(&disabled_dir).map_err(Error::Io)?;

    if dest.exists() {
        return Err(Error::Internal("目标位置已有同名目录".into()));
    }

    let meta = std::fs::symlink_metadata(skill_path).map_err(Error::Io)?;
    if meta.file_type().is_symlink() {
        let target = std::fs::canonicalize(skill_path).map_err(Error::Io)?;
        std::fs::remove_file(skill_path).map_err(Error::Io)?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &dest).map_err(Error::Io)?;
    } else {
        std::fs::rename(skill_path, &dest).map_err(Error::Io)?;
    }

    agent::invalidate_skills_cache();
    Ok(())
}

/// 获取 deactivated 路径（兼容旧接口）
#[tauri::command]
pub fn skills_get_deactivated_path(path: String) -> Result<String> {
    let disabled_marker = std::path::Path::new(&path).join(".disabled");
    Ok(disabled_marker.to_string_lossy().to_string())
}

/// 卸载 skill：移到系统废纸篓（可恢复），而非永久删除
#[tauri::command]
pub fn skills_uninstall(path: String) -> Result<()> {
    if !agent::validate_skill_dir(&path).unwrap_or(true) {
        return Err(Error::Internal("不在已扫描的 skills 清单里".into()));
    }

    let skill_path = Path::new(&path);
    if !skill_path.exists() {
        return Err(Error::Internal("skill 目录不存在".into()));
    }

    trash_or_delete(skill_path)?;
    agent::invalidate_skills_cache();
    Ok(())
}

/// 移到系统废纸篓，失败则永久删除
fn trash_or_delete(skill_path: &Path) -> Result<()> {
    let path_str = skill_path.to_string_lossy().to_string();

    // macOS: 尝试 Finder AppleScript
    if cfg!(target_os = "macos") {
        let result = std::process::Command::new("osascript")
            .args([
                "-e",
                "on run argv",
                "-e",
                "tell application \"Finder\" to delete (POSIX file (item 1 of argv) as alias)",
                "-e",
                "end run",
                &path_str,
            ])
            .output();

        match result {
            Ok(output) if output.status.success() => return Ok(()),
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if stderr.contains("-1743") || stderr.contains("-600") {
                    // Finder 自动化未授权，降级为永久删除
                    return force_delete(skill_path);
                }
                return Err(Error::Internal(format!("移到废纸篓失败: {}", stderr)));
            }
            Err(_) => {
                // osascript 不可用，降级为永久删除
                return force_delete(skill_path);
            }
        }
    }

    // Linux: 尝试 gio trash / trash-put / trash
    if cfg!(target_os = "linux") {
        for cmd in &["gio", "trash-put", "trash"] {
            if let Ok(output) = std::process::Command::new(cmd).arg(&path_str).output() {
                if output.status.success() {
                    return Ok(());
                }
            }
        }
    }

    // 降级：永久删除
    force_delete(skill_path)
}

fn force_delete(skill_path: &Path) -> Result<()> {
    if skill_path.is_dir() {
        std::fs::remove_dir_all(skill_path).map_err(Error::Io)?;
    } else {
        std::fs::remove_file(skill_path).map_err(Error::Io)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn test_dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join("n2-test-skills").join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn test_force_delete_dir() {
        let dir = test_dir("fd_dir");
        let sub = dir.join("subdir");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("f.txt"), b"x").unwrap();
        force_delete(&dir).expect("force_delete dir");
        assert!(!dir.exists());
    }

    #[test]
    fn test_force_delete_file() {
        let dir = test_dir("fd_file");
        let f = dir.join("target.txt");
        std::fs::write(&f, b"x").unwrap();
        force_delete(&f).expect("force_delete file");
        assert!(!f.exists());
    }

    #[test]
    fn test_force_delete_nonexistent() {
        let path = Path::new("/tmp/_n2_nonexistent_skill_");
        let r = force_delete(path);
        assert!(r.is_err());
    }

    #[test]
    fn test_skills_disable_invalid_path() {
        let r = skills_disable("/tmp/_n2_nonexistent_skill_".into());
        // Should error because path doesn't match skill dir validation
        assert!(r.is_err());
    }
}
