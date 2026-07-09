//! runtime/native/plugin_system.rs — 插件系统（参考 Claude Code 插件架构）
//!
//! 实现对 Claude Code 插件系统的兼容：
//!   - 从 plugins/ 目录发现插件（每个插件一个目录）
//!   - 加载 plugin.json 元数据
//!   - 注册 hooks / commands / agents / skills
//!   - 加载 .local.md 规则
//!
//! 插件目录结构（兼容 Claude Code）：
//!   plugins/<plugin-name>/
//!     .claude-plugin/
//!       plugin.json          # 元数据
//!     hooks/
//!       hooks.json            # hook 注册表
//!       pretooluse.py         # PreToolUse handler
//!       posttooluse.py        # PostToolUse handler
//!       stop.py               # Stop handler
//!       userpromptsubmit.py   # UserPromptSubmit handler
//!     commands/               # 自定义命令
//!       *.md
//!     agents/                 # 自定义 agent
//!       *.md
//!     skills/                 # 自定义技能
//!       <skill-name>/
//!         SKILL.md
//!

use crate::runtime::native::hook_pipeline::{
    HookGroup, HookPipeline, HookPoint,
};
use crate::runtime::native::rule_engine::RuleEngine;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ──────────────────────────────────────────────
// Plugin 元数据
// ──────────────────────────────────────────────

/// 插件元数据（对应 plugin.json）
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginMeta {
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: Option<PluginAuthor>,
    #[serde(default)]
    pub category: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginAuthor {
    pub name: String,
    #[serde(default)]
    pub email: String,
}

/// Hook 注册表（对应 hooks.json）
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HooksRegistry {
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub hooks: HashMap<String, Vec<serde_json::Value>>, // 保持 raw JSON 以支持灵活解析
}

// ──────────────────────────────────────────────
// 加载的插件（运行时）
// ──────────────────────────────────────────────

/// 运行时插件实例
pub struct Plugin {
    pub meta: PluginMeta,
    pub root: PathBuf,
    pub hooks: HooksRegistry,
}

impl Plugin {
    pub fn name(&self) -> &str { &self.meta.name }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpManifest {
    pub plugin_name: String,
    pub path: PathBuf,
    pub enabled: bool,
    pub config: serde_json::Value,
}

// ──────────────────────────────────────────────
// 插件管理器
// ──────────────────────────────────────────────

/// 插件管理器 — 负责发现、加载、注册插件
pub struct PluginManager {
    plugins: Vec<Plugin>,
    /// 插件搜索路径
    search_paths: Vec<PathBuf>,
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
            search_paths: Vec::new(),
        }
    }

    /// 添加插件搜索路径
    pub fn add_search_path(&mut self, path: PathBuf) {
        if !self.search_paths.contains(&path) {
            self.search_paths.push(path);
        }
    }

    /// 发现并加载所有插件
    pub fn discover_all(&mut self) {
        let paths: Vec<PathBuf> = self.search_paths.clone();
        for path in &paths {
            self.discover_in(path);
        }
    }

    /// 在单个目录中发现插件
    fn discover_in(&mut self, dir: &Path) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            let plugin_root = entry.path();
            if !plugin_root.is_dir() {
                continue;
            }

            let plugin_json = plugin_root.join(".claude-plugin").join("plugin.json");
            if !plugin_json.exists() {
                continue;
            }

            match self.load_plugin(&plugin_root) {
                Ok(plugin) => {
                    println!("[PluginManager] Loaded plugin: {} v{}", plugin.meta.name, plugin.meta.version);
                    self.plugins.push(plugin);
                }
                Err(e) => {
                    eprintln!("[PluginManager] Failed to load plugin '{}': {e}", plugin_root.display());
                }
            }
        }
    }

    /// 加载单个插件
    fn load_plugin(&self, root: &Path) -> Result<Plugin, String> {
        let meta_path = root.join(".claude-plugin").join("plugin.json");
        let meta_content = std::fs::read_to_string(&meta_path)
            .map_err(|e| format!("Cannot read plugin.json: {e}"))?;
        let meta: PluginMeta = serde_json::from_str(&meta_content)
            .map_err(|e| format!("Invalid plugin.json: {e}"))?;

        // 加载 hooks.json（可选）
        let hooks_registry = self.load_hooks(root);

        Ok(Plugin {
            meta,
            root: root.to_path_buf(),
            hooks: hooks_registry,
        })
    }

    /// 加载插件的 hooks.json
    fn load_hooks(&self, root: &Path) -> HooksRegistry {
        let hooks_json = root.join("hooks").join("hooks.json");
        if !hooks_json.exists() {
            return HooksRegistry {
                description: String::new(),
                hooks: HashMap::new(),
            };
        }

        std::fs::read_to_string(&hooks_json)
            .ok()
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or(HooksRegistry {
                description: String::new(),
                hooks: HashMap::new(),
            })
    }

    /// 将插件的 hooks 注册到 HookPipeline
    pub fn register_hooks(&self, pipeline: &mut HookPipeline) {
        for plugin in &self.plugins {
            for (hook_point_str, groups_raw) in &plugin.hooks.hooks {
                let hook_point = match HookPoint::from_str(hook_point_str) {
                    Some(p) => p,
                    None => continue,
                };

                for group_value in groups_raw {
                    if let Some(group) = HookGroup::from_json(group_value) {
                        for entry in &group.hooks {
                            pipeline.register_script_hook(hook_point.clone(), entry.clone());
                        }
                        println!("[PluginManager] Registered {} hooks '{}' → {:?}",
                            group.hooks.len(), plugin.name(), hook_point);
                    }
                }
            }
        }
    }

    /// 将插件的 .local.md 规则加载到 RuleEngine
    pub fn load_plugin_rules(&self, rule_engine: &mut RuleEngine) {
        for plugin in &self.plugins {
            // 检查插件目录下的 rules/ 或 .claude/ 目录
            let rules_dir = plugin.root.join("rules");
            if rules_dir.exists() && rules_dir.is_dir() {
                if let Ok(entries) = std::fs::read_dir(&rules_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.extension().and_then(|e| e.to_str()) == Some("md") {
                            match crate::runtime::native::rule_engine::load_rule_file(&path) {
                                Ok(rule) => {
                                    println!("[PluginManager] Loaded rule '{}' from plugin '{}'",
                                        rule.name, plugin.name());
                                    rule_engine.add_rule(rule);
                                }
                                Err(e) => {
                                    eprintln!("[PluginManager] Failed to load rule '{}': {e}", path.display());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// 获取所有已加载的插件元信息
    pub fn list_plugins(&self) -> Vec<PluginMeta> {
        self.plugins.iter().map(|p| p.meta.clone()).collect()
    }

    pub fn hooks_summary(&self) -> Vec<(String, String, usize)> {
        let mut summary = Vec::new();
        for plugin in &self.plugins {
            for (hook_point, groups) in &plugin.hooks.hooks {
                let count = groups
                    .iter()
                    .filter_map(HookGroup::from_json)
                    .map(|group| group.hooks.len())
                    .sum();
                summary.push((plugin.name().to_string(), hook_point.clone(), count));
            }
        }
        summary
    }

    pub fn plugin_roots(&self) -> Vec<PathBuf> {
        self.plugins.iter().map(|p| p.root.clone()).collect()
    }

    pub fn list_mcp_manifests(&self) -> Vec<McpManifest> {
        self.plugins
            .iter()
            .filter_map(|plugin| {
                let path = plugin.root.join(".mcp.json");
                if !path.exists() {
                    return None;
                }
                let config = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|content| serde_json::from_str(&content).ok())
                    .unwrap_or_else(|| serde_json::json!({}));
                Some(McpManifest {
                    plugin_name: plugin.name().to_string(),
                    path,
                    enabled: false,
                    config,
                })
            })
            .collect()
    }

    /// 按名称查找插件
    pub fn find_plugin(&self, name: &str) -> Option<&Plugin> {
        self.plugins.iter().find(|p| p.meta.name == name)
    }

    /// 获取已加载插件数量
    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }
}

// ──────────────────────────────────────────────
// 测试
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_meta_deserialize_minimal() {
        let json = r#"{"name": "test-plugin"}"#;
        let meta: PluginMeta = serde_json::from_str(json).unwrap();
        assert_eq!(meta.name, "test-plugin");
        assert_eq!(meta.version, "");
        assert_eq!(meta.description, "");
    }

    #[test]
    fn test_plugin_meta_deserialize_full() {
        let json = r#"{
            "name": "code-review",
            "version": "1.0.0",
            "description": "Review code",
            "category": "development"
        }"#;
        let meta: PluginMeta = serde_json::from_str(json).unwrap();
        assert_eq!(meta.name, "code-review");
        assert_eq!(meta.version, "1.0.0");
    }

    #[test]
    fn test_hooks_registry_deserialize() {
        let json = r#"{
            "description": "Test hooks",
            "hooks": {
                "PreToolUse": [{
                    "hooks": [{
                        "type": "command",
                        "command": "python3 test.py",
                        "timeout": 10
                    }]
                }]
            }
        }"#;
        let reg: HooksRegistry = serde_json::from_str(json).unwrap();
        assert_eq!(reg.description, "Test hooks");
        assert!(reg.hooks.contains_key("PreToolUse"));
        let groups = reg.hooks.get("PreToolUse").unwrap();
        // groups is Vec<serde_json::Value>, parse first element as HookGroup
        if let Some(group) = groups.first() {
            if let Some(hook_group) = crate::runtime::native::hook_pipeline::HookGroup::from_json(group) {
                assert!(!hook_group.hooks.is_empty());
                if let Some(first_hook) = hook_group.hooks.first() {
                    assert_eq!(first_hook.command, "python3 test.py");
                }
            }
        }
    }

    #[test]
    fn test_discover_in_nonexistent() {
        let mut pm = PluginManager::new();
        pm.discover_in(Path::new("/nonexistent/path"));
        assert_eq!(pm.plugin_count(), 0);
    }

    #[test]
    fn test_add_search_path_dedup() {
        let mut pm = PluginManager::new();
        pm.add_search_path(PathBuf::from("/a"));
        pm.add_search_path(PathBuf::from("/a"));
        assert_eq!(pm.search_paths.len(), 1);
    }
}
