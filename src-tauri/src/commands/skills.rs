use crate::{Error, Result};

#[tauri::command]
pub fn skills_enable(path: String) -> Result<()> {
    let disabled_marker = std::path::Path::new(&path).join(".disabled");
    if disabled_marker.exists() {
        std::fs::remove_file(&disabled_marker).map_err(Error::Io)?;
    }
    Ok(())
}

#[tauri::command]
pub fn skills_disable(path: String) -> Result<()> {
    let disabled_marker = std::path::Path::new(&path).join(".disabled");
    if !disabled_marker.exists() {
        std::fs::File::create(&disabled_marker).map_err(Error::Io)?;
    }
    Ok(())
}

#[tauri::command]
pub fn skills_get_deactivated_path(path: String) -> Result<String> {
    let disabled_marker = std::path::Path::new(&path).join(".disabled");
    Ok(disabled_marker.to_string_lossy().to_string())
}

#[tauri::command]
pub fn skills_uninstall(path: String) -> Result<()> {
    let skill_path = std::path::Path::new(&path);
    if skill_path.exists() {
        std::fs::remove_dir_all(skill_path).map_err(Error::Io)?;
    }
    Ok(())
}
