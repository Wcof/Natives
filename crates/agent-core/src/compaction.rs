//! Context compaction helpers (Phase 6 minimum).
//!
//! Guarantees for tool-call integrity:
//! - Never drop an assistant message that contains tool_calls without its
//!   matching tool results (or strip tool_calls if results are gone).
//! - Preserve tool_call_id associations.
//! - Emit a summary of trimmed tool outputs for re-injection.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactResult {
    pub messages: Vec<Value>,
    pub dropped_tool_outputs: usize,
    pub repaired_dangling: usize,
    pub summary: String,
}

/// Repair dangling tool calls: if an assistant message references tool_call ids
/// that have no subsequent tool role message, strip those tool_calls or inject
/// a synthetic error tool result (prefer strip for safety with providers).
pub fn repair_dangling_tool_calls(messages: &[Value]) -> (Vec<Value>, usize) {
    let mut result_ids = std::collections::HashSet::new();
    for m in messages {
        let role = m.get("role").and_then(|v| v.as_str()).unwrap_or("");
        if role == "tool" {
            if let Some(id) = m.get("tool_call_id").and_then(|v| v.as_str()) {
                result_ids.insert(id.to_string());
            }
        }
    }
    let mut repaired = 0usize;
    let mut out = Vec::with_capacity(messages.len());
    for m in messages {
        let role = m.get("role").and_then(|v| v.as_str()).unwrap_or("");
        if role != "assistant" {
            out.push(m.clone());
            continue;
        }
        let Some(calls) = m.get("tool_calls").and_then(|v| v.as_array()) else {
            out.push(m.clone());
            continue;
        };
        let kept: Vec<Value> = calls
            .iter()
            .filter(|c| {
                let id = c.get("id").and_then(|v| v.as_str()).unwrap_or("");
                result_ids.contains(id)
            })
            .cloned()
            .collect();
        if kept.len() != calls.len() {
            repaired += calls.len() - kept.len();
        }
        let mut msg = m.clone();
        if kept.is_empty() {
            if let Some(obj) = msg.as_object_mut() {
                obj.remove("tool_calls");
            }
        } else if let Some(obj) = msg.as_object_mut() {
            obj.insert("tool_calls".into(), Value::Array(kept));
        }
        out.push(msg);
    }
    (out, repaired)
}

/// Compact by trimming large tool outputs while keeping structure.
pub fn compact_messages(messages: &[Value], max_tool_chars: usize) -> CompactResult {
    let (repaired, repaired_dangling) = repair_dangling_tool_calls(messages);
    let mut dropped_tool_outputs = 0usize;
    let mut summaries = Vec::new();
    let mut out = Vec::new();
    for m in repaired {
        let role = m.get("role").and_then(|v| v.as_str()).unwrap_or("");
        if role == "tool" {
            let content = m
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if content.len() > max_tool_chars {
                dropped_tool_outputs += 1;
                let preview: String = content.chars().take(max_tool_chars / 2).collect();
                let id = m
                    .get("tool_call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                summaries.push(format!(
                    "tool_call_id={id} truncated {}→{} chars",
                    content.len(),
                    preview.len()
                ));
                let mut trimmed = m.clone();
                if let Some(obj) = trimmed.as_object_mut() {
                    obj.insert(
                        "content".into(),
                        json!(format!(
                            "{preview}\n…[truncated {} chars for compaction]",
                            content.len().saturating_sub(preview.len())
                        )),
                    );
                }
                out.push(trimmed);
                continue;
            }
        }
        out.push(m);
    }
    CompactResult {
        messages: out,
        dropped_tool_outputs,
        repaired_dangling,
        summary: if summaries.is_empty() {
            "no tool outputs truncated".into()
        } else {
            summaries.join("; ")
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_dangling_tool_calls() {
        let msgs = vec![
            json!({"role":"user","content":"hi"}),
            json!({
                "role":"assistant",
                "content":"",
                "tool_calls":[
                    {"id":"c1","type":"function","function":{"name":"x","arguments":"{}"}},
                    {"id":"c2","type":"function","function":{"name":"y","arguments":"{}"}}
                ]
            }),
            json!({"role":"tool","tool_call_id":"c1","content":"ok"}),
        ];
        let (fixed, n) = repair_dangling_tool_calls(&msgs);
        assert_eq!(n, 1);
        let calls = fixed[1].get("tool_calls").unwrap().as_array().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["id"], "c1");
    }

    #[test]
    fn truncates_large_tool_output() {
        let big = "x".repeat(5000);
        let msgs = vec![
            json!({"role":"assistant","tool_calls":[{"id":"t1","type":"function","function":{"name":"r","arguments":"{}"}}]}),
            json!({"role":"tool","tool_call_id":"t1","content": big}),
        ];
        let res = compact_messages(&msgs, 200);
        assert_eq!(res.dropped_tool_outputs, 1);
        let content = res.messages[1]["content"].as_str().unwrap();
        assert!(content.len() < 5000);
        assert!(content.contains("truncated"));
    }
}
