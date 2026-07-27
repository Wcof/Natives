//! Minimal multi-file patch format (Codex/Grok-inspired, independent reimplementation).
//!
//! ```text
//! *** Begin Patch
//! *** Add File: path/to/new.txt
//! +line
//! *** Update File: path/to/old.txt
//! @@
//! -old
//! +new
//! *** Delete File: path/to/gone.txt
//! *** Move File: old.txt -> new.txt
//! *** End Patch
//! ```
//!
//! Also accepts a simple JSON body: `{ "files": [ { "op": "add|update|delete|move", ... } ] }`
//! or legacy `{ "path", "content" }` full-file write (mapped to update/add).

use crate::ToolError;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchOp {
    Add {
        path: String,
        content: String,
    },
    Update {
        path: String,
        /// When Some, full-file replace. When None, content is unified-ish hunks already applied offline.
        content: String,
    },
    Delete {
        path: String,
    },
    Move {
        from: String,
        to: String,
    },
}

pub fn parse_patch_input(input: &Value) -> Result<Vec<PatchOp>, ToolError> {
    // Legacy single-file full write
    if let (Some(path), Some(content)) = (
        input.get("path").and_then(Value::as_str),
        input.get("content").and_then(Value::as_str),
    ) {
        if input.get("patch").is_none() && input.get("files").is_none() {
            reject_escape(path)?;
            return Ok(vec![PatchOp::Update {
                path: path.to_string(),
                content: content.to_string(),
            }]);
        }
    }

    if let Some(files) = input.get("files").and_then(Value::as_array) {
        let mut ops = Vec::new();
        for f in files {
            let op = f.get("op").and_then(Value::as_str).unwrap_or("update");
            match op {
                "add" => {
                    let path = required_path(f, "path")?;
                    let content = f
                        .get("content")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    ops.push(PatchOp::Add { path, content });
                }
                "update" => {
                    let path = required_path(f, "path")?;
                    let content = f
                        .get("content")
                        .and_then(Value::as_str)
                        .ok_or_else(|| err("missing content for update"))?
                        .to_string();
                    ops.push(PatchOp::Update { path, content });
                }
                "delete" => {
                    let path = required_path(f, "path")?;
                    ops.push(PatchOp::Delete { path });
                }
                "move" => {
                    let from = required_path(f, "from").or_else(|_| required_path(f, "path"))?;
                    let to = required_path(f, "to")?;
                    ops.push(PatchOp::Move { from, to });
                }
                other => {
                    return Err(err(&format!("unknown patch op `{other}`")));
                }
            }
        }
        if ops.is_empty() {
            return Err(err("files array is empty"));
        }
        return Ok(ops);
    }

    if let Some(patch) = input.get("patch").and_then(Value::as_str) {
        return parse_begin_patch(patch);
    }

    Err(err(
        "apply_patch requires `patch` text, `files` array, or legacy path+content",
    ))
}

fn parse_begin_patch(text: &str) -> Result<Vec<PatchOp>, ToolError> {
    let mut ops = Vec::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed == "*** Begin Patch"
            || trimmed == "*** End Patch"
            || trimmed.starts_with("@@")
        {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("*** Add File:") {
            let path = rest.trim().to_string();
            reject_escape(&path)?;
            let content = collect_plus_body(&mut lines);
            ops.push(PatchOp::Add { path, content });
        } else if let Some(rest) = trimmed.strip_prefix("*** Update File:") {
            let path = rest.trim().to_string();
            reject_escape(&path)?;
            // Prefer full content if all lines are + prefixed without mixed context;
            // otherwise build by applying + / - to empty or existing (full rewrite from + lines).
            let content = collect_update_body(&mut lines);
            ops.push(PatchOp::Update { path, content });
        } else if let Some(rest) = trimmed.strip_prefix("*** Delete File:") {
            let path = rest.trim().to_string();
            reject_escape(&path)?;
            ops.push(PatchOp::Delete { path });
        } else if let Some(rest) = trimmed.strip_prefix("*** Move File:") {
            let rest = rest.trim();
            let (from, to) = rest
                .split_once("->")
                .ok_or_else(|| err("Move File expects `old -> new`"))?;
            let from = from.trim().to_string();
            let to = to.trim().to_string();
            reject_escape(&from)?;
            reject_escape(&to)?;
            ops.push(PatchOp::Move { from, to });
        }
    }
    if ops.is_empty() {
        return Err(err("no patch operations found"));
    }
    Ok(ops)
}

fn collect_plus_body(lines: &mut std::iter::Peekable<std::str::Lines<'_>>) -> String {
    let mut out = String::new();
    while let Some(peek) = lines.peek() {
        let t = peek.trim_end();
        if t.starts_with("*** ") {
            break;
        }
        let line = lines.next().unwrap();
        if let Some(body) = line.strip_prefix('+') {
            out.push_str(body);
            out.push('\n');
        } else if line.starts_with('-') || line.starts_with("@@") {
            continue;
        } else if !line.starts_with('\\') {
            // bare line treated as content
            out.push_str(line);
            out.push('\n');
        }
    }
    // strip trailing newline for file write consistency
    if out.ends_with('\n') {
        out.pop();
    }
    out
}

fn collect_update_body(lines: &mut std::iter::Peekable<std::str::Lines<'_>>) -> String {
    // If the update only has + lines (common AI full rewrite), join them.
    // Mixed -/+ without base file is treated as full content of remaining + lines.
    collect_plus_body(lines)
}

fn required_path(v: &Value, key: &str) -> Result<String, ToolError> {
    let path = v
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| err(&format!("missing `{key}`")))?
        .to_string();
    reject_escape(&path)?;
    Ok(path)
}

fn reject_escape(path: &str) -> Result<(), ToolError> {
    if path.is_empty() {
        return Err(err("empty path"));
    }
    if path.starts_with('/')
        || path.contains(':') && cfg!(windows) && path.chars().nth(1) == Some(':')
    {
        // Absolute paths rejected — must be project-relative
        if std::path::Path::new(path).is_absolute() {
            return Err(ToolError {
                code: "path_escape".into(),
                message: "apply_patch only accepts project-relative paths".into(),
                retryable: false,
            });
        }
    }
    if path.contains("..") {
        return Err(ToolError {
            code: "path_escape".into(),
            message: "path traversal rejected".into(),
            retryable: false,
        });
    }
    Ok(())
}

fn err(msg: &str) -> ToolError {
    ToolError {
        code: "invalid_input".into(),
        message: msg.into(),
        retryable: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_legacy_path_content() {
        let ops = parse_patch_input(&json!({"path":"a.txt","content":"hi"})).unwrap();
        assert_eq!(
            ops,
            vec![PatchOp::Update {
                path: "a.txt".into(),
                content: "hi".into()
            }]
        );
    }

    #[test]
    fn parses_files_array() {
        let ops = parse_patch_input(&json!({
            "files": [
                {"op":"add","path":"n.txt","content":"x"},
                {"op":"delete","path":"old.txt"},
                {"op":"move","from":"a","to":"b"}
            ]
        }))
        .unwrap();
        assert_eq!(ops.len(), 3);
    }

    #[test]
    fn parses_begin_patch() {
        let patch = r#"*** Begin Patch
*** Add File: src/new.rs
+fn main() {}
*** Delete File: dead.txt
*** End Patch
"#;
        let ops = parse_patch_input(&json!({"patch": patch})).unwrap();
        assert_eq!(ops.len(), 2);
        assert!(matches!(ops[0], PatchOp::Add { .. }));
        assert!(matches!(ops[1], PatchOp::Delete { .. }));
    }

    #[test]
    fn rejects_dotdot() {
        assert!(parse_patch_input(&json!({"path":"../x","content":"a"})).is_err());
    }
}
