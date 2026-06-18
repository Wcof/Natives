use crate::{Error, Result};

#[tauri::command]
pub fn skills_enable(path: String) -> Result<()> {
    // TODO: Implement in Loop 11
    Err(Error::NotImplemented(format!("skills:enable({path})")))
}

#[tauri::command]
pub fn skills_disable(path: String) -> Result<()> {
    // TODO: Implement in Loop 11
    Err(Error::NotImplemented(format!("skills:disable({path})")))
}

#[tauri::command]
pub fn skills_get_deactivated_path(path: String) -> Result<String> {
    // TODO: Implement in Loop 11
    Err(Error::NotImplemented(format!("skills:getDeactivatedPath({path})")))
}

#[tauri::command]
pub fn skills_uninstall(path: String) -> Result<()> {
    // TODO: Implement in Loop 11
    Err(Error::NotImplemented(format!("skills:uninstall({path})")))
}
