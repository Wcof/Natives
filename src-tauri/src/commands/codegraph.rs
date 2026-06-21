use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// RTK gain data — parsed from `rtk gain` text output
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RtkGainResult {
    /// Total tokens saved across all commands
    pub total_saved: u64,
    /// Total commands tracked
    pub total_commands: u64,
    /// Per-command breakdown (top entries)
    pub commands: Vec<RtkGainCommand>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RtkGainCommand {
    pub command: String,
    pub count: u64,
    pub tokens_saved: u64,
}

/// Execute `rtk gain` (text output) and parse token savings data.
///
/// `rtk gain --json` is NOT a valid flag — we parse the structured text table instead.
/// If `rtk` is not installed or the command fails, returns None (frontend shows empty).
#[tauri::command]
pub fn rtk_gain() -> Option<RtkGainResult> {
    let output = std::process::Command::new("rtk")
        .args(["gain"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_rtk_gain_output(&stdout)
}

/// Parse `rtk gain` text output to extract command stats.
///
/// Expected format (after header lines):
/// ```text
///   #  Command                   Count   Saved    Avg%    Time  Impact
/// ────────────────────────────────────────────────────────────────────────
///  1.  rtk vitest run              182    7.4M   93.2%   16.1s  ██████████
///  2.  rtk grep                   1243    1.4M   27.9%    18ms  ██░░░░░░░░
/// ```
fn parse_rtk_gain_output(text: &str) -> Option<RtkGainResult> {
    let mut total_saved: u64 = 0;
    let mut total_commands: u64 = 0;
    let mut commands: Vec<RtkGainCommand> = Vec::new();

    // First pass: extract totals from header
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(val) = trimmed.strip_prefix("Total commands:") {
            total_commands = val.trim().split_whitespace().next()
                .and_then(|s| s.replace(',', "").parse().ok())
                .unwrap_or(0);
        }
        if let Some(val) = trimmed.strip_prefix("Tokens saved:") {
            // e.g. "11.1M (65.4%)"
            let raw = val.trim().split_whitespace().next().unwrap_or("");
            total_saved = parse_token_amount(raw);
        }
    }

    // Second pass: parse "By Command" table
    let mut in_table = false;
    for line in text.lines() {
        let trimmed = line.trim();

        // Detect table header
        if trimmed.starts_with("#") && trimmed.contains("Command") && trimmed.contains("Count") {
            in_table = true;
            continue;
        }
        // Skip separator line (─)
        if in_table && trimmed.starts_with('─') {
            continue;
        }
        // Stop at empty line or end of table
        if in_table && trimmed.is_empty() {
            break;
        }

        if in_table {
            // Parse: " 1.  rtk vitest run              182    7.4M   93.2%   16.1s  ██████████"
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 4 {
                // First token is "#." like "1."
                let cmd_start = if parts[0].ends_with('.') { 1 } else { 0 };
                // Find the count field (numeric) and saved field (ends with K/M)
                let mut count_idx = None;
                let mut saved_idx = None;
                for (i, part) in parts.iter().enumerate() {
                    if part.chars().all(|c| c.is_ascii_digit() || c == ',') {
                        // This could be the count
                        if count_idx.is_none() {
                            count_idx = Some(i);
                        }
                    } else if part.ends_with('K') || part.ends_with('M') {
                        if saved_idx.is_none() {
                            saved_idx = Some(i);
                        }
                    }
                }

                if let (Some(ci), Some(si)) = (count_idx, saved_idx) {
                    let cmd_parts = &parts[cmd_start..ci];
                    let command = cmd_parts.join(" ");
                    let count: u64 = parts[ci].replace(',', "").parse().unwrap_or(0);
                    let saved = parse_token_amount(parts[si]);

                    if !command.is_empty() && (count > 0 || saved > 0) {
                        commands.push(RtkGainCommand { command, count, tokens_saved: saved });
                    }
                }
            }
        }
    }

    // If header parsing failed, fall back to table sum
    if total_commands == 0 {
        total_commands = commands.iter().map(|c| c.count).sum();
    }
    if total_saved == 0 {
        total_saved = commands.iter().map(|c| c.tokens_saved).sum();
    }

    Some(RtkGainResult { total_saved, total_commands, commands })
}

/// Parse a token amount like "11.1M", "7.4M", "176.4K", "1234"
fn parse_token_amount(s: &str) -> u64 {
    let s = s.trim();
    if s.ends_with('M') {
        let num: f64 = s[..s.len()-1].parse().unwrap_or(0.0);
        (num * 1_000_000.0) as u64
    } else if s.ends_with('K') {
        let num: f64 = s[..s.len()-1].parse().unwrap_or(0.0);
        (num * 1_000.0) as u64
    } else if s.ends_with('B') {
        let num: f64 = s[..s.len()-1].parse().unwrap_or(0.0);
        (num * 1_000_000_000.0) as u64
    } else {
        s.replace(',', "").parse().unwrap_or(0)
    }
}

// ── Code Graph ──

/// CodeGraph node — represents a single node in the code hierarchy
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeGraphNode {
    pub name: String,
    pub kind: String, // "dir" | "file" | "symbol" | "dep"
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    pub children: Vec<CodeGraphNode>,
}

/// Read or generate CodeGraph data.
///
/// Priority:
/// 1. If `.codegraph/` directory exists at project root, read its structure.
/// 2. If `codegraph` CLI is available, run `codegraph explore` to generate data.
/// 3. Fall back to reading the project src/ directory as a simple tree.
#[tauri::command]
pub fn read_codegraph() -> Vec<CodeGraphNode> {
    let project_root = find_project_root();

    // Priority 1: Check existing .codegraph/ directory
    let cg_dir = project_root.join(".codegraph");
    if cg_dir.exists() && cg_dir.is_dir() {
        return read_codegraph_dir_tree(&cg_dir, &cg_dir);
    }

    // Priority 2: Try to generate with `codegraph explore` CLI
    let generated = generate_codegraph_via_cli(&project_root);
    if let Some(nodes) = generated {
        return nodes;
    }

    // Priority 3: Fall back to simple src/ directory tree
    let src_dir = project_root.join("src");
    if src_dir.exists() && src_dir.is_dir() {
        return vec![CodeGraphNode {
            name: "src".into(),
            kind: "dir".into(),
            path: Some(src_dir.to_string_lossy().to_string()),
            symbol_type: None,
            line: None,
            children: read_simple_tree(&src_dir),
        }];
    }

    vec![]
}

/// Try to find the project root by checking common locations.
fn find_project_root() -> PathBuf {
    // Try CWD first
    if let Ok(cwd) = std::env::current_dir() {
        // Check if CWD or CWD/parent has src/ or .codegraph/
        let mut check = Some(cwd.as_path());
        while let Some(dir) = check {
            if dir.join("src").exists() || dir.join(".codegraph").exists() || dir.join("AGENTS.md").exists() {
                return dir.to_path_buf();
            }
            check = dir.parent();
        }
        cwd
    } else {
        PathBuf::from(".")
    }
}

/// Try to generate CodeGraph data using the `codegraph explore` CLI.
fn generate_codegraph_via_cli(root: &PathBuf) -> Option<Vec<CodeGraphNode>> {
    let output = std::process::Command::new("codegraph")
        .args(["explore", root.to_string_lossy().as_ref(), "--format", "json"])
        .output()
        .ok()?;

    if !output.status.success() {
        // codegraph CLI might not be installed or failed
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Try to parse the output as JSON nodes
    if let Ok(nodes) = serde_json::from_str::<Vec<CodeGraphNode>>(&stdout) {
        return Some(nodes);
    }

    None
}

/// Recursively read a .codegraph/ directory into CodeGraphNode tree.
fn read_codegraph_dir_tree(dir: &PathBuf, root: &PathBuf) -> Vec<CodeGraphNode> {
    let mut children = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
                .unwrap_or_default();

            if path.is_dir() {
                children.push(CodeGraphNode {
                    name,
                    kind: "dir".into(),
                    path: Some(path.to_string_lossy().to_string()),
                    symbol_type: None,
                    line: None,
                    children: read_codegraph_dir_tree(&path, root),
                });
            } else if path.extension().map(|e| e == "json").unwrap_or(false) {
                // Parse JSON index files for structured symbol data
                let json_children = try_parse_json_index(&path);
                children.push(CodeGraphNode {
                    name,
                    kind: "index".into(),
                    path: Some(path.to_string_lossy().to_string()),
                    symbol_type: None,
                    line: None,
                    children: json_children,
                });
            } else {
                children.push(CodeGraphNode {
                    name,
                    kind: "file".into(),
                    path: Some(path.to_string_lossy().to_string()),
                    symbol_type: None,
                    line: None,
                    children: vec![],
                });
            }
        }
    }
    children
}

/// Build a simple tree from src/ directory (fallback when no CodeGraph)
fn read_simple_tree(dir: &PathBuf) -> Vec<CodeGraphNode> {
    let mut children = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten().take(50) {
            let path = entry.path();
            let name = path.file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
                .unwrap_or_default();

            if path.is_dir() {
                let sub = read_simple_tree(&path);
                children.push(CodeGraphNode {
                    name,
                    kind: "dir".into(),
                    path: Some(path.to_string_lossy().to_string()),
                    symbol_type: None,
                    line: None,
                    children: sub,
                });
            } else {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                let kind = match ext {
                    "ts" | "tsx" | "js" | "jsx" => "source",
                    "rs" => "rust",
                    "css" | "scss" => "style",
                    "json" | "toml" | "yaml" | "yml" => "config",
                    "md" | "mdx" => "doc",
                    _ => "file",
                };
                children.push(CodeGraphNode {
                    name,
                    kind: kind.into(),
                    path: Some(path.to_string_lossy().to_string()),
                    symbol_type: None,
                    line: None,
                    children: vec![],
                });
            }
        }
    }
    children
}

/// Try to parse a JSON file as a CodeGraph index and extract nodes.
fn try_parse_json_index(path: &std::path::Path) -> Vec<CodeGraphNode> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    let value: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    // Try various known CodeGraph JSON formats
    let candidates = match &value {
        serde_json::Value::Array(arr) => arr.clone(),
        serde_json::Value::Object(map) => {
            if let Some(nodes) = map.get("nodes").and_then(|n| n.as_array()) {
                nodes.clone()
            } else if let Some(children) = map.get("children").and_then(|c| c.as_array()) {
                children.clone()
            } else if let Some(entries) = map.get("entries").and_then(|e| e.as_array()) {
                entries.clone()
            } else {
                return vec![];
            }
        }
        _ => return vec![],
    };

    candidates.iter().filter_map(|v| {
        let name = v.get("name").and_then(|n| n.as_str())?.to_string();
        let kind = v.get("kind").and_then(|k| k.as_str()).unwrap_or("symbol").to_string();
        let path = v.get("path").and_then(|p| p.as_str()).map(|s| s.to_string());
        let symbol_type = v.get("symbolType").and_then(|t| t.as_str()).or_else(|| {
            v.get("type").and_then(|t| t.as_str())
        }).map(|s| s.to_string());
        let line = v.get("line").and_then(|l| l.as_u64());
        let children = v.get("children")
            .and_then(|c| c.as_array())
            .map(|arr| {
                arr.iter().filter_map(|cv| {
                    let n = cv.get("name").and_then(|n| n.as_str())?.to_string();
                    let k = cv.get("kind").and_then(|k| k.as_str()).unwrap_or("symbol").to_string();
                    Some(CodeGraphNode {
                        name: n, kind: k, path: None, symbol_type: None, line: None, children: vec![],
                    })
                }).collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Some(CodeGraphNode { name, kind, path, symbol_type, line, children })
    }).collect()
}
