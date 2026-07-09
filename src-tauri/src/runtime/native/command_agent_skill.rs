//! runtime/native/command_agent_skill.rs — Commands/Agents/Skills 系统
//!
//! 精确复刻 Claude Code 的 Commands / Agents / Skills 体系：
//!
//! ## Commands（自定义命令）
//!   文件位置: .claude/commands/<name>.md 或 plugins/<name>/commands/<name>.md
//!   格式: YAML frontmatter + Markdown 指令体
//!
//!   ```yaml
//!   ---
//!   name: my-command
//!   description: Does something useful
//!   allowed-tools: Bash(git checkout --branch:*), Bash(git add:*)
//!   argument-hint: Optional feature description
//!   ---
//!   Prompt body...
//!   ```
//!
//! ## Agents（自定义代理）
//!   文件位置: plugins/<name>/agents/<name>.md
//!   格式: YAML frontmatter + Markdown agent body
//!
//!   ```yaml
//!   ---
//!   name: code-architect
//!   description: Designs feature architectures
//!   tools: Glob, Grep, LS, Read, NotebookRead
//!   model: sonnet
//!   color: green
//!   ---
//!   Agent system prompt...
//!   ```
//!
//! ## Skills（自定义技能）
//!   文件位置: plugins/<name>/skills/<skill-name>/SKILL.md
//!   格式: YAML frontmatter + Markdown skill body
//!
//!   ```yaml
//!   ---
//!   name: frontend-design
//!   description: Guidance for visual design
//!   license: Complete terms in LICENSE.txt
//!   version: 1.0.0
//!   ---
//!   Skill instructions...
//!   ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ══════════════════════════════════════════════════════════════════════════════
// 通用 Frontmatter 解析
// ══════════════════════════════════════════════════════════════════════════════

/// 解析 YAML frontmatter 和 Markdown 消息体
/// 支持：key: value 和 - list items (conditions)
fn parse_frontmatter(content: &str) -> (HashMap<String, String>, String) {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return (HashMap::new(), content.to_string());
    }

    let after_first = trimmed[3..].trim_start();
    let end_marker = after_first.find("\n---");
    let yaml_end = match end_marker {
        Some(pos) => pos,
        None => return (HashMap::new(), content.to_string()),
    };

    let yaml_text = &after_first[..yaml_end];
    let body = after_first[yaml_end + 5..].trim().to_string();

    let mut frontmatter = HashMap::new();
    let mut current_key: Option<String> = None;
    let mut multi_line_value = String::new();
    let mut in_multi_line = false;

    for line in yaml_text.lines() {
        let stripped = line.trim();
        if stripped.is_empty() || stripped.starts_with('#') {
            continue;
        }

        // List items (- value)
        if stripped.starts_with("- ") {
            if let Some(ref key) = current_key {
                let val = stripped[2..].trim().trim_matches('"').to_string();
                if !multi_line_value.is_empty() {
                    multi_line_value.push_str(",");
                }
                multi_line_value.push_str(&val);
            }
            continue;
        }

        // Continuation of multi-line value (indented)
        let indent = line.len() - line.trim_start().len();
        if in_multi_line && indent >= 2 && !stripped.contains(':') {
            continue; // Skip multi-line content (we only collect first-level values)
        }
        in_multi_line = false;

        // key: value pairs
        if let Some(colon) = stripped.find(':') {
            let key = stripped[..colon].trim().to_string();
            let value_str = stripped[colon + 1..].trim().to_string();

            // Save previous multi-line value
            if let Some(ref k) = current_key {
                if !multi_line_value.is_empty() {
                    frontmatter.insert(k.clone(), multi_line_value.clone());
                    multi_line_value.clear();
                }
            }

            // Check if value is empty (multi-line or list follows)
            if value_str.is_empty() || value_str == "[" || value_str == "]" {
                current_key = Some(key);
                multi_line_value = String::new();
                in_multi_line = true;
                // Handle [list items]
                let value_str_trimmed = value_str.trim();
                if value_str_trimmed == "[" || value_str_trimmed == "]" {
                    // JSON array syntax - skip
                }
            } else {
                // Handle JSON array values like ["Read", "Write"]
                if value_str.starts_with('[') {
                    let inner = value_str.trim_start_matches('[').trim_end_matches(']');
                    let val = inner.split(',').map(|s| s.trim().trim_matches('"').to_string())
                        .collect::<Vec<_>>().join(",");
                    frontmatter.insert(key, val);
                } else {
                    let val = value_str.trim_matches('"').to_string();
                    frontmatter.insert(key, val);
                }
                current_key = None;
            }
        }
    }

    // Save last multi-line value
    if let Some(ref k) = current_key {
        if !multi_line_value.is_empty() {
            frontmatter.insert(k.clone(), multi_line_value);
        }
    }

    (frontmatter, body)
}

/// 从 frontmatter 中获取逗号分隔的列表
fn get_list(fm: &HashMap<String, String>, key: &str) -> Vec<String> {
    fm.get(key)
        .map(|v| v.split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
        )
        .unwrap_or_default()
}

// ══════════════════════════════════════════════════════════════════════════════
// Commands
// ══════════════════════════════════════════════════════════════════════════════

/// 命令（从 .md 文件解析）
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Command {
    /// 命令名称
    pub name: String,
    /// 描述
    pub description: String,
    /// 允许的工具列表（格式如 "Bash(git checkout*)", "Bash(git add:*)"）
    pub allowed_tools: Vec<String>,
    /// 参数提示
    pub argument_hint: Option<String>,
    /// 文件路径
    pub file_path: PathBuf,
    /// 完整指令体（Markdown）
    pub body: String,
}

/// 加载所有命令（从 .claude/commands/ 和 plugins/<name>/commands/）
pub fn load_commands(project_root: &Path, plugin_roots: &[PathBuf]) -> Vec<Command> {
    let mut commands = Vec::new();

    // 从项目 .claude/commands/ 加载
    let project_commands = project_root.join(".claude").join("commands");
    if project_commands.exists() {
        commands.extend(load_commands_from_dir(&project_commands));
    }

    // 从插件 commands/ 目录加载
    for plugin_root in plugin_roots {
        let plugin_commands = plugin_root.join("commands");
        if plugin_commands.exists() {
            commands.extend(load_commands_from_dir(&plugin_commands));
        }
    }

    commands
}

fn load_commands_from_dir(dir: &Path) -> Vec<Command> {
    let mut commands = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return commands,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        match load_command(&path) {
            Ok(cmd) => commands.push(cmd),
            Err(e) => eprintln!("[Commands] Failed to load '{}': {e}", path.display()),
        }
    }

    commands
}

fn load_command(path: &Path) -> Result<Command, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Cannot read: {e}"))?;

    let (frontmatter, body) = parse_frontmatter(&content);

    let name = frontmatter.get("name")
        .cloned()
        .or_else(|| path.file_stem().and_then(|s| s.to_str()).map(String::from))
        .unwrap_or_else(|| "unnamed".to_string());

    let description = frontmatter.get("description")
        .cloned()
        .unwrap_or_default();

    let allowed_tools = get_list(&frontmatter, "allowed-tools");
    let argument_hint = frontmatter.get("argument-hint").cloned();

    Ok(Command {
        name,
        description,
        allowed_tools,
        argument_hint,
        file_path: path.to_path_buf(),
        body,
    })
}

// ══════════════════════════════════════════════════════════════════════════════
// Agents
// ══════════════════════════════════════════════════════════════════════════════

/// Agent 定义（从 plugins/<name>/agents/<name>.md 解析）
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    /// Agent 名称
    pub name: String,
    /// 描述
    pub description: String,
    /// 所属插件
    pub plugin_name: String,
    /// 允许的工具列表
    pub tools: Vec<String>,
    /// 模型指定
    pub model: Option<String>,
    /// 显示颜色
    pub color: Option<String>,
    /// 文件路径
    pub file_path: PathBuf,
    /// 完整 Agent body（system prompt）
    pub body: String,
    /// 触发条件（可选）
    pub trigger: Option<String>,
}

/// 加载所有 Agent（从 plugins/<name>/agents/）
pub fn load_agents(plugin_roots: &[PathBuf]) -> Vec<Agent> {
    let mut agents = Vec::new();

    for plugin_root in plugin_roots {
        let agents_dir = plugin_root.join("agents");
        if !agents_dir.exists() {
            continue;
        }
        let plugin_name = plugin_root.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let entries = match std::fs::read_dir(&agents_dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }

            match load_agent(&path, &plugin_name) {
                Ok(agent) => agents.push(agent),
                Err(e) => eprintln!("[Agent] Failed to load '{}': {e}", path.display()),
            }
        }
    }

    agents
}

fn load_agent(path: &Path, plugin_name: &str) -> Result<Agent, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Cannot read: {e}"))?;

    let (frontmatter, body) = parse_frontmatter(&content);

    let name = frontmatter.get("name")
        .cloned()
        .or_else(|| path.file_stem().and_then(|s| s.to_str()).map(String::from))
        .unwrap_or_else(|| "unnamed".to_string());

    let description = frontmatter.get("description").cloned().unwrap_or_default();
    let tools = get_list(&frontmatter, "tools");
    let model = frontmatter.get("model").cloned();
    let color = frontmatter.get("color").cloned();
    let trigger = frontmatter.get("trigger").cloned();

    Ok(Agent {
        name,
        description,
        plugin_name: plugin_name.to_string(),
        tools,
        model,
        color,
        file_path: path.to_path_buf(),
        body,
        trigger,
    })
}

// ══════════════════════════════════════════════════════════════════════════════
// Skills
// ══════════════════════════════════════════════════════════════════════════════

/// Skill 定义（从 plugins/<name>/skills/<skill-name>/SKILL.md 解析）
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    /// Skill 名称
    pub name: String,
    /// 描述
    pub description: String,
    /// 所属插件
    pub plugin_name: String,
    /// 许可证（可选）
    pub license: Option<String>,
    /// 版本
    pub version: Option<String>,
    /// 文件路径
    pub file_path: PathBuf,
    /// 完整 Skill body
    pub body: String,
}

/// 加载所有 Skills（从 plugins/<name>/skills/<skill-name>/SKILL.md）
pub fn load_skills(plugin_roots: &[PathBuf]) -> Vec<Skill> {
    let mut skills = Vec::new();

    for plugin_root in plugin_roots {
        let skills_dir = plugin_root.join("skills");
        if !skills_dir.exists() {
            continue;
        }
        let plugin_name = plugin_root.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let entries = match std::fs::read_dir(&skills_dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let skill_root = entry.path();
            if !skill_root.is_dir() {
                continue;
            }

            let skill_md = skill_root.join("SKILL.md");
            if !skill_md.exists() {
                continue;
            }

            match load_skill(&skill_md, &plugin_name) {
                Ok(skill) => skills.push(skill),
                Err(e) => eprintln!("[Skill] Failed to load '{}': {e}", skill_md.display()),
            }
        }
    }

    skills
}

fn load_skill(path: &Path, plugin_name: &str) -> Result<Skill, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Cannot read: {e}"))?;

    let (frontmatter, body) = parse_frontmatter(&content);

    let name = frontmatter.get("name")
        .cloned()
        .or_else(|| {
            path.parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .map(String::from)
        })
        .unwrap_or_else(|| "unnamed".to_string());

    let description = frontmatter.get("description").cloned().unwrap_or_default();
    let license = frontmatter.get("license").cloned();
    let version = frontmatter.get("version").cloned();

    Ok(Skill {
        name,
        description,
        plugin_name: plugin_name.to_string(),
        license,
        version,
        file_path: path.to_path_buf(),
        body,
    })
}

// ══════════════════════════════════════════════════════════════════════════════
// Command/Agent/Skill 管理器
// ══════════════════════════════════════════════════════════════════════════════

/// 命令/代理/技能管理器
pub struct CasManager {
    commands: Vec<Command>,
    agents: Vec<Agent>,
    skills: Vec<Skill>,
    project_root: PathBuf,
    plugin_roots: Vec<PathBuf>,
}

impl CasManager {
    pub fn new(project_root: PathBuf, plugin_roots: Vec<PathBuf>) -> Self {
        Self {
            commands: Vec::new(),
            agents: Vec::new(),
            skills: Vec::new(),
            project_root,
            plugin_roots,
        }
    }

    /// 加载所有 Commands/Agents/Skills
    pub fn load_all(&mut self) {
        self.commands = load_commands(&self.project_root, &self.plugin_roots);
        self.agents = load_agents(&self.plugin_roots);
        self.skills = load_skills(&self.plugin_roots);

        println!(
            "[CASManager] Loaded {} commands, {} agents, {} skills",
            self.commands.len(),
            self.agents.len(),
            self.skills.len(),
        );
    }

    /// 获取所有命令
    pub fn commands(&self) -> &[Command] { &self.commands }

    /// 获取所有 agent
    pub fn agents(&self) -> &[Agent] { &self.agents }

    /// 获取所有 skill
    pub fn skills(&self) -> &[Skill] { &self.skills }

    /// 按名称查找命令
    pub fn find_command(&self, name: &str) -> Option<&Command> {
        self.commands.iter().find(|c| c.name == name)
    }

    /// 按名称查找 agent
    pub fn find_agent(&self, name: &str) -> Option<&Agent> {
        self.agents.iter().find(|a| a.name == name)
    }

    /// 按名称查找 skill
    pub fn find_skill(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|s| s.name == name)
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// 测试
// ══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_frontmatter_simple() {
        let content = r#"---
name: test-command
description: A test
---

Execute this command.
"#;
        let (fm, body) = parse_frontmatter(content);
        assert_eq!(fm.get("name").map(|s| s.as_str()), Some("test-command"));
        assert_eq!(fm.get("description").map(|s| s.as_str()), Some("A test"));
        assert!(body.contains("Execute"));
    }

    #[test]
    fn test_parse_frontmatter_no_frontmatter() {
        let content = "Just plain text";
        let (fm, body) = parse_frontmatter(content);
        assert!(fm.is_empty());
        assert_eq!(body, content);
    }

    #[test]
    fn test_parse_frontmatter_allowed_tools() {
        let content = r#"---
name: test
allowed-tools: Bash(git checkout --branch:*), Bash(git add:*), Bash(git commit:*)
description: Test command
---

Body.
"#;
        let (fm, _) = parse_frontmatter(content);
        assert!(fm.contains_key("allowed-tools"));
        let tools: Vec<&str> = fm.get("allowed-tools").unwrap().split(',').collect();
        assert!(tools.len() >= 3);
    }

    #[test]
    fn test_parse_frontmatter_argument_hint() {
        let content = r#"---
name: test
description: A test
argument-hint: Optional feature description
---

Body.
"#;
        let (fm, _) = parse_frontmatter(content);
        assert_eq!(fm.get("argument-hint").map(|s| s.as_str()), Some("Optional feature description"));
    }

    #[test]
    fn test_load_command_all_fields() {
        let dir = std::env::temp_dir().join("natives_test_cmd_fields");
        let _ = std::fs::create_dir_all(&dir);
        let cmd_path = dir.join("test.md");
        std::fs::write(&cmd_path, r#"---
name: mycmd
description: My command
allowed-tools: Bash(git commit:*), Bash(git push:*)
argument-hint: Enter message
---

Run this command.
"#).unwrap();

        let cmd = load_command(&cmd_path).unwrap();
        assert_eq!(cmd.name, "mycmd");
        assert_eq!(cmd.description, "My command");
        assert_eq!(cmd.argument_hint.as_deref(), Some("Enter message"));
        assert!(!cmd.allowed_tools.is_empty());
    }

    #[test]
    fn test_load_agent_all_fields() {
        let dir = std::env::temp_dir().join("natives_test_agent_fields");
        let agent_dir = dir.join("agents");
        let _ = std::fs::create_dir_all(&agent_dir);
        std::fs::write(agent_dir.join("test-agent.md"), r#"---
name: test-agent
description: Test agent
tools: Glob, Grep, LS, Read
model: sonnet
color: green
trigger: on_error
---

You are a test agent.
"#).unwrap();

        let agents = load_agents(&[dir]);
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].model.as_deref(), Some("sonnet"));
        assert_eq!(agents[0].color.as_deref(), Some("green"));
        assert_eq!(agents[0].trigger.as_deref(), Some("on_error"));
        assert_eq!(agents[0].tools.len(), 4);
    }

    #[test]
    fn test_load_skill_all_fields() {
        let dir = std::env::temp_dir().join("natives_test_skill_fields");
        let skill_dir = dir.join("skills").join("test-skill");
        let _ = std::fs::create_dir_all(&skill_dir);
        std::fs::write(skill_dir.join("SKILL.md"), r#"---
name: test-skill
description: Test skill
license: MIT
version: 1.0.0
---

Skill instructions.
"#).unwrap();

        let skills = load_skills(&[dir]);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "test-skill");
        assert_eq!(skills[0].license.as_deref(), Some("MIT"));
        assert_eq!(skills[0].version.as_deref(), Some("1.0.0"));
    }

    #[test]
    fn test_load_commands_no_dir() {
        let dir = std::env::temp_dir().join("natives_test_nonexistent");
        let cmds = load_commands_from_dir(&dir);
        assert!(cmds.is_empty());
    }

    #[test]
    fn test_cas_manager_new() {
        let manager = CasManager::new(PathBuf::from("/tmp"), vec![]);
        assert!(manager.commands().is_empty());
        assert!(manager.agents().is_empty());
        assert!(manager.skills().is_empty());
    }

    #[test]
    fn test_find_nonexistent() {
        let manager = CasManager::new(PathBuf::from("/tmp"), vec![]);
        assert!(manager.find_command("nonexistent").is_none());
        assert!(manager.find_agent("nonexistent").is_none());
        assert!(manager.find_skill("nonexistent").is_none());
    }

    #[test]
    fn test_list_tools_parsing() {
        let mut fm = HashMap::new();
        fm.insert("tools".to_string(), "Glob, Grep, LS, Read".to_string());
        let tools = get_list(&fm, "tools");
        assert_eq!(tools.len(), 4);
        assert_eq!(tools[0], "Glob");
        assert_eq!(tools[2], "LS");
    }

    #[test]
    fn test_agent_tools_list() {
        let mut fm = HashMap::new();
        fm.insert("tools".to_string(), "Glob,Grep,LS,Read,NotebookRead".to_string());
        let tools = get_list(&fm, "tools");
        assert_eq!(tools.len(), 5);
        assert!(tools.contains(&"Glob".to_string()));
        assert!(tools.contains(&"NotebookRead".to_string()));
    }
}
