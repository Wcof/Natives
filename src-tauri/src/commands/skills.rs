use crate::{agent, AppState, Error, Result};
use std::path::Path;
use tauri::State;

/// 启用 skill：从 `_disabled/name` 移回父目录 `name`
/// 处理符号链接：解析绝对目标后删旧链建新链
#[tauri::command]
pub fn skills_enable(path: String) -> Result<()> {
    if !agent::validate_skill_dir(&path).unwrap_or(false) {
        return Err(Error::Internal("skill not in scanned skills list".into()));
    }

    let skill_path = Path::new(&path);
    let skill_name = match skill_path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n.to_string(),
        None => return Err(Error::Internal("invalid skill path".into())),
    };

    let parent = match skill_path.parent() {
        Some(p) => p,
        None => return Err(Error::Internal("invalid path".into())),
    };

    let parent_name = parent.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if parent_name != "_disabled" {
        agent::invalidate_skills_cache();
        return Ok(());
    }

    let grandparent = match parent.parent() {
        Some(p) => p,
        None => return Err(Error::Internal("invalid path".into())),
    };
    let dest = grandparent.join(&skill_name);

    if dest.exists() {
        return Err(Error::Internal(
            "a directory with the same name already exists at the target location".into(),
        ));
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
    if !agent::validate_skill_dir(&path).unwrap_or(false) {
        return Err(Error::Internal("skill not in scanned skills list".into()));
    }

    let skill_path = Path::new(&path);
    let skill_name = match skill_path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n.to_string(),
        None => return Err(Error::Internal("invalid skill path".into())),
    };

    let parent = match skill_path.parent() {
        Some(p) => p,
        None => return Err(Error::Internal("invalid path".into())),
    };

    let disabled_dir = parent.join("_disabled");
    let dest = disabled_dir.join(&skill_name);

    std::fs::create_dir_all(&disabled_dir).map_err(Error::Io)?;

    if dest.exists() {
        return Err(Error::Internal(
            "a directory with the same name already exists at the target location".into(),
        ));
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
pub async fn skills_uninstall(path: String, state: State<'_, AppState>) -> Result<()> {
    let _permit = state
        .skills_trash_slots
        .acquire()
        .await
        .map_err(|e| Error::Internal(format!("skills trash semaphore closed: {e}")))?;

    tokio::task::spawn_blocking(move || {
        if !agent::validate_skill_dir(&path).unwrap_or(false) {
            return Err(Error::Internal("skill not in scanned skills list".into()));
        }

        let skill_path = Path::new(&path);
        if !skill_path.exists() {
            return Err(Error::Internal("skill directory does not exist".into()));
        }

        move_to_trash(skill_path)
    })
    .await
    .map_err(|e| Error::Internal(format!("skills trash task failed: {e}")))??;

    agent::invalidate_skills_cache();
    Ok(())
}

fn move_to_trash(skill_path: &Path) -> Result<()> {
    move_to_trash_with(skill_path, system_trash_delete)
}

fn move_to_trash_with<F, E>(skill_path: &Path, trash_delete: F) -> Result<()>
where
    F: FnOnce(&Path) -> std::result::Result<(), E>,
    E: std::fmt::Display,
{
    trash_delete(skill_path)
        .map_err(|e| Error::Internal(format!("failed to move skill to system trash: {e}")))
}

#[cfg(target_os = "macos")]
fn system_trash_delete(skill_path: &Path) -> std::result::Result<(), trash::Error> {
    use trash::macos::{DeleteMethod, TrashContextExtMacos};

    let mut context = trash::TrashContext::new();
    context.set_delete_method(DeleteMethod::NsFileManager);
    context.delete(skill_path)
}

#[cfg(not(target_os = "macos"))]
fn system_trash_delete(skill_path: &Path) -> std::result::Result<(), trash::Error> {
    trash::delete(skill_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join("n2-test-skills").join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn trash_failure_preserves_skill_fixture() {
        let skill_dir = test_dir("trash_failure_preserves_skill_fixture");
        let skill_file = skill_dir.join("SKILL.md");
        std::fs::write(&skill_file, b"fixture").expect("write temporary skill fixture");

        let result = move_to_trash_with(&skill_dir, |_path| {
            Err::<(), _>("injected trash permission failure")
        });

        assert!(matches!(
            result,
            Err(Error::Internal(message))
                if message.contains("injected trash permission failure")
        ));
        assert!(skill_dir.exists());
        assert_eq!(
            std::fs::read(&skill_file).expect("read preserved skill fixture"),
            b"fixture"
        );

        std::fs::remove_dir_all(skill_dir).expect("clean temporary skill fixture");
    }

    #[test]
    fn trash_success_returns_ok_without_fallback() {
        let skill_dir = test_dir("trash_success_returns_ok_without_fallback");
        let result = move_to_trash_with(&skill_dir, |_path| Ok::<(), &str>(()));

        assert!(result.is_ok());

        std::fs::remove_dir_all(skill_dir).expect("clean temporary skill fixture");
    }

    #[test]
    fn test_skills_disable_invalid_path() {
        let r = skills_disable("/tmp/_n2_nonexistent_skill_".into());
        // Should error because path doesn't match skill dir validation
        assert!(r.is_err());
    }
}
