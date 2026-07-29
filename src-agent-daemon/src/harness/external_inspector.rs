//! External CLI Runtime Configuration Inspector.
//!
//! Real inspection of local `.claude` (settings, hooks, plugins) and Codex
//! `config.toml` files, mapping discovered hooks to Native Harness events and
//! flagging unmapped external features as `unsupported`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalRuntimeInfo {
    pub id: String,
    pub name: String,
    pub read_only: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredExternalSource {
    pub runtime: String,
    pub source: String,
    pub absolute_path: String,
    pub status: String,
    pub supported_events_mapped: Vec<String>,
    pub unsupported_events: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalInspectionReport {
    pub external_runtimes: Vec<ExternalRuntimeInfo>,
    pub discovered: Vec<DiscoveredExternalSource>,
}

/// Inspect external CLI configuration files under `project_path`.
pub fn inspect_external_runtimes(project_path: Option<&Path>) -> ExternalInspectionReport {
    let mut runtimes = vec![
        ExternalRuntimeInfo {
            id: "claude".into(),
            name: "Claude CLI".into(),
            read_only: true,
            config_path: None,
        },
        ExternalRuntimeInfo {
            id: "codex".into(),
            name: "Codex CLI".into(),
            read_only: true,
            config_path: None,
        },
    ];

    let mut discovered = Vec::new();

    if let Some(path) = project_path {
        // 1. Inspect Claude CLI (.claude/settings.json, .claude/hooks.json)
        if let Some(src) = inspect_claude_config(path) {
            runtimes[0].config_path = Some(src.absolute_path.clone());
            discovered.push(src);
        }

        // 2. Inspect Codex CLI (.codex/config.toml, codex.toml)
        if let Some(src) = inspect_codex_config(path) {
            runtimes[1].config_path = Some(src.absolute_path.clone());
            discovered.push(src);
        }
    }

    ExternalInspectionReport {
        external_runtimes: runtimes,
        discovered,
    }
}

fn inspect_claude_config(project_path: &Path) -> Option<DiscoveredExternalSource> {
    let claude_dir = project_path.join(".claude");
    let settings_file = claude_dir.join("settings.json");
    let hooks_file = claude_dir.join("hooks.json");

    if !settings_file.exists() && !hooks_file.exists() && !claude_dir.exists() {
        return None;
    }

    let target_file = if hooks_file.exists() {
        hooks_file
    } else if settings_file.exists() {
        settings_file
    } else {
        claude_dir
    };

    let abs_path = target_file.to_string_lossy().to_string();
    let rel_source = target_file
        .strip_prefix(project_path)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| ".claude".into());

    let mut mapped = Vec::new();
    let mut unsupported = Vec::new();
    let mut status = "active".to_string();

    if target_file.is_file() {
        if let Ok(content) = fs::read_to_string(&target_file) {
            if let Ok(val) = serde_json::from_str::<Value>(&content) {
                parse_claude_hooks(&val, &mut mapped, &mut unsupported);
            } else {
                status = "parse_error".to_string();
            }
        }
    }

    Some(DiscoveredExternalSource {
        runtime: "claude".into(),
        source: rel_source,
        absolute_path: abs_path,
        status,
        supported_events_mapped: mapped,
        unsupported_events: unsupported,
        details: None,
    })
}

fn parse_claude_hooks(val: &Value, mapped: &mut Vec<String>, unsupported: &mut Vec<String>) {
    let hooks_obj = val.get("hooks").or_else(|| val.get("events"));
    if let Some(obj) = hooks_obj.and_then(|h| h.as_object()) {
        for key in obj.keys() {
            match key.as_str() {
                "PreToolUse" | "pre_tool_use" | "BeforeTool" => {
                    if !mapped.contains(&"PreToolUse".to_string()) {
                        mapped.push("PreToolUse".to_string());
                    }
                }
                "PostToolUse" | "post_tool_use" | "AfterTool" => {
                    if !mapped.contains(&"PostToolUse".to_string()) {
                        mapped.push("PostToolUse".to_string());
                    }
                }
                "SessionStart" | "session_start" | "OnStart" => {
                    if !mapped.contains(&"SessionStart".to_string()) {
                        mapped.push("SessionStart".to_string());
                    }
                }
                "UserPromptSubmit" | "prompt_submit" => {
                    if !mapped.contains(&"UserPromptSubmit".to_string()) {
                        mapped.push("UserPromptSubmit".to_string());
                    }
                }
                "Stop" | "session_end" => {
                    if !mapped.contains(&"Stop".to_string()) {
                        mapped.push("Stop".to_string());
                    }
                }
                other => {
                    if !unsupported.contains(&other.to_string()) {
                        unsupported.push(other.to_string());
                    }
                }
            }
        }
    }
}

fn inspect_codex_config(project_path: &Path) -> Option<DiscoveredExternalSource> {
    let codex_dir = project_path.join(".codex");
    let config_file = codex_dir.join("config.toml");
    let root_codex_file = project_path.join("codex.toml");

    let target_file = if config_file.exists() {
        config_file
    } else if root_codex_file.exists() {
        root_codex_file
    } else if codex_dir.exists() {
        codex_dir
    } else {
        return None;
    };

    let abs_path = target_file.to_string_lossy().to_string();
    let rel_source = target_file
        .strip_prefix(project_path)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| ".codex/config.toml".into());

    let mut mapped = Vec::new();
    let mut unsupported = Vec::new();
    let mut status = "active".to_string();

    if target_file.is_file() {
        if let Ok(content) = fs::read_to_string(&target_file) {
            parse_codex_toml_lines(&content, &mut mapped, &mut unsupported);
        } else {
            status = "parse_error".to_string();
        }
    }

    Some(DiscoveredExternalSource {
        runtime: "codex".into(),
        source: rel_source,
        absolute_path: abs_path,
        status,
        supported_events_mapped: mapped,
        unsupported_events: unsupported,
        details: None,
    })
}

fn parse_codex_toml_lines(content: &str, mapped: &mut Vec<String>, unsupported: &mut Vec<String>) {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }

        if trimmed.contains("pre_tool") || trimmed.contains("PreToolUse") {
            if !mapped.contains(&"PreToolUse".to_string()) {
                mapped.push("PreToolUse".to_string());
            }
        } else if trimmed.contains("post_tool") || trimmed.contains("PostToolUse") {
            if !mapped.contains(&"PostToolUse".to_string()) {
                mapped.push("PostToolUse".to_string());
            }
        } else if trimmed.contains("session_start") {
            if !mapped.contains(&"SessionStart".to_string()) {
                mapped.push("SessionStart".to_string());
            }
        } else if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let section = &trimmed[1..trimmed.len() - 1];
            if section != "hooks" && section != "config" {
                let sec_name = section.to_string();
                if !unsupported.contains(&sec_name) {
                    unsupported.push(sec_name);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_external_directories_do_not_fabricate_hooks() {
        let root =
            std::env::temp_dir().join(format!("external-inspector-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join(".claude")).unwrap();
        fs::create_dir_all(root.join(".codex")).unwrap();

        let report = inspect_external_runtimes(Some(&root));
        assert_eq!(report.discovered.len(), 2);
        assert!(report
            .discovered
            .iter()
            .all(|source| source.supported_events_mapped.is_empty()
                && source.unsupported_events.is_empty()));

        fs::remove_dir_all(root).unwrap();
    }
}
