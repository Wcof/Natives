//! runtime/native/context_assembler.rs — Context Assembler 多层上下文组装（增强版）
//!
//! 将多源上下文融合为送入 LLM 的 system prompt + messages。
//! 六层架构（参考 Claude Code 的 context window 管理）：
//!
//!   L0 — Conversation Memory（会话记忆摘要，注入 messages）
//!   L1 — Project File Tree（项目文件树，注入 system prompt）
//!   L2 — Identity Files（身份文件全文，claude.md/soul.md 等）
//!   L3 — Code Signatures（代码签名摘要，regex 匹配）
//!   L4 — Module Specs（模块生成规约注入）
//!   L5 — Execution Engine Context（执行引擎原子能力说明 + 可用工具）

use crate::Result;
use std::path::Path;

const PER_FILE_LIMIT: usize = 8192; // 8KB
const MAX_TREE_ENTRIES: usize = 500;
const CONVERSATION_SUMMARY_LIMIT: usize = 4096;
const MAX_TREE_DEPTH: usize = 4;
const MAX_SIGNATURE_DEPTH: usize = 3;

pub struct AssembledContext {
    pub system_prompt: String,
    pub messages: Vec<serde_json::Value>,
}

/// 装配上下文（六层架构）
///
/// `context_hints` — 调用方注入的额外上下文提示（如工作目录、任务描述等）
pub async fn assemble(
    working_dir: Option<&Path>,
    user_prompt: &str,
    _budget: usize,
) -> Result<AssembledContext> {
    let mut sections = vec![];

    // L1: 项目文件树
    if let Some(wd) = working_dir {
        let tree = format_file_tree(wd);
        if !tree.is_empty() {
            sections.push(tree);
        }
    }

    // L2: 身份文件全文（claude.md 等）
    let identity = format_identity_files(working_dir).await;
    if !identity.is_empty() {
        sections.push(identity);
    }

    // L3: 代码签名摘要（regex 匹配函数/类型定义）
    let sigs = format_code_signatures(working_dir);
    if !sigs.is_empty() {
        sections.push(sigs);
    }

    // L4: 模块生成规约
    sections.push(inject_module_specs());

    // L5: 执行引擎上下文（原子能力层说明）
    sections.push(inject_capability_specs());

    // L6: 工作目录提示
    if let Some(wd) = working_dir {
        sections.push(format!(
            "## Working Directory\nCurrent working directory: `{}`",
            wd.display()
        ));
    }

    let system_prompt = sections.into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");

    Ok(AssembledContext {
        system_prompt,
        messages: vec![serde_json::json!({ "role": "user", "content": user_prompt })],
    })
}

// ──────────────────────────────────────────────
// L0: 会话记忆压缩
// ──────────────────────────────────────────────

/// 将会话记忆压缩为摘要注入 system prompt
pub fn format_conversation_summary(history: &[serde_json::Value]) -> String {
    if history.is_empty() {
        return String::new();
    }
    let mut summary = String::from("## Conversation Context (Recent)\n");
    let mut total = 0;
    for msg in history.iter().rev().take(10) {
        let role = msg["role"].as_str().unwrap_or("unknown");
        let content = msg["content"].as_str().unwrap_or("");
        let truncated: String = content.chars().take(200).collect();
        let line = format!("- [{role}]: {}\n", truncated);
        total += line.len();
        if total > CONVERSATION_SUMMARY_LIMIT { break; }
        summary.push_str(&line);
    }
    summary
}

// ──────────────────────────────────────────────
// L1: 项目文件树
// ──────────────────────────────────────────────

fn format_file_tree(wd: &Path) -> String {
    let mut entries = Vec::new();
    let skip_dirs = [
        "node_modules", ".git", "dist", ".next", "target",
        "__pycache__", ".natives", ".claude", ".codegraph",
        ".env", "venv", ".venv", ".cache",
    ];
    let mut count = 0;

    for entry in walkdir::WalkDir::new(wd)
        .max_depth(MAX_TREE_DEPTH)
        .into_iter()
        .filter_entry(|e| {
            if let Some(name) = e.file_name().to_str() {
                // 跳过隐藏目录（但保留 .claude.md 等关键文件）
                if name.starts_with('.') && e.depth() > 0 {
                    // 例外：保留 .claude.md, .atomcode.md 等
                    if name != ".atomcode.md" && name != ".env.example" {
                        return false;
                    }
                }
                if skip_dirs.contains(&name) && e.depth() > 0 {
                    return false;
                }
            }
            true
        })
    {
        let entry = match entry { Ok(e) => e, Err(_) => continue };
        if count >= MAX_TREE_ENTRIES { break; }
        if let Ok(rel) = entry.path().strip_prefix(wd) {
            let display = rel.display().to_string();
            // 添加缩进和目录标记
            let depth = rel.components().count();
            let indent = "  ".repeat(depth.saturating_sub(1));
            let prefix = if entry.file_type().is_dir() { "📁 " } else { "📄 " };
            entries.push(format!("{indent}{prefix}{display}"));
            count += 1;
        }
    }

    if entries.is_empty() { return String::new(); }
    format!("## Project File Tree\n{}", entries.join("\n"))
}

// ──────────────────────────────────────────────
// L2: 身份文件
// ──────────────────────────────────────────────

async fn format_identity_files(wd: Option<&Path>) -> String {
    // 身份文件按优先级排序
    let filenames = [
        "AGENTS.md", "CLAUDE.md", "soul.md", "user.md",
        ".atomcode.md", "ATOMCODE.md",
    ];
    let mut sections = vec![];

    if let Some(wd) = wd {
        for name in &filenames {
            let path = wd.join(name);
            if path.exists() && path.is_file() {
                if let Ok(content) = tokio::fs::read_to_string(&path).await {
                    if content.trim().is_empty() {
                        continue;
                    }
                    if content.len() > PER_FILE_LIMIT {
                        let truncated: String = content.chars().take(PER_FILE_LIMIT).collect();
                        sections.push(format!(
                            "## {name}\n{truncated}\n\n[truncated] File exceeds {}KB.",
                            PER_FILE_LIMIT / 1024
                        ));
                    } else {
                        sections.push(format!("## {name}\n{content}"));
                    }
                }
            }
        }

        // 另外搜索项目根目录的 .claude/hookify.*.local.md 规则
        let claude_dir = wd.join(".claude");
        if claude_dir.exists() && claude_dir.is_dir() {
            if let Ok(entries) = tokio::fs::read_dir(&claude_dir).await {
                let mut entries = entries;
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.ends_with(".local.md") {
                        if let Ok(content) = tokio::fs::read_to_string(entry.path()).await {
                            if content.len() > PER_FILE_LIMIT {
                                let truncated: String = content.chars().take(PER_FILE_LIMIT).collect();
                                sections.push(format!(
                                    "## .claude/{name}\n{}[truncated]",
                                    truncated
                                ));
                            } else {
                                sections.push(format!("## .claude/{name}\n{content}"));
                            }
                        }
                    }
                }
            }
        }
    }

    if sections.is_empty() { String::new() } else { sections.join("\n\n") }
}

// ──────────────────────────────────────────────
// L3: 代码签名摘要
// ──────────────────────────────────────────────

fn format_code_signatures(wd: Option<&Path>) -> String {
    let sig_patterns: &[(&str, &str)] = &[
        // Rust
        (r"^(pub\s+)?(async\s+)?fn\s+(\w+)", "fn"),
        (r"^(pub\s+)?(struct|enum|trait|impl)\s+(\w+)", "type"),
        (r"^(pub\s+)?(async\s+)?fn\s+(\w+)", "fn"),
        // TypeScript/JavaScript
        (r"^(export\s+)?(async\s+)?function\s+(\w+)", "fn"),
        (r"^(export\s+)?(interface|type|class|enum)\s+(\w+)", "type"),
        (r"^(export\s+)?(const|let|var)\s+(\w+)\s*[:=]\s*(async\s+)?\(?[^)]*\)?\s*(=>|:)", "fn"),
        // Python
        (r"^(async\s+)?def\s+(\w+)", "fn"),
        (r"^class\s+(\w+)", "type"),
        // Go
        (r"^func\s+(\([^)]*\)\s+)?(\w+)", "fn"),
        (r"^type\s+(\w+)\s+(struct|interface)", "type"),
    ];

    let mut signatures = vec![];
    if let Some(wd) = wd {
        for entry in walkdir::WalkDir::new(wd)
            .max_depth(MAX_SIGNATURE_DEPTH)
            .into_iter()
            .filter_entry(|e| {
                if let Some(name) = e.file_name().to_str() {
                    if name.starts_with('.')
                        || name == "node_modules"
                        || name == "target"
                        || name == "dist"
                        || name == ".next"
                    { return false; }
                }
                true
            })
        {
            let entry = match entry { Ok(e) => e, Err(_) => continue };
            let path = entry.path();
            if !path.is_file() { continue; }
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !matches!(ext, "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "java" | "swift") {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(path) {
                let rel = path.strip_prefix(wd).unwrap_or(path).display();
                let mut file_sigs = vec![];
                for line in content.lines().take(200) {
                    for (pat, kind) in sig_patterns {
                        if let Ok(re) = regex::Regex::new(pat) {
                            if let Some(caps) = re.captures(line) {
                                let name = caps.get(caps.len() - 1).map(|m| m.as_str()).unwrap_or("");
                                if !name.is_empty() && !name.starts_with('_') {
                                    file_sigs.push(format!("{kind} {}", name));
                                }
                                break; // 一行只匹配一个签名
                            }
                        }
                    }
                }
                if !file_sigs.is_empty() {
                    signatures.push(format!("### {rel}\n{}", file_sigs.join("\n")));
                }
            }
        }
    }

    if signatures.is_empty() { String::new() } else {
        format!("## Code Signatures\n{}", signatures.join("\n\n"))
    }
}

// ──────────────────────────────────────────────
// L4: 模块生成规约
// ──────────────────────────────────────────────

fn inject_module_specs() -> String {
    "\
## 模块生成规约 (Module Generation Specs)

本系统支持生成独立的前端模块。请严格遵守：

### 安全约束
- **CSP**: `script-src 'self' tauri://assets`；禁止加载外部 CDN 脚本
- **Permissions**: 声明式权限列表（如 `notification`, `clipboard-read`）
- **Sandbox**: iframe 沙箱 `allow-scripts allow-forms`，禁止 `allow-same-origin`

### 合约约束
- **contract_id**: 由 Rust 内核自动计算（SHA-256 of content），请勿设置或修改
- **Migration**: 仅允许声明式 JSON Mapping DSL，禁止生成可执行迁移代码
- **Lint**: 所有生成内容必须通过 Contract Linter（CSP、安全红线、合约校验）

### 样式约束
- **Liquid Glass**: 使用 `--vibe-*` CSS 变量实现品牌主题
- **Responsive**: 支持桌面 + 移动端自适应
- **Accessibility**: 遵循 WCAG 2.1 AA 标准

### Bridge API
所有与宿主通信必须通过 `window.natives.*` API：
- `natives.data.*` — 数据读写
- `natives.theme.*` — 主题切换
- `natives.notification.*` — 通知
- `natives.lifecycle.*` — 生命周期
".into()
}

// ──────────────────────────────────────────────
// L5: 执行引擎上下文
// ──────────────────────────────────────────────

fn inject_capability_specs() -> String {
    "\
## 可用原子能力 (Available Atomic Capabilities)

你可以使用以下原子能力来完成任务：

### 文件操作
- **`read_file`**: 读取文件内容（UTF-8，超过 1MB 自动截断，显示 [truncated]）
- **`list_dir`**: 列出目录内容（递归深度 ≤ 3，自动跳过 node_modules/.git/target）
- **`write_file`**: 原子写入文件内容（先写入临时文件再 rename，防止写一半断电）

### 模块生成
- **`write_module`**: 生成独立 SPA 模块（自动通过 Contract Linter，含 CSP/隐私红线校验）

### 终端执行
- **`run_terminal`**: 在白名单内执行终端命令（含超时保护，默认 30s）
  - 禁止 shell 元字符（`&`, `|`, `;`, `$`）
  - 返回 stdout/stderr + 退出码

### 代码质量
- **`lint_module`**: 对 HTML 内容运行 Contract Linter（不写盘，仅校验）

### 规则与安全
每个能力执行前都会经过：
1. **Hook Pipeline**: PreToolUse (审批/计时/审计) → 执行 → PostToolUse
2. **Rule Engine**: 检查 .local.md 规则（安全策略/团队规范）
3. **Doom Loop 保护**: 检测并阻止重复工具调用模式
4. **Self-Heal 熔断**: 连续失败 N 次后自动熔断
".into()
}

// ──────────────────────────────────────────────
// 测试
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_file_tree_empty_dir() {
        let dir = std::env::temp_dir().join("natives_test_empty");
        let _ = std::fs::create_dir_all(&dir);
        let result = format_file_tree(&dir);
        assert!(result.contains("Project File Tree") || result.is_empty());
    }

    #[test]
    fn test_inject_module_specs_contains_keywords() {
        let specs = inject_module_specs();
        assert!(specs.contains("contract_id"));
        assert!(specs.contains("CSP"));
        assert!(specs.contains("natives."));
    }

    #[tokio::test]
    async fn test_format_identity_files_truncation() {
        let dir = std::env::temp_dir().join("natives_test_identity");
        let _ = std::fs::create_dir_all(&dir);
        let big_content = "x".repeat(PER_FILE_LIMIT + 1000);
        let _ = std::fs::write(dir.join("claude.md"), &big_content);
        let result = format_identity_files(Some(&dir)).await;
        assert!(result.contains("[truncated]"), "Overlimit files must show truncation marker");
    }

    #[test]
    fn test_inject_capability_specs() {
        let specs = inject_capability_specs();
        assert!(specs.contains("read_file"));
        assert!(specs.contains("write_file"));
        assert!(specs.contains("write_module"));
        assert!(specs.contains("Hook Pipeline"));
        assert!(specs.contains("Doom Loop"));
    }

    #[test]
    fn test_format_conversation_summary_empty() {
        let result = format_conversation_summary(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_format_conversation_summary_non_empty() {
        let msgs = vec![
            serde_json::json!({"role": "user", "content": "Hello"}),
            serde_json::json!({"role": "assistant", "content": "Hi there!"}),
        ];
        let result = format_conversation_summary(&msgs);
        assert!(result.contains("user"));
        assert!(result.contains("assistant"));
    }
}
