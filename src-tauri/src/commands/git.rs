use crate::{git, Error, Result};
use serde_json::Value as JsonValue;

#[tauri::command]
pub fn git_status(dir_path: String) -> Result<JsonValue> {
    let status = git::git_status(&dir_path)?;
    serde_json::to_value(status).map_err(|e| Error::Internal(e.to_string()))
}

#[tauri::command]
pub fn git_diff(file_path: String) -> Result<String> {
    git::git_diff(&file_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join("n2-test-git").join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn test_git_status_non_repo() {
        let dir = test_dir("gs_nr");
        let r = git_status(dir.to_string_lossy().into());
        // Non-repo dir should error — any error is valid
        assert!(r.is_err(), "non-repo dir must return error");
        let _msg = r.unwrap_err().to_string();
        // Just ensure no panic — error message varies by system git config
    }

    #[test]
    fn test_git_diff_non_repo() {
        let r = git_diff("/tmp/_n2_no_repo_".into());
        assert!(r.is_err());
    }

    #[test]
    fn test_git_status_empty_path() {
        let r = git_status("".into());
        assert!(r.is_err());
    }
}
