//! runtime/native/rule_engine.rs — 规则引擎（完整 .local.md 支持）
//!
//! 参考 Claude Code hookify 插件的规则体系，实现：
//!   - .local.md 文件解析（YAML frontmatter + Markdown 消息体）
//!   - 条件运算符：regex_match / contains / not_contains / equals / path_match
//!   - 规则动作：allow / warn / block
//!   - 规则优先级排序
//!   - 从 .claude/ 目录加载用户自定义规则
//!
//! 文件格式（对标 Claude Code .local.md）：
//!   ---
//!   name: rule-name
//!   enabled: true
//!   event: bash    # bash / file / stop / prompt / all
//!   action: warn   # warn / block
//!   conditions:
//!     - field: command
//!       operator: regex_match
//!       pattern: "rm -rf"
//!   ---
//!
//!   ⚠️ Warning message body...
//!

use serde::Deserialize;
use std::path::Path;
use super::capability::CapabilityRequest;

// ──────────────────────────────────────────────
// 条件与规则定义
// ──────────────────────────────────────────────

/// 条件运算符（对标 Claude Code hookify 的 operator 字段）
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub enum ConditionOperator {
    /// 正则匹配
    #[serde(rename = "regex_match")]
    RegexMatch,
    /// 包含子串
    #[serde(rename = "contains")]
    Contains,
    /// 不包含子串
    #[serde(rename = "not_contains")]
    NotContains,
    /// 完全匹配
    #[serde(rename = "equals")]
    Equals,
    /// 路径匹配（支持 glob）
    #[serde(rename = "path_match")]
    PathMatch,
}

impl Default for ConditionOperator {
    fn default() -> Self {
        ConditionOperator::RegexMatch
    }
}

/// 规则匹配条件
#[derive(Clone, Debug, Deserialize)]
pub struct Condition {
    /// 要匹配的字段（如 "command", "new_text", "file_path", "old_text", "content"）
    pub field: String,
    /// 匹配运算符（默认 regex_match）
    #[serde(default)]
    pub operator: ConditionOperator,
    /// 匹配模式/正则
    pub pattern: String,
}

/// 规则触发时执行的动作
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub enum RuleAction {
    /// 允许执行
    #[serde(rename = "allow")]
    Allow,
    /// 警告（记录日志 + 发提示，不阻止）
    #[serde(rename = "warn")]
    Warn,
    /// 阻止执行
    #[serde(rename = "block")]
    Block,
}

impl Default for RuleAction {
    fn default() -> Self {
        RuleAction::Allow
    }
}

/// 规则事件类型
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub enum RuleEvent {
    #[serde(rename = "bash")]
    Bash,
    #[serde(rename = "file")]
    File,
    #[serde(rename = "stop")]
    Stop,
    #[serde(rename = "prompt")]
    Prompt,
    #[serde(rename = "all")]
    All,
}

impl Default for RuleEvent {
    fn default() -> Self {
        RuleEvent::All
    }
}

/// 一条从 .local.md 解析得到的策略规则
#[derive(Clone, Debug, Deserialize)]
pub struct Rule {
    /// 规则名称
    pub name: String,
    /// 是否启用
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 匹配事件类型
    #[serde(default)]
    pub event: RuleEvent,
    /// 规则动作
    #[serde(default)]
    pub action: RuleAction,
    /// 条件列表
    #[serde(default)]
    pub conditions: Vec<Condition>,
    /// Hookify 简写：pattern 会按 event 展开成默认字段条件。
    #[serde(default)]
    pub pattern: Option<String>,
    /// 工具匹配模式（可选，覆盖自动推断）
    pub tool_matcher: Option<String>,
    /// 消息体（Markdown 格式的警告/提示文本）
    #[serde(default)]
    pub message: String,
    /// 优先级（高值先评估）
    #[serde(default)]
    pub priority: i32,
}

fn default_true() -> bool { true }

impl Rule {
    /// 从解析后的 frontmatter 和消息体创建 Rule
    pub fn from_parts(frontmatter: &serde_json::Value, message: &str) -> Result<Self, String> {
        #[derive(Deserialize)]
        struct RuleFrontmatter {
            name: String,
            #[serde(default = "default_true")]
            enabled: bool,
            #[serde(default)]
            event: RuleEvent,
            #[serde(default)]
            action: RuleAction,
            #[serde(default)]
            conditions: Vec<Condition>,
            #[serde(default)]
            pattern: Option<String>,
            tool_matcher: Option<String>,
            #[serde(default)]
            priority: i32,
        }

        let mut fm: RuleFrontmatter = serde_json::from_value(frontmatter.clone())
            .map_err(|e| format!("Invalid rule frontmatter: {e}"))?;
        if fm.conditions.is_empty() {
            if let Some(pattern) = fm.pattern.clone() {
                fm.conditions.push(Condition {
                    field: default_pattern_field(&fm.event).to_string(),
                    operator: ConditionOperator::RegexMatch,
                    pattern,
                });
            }
        }

        Ok(Rule {
            name: fm.name,
            enabled: fm.enabled,
            event: fm.event,
            action: fm.action,
            conditions: fm.conditions,
            pattern: fm.pattern,
            tool_matcher: fm.tool_matcher,
            message: message.to_string(),
            priority: fm.priority,
        })
    }
}

// ──────────────────────────────────────────────
// .local.md 解析器（YAML frontmatter）
// ──────────────────────────────────────────────

/// 解析 .local.md 格式文件
///
/// 格式：
///   ---
///   yaml frontmatter
///   ---
///   Markdown 消息体
pub fn parse_local_md(content: &str) -> (serde_json::Value, String) {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return (serde_json::json!({}), content.to_string());
    }

    let after_first = trimmed[3..].trim_start();
    let end_marker = after_first.find("\n---");
    let yaml_end = match end_marker {
        Some(pos) => pos,
        None => return (serde_json::json!({}), content.to_string()),
    };

    let yaml_text = &after_first[..yaml_end];
    let body = after_first[yaml_end + 5..].trim().to_string();

    let parsed = serde_yaml::from_str::<serde_yaml::Value>(yaml_text)
        .ok()
        .and_then(|value| serde_json::to_value(value).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    (parsed, body)
}

fn default_pattern_field(event: &RuleEvent) -> &'static str {
    match event {
        RuleEvent::Bash => "command",
        RuleEvent::File => "file_path",
        RuleEvent::Prompt => "content",
        RuleEvent::Stop | RuleEvent::All => "arguments",
    }
}

/// 从文件路径加载并解析 .local.md 规则
pub fn load_rule_file(path: &Path) -> Result<Rule, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read rule file '{}': {}", path.display(), e))?;

    let (frontmatter, message) = parse_local_md(&content);

    if frontmatter.is_null() || !frontmatter.as_object().map_or(false, |o| !o.is_empty()) {
        return Err(format!("Missing or empty YAML frontmatter in '{}'", path.display()));
    }

    Rule::from_parts(&frontmatter, &message)
}

/// 从 .claude 目录加载所有规则文件
///
/// 扫描 .claude/hookify.*.local.md 或 .claude/*.local.md 文件
pub fn load_rules_from_dir(claude_dir: &Path) -> Vec<Rule> {
    let mut rules = Vec::new();

    let dir = match std::fs::read_dir(claude_dir) {
        Ok(d) => d,
        Err(_) => return rules,
    };

    for entry in dir.flatten() {
        let path = entry.path();
        let file_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };

        // 匹配 hookify.*.local.md 或 *.local.md
        if !file_name.ends_with(".local.md") {
            continue;
        }

        match load_rule_file(&path) {
            Ok(rule) => {
                if rule.enabled {
                    rules.push(rule);
                }
            }
            Err(e) => {
                eprintln!("[RuleEngine] Warning: Failed to load rule '{}': {e}", path.display());
            }
        }
    }

    // 按优先级降序排序
    rules.sort_by(|a, b| b.priority.cmp(&a.priority));
    rules
}

// ──────────────────────────────────────────────
// 规则引擎
// ──────────────────────────────────────────────

/// 规则引擎 — 管理规则并评估工具请求
pub struct RuleEngine {
    rules: Vec<Rule>,
}

impl RuleEngine {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// 注册一条规则
    pub fn add_rule(&mut self, rule: Rule) {
        self.rules.push(rule);
        self.rules.sort_by(|a, b| b.priority.cmp(&a.priority));
    }

    /// 从 .claude 目录加载规则
    pub fn load_rules(&mut self, claude_dir: &Path) {
        let loaded = load_rules_from_dir(claude_dir);
        self.rules.extend(loaded);
        self.rules.sort_by(|a, b| b.priority.cmp(&a.priority));
    }

    /// 获取所有规则
    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// 评估规则引擎——对请求匹配所有规则（精确匹配 Claude Code rule_engine.py 逻辑）
    ///
    /// 实现逻辑（与 Python 版完全一致）：
    ///   1. 遍历所有规则，收集匹配的 blocking 和 warning 规则
    ///   2. blocking 规则优先于 warning 规则
    ///   3. 多个匹配规则的消息合并在一起
    ///   4. 检查 tool_matcher + conditions
    pub fn evaluate(&self, request: &CapabilityRequest) -> RuleEvaluation {
        let mut blocking_rules: Vec<&Rule> = Vec::new();
        let mut warning_rules: Vec<&Rule> = Vec::new();

        for rule in &self.rules {
            if self.matches_rule(rule, request) {
                match rule.action {
                    RuleAction::Block => blocking_rules.push(rule),
                    RuleAction::Warn => warning_rules.push(rule),
                    RuleAction::Allow => {} // 不允许明确标记为 allow 的规则阻止
                }
            }
        }

        // blocking 规则优先
        if !blocking_rules.is_empty() {
            let messages: Vec<String> = blocking_rules.iter().map(|r| {
                format!("**[{}]**\n{}", r.name, r.message)
            }).collect();
            return RuleEvaluation {
                action: RuleAction::Block,
                matched_rule: Some(blocking_rules[0].name.clone()),
                message: messages.join("\n\n"),
                all_matched_rules: blocking_rules.iter().map(|r| r.name.clone()).collect(),
            };
        }

        // warning 规则
        if !warning_rules.is_empty() {
            let messages: Vec<String> = warning_rules.iter().map(|r| {
                format!("**[{}]**\n{}", r.name, r.message)
            }).collect();
            return RuleEvaluation {
                action: RuleAction::Warn,
                matched_rule: Some(warning_rules[0].name.clone()),
                message: messages.join("\n\n"),
                all_matched_rules: warning_rules.iter().map(|r| r.name.clone()).collect(),
            };
        }

        RuleEvaluation {
            action: RuleAction::Allow,
            matched_rule: None,
            message: String::new(),
            all_matched_rules: vec![],
        }
    }

    /// 评估规则引擎（事件过滤版本）
    pub fn evaluate_for_event(&self, request: &CapabilityRequest, event: &RuleEvent) -> RuleEvaluation {
        let mut blocking_rules: Vec<&Rule> = Vec::new();
        let mut warning_rules: Vec<&Rule> = Vec::new();

        for rule in &self.rules {
            // 事件类型过滤
            let event_match = match &rule.event {
                RuleEvent::All => true,
                e => e == event,
            };
            if !event_match {
                continue;
            }

            if self.matches_rule(rule, request) {
                match rule.action {
                    RuleAction::Block => blocking_rules.push(rule),
                    RuleAction::Warn => warning_rules.push(rule),
                    RuleAction::Allow => {}
                }
            }
        }

        if !blocking_rules.is_empty() {
            let messages: Vec<String> = blocking_rules.iter().map(|r| {
                format!("**[{}]**\n{}", r.name, r.message)
            }).collect();
            return RuleEvaluation {
                action: RuleAction::Block,
                matched_rule: Some(blocking_rules[0].name.clone()),
                message: messages.join("\n\n"),
                all_matched_rules: blocking_rules.iter().map(|r| r.name.clone()).collect(),
            };
        }

        if !warning_rules.is_empty() {
            let messages: Vec<String> = warning_rules.iter().map(|r| {
                format!("**[{}]**\n{}", r.name, r.message)
            }).collect();
            return RuleEvaluation {
                action: RuleAction::Warn,
                matched_rule: Some(warning_rules[0].name.clone()),
                message: messages.join("\n\n"),
                all_matched_rules: warning_rules.iter().map(|r| r.name.clone()).collect(),
            };
        }

        RuleEvaluation {
            action: RuleAction::Allow,
            matched_rule: None,
            message: String::new(),
            all_matched_rules: vec![],
        }
    }

    /// 检查规则是否匹配（检查 tool_matcher + conditions，精确匹配 Python _rule_matches）
    fn matches_rule(&self, rule: &Rule, request: &CapabilityRequest) -> bool {
        // 检查 tool_matcher（如果指定）
        if let Some(ref matcher) = rule.tool_matcher {
            if !glob_match(matcher, &request.name) {
                return false;
            }
        }

        // 必须至少有一个条件
        if rule.conditions.is_empty() {
            return false;
        }

        // 所有条件必须匹配
        self.matches(&rule.conditions, request)
    }

    /// 检查所有条件是否匹配
    fn matches(&self, conditions: &[Condition], request: &CapabilityRequest) -> bool {
        if conditions.is_empty() {
            return false; // 无条件的规则不自动匹配
        }

        for condition in conditions {
            // 提取字段值
            let value = self.extract_field(&condition.field, request);
            let value_str = match &value {
                Some(v) => v.as_str().map(|s| s.to_string())
                    .or_else(|| Some(v.to_string())),
                None => None,
            };

            let value_str = match value_str {
                Some(s) => s,
                None => return false,
            };

            // 应用运算符
            let matched = match condition.operator {
                ConditionOperator::RegexMatch => {
                    regex_matches(&condition.pattern, &value_str)
                }
                ConditionOperator::Contains => {
                    value_str.contains(&condition.pattern)
                }
                ConditionOperator::NotContains => {
                    !value_str.contains(&condition.pattern)
                }
                ConditionOperator::Equals => {
                    value_str == condition.pattern
                }
                ConditionOperator::PathMatch => {
                    glob_match(&condition.pattern, &value_str)
                }
            };

            if !matched {
                return false;
            }
        }

        true
    }

    /// 从请求中提取字段值
    fn extract_field(&self, field: &str, request: &CapabilityRequest) -> Option<serde_json::Value> {
        match field {
            "tool_name" | "name" => {
                Some(serde_json::Value::String(request.name.clone()))
            }
            "command" => {
                request.arguments.get("command").cloned()
                    .or_else(|| request.arguments.get("cmd").cloned())
            }
            "new_text" | "content" => {
                request.arguments.get("content").cloned()
                    .or_else(|| request.arguments.get("htmlContent").cloned())
            }
            "old_text" => {
                request.arguments.get("oldContent").cloned()
                    .or_else(|| request.arguments.get("old_text").cloned())
            }
            "file_path" | "path" => {
                request.arguments.get("path").cloned()
            }
            "arguments" => {
                Some(serde_json::to_value(&request.arguments).unwrap_or_default())
            }
            "working_dir" | "cwd" => {
                request.working_dir.as_ref()
                    .map(|p| serde_json::Value::String(p.to_string_lossy().to_string()))
            }
            _ => None,
        }
    }
}

/// 规则评估结果
#[derive(Clone, Debug)]
pub struct RuleEvaluation {
    pub action: RuleAction,
    pub matched_rule: Option<String>,
    pub message: String,
    /// 所有匹配的规则名列表（与 Python 版一致）
    pub all_matched_rules: Vec<String>,
}

// ──────────────────────────────────────────────
// 工具函数
// ──────────────────────────────────────────────

/// 正则匹配
fn regex_matches(pattern: &str, text: &str) -> bool {
    regex::Regex::new(pattern)
        .map(|re| re.is_match(text))
        .unwrap_or(false)
}

/// 简单的 glob 匹配（支持 * 通配符）
fn glob_match(pattern: &str, name: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(suffix) = pattern.strip_suffix('*') {
        return name.starts_with(suffix);
    }
    if let Some(prefix) = pattern.strip_prefix('*') {
        return name.ends_with(prefix);
    }
    pattern == name
}

// ──────────────────────────────────────────────
// 测试
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_request(name: &str, args: serde_json::Value, cwd: Option<&str>) -> CapabilityRequest {
        CapabilityRequest {
            call_id: "t1".into(),
            name: name.into(),
            arguments: args,
            working_dir: cwd.map(|s| Path::new(s).to_path_buf()),
        }
    }

    #[test]
    fn test_parse_local_md_simple() {
        let content = r#"---
name: test-rule
enabled: true
event: bash
action: warn
---

⚠️ Warning message
"#;
        let (fm, msg) = parse_local_md(content);
        assert_eq!(fm["name"], "test-rule");
        assert_eq!(fm["enabled"], true);
        assert!(msg.contains("Warning"));
    }

    #[test]
    fn test_parse_local_md_no_frontmatter() {
        let content = "Just a message";
        let (fm, msg) = parse_local_md(content);
        assert!(fm.as_object().unwrap().is_empty());
        assert_eq!(msg, content);
    }

    #[test]
    fn test_rule_from_parts() {
        let fm = json!({
            "name": "test",
            "enabled": true,
            "event": "bash",
            "action": "warn",
            "conditions": [
                {"field": "command", "operator": "regex_match", "pattern": "rm"}
            ]
        });
        let rule = Rule::from_parts(&fm, "Be careful!").unwrap();
        assert_eq!(rule.name, "test");
        assert_eq!(rule.action, RuleAction::Warn);
        assert_eq!(rule.conditions.len(), 1);
    }

    #[test]
    fn test_hookify_pattern_expands_to_event_default_field() {
        let fm = json!({
            "name": "block-dangerous-rm",
            "enabled": true,
            "event": "bash",
            "pattern": "rm\\s+-rf",
            "action": "block"
        });

        let rule = Rule::from_parts(&fm, "Dangerous command").unwrap();

        assert_eq!(rule.conditions.len(), 1);
        assert_eq!(rule.conditions[0].field, "command");
        assert_eq!(rule.conditions[0].operator, ConditionOperator::RegexMatch);
    }

    #[test]
    fn test_parse_local_md_with_nested_conditions_yaml() {
        let content = r#"---
name: warn-sensitive-files
enabled: true
event: file
action: warn
conditions:
  - field: file_path
    operator: regex_match
    pattern: \.env$
  - field: content
    operator: contains
    pattern: KEY
---
Sensitive file edit detected.
"#;

        let (frontmatter, body) = parse_local_md(content);
        let rule = Rule::from_parts(&frontmatter, &body).unwrap();

        assert_eq!(rule.conditions.len(), 2);
        assert_eq!(rule.conditions[0].field, "file_path");
        assert!(rule.message.contains("Sensitive file"));
    }

    #[test]
    fn test_evaluate_allow_by_default() {
        let engine = RuleEngine::new();
        let req = make_request("read_file", json!({"path": "/etc/passwd"}), None);
        let result = engine.evaluate(&req);
        assert_eq!(result.action, RuleAction::Allow);
    }

    #[test]
    fn test_evaluate_block_regex() {
        let mut engine = RuleEngine::new();
        engine.add_rule(Rule {
            name: "block-etc".into(),
            enabled: true,
            event: RuleEvent::All,
            action: RuleAction::Block,
            conditions: vec![Condition {
                field: "path".into(),
                operator: ConditionOperator::RegexMatch,
                pattern: r"^/etc/".into(),
            }],
            pattern: None,
            tool_matcher: Some("read_file".into()),
            message: "Blocked /etc".into(),
            priority: 100,
        });

        let req = make_request("read_file", json!({"path": "/etc/passwd"}), None);
        let result = engine.evaluate(&req);
        assert_eq!(result.action, RuleAction::Block);

        let req2 = make_request("read_file", json!({"path": "/home/user/doc.txt"}), None);
        let result2 = engine.evaluate(&req2);
        assert_eq!(result2.action, RuleAction::Allow);
    }

    #[test]
    fn test_evaluate_contains_operator() {
        let mut engine = RuleEngine::new();
        engine.add_rule(Rule {
            name: "warn-rm".into(),
            enabled: true,
            event: RuleEvent::Bash,
            action: RuleAction::Warn,
            conditions: vec![Condition {
                field: "command".into(),
                operator: ConditionOperator::Contains,
                pattern: "rm -rf".into(),
            }],
            pattern: None,
            tool_matcher: Some("run_terminal".into()),
            message: "Dangerous command".into(),
            priority: 100,
        });

        let req = make_request("run_terminal", json!({"command": "rm -rf /tmp"}), None);
        let result = engine.evaluate(&req);
        assert_eq!(result.action, RuleAction::Warn);
    }

    #[test]
    fn test_evaluate_not_contains_operator() {
        let mut engine = RuleEngine::new();
        engine.add_rule(Rule {
            name: "require-test".into(),
            enabled: true,
            event: RuleEvent::Stop,
            action: RuleAction::Block,
            conditions: vec![Condition {
                field: "content".into(),
                operator: ConditionOperator::NotContains,
                pattern: "npm test".into(),
            }],
            pattern: None,
            tool_matcher: None,
            message: "Tests required!".into(),
            priority: 100,
        });

        let req = make_request("some_tool", json!({"content": "Just some work"}), None);
        let result = engine.evaluate_for_event(&req, &RuleEvent::Stop);
        assert_eq!(result.action, RuleAction::Block);
    }

    #[test]
    fn test_glob_match_exact() {
        assert!(glob_match("read_file", "read_file"));
        assert!(!glob_match("read_file", "write_file"));
    }

    #[test]
    fn test_glob_match_wildcard() {
        assert!(glob_match("write_*", "write_file"));
        assert!(!glob_match("write_*", "read_file"));
    }

    #[test]
    fn test_regex_matches() {
        assert!(regex_matches(r"rm\s+-rf", "rm -rf /"));
        assert!(!regex_matches(r"rm\s+-rf", "rm -r /"));
    }

    #[test]
    fn test_load_rules_from_dir_nonexistent() {
        let rules = load_rules_from_dir(Path::new("/nonexistent/path"));
        assert!(rules.is_empty());
    }

    #[test]
    fn test_rule_priority_sorting() {
        let mut engine = RuleEngine::new();
        engine.add_rule(Rule {
            name: "low".into(), enabled: true, event: RuleEvent::All,
            action: RuleAction::Warn, conditions: vec![
                Condition { field: "name".into(), operator: ConditionOperator::Equals, pattern: "test".into() }
            ], pattern: None, tool_matcher: None, message: "low".into(), priority: 10,
        });
        engine.add_rule(Rule {
            name: "high".into(), enabled: true, event: RuleEvent::All,
            action: RuleAction::Block, conditions: vec![
                Condition { field: "name".into(), operator: ConditionOperator::Equals, pattern: "test".into() }
            ], pattern: None, tool_matcher: None, message: "high".into(), priority: 100,
        });

        let req = make_request("test", json!({}), None);
        let result = engine.evaluate(&req);
        // 高优先级先评估
        assert_eq!(result.matched_rule.as_deref(), Some("high"));
        assert_eq!(result.action, RuleAction::Block);
    }
}
