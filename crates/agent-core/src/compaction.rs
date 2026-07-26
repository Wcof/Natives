//! Context compaction helpers.
//!
//! Two compaction shapes live here:
//! - **Mechanical** ([`compact_messages`]) — trim oversized tool outputs and
//!   repair dangling tool calls. Cheap, offline, always available.
//! - **Model-backed** ([`choose_summary_split`] + [`render_transcript_for_summary`]
//!   + [`apply_model_summary`]) — replace an old prefix of the history with a
//!   structured summary written by the model. The provider round trip itself is
//!   driven by the engine; this module owns the pure parts so they stay testable
//!   without a provider.
//!
//! Guarantees for tool-call integrity (providers hard-error when these break):
//! - Never drop an assistant message that contains tool_calls without its
//!   matching tool results (or strip tool_calls if results are gone).
//! - Never keep a tool result whose originating assistant tool_call is gone.
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
    /// Number of leading messages replaced by a model-written summary.
    /// `0` means the result came from mechanical compaction only.
    #[serde(default)]
    pub summarized_messages: usize,
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
        summarized_messages: 0,
    }
}

// ---------------------------------------------------------------------------
// Model-backed summarization (pure parts)
// ---------------------------------------------------------------------------

/// Marker prefixed to the injected summary message so downstream readers (and
/// the model itself) can tell a compaction summary from ordinary history.
pub const SUMMARY_MARKER: &str = "[context-summary]";

/// System prompt for the summarization round trip.
///
/// The five sections are the contract: the summary exists so the agent can
/// resume work, not so a human can admire prose.
pub const SUMMARY_SYSTEM_PROMPT: &str = "\
You are compacting the conversation history of an autonomous coding agent. The \
transcript below is about to be deleted and replaced by your summary, so the \
summary is the only thing the agent will remember about it.

Write a dense, factual summary using exactly these five sections, in this order:

## Goal
What the user asked for, in their terms.

## Completed
Work already finished. Be specific enough that the agent will not redo it.

## Current state
Where things stand right now: files changed, commands run and their outcome, \
what is verified and what is not.

## Open questions
Unresolved problems, failures, and decisions still to be made.

## Next steps
The concrete actions that were planned or in progress.

Rules:
- Keep concrete identifiers verbatim: file paths, symbol names, commands, error \
text, ids, and the reasons behind decisions.
- Never invent facts. If a section has nothing, write \"none\".
- Do not address the user, do not ask questions, do not add preamble or closing \
remarks. Output only the five sections.";

fn role_of(message: &Value) -> &str {
    message.get("role").and_then(Value::as_str).unwrap_or("")
}

/// Choose the index where the retained tail begins.
///
/// The tail is kept verbatim; everything before it is summarized. The split is
/// pulled backwards until the tail no longer starts on a tool result, so the
/// assistant message that issued those calls always travels with them.
///
/// Returns `0` when nothing can safely be summarized.
pub fn choose_summary_split(messages: &[Value], keep_tail: usize) -> usize {
    if messages.len() <= keep_tail {
        return 0;
    }
    let mut split = messages.len() - keep_tail;
    while split > 0 && role_of(&messages[split]) == "tool" {
        split -= 1;
    }
    split
}

/// Drop tool results whose originating assistant tool_call is no longer present.
///
/// The mirror image of [`repair_dangling_tool_calls`]: providers reject a tool
/// result block that has no preceding tool call just as hard as the reverse.
pub fn drop_orphan_tool_results(messages: &[Value]) -> (Vec<Value>, usize) {
    let mut known_ids = std::collections::HashSet::new();
    let mut dropped = 0usize;
    let mut out = Vec::with_capacity(messages.len());
    for m in messages {
        if role_of(m) == "assistant" {
            if let Some(calls) = m.get("tool_calls").and_then(|v| v.as_array()) {
                for call in calls {
                    if let Some(id) = call.get("id").and_then(Value::as_str) {
                        known_ids.insert(id.to_string());
                    }
                }
            }
        }
        if role_of(m) == "tool" {
            let id = m
                .get("tool_call_id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !known_ids.contains(id) {
                dropped += 1;
                continue;
            }
        }
        out.push(m.clone());
    }
    (out, dropped)
}

/// Render a transcript of the messages about to be dropped, for the summarizer.
///
/// Bounded on both axes: each message is truncated to `max_message_chars` and
/// the whole transcript to `max_total_chars`. When the budget binds, the most
/// recent messages win, but the very first message is always kept — it usually
/// carries the original goal.
pub fn render_transcript_for_summary(
    messages: &[Value],
    max_total_chars: usize,
    max_message_chars: usize,
) -> String {
    if messages.is_empty() {
        return String::new();
    }
    let lines: Vec<String> = messages
        .iter()
        .map(|m| render_message_line(m, max_message_chars))
        .collect();

    let total: usize = lines.iter().map(|l| l.len() + 1).sum();
    if total <= max_total_chars {
        return lines.join("\n");
    }

    // Budget binds: first message + as many recent messages as fit.
    let head = lines[0].clone();
    let mut budget = max_total_chars.saturating_sub(head.len() + 64);
    let mut tail: Vec<String> = Vec::new();
    for line in lines.iter().skip(1).rev() {
        if line.len() + 1 > budget {
            break;
        }
        budget -= line.len() + 1;
        tail.push(line.clone());
    }
    tail.reverse();
    let elided = lines.len().saturating_sub(1 + tail.len());
    let mut out = vec![head];
    if elided > 0 {
        out.push(format!("…[{elided} intermediate messages elided]"));
    }
    out.extend(tail);
    out.join("\n")
}

fn render_message_line(message: &Value, max_message_chars: usize) -> String {
    let role = role_of(message);
    let mut body = message
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if let Some(calls) = message.get("tool_calls").and_then(|v| v.as_array()) {
        let rendered: Vec<String> = calls
            .iter()
            .map(|c| {
                let name = c
                    .get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("?");
                let args = c
                    .get("function")
                    .and_then(|f| f.get("arguments"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                format!("{name}({})", truncate_chars(args, 200))
            })
            .collect();
        if !rendered.is_empty() {
            if !body.is_empty() {
                body.push('\n');
            }
            body.push_str(&format!("calls: {}", rendered.join(", ")));
        }
    }
    let label = match role {
        "tool" => {
            let name = message
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            format!("tool:{name}")
        }
        other if other.is_empty() => "unknown".to_string(),
        other => other.to_string(),
    };
    format!("{label}: {}", truncate_chars(&body, max_message_chars))
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let kept: String = text.chars().take(max_chars).collect();
    let omitted = text.chars().count() - max_chars;
    format!("{kept}…[{omitted} chars elided]")
}

/// Wrap the model's summary text into a history message.
pub fn summary_message(summary: &str, omitted_messages: usize) -> Value {
    json!({
        "role": "system",
        "content": format!(
            "{SUMMARY_MARKER} The first {omitted_messages} messages of this conversation were \
replaced by the summary below to stay inside the context budget. Treat it as an accurate \
record of work already done; continue from it instead of redoing that work.\n\n{}",
            summary.trim()
        ),
    })
}

/// Build the compacted history: `[summary] + mechanically compacted tail`.
///
/// `split` must come from [`choose_summary_split`]. Tool-call integrity is
/// enforced in both directions on the retained tail.
pub fn apply_model_summary(
    messages: &[Value],
    split: usize,
    summary: &str,
    max_tool_chars: usize,
) -> CompactResult {
    let split = split.min(messages.len());
    let (tail, orphans) = drop_orphan_tool_results(&messages[split..]);
    let compacted = compact_messages(&tail, max_tool_chars);
    let mut out = Vec::with_capacity(compacted.messages.len() + 1);
    out.push(summary_message(summary, split));
    out.extend(compacted.messages);
    CompactResult {
        messages: out,
        dropped_tool_outputs: compacted.dropped_tool_outputs,
        repaired_dangling: compacted.repaired_dangling + orphans,
        summary: summary.trim().to_string(),
        summarized_messages: split,
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
        assert_eq!(res.summarized_messages, 0);
    }

    /// call/result history: user, assistant(c1,c2), tool(c1), tool(c2), assistant, user
    fn tool_history() -> Vec<Value> {
        vec![
            json!({"role":"user","content":"start"}),
            json!({"role":"assistant","content":"planning"}),
            json!({"role":"user","content":"go on"}),
            json!({
                "role":"assistant","content":"",
                "tool_calls":[
                    {"id":"c1","type":"function","function":{"name":"read","arguments":"{\"p\":\"a\"}"}},
                    {"id":"c2","type":"function","function":{"name":"read","arguments":"{\"p\":\"b\"}"}}
                ]
            }),
            json!({"role":"tool","tool_call_id":"c1","name":"read","content":"a body"}),
            json!({"role":"tool","tool_call_id":"c2","name":"read","content":"b body"}),
            json!({"role":"assistant","content":"read both"}),
        ]
    }

    #[test]
    fn split_never_starts_tail_on_a_tool_result() {
        let msgs = tool_history();
        // keep_tail = 3 would start the tail at index 4 (a tool result).
        let split = choose_summary_split(&msgs, 3);
        assert_eq!(split, 3, "split must back up onto the assistant that made the calls");
        assert_ne!(msgs[split]["role"], "tool");
    }

    #[test]
    fn split_returns_zero_when_history_is_short() {
        let msgs = tool_history();
        assert_eq!(choose_summary_split(&msgs, 99), 0);
    }

    #[test]
    fn apply_model_summary_keeps_tool_pairs_intact() {
        let msgs = tool_history();
        let split = choose_summary_split(&msgs, 3);
        let res = apply_model_summary(&msgs, split, "## Goal\nship it", 4_000);
        assert_eq!(res.summarized_messages, 3);
        let head = res.messages[0]["content"].as_str().unwrap();
        assert!(head.starts_with(SUMMARY_MARKER));
        assert!(head.contains("ship it"));
        // Assistant tool_calls and both results survive together.
        let calls = res.messages[1]["tool_calls"].as_array().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(res.messages[2]["tool_call_id"], "c1");
        assert_eq!(res.messages[3]["tool_call_id"], "c2");
        assert_eq!(res.messages.len(), 5);
    }

    #[test]
    fn apply_model_summary_drops_orphan_tool_results() {
        // Tail deliberately starts on a tool result whose caller is gone.
        let msgs = tool_history();
        let res = apply_model_summary(&msgs, 4, "## Goal\nx", 4_000);
        assert!(res.repaired_dangling >= 2, "both orphan results must go");
        assert!(
            res.messages
                .iter()
                .all(|m| m.get("role").and_then(Value::as_str) != Some("tool")),
            "no orphan tool result may survive"
        );
    }

    #[test]
    fn transcript_render_is_bounded_and_keeps_the_first_message() {
        let mut msgs = vec![json!({"role":"user","content":"the original goal"})];
        for i in 0..40 {
            msgs.push(json!({"role":"assistant","content": format!("step {i} {}", "z".repeat(500))}));
        }
        let rendered = render_transcript_for_summary(&msgs, 4_000, 200);
        assert!(rendered.len() <= 4_000, "rendered {} chars", rendered.len());
        assert!(rendered.contains("the original goal"));
        assert!(rendered.contains("intermediate messages elided"));
        assert!(rendered.contains("step 39"), "most recent messages must survive");
    }

    #[test]
    fn transcript_render_includes_tool_calls_and_names() {
        let msgs = tool_history();
        let rendered = render_transcript_for_summary(&msgs, 100_000, 500);
        assert!(rendered.contains("calls: read("));
        assert!(rendered.contains("tool:read: a body"));
    }
}
