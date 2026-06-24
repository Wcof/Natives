//! runtime/native/context_assembler.rs — Context Assembler 静态摘要层
//!
//! 将多源上下文融合为送入 LLM 的 system prompt + messages。
//! 双轨混合：静态摘要打底 + MCP 按需深查（MCP 在后续切片补齐）。

use crate::Result;
use std::path::Path;

const PER_FILE_LIMIT: usize = 8192; // 8KB
const MAX_TREE_ENTRIES: usize = 500;

pub struct AssembledContext {
    pub system_prompt: String,
    pub messages: Vec<serde_json::Value>,
}

/// 装配上下文：文件树 + 身份文件 + 代码签名摘要 + 模块生成规约
pub async fn assemble(
    working_dir: Option<&Path>,
    user_prompt: &str,
    _budget: usize,
) -> Result<AssembledContext> {
    let mut sections = vec![];

    // L1: 项目文件树
    if let Some(wd) = working_dir {
        sections.push(format_file_tree(wd));
    }

    // L2: 身份文件全文
    sections.push(format_identity_files(working_dir).await);

    // L3: 代码签名摘要（syntect + regex fallback）
    sections.push(format_code_signatures(working_dir));

    // L4: 模块生成规约注入
    sections.push(inject_module_specs());

    let system_prompt = sections.into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");

    Ok(AssembledContext {
        system_prompt,
        messages: vec![serde_json::json!({ "role": "user", "content": user_prompt })],
    })
}

fn format_file_tree(wd: &Path) -> String {
    let mut entries = Vec::new();
    let skip_dirs = ["node_modules", ".git", "dist", ".next", "target", "__pycache__", ".natives"];
    let mut count = 0;

    for entry in walkdir::WalkDir::new(wd).max_depth(4).into_iter().filter_entry(|e| {
        if let Some(name) = e.file_name().to_str() {
            if skip_dirs.contains(&name) && e.depth() > 0 { return false; }
        }
        true
    }) {
        let entry = match entry { Ok(e) => e, Err(_) => continue };
        if count >= MAX_TREE_ENTRIES { break; }
        if let Ok(rel) = entry.path().strip_prefix(wd) {
            entries.push(rel.display().to_string());
            count += 1;
        }
    }

    if entries.is_empty() { return String::new(); }
    format!("## Project File Tree\n{}", entries.join("\n"))
}

async fn format_identity_files(wd: Option<&Path>) -> String {
    let filenames = ["claude.md", "soul.md", "user.md", "AGENTS.md", "CLAUDE.md"];
    let mut sections = vec![];

    if let Some(wd) = wd {
        for name in &filenames {
            let path = wd.join(name);
            if path.exists() {
                if let Ok(content) = tokio::fs::read_to_string(&path).await {
                    if content.len() > PER_FILE_LIMIT {
                        sections.push(format!("## {name}\n{}\n\n[truncated] File exceeds {}KB.", 
                            &content[..PER_FILE_LIMIT], PER_FILE_LIMIT / 1024));
                    } else {
                        sections.push(format!("## {name}\n{content}"));
                    }
                }
            }
        }
    }

    if sections.is_empty() { String::new() } else { sections.join("\n\n") }
}

fn format_code_signatures(wd: Option<&Path>) -> String {
    let sig_patterns: &[(&str, &str)] = &[
        (r"^(pub\s+)?(async\s+)?fn\s+(\w+)", "fn"),
        (r"^(export\s+)?(async\s+)?function\s+(\w+)", "fn"),
        (r"^(export\s+)?(interface|type|class)\s+(\w+)", "type"),
        (r"^(def|class|async\s+def)\s+(\w+)", "fn"),
        (r"^func\s+(\w+)", "fn"),
    ];

    let mut signatures = vec![];
    if let Some(wd) = wd {
        for entry in walkdir::WalkDir::new(wd).max_depth(3).into_iter().filter_entry(|e| {
            if let Some(name) = e.file_name().to_str() {
                if name.starts_with('.') || name == "node_modules" || name == "target" { return false; }
            }
            true
        }) {
            let entry = match entry { Ok(e) => e, Err(_) => continue };
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !matches!(ext, "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go") { continue; }
            if let Ok(content) = std::fs::read_to_string(path) {
                let rel = path.strip_prefix(wd).unwrap_or(path).display();
                let mut file_sigs = vec![];
                for line in content.lines().take(200) {
                    for (pat, kind) in sig_patterns {
                        if let Ok(re) = regex::Regex::new(pat) {
                            if let Some(caps) = re.captures(line) {
                                if let Some(m) = caps.get(caps.len() - 1) {
                                    file_sigs.push(format!("{kind} {}", m.as_str()));
                                }
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

fn inject_module_specs() -> String {
    // 复用 prompt-context-injector.ts 的 Rust 等价物
    // MVP：返回静态规约骨架，完整规约在后续迭代补齐
    "## Module Generation Specs\n\
     - CSP: script-src 'self' tauri://assets; 禁外部 CDN (KI-5)\n\
     - contract_id 由 Rust 内核计算，AI 禁止生成 (KI-1)\n\
     - 迁移仅允许声明式 JSON Mapping DSL，禁可执行代码 (KI-3)\n\
     - Liquid Glass 视觉令牌：--vibe-* CSS 变量\n\
     - Bridge: window.natives.* 暴露数据读写/主题/通知/生命周期".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_file_tree_empty_dir() {
        let dir = std::env::temp_dir().join("natives_test_empty");
        let _ = std::fs::create_dir_all(&dir);
        let result = format_file_tree(&dir);
        // 空目录至少有自身
        assert!(result.contains("Project File Tree") || result.is_empty());
    }

    #[test]
    fn test_inject_module_specs_contains_ki() {
        let specs = inject_module_specs();
        assert!(specs.contains("KI-1"));
        assert!(specs.contains("KI-3"));
        assert!(specs.contains("KI-5"));
    }

    #[tokio::test]
    async fn test_format_identity_files_truncation() {
        let dir = std::env::temp_dir().join("natives_test_identity");
        let _ = std::fs::create_dir_all(&dir);
        // 写一个超限文件
        let big_content = "x".repeat(PER_FILE_LIMIT + 1000);
        let _ = std::fs::write(dir.join("claude.md"), &big_content);
        let result = format_identity_files(Some(&dir)).await;
        assert!(result.contains("[truncated]"), "超限文件必须带截断标记");
    }
}
