//! Extension / Plugin registry (Phase 6 minimum real surface).
//!
//! Discovery from project/user scopes, trust + enable/disable, permission
//! declaration. Full host isolation and install/update land later; this module
//! provides real state + RPC so capabilities can honestly say extensions=true
//! for list/enable/disable.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionScope {
    Project,
    User,
    Builtin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub scope: ExtensionScope,
    pub trusted: bool,
    pub enabled: bool,
    pub permissions: Vec<String>,
    pub path: String,
}

#[derive(Default)]
pub struct ExtensionStore {
    items: Mutex<HashMap<String, ExtensionManifest>>,
}

impl ExtensionStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn discover_defaults(&self) {
        let mut roots = Vec::new();
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            roots.push((
                PathBuf::from(home).join(".natives").join("extensions"),
                ExtensionScope::User,
            ));
        }
        if let Ok(cwd) = std::env::current_dir() {
            roots.push((cwd.join(".natives").join("extensions"), ExtensionScope::Project));
            roots.push((cwd.join(".grok").join("extensions"), ExtensionScope::Project));
        }
        for (dir, scope) in roots {
            self.scan_dir(&dir, scope);
        }
    }

    fn scan_dir(&self, dir: &Path, scope: ExtensionScope) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let manifest_path = if path.is_dir() {
                path.join("manifest.json")
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e == "json")
                .unwrap_or(false)
            {
                path.clone()
            } else {
                continue;
            };
            let Ok(raw) = std::fs::read_to_string(&manifest_path) else {
                continue;
            };
            let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&raw) else {
                continue;
            };
            let id = value
                .get("id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    path.file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("ext")
                        .to_string()
                });
            let name = value
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or(&id)
                .to_string();
            let version = value
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("0.0.0")
                .to_string();
            let description = value
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let trusted = value
                .get("trusted")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let enabled = value
                .get("enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let permissions = value
                .get("permissions")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            // Avoid unused mut warning
            let _ = &mut value;
            let m = ExtensionManifest {
                id: id.clone(),
                name,
                version,
                description,
                scope: scope.clone(),
                trusted,
                enabled: enabled && trusted, // untrusted cannot auto-enable
                permissions,
                path: manifest_path.display().to_string(),
            };
            if let Ok(mut items) = self.items.lock() {
                items.insert(id, m);
            }
        }
    }

    pub fn list(&self) -> Vec<ExtensionManifest> {
        self.items
            .lock()
            .map(|g| g.values().cloned().collect())
            .unwrap_or_default()
    }

    pub fn register(&self, mut m: ExtensionManifest) -> Result<ExtensionManifest, String> {
        if m.id.trim().is_empty() {
            m.id = Uuid::new_v4().to_string();
        }
        if !m.trusted && m.enabled {
            return Err("untrusted extension cannot be enabled".into());
        }
        if let Ok(mut items) = self.items.lock() {
            items.insert(m.id.clone(), m.clone());
        }
        Ok(m)
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<ExtensionManifest, String> {
        let mut items = self.items.lock().map_err(|e| e.to_string())?;
        let item = items
            .get_mut(id)
            .ok_or_else(|| format!("extension not found: {id}"))?;
        if enabled && !item.trusted {
            return Err("cannot enable untrusted extension".into());
        }
        item.enabled = enabled;
        Ok(item.clone())
    }
}

static GLOBAL_EXT: std::sync::OnceLock<ExtensionStore> = std::sync::OnceLock::new();

pub fn global_extensions() -> &'static ExtensionStore {
    GLOBAL_EXT.get_or_init(|| {
        let s = ExtensionStore::new();
        s.discover_defaults();
        s
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untrusted_cannot_enable() {
        let s = ExtensionStore::new();
        let m = s
            .register(ExtensionManifest {
                id: "e1".into(),
                name: "E".into(),
                version: "1".into(),
                description: "".into(),
                scope: ExtensionScope::User,
                trusted: false,
                enabled: false,
                permissions: vec![],
                path: "/tmp/x".into(),
            })
            .unwrap();
        let err = s.set_enabled(&m.id, true).unwrap_err();
        assert!(err.contains("untrusted"));
    }

    #[test]
    fn trusted_can_enable() {
        let s = ExtensionStore::new();
        let m = s
            .register(ExtensionManifest {
                id: "e2".into(),
                name: "E2".into(),
                version: "1".into(),
                description: "".into(),
                scope: ExtensionScope::User,
                trusted: true,
                enabled: false,
                permissions: vec!["tools.read".into()],
                path: "/tmp/y".into(),
            })
            .unwrap();
        let on = s.set_enabled(&m.id, true).unwrap();
        assert!(on.enabled);
    }
}
