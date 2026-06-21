//! WeChat ClawBot IPC commands
//!
//! Reference: fanbox/electron/main.js:768-802 (12 wechat:* IPC handlers)

use crate::AppState;
use serde_json::Value;
use tauri::State;

#[tauri::command]
pub fn wechat_env(state: State<'_, AppState>) -> Result<Value, crate::Error> {
    let bridge_lock = state.wechat_bridge.lock().map_err(|e| crate::Error::Internal(e.to_string()))?;
    if let Some(bridge) = bridge_lock.as_ref() {
        Ok(bridge.env())
    } else {
        Ok(serde_json::json!({ "state": "uninstalled" }))
    }
}

#[tauri::command]
pub fn wechat_login(state: State<'_, AppState>) -> Result<Value, crate::Error> {
    let mut bridge_lock = state.wechat_bridge.lock().map_err(|e| crate::Error::Internal(e.to_string()))?;
    if let Some(bridge) = bridge_lock.as_mut() {
        bridge.login().map_err(crate::Error::Internal)
    } else {
        Err(crate::Error::Internal("bridge not initialized".into()))
    }
}

#[tauri::command]
pub fn wechat_disconnect(state: State<'_, AppState>) -> Result<Value, crate::Error> {
    let mut bridge_lock = state.wechat_bridge.lock().map_err(|e| crate::Error::Internal(e.to_string()))?;
    if let Some(bridge) = bridge_lock.as_mut() {
        Ok(bridge.disconnect())
    } else {
        Ok(serde_json::json!({ "ok": true }))
    }
}

#[tauri::command]
pub fn wechat_check(state: State<'_, AppState>) -> Result<Value, crate::Error> {
    let mut bridge_lock = state.wechat_bridge.lock().map_err(|e| crate::Error::Internal(e.to_string()))?;
    if let Some(bridge) = bridge_lock.as_mut() {
        Ok(bridge.check())
    } else {
        Ok(serde_json::json!({ "ok": false, "state": "uninstalled" }))
    }
}

#[tauri::command]
pub fn wechat_send(text: String, state: State<'_, AppState>) -> Result<Value, crate::Error> {
    let mut bridge_lock = state.wechat_bridge.lock().map_err(|e| crate::Error::Internal(e.to_string()))?;
    if let Some(bridge) = bridge_lock.as_mut() {
        bridge.send(&text).map_err(crate::Error::Internal)
    } else {
        Err(crate::Error::Internal("not connected".into()))
    }
}

#[tauri::command]
pub fn wechat_set_target(target: String, state: State<'_, AppState>) -> Result<(), crate::Error> {
    let mut bridge_lock = state.wechat_bridge.lock().map_err(|e| crate::Error::Internal(e.to_string()))?;
    if let Some(bridge) = bridge_lock.as_mut() {
        bridge.set_target(&target);
    }
    Ok(())
}

#[tauri::command]
pub fn wechat_set_cwd(dir: String, state: State<'_, AppState>) -> Result<(), crate::Error> {
    let mut bridge_lock = state.wechat_bridge.lock().map_err(|e| crate::Error::Internal(e.to_string()))?;
    if let Some(bridge) = bridge_lock.as_mut() {
        bridge.set_cwd(&dir);
    }
    Ok(())
}

#[tauri::command]
pub fn wechat_set_persona(persona: String, state: State<'_, AppState>) -> Result<(), crate::Error> {
    let mut bridge_lock = state.wechat_bridge.lock().map_err(|e| crate::Error::Internal(e.to_string()))?;
    if let Some(bridge) = bridge_lock.as_mut() {
        bridge.set_persona(&persona);
    }
    Ok(())
}

#[tauri::command]
pub fn wechat_detect_agents() -> Result<Value, crate::Error> {
    Ok(crate::wechat::driver::detect_agents())
}

#[tauri::command]
pub fn wechat_status(state: State<'_, AppState>) -> Result<Value, crate::Error> {
    let bridge_lock = state.wechat_bridge.lock().map_err(|e| crate::Error::Internal(e.to_string()))?;
    if let Some(bridge) = bridge_lock.as_ref() {
        Ok(serde_json::json!({
            "state": bridge.state.as_str(),
            "connected": bridge.is_connected(),
            "target": bridge.target,
            "cwd": bridge.cwd
        }))
    } else {
        Ok(serde_json::json!({ "state": "uninstalled" }))
    }
}
