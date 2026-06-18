use crate::{Error, Result};

#[tauri::command]
pub fn state_save(module_id: String, state: String) -> Result<()> {
    // TODO: Implement in Loop 6
    Err(Error::NotImplemented(format!("state:save({module_id})")))
}

#[tauri::command]
pub fn state_load(module_id: String) -> Result<Option<String>> {
    // TODO: Implement in Loop 6
    Err(Error::NotImplemented(format!("state:load({module_id})")))
}

#[tauri::command]
pub fn state_clear(module_id: String) -> Result<()> {
    // TODO: Implement in Loop 6
    Err(Error::NotImplemented(format!("state:clear({module_id})")))
}
