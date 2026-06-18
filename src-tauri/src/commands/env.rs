use crate::{Error, Result};
use serde_json::Value as JsonValue;

#[tauri::command]
pub fn env_get_variables(profile_id: String) -> Result<JsonValue> {
    // TODO: Implement in Loop 7
    Err(Error::NotImplemented(format!("env:getVariables({profile_id})")))
}

#[tauri::command]
pub fn env_get_default_profile() -> Result<String> {
    // TODO: Implement in Loop 7
    Err(Error::NotImplemented("env:getDefaultProfile".into()))
}

#[tauri::command]
pub fn env_list_profiles() -> Result<Vec<String>> {
    // TODO: Implement in Loop 7
    Err(Error::NotImplemented("env:listProfiles".into()))
}

#[tauri::command]
pub fn env_create_profile(name: String) -> Result<()> {
    // TODO: Implement in Loop 7
    Err(Error::NotImplemented(format!("env:createProfile({name})")))
}

#[tauri::command]
pub fn env_delete_profile(name: String) -> Result<()> {
    // TODO: Implement in Loop 7
    Err(Error::NotImplemented(format!("env:deleteProfile({name})")))
}

#[tauri::command]
pub fn env_set_default_profile(name: String) -> Result<()> {
    // TODO: Implement in Loop 7
    Err(Error::NotImplemented(format!("env:setDefaultProfile({name})")))
}

#[tauri::command]
pub fn env_set_variable(profile_id: String, key: String, value: String) -> Result<()> {
    // TODO: Implement in Loop 7
    Err(Error::NotImplemented(format!("env:setVariable({profile_id}, {key})")))
}

#[tauri::command]
pub fn env_delete_variable(profile_id: String, key: String) -> Result<()> {
    // TODO: Implement in Loop 7
    Err(Error::NotImplemented(format!("env:deleteVariable({profile_id}, {key})")))
}

#[tauri::command]
pub fn env_encrypt(text: String) -> Result<String> {
    // TODO: Implement in Loop 7 — use keychain or OS-level encryption
    Err(Error::NotImplemented("env:encrypt".into()))
}

#[tauri::command]
pub fn env_decrypt(encrypted: String) -> Result<String> {
    // TODO: Implement in Loop 7
    Err(Error::NotImplemented("env:decrypt".into()))
}
