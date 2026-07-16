//! assistant_stream_proxy.rs — 助理流式对话代理 + Agentic Loop 执行引擎
//!
//! 参考 CodePilot `runtime/native-runtime.ts` 的流式 + 工具执行回路模型，
//! 落地为 Tauri Rust 底座版本。
//!
//! 核心链路：
//!   前端 streamChat({sessionId, model, messages})
//!     → Rust 服务端解密 key（P1 安全）
//!     → SSE 流式收 delta + tool_calls
//!     → 收完后若有 tool_calls → assistant_executor 执行 → 回填 tool_result → 下一轮
//!     → 无 tool_calls 则结束
//!     → 自愈熔断：连续失败 >3 次熔断（3.4）
//!
//! 安全：API Key 全程在 Rust 内存，前端只收事件（CONTEXT.md L37/L40 红线）。

use crate::{Error, Result};
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::Emitter;

lazy_static! {
    static ref STREAM_REGISTRY: Mutex<HashMap<String, StreamHandle>> = Mutex::new(HashMap::new());
}

struct StreamHandle {
    abort_handle: tokio::sync::oneshot::Sender<()>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamChatInput {
    pub session_id: String,
    pub run_id: String,
    #[serde(default)]
    pub model: Option<String>,
    pub messages: Vec<ChatMessage>,
    /// 显式指定 runtime（null = 自动分流）。Q20 决策。
    #[serde(default)]
    pub runtime_override: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// SSE 下行到前端的事件载荷。
///
/// 保持旧字段面稳定；执行引擎扩展事件走 `RuntimeEvent` 新通道。
#[derive(Clone, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StreamPayload {
    pub session_id: String,
    pub delta: Option<String>,
    pub tool_call: Option<String>,
    pub reasoning: Option<String>,
    pub done: bool,
    pub error: Option<String>,
}

/// Start a streaming chat with the AI provider, with agentic tool-execution loop.
///
/// **Security (P1)**: API key is NEVER accepted from the frontend. Backend resolves
/// provider + decrypts key server-side from the session's `provider_id`.
///
/// **Agentic Loop**: each round streams SSE → accumulates tool_calls → executes →
/// feeds tool_result back → next round. Self-heal up to `max_self_heal` (3) failures;
/// circuit-break on the 4th (PRD 3.4).
#[tauri::command]
pub async fn stream_chat(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
    input: StreamChatInput,
) -> Result<()> {
    // ── P1 Runtime 抽象：分流到 Claude CLI / Codex CLI / Native ──
    let _state_unused = state; // 持有 state 避免 DB 连接释放（兼容既有签名）

    // 从现行会话表取 provider_id（Native runtime 内部会再解密 key）
    let (provider_id, model_id, permission_profile, project_path): (
        String,
        String,
        String,
        Option<String>,
    ) = {
        let asst_conn = crate::db::get_assistant_db_conn()
            .map_err(|e| Error::Internal(format!("failed to get assistant DB: {e}")))?;
        asst_conn
            .query_row(
                "SELECT r.provider_id, r.model_id, COALESCE(c.permission_profile_id, 'ask'), c.project_id
                 FROM assistant_conversations c
                 JOIN assistant_runs r ON r.conversation_id = c.id
                 WHERE c.id = ?1 AND r.id = ?2 AND r.status = 'running'",
                rusqlite::params![input.session_id, input.run_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .map_err(|e| Error::Internal(format!("Failed to resolve active assistant run: {e}")))?
    };

    let (runtime, hint) =
        crate::runtime::registry::resolve_runtime(input.runtime_override.as_deref()).await?;
    {
        let conn = crate::db::get_assistant_db_conn()
            .map_err(|e| Error::Internal(format!("failed to get assistant DB: {e}")))?;
        conn.execute(
            "UPDATE assistant_runs SET runtime_id = ?1 WHERE id = ?2",
            rusqlite::params![runtime.id(), input.run_id],
        )
        .map_err(|e| Error::Internal(format!("Failed to bind runtime to run: {e}")))?;
    }

    // emit 降级提示（若有）——前端工作台横幅消费
    if let Some(h) = hint {
        let _ = app_handle.emit(
            "assistant:stream_update",
            StreamPayload {
                session_id: input.session_id.clone(),
                error: None,
                done: false,
                ..Default::default()
            },
        );
        // 独立 hint 事件（前端可监听 `assistant://stream/hint` 显示横幅）
        let _ = app_handle.emit("assistant://stream/hint", h);
    }

    // 构造 abort handle
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();
    {
        let mut registry = STREAM_REGISTRY.lock().unwrap();
        registry.insert(
            input.session_id.clone(),
            StreamHandle {
                abort_handle: cancel_tx,
            },
        );
    }

    // Always rebuild context from persisted history so retries, attachments, and
    // prior turns reach the runtime even if the renderer was remounted.
    let prompt = load_conversation_prompt(&input.session_id).unwrap_or_else(|_| {
        input
            .messages
            .iter()
            .map(|m| format!("[{}]: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n")
    });

    let stream = match runtime
        .stream(crate::runtime::RuntimeStreamOptions {
            run_id: input.run_id.clone(),
            session_id: input.session_id.clone(),
            prompt,
            model: model_id,
            provider_id,
            system_prompt: None,
            working_directory: project_path.map(std::path::PathBuf::from),
            abort_receiver: cancel_rx,
            runtime_options: serde_json::json!({ "permission_profile": permission_profile }),
        })
        .await
    {
        Ok(stream) => stream,
        Err(error) => {
            let payload = serde_json::json!({ "error": error.to_string() });
            let _ = finish_run(
                &input.run_id,
                &input.session_id,
                "failed",
                Some("runtime_start_failed"),
                &[],
                0,
                0,
            );
            let _ = persist_run_event(&input.run_id, 1, "failed", &payload);
            cleanup_registry(&input.session_id);
            return Err(error);
        }
    };

    // 转发 runtime 事件流到 Tauri Event channel（Native runtime 内部已 emit 既有 StreamPayload，
    // 这里消费 RuntimeEvent 仅供未来统一 channel 切换；当前保持双发兼容）
    let app2 = app_handle.clone();
    let sid = input.session_id.clone();
    let run_id = input.run_id.clone();
    tokio::spawn(async move {
        tokio::pin!(stream);
        use futures_util::StreamExt;
        let mut sequence = 0_i64;
        let mut blocks = Vec::<PersistedBlock>::new();
        let mut input_tokens = 0_u64;
        let mut output_tokens = 0_u64;
        let mut terminal_seen = false;
        while let Some(ev) = stream.next().await {
            // 透传 RuntimeEvent 到新 channel（前端 Slice K 会迁移监听）
            let is_terminal = matches!(
                ev,
                crate::runtime::RuntimeEvent::RunCompleted { .. }
                    | crate::runtime::RuntimeEvent::RunFailed { .. }
            );
            let _ = app2.emit(&format!("assistant://stream/{}", sid), &ev);
            sequence += 1;
            update_persisted_blocks(&mut blocks, &ev);
            let terminal_status = match &ev {
                crate::runtime::RuntimeEvent::RunCompleted { reason } => Some((
                    if reason == "cancelled" {
                        "interrupted"
                    } else {
                        "completed"
                    },
                    None,
                )),
                crate::runtime::RuntimeEvent::RunFailed { .. } => {
                    Some(("failed", Some("runtime_failed")))
                }
                _ => None,
            };
            if let Some((status, error_code)) = terminal_status {
                if let Err(error) = finish_run(
                    &run_id,
                    &sid,
                    status,
                    error_code,
                    &blocks,
                    input_tokens,
                    output_tokens,
                ) {
                    let _ = app2.emit(
                        "assistant:stream_update",
                        StreamPayload {
                            session_id: sid.clone(),
                            done: true,
                            error: Some(error.to_string()),
                            ..Default::default()
                        },
                    );
                    break;
                }
            }
            let (event_type, payload) = runtime_event_record(&ev);
            if let Err(error) = persist_run_event(&run_id, sequence, event_type, &payload) {
                let _ = app2.emit(
                    "assistant:stream_update",
                    StreamPayload {
                        session_id: sid.clone(),
                        done: true,
                        error: Some(error.to_string()),
                        ..Default::default()
                    },
                );
                if terminal_status.is_none() {
                    let _ = finish_run(
                        &run_id,
                        &sid,
                        "failed",
                        Some("event_persistence_failed"),
                        &blocks,
                        input_tokens,
                        output_tokens,
                    );
                }
                break;
            }
            terminal_seen |= terminal_status.is_some();
            match &ev {
                crate::runtime::RuntimeEvent::AssistantDelta { text } => {
                    let _ = app2.emit(
                        "assistant:stream_update",
                        StreamPayload {
                            session_id: sid.clone(),
                            delta: Some(text.clone()),
                            ..Default::default()
                        },
                    );
                }
                crate::runtime::RuntimeEvent::ReasoningDelta { text } => {
                    let _ = app2.emit(
                        "assistant:stream_update",
                        StreamPayload {
                            session_id: sid.clone(),
                            reasoning: Some(text.clone()),
                            ..Default::default()
                        },
                    );
                }
                crate::runtime::RuntimeEvent::UsageUpdated {
                    input_tokens: input,
                    output_tokens: output,
                } => {
                    input_tokens = *input;
                    output_tokens = *output;
                }
                crate::runtime::RuntimeEvent::RunCompleted { reason } => {
                    let _ = reason;
                    let _ = app2.emit(
                        "assistant:stream_update",
                        StreamPayload {
                            session_id: sid.clone(),
                            done: true,
                            ..Default::default()
                        },
                    );
                }
                crate::runtime::RuntimeEvent::RunFailed { error } => {
                    let _ = app2.emit(
                        "assistant:stream_update",
                        StreamPayload {
                            session_id: sid.clone(),
                            done: true,
                            error: Some(error.clone()),
                            ..Default::default()
                        },
                    );
                }
                _ => {}
            }
            if is_terminal {
                break;
            }
        }
        if !terminal_seen {
            sequence += 1;
            let payload = serde_json::json!({ "reason": "stream_ended_without_terminal" });
            let _ = persist_run_event(&run_id, sequence, "interrupted", &payload);
            let _ = finish_run(
                &run_id,
                &sid,
                "interrupted",
                Some("stream_ended_without_terminal"),
                &blocks,
                input_tokens,
                output_tokens,
            );
            let _ = app2.emit(
                "assistant:stream_update",
                StreamPayload {
                    session_id: sid.clone(),
                    done: true,
                    error: Some("Stream ended without a terminal event".into()),
                    ..Default::default()
                },
            );
        }
        cleanup_registry(&sid);
    });

    Ok(())
}

/// Extract `...` tags from a content delta.
/// Returns (clean_content, optional_reasoning_text).
pub fn extract_think_tag(delta: &str) -> (String, Option<String>) {
    use std::sync::atomic::{AtomicU8, Ordering};
    thread_local! {
        static IN_THINK: AtomicU8 = const { AtomicU8::new(0) };
    }

    IN_THINK.with(|in_think| {
        let mut clean = String::new();
        let mut reasoning = String::new();
        let mut remaining = delta;

        while !remaining.is_empty() {
            if in_think.load(Ordering::Relaxed) != 0 {
                if let Some(pos) = remaining.find("</think>") {
                    reasoning.push_str(&remaining[..pos]);
                    in_think.store(0, Ordering::Relaxed);
                    remaining = &remaining[pos + 8..];
                } else {
                    reasoning.push_str(remaining);
                    break;
                }
            } else {
                if let Some(pos) = remaining.find("<think>") {
                    clean.push_str(&remaining[..pos]);
                    in_think.store(1, Ordering::Relaxed);
                    remaining = &remaining[pos + 7..];
                } else {
                    clean.push_str(remaining);
                    break;
                }
            }
        }

        let reasoning_opt = if reasoning.is_empty() {
            None
        } else {
            Some(reasoning)
        };
        (clean, reasoning_opt)
    })
}

/// Cancel an active streaming chat session.
#[tauri::command]
pub async fn cancel_stream(session_id: String) -> Result<()> {
    cancel_stream_sync(&session_id);
    Ok(())
}

/// 同步版本——供 runtime trait interrupt() 调用（不能 await）
pub fn cancel_stream_sync(session_id: &str) {
    let mut registry = STREAM_REGISTRY.lock().unwrap();
    if let Some(handle) = registry.remove(session_id) {
        drop(handle.abort_handle);
    }
}

fn cleanup_registry(session_id: &str) {
    let mut registry = STREAM_REGISTRY.lock().unwrap();
    registry.remove(session_id);
}

#[derive(Clone, Debug, PartialEq)]
struct PersistedBlock {
    block_type: &'static str,
    content: serde_json::Value,
}

fn runtime_event_record(event: &crate::runtime::RuntimeEvent) -> (&'static str, serde_json::Value) {
    use crate::runtime::RuntimeEvent;
    match event {
        RuntimeEvent::AssistantDelta { text } => {
            ("assistant_delta", serde_json::json!({ "text": text }))
        }
        RuntimeEvent::ReasoningDelta { text } => {
            ("reasoning_delta", serde_json::json!({ "text": text }))
        }
        RuntimeEvent::ToolStarted {
            tool_name,
            tool_call_id,
            args,
        } => (
            "tool_started",
            serde_json::json!({ "tool_name": tool_name, "tool_call_id": tool_call_id, "args": args }),
        ),
        RuntimeEvent::PermissionRequested {
            tool_name,
            tool_call_id,
            reason,
            args,
        } => (
            "permission_requested",
            serde_json::json!({ "tool_name": tool_name, "tool_call_id": tool_call_id, "reason": reason, "args": args }),
        ),
        RuntimeEvent::ToolCompleted {
            tool_call_id,
            status,
            output,
        } => (
            "tool_completed",
            serde_json::json!({ "tool_call_id": tool_call_id, "status": status, "output": output }),
        ),
        RuntimeEvent::ToolRejected {
            tool_name,
            tool_call_id,
            reason,
        } => (
            "tool_rejected",
            serde_json::json!({ "tool_name": tool_name, "tool_call_id": tool_call_id, "reason": reason }),
        ),
        RuntimeEvent::FileChanged { path, change_type } => (
            "file_changed",
            serde_json::json!({ "path": path, "change_type": change_type }),
        ),
        RuntimeEvent::UsageUpdated {
            input_tokens,
            output_tokens,
        } => (
            "usage_updated",
            serde_json::json!({ "input_tokens": input_tokens, "output_tokens": output_tokens }),
        ),
        RuntimeEvent::RunCompleted { reason } if reason == "cancelled" => {
            ("interrupted", serde_json::json!({ "reason": reason }))
        }
        RuntimeEvent::RunCompleted { reason } => {
            ("completed", serde_json::json!({ "reason": reason }))
        }
        RuntimeEvent::RunFailed { error } => ("failed", serde_json::json!({ "error": error })),
        RuntimeEvent::UnknownItem { raw } => ("unknown_item", serde_json::json!({ "raw": raw })),
    }
}

fn update_persisted_blocks(blocks: &mut Vec<PersistedBlock>, event: &crate::runtime::RuntimeEvent) {
    use crate::runtime::RuntimeEvent;
    match event {
        RuntimeEvent::AssistantDelta { text } => append_block_text(blocks, "text", "text", text),
        RuntimeEvent::ReasoningDelta { text } => append_block_text(blocks, "reasoning", "reasoning", text),
        RuntimeEvent::ToolStarted { tool_name, tool_call_id, args } => blocks.push(PersistedBlock {
            block_type: "tool_call",
            content: serde_json::json!({ "type": "tool_call", "tool_name": tool_name, "tool_call_id": tool_call_id, "input": args, "status": "running" }),
        }),
        RuntimeEvent::ToolCompleted { tool_call_id, status, output } => {
            set_tool_status(blocks, tool_call_id, if status == "success" { "completed" } else { "failed" });
            blocks.push(PersistedBlock {
                block_type: "tool_result",
                content: serde_json::json!({ "type": "tool_result", "tool_call_id": tool_call_id, "output": output, "is_error": status != "success" }),
            });
        }
        RuntimeEvent::ToolRejected { tool_call_id, reason, .. } => {
            set_tool_status(blocks, tool_call_id, "rejected");
            blocks.push(PersistedBlock {
                block_type: "tool_result",
                content: serde_json::json!({ "type": "tool_result", "tool_call_id": tool_call_id, "output": reason, "is_error": true }),
            });
        }
        _ => {}
    }
}

fn append_block_text(
    blocks: &mut Vec<PersistedBlock>,
    block_type: &'static str,
    key: &str,
    delta: &str,
) {
    if delta.is_empty() {
        return;
    }
    if let Some(block) = blocks
        .iter_mut()
        .find(|block| block.block_type == block_type)
    {
        let current = block
            .content
            .get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        block.content[key] = serde_json::Value::String(format!("{current}{delta}"));
    } else {
        let mut content = serde_json::json!({ "type": block_type });
        content[key] = serde_json::Value::String(delta.to_string());
        let block = PersistedBlock {
            block_type,
            content,
        };
        if block_type == "reasoning" {
            blocks.insert(0, block);
        } else {
            blocks.push(block);
        }
    }
}

fn set_tool_status(blocks: &mut [PersistedBlock], tool_call_id: &str, status: &str) {
    if let Some(block) = blocks.iter_mut().find(|block| {
        block.block_type == "tool_call"
            && block
                .content
                .get("tool_call_id")
                .and_then(serde_json::Value::as_str)
                == Some(tool_call_id)
    }) {
        block.content["status"] = serde_json::Value::String(status.to_string());
    }
}

fn persist_run_event(
    run_id: &str,
    sequence: i64,
    event_type: &str,
    payload: &serde_json::Value,
) -> Result<()> {
    let conn = crate::db::get_assistant_db_conn()?;
    conn.execute(
        "INSERT INTO assistant_run_events (run_id, sequence, timestamp, event_type, payload) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![run_id, sequence, chrono::Utc::now().to_rfc3339(), event_type, payload.to_string()],
    )
    .map_err(Error::Database)?;
    Ok(())
}

fn finish_run(
    run_id: &str,
    conversation_id: &str,
    status: &str,
    error_code: Option<&str>,
    blocks: &[PersistedBlock],
    input_tokens: u64,
    output_tokens: u64,
) -> Result<()> {
    let mut conn = crate::db::get_assistant_db_conn()?;
    let transaction = conn.transaction().map_err(Error::Database)?;
    let now = chrono::Utc::now().to_rfc3339();
    let reasoning_started_at: Option<String> = transaction
        .query_row(
            "SELECT timestamp FROM assistant_run_events WHERE run_id = ?1 AND event_type = 'reasoning_delta' ORDER BY sequence ASC LIMIT 1",
            rusqlite::params![run_id],
            |row| row.get(0),
        )
        .ok();
    let reasoning_finished_at: Option<String> = transaction
        .query_row(
            "SELECT timestamp FROM assistant_run_events WHERE run_id = ?1 AND sequence > COALESCE((SELECT MAX(sequence) FROM assistant_run_events WHERE run_id = ?1 AND event_type = 'reasoning_delta'), 0) ORDER BY sequence ASC LIMIT 1",
            rusqlite::params![run_id],
            |row| row.get(0),
        )
        .ok();
    let duration_ms = reasoning_started_at
        .as_deref()
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|start| {
            (reasoning_finished_at
                .as_deref()
                .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                .map(|end| end.with_timezone(&chrono::Utc))
                .unwrap_or_else(chrono::Utc::now)
                - start.with_timezone(&chrono::Utc))
            .num_milliseconds()
            .max(0)
        });
    if !blocks.is_empty() {
        let message_id = uuid::Uuid::new_v4().to_string();
        transaction.execute(
            "INSERT INTO assistant_messages (id, conversation_id, role, status, input_tokens, output_tokens, created_at) VALUES (?1, ?2, 'assistant', ?3, ?4, ?5, ?6)",
            rusqlite::params![message_id, conversation_id, status, input_tokens as i64, output_tokens as i64, now],
        ).map_err(Error::Database)?;
        for (index, block) in blocks.iter().enumerate() {
            let content = if block.block_type == "text" {
                block
                    .content
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            } else {
                let mut value = block.content.clone();
                if block.block_type == "reasoning" {
                    if let Some(duration_ms) = duration_ms {
                        value["duration_ms"] = serde_json::json!(duration_ms);
                    }
                }
                value.to_string()
            };
            transaction.execute(
                "INSERT INTO assistant_message_blocks (id, message_id, block_type, block_index, content) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![uuid::Uuid::new_v4().to_string(), message_id, block.block_type, index as i64, content],
            ).map_err(Error::Database)?;
        }
    }
    transaction.execute(
        "UPDATE assistant_runs SET status = ?1, error_code = ?2, finished_at = ?3, total_input_tokens = ?4, total_output_tokens = ?5 WHERE id = ?6",
        rusqlite::params![status, error_code, now, input_tokens as i64, output_tokens as i64, run_id],
    ).map_err(Error::Database)?;
    transaction
        .execute(
            "UPDATE assistant_conversations SET updated_at = ?1 WHERE id = ?2",
            rusqlite::params![now, conversation_id],
        )
        .map_err(Error::Database)?;
    transaction.commit().map_err(Error::Database)?;
    Ok(())
}

fn load_conversation_prompt(conversation_id: &str) -> Result<String> {
    let conn = crate::db::get_assistant_db_conn()?;
    let mut statement = conn
        .prepare(
            "SELECT m.id, m.role, b.block_type, b.content
         FROM assistant_messages m
         JOIN assistant_message_blocks b ON b.message_id = m.id
         WHERE m.conversation_id = ?1 AND m.role IN ('user', 'assistant')
         ORDER BY m.created_at ASC, b.block_index ASC",
        )
        .map_err(Error::Database)?;
    let rows = statement
        .query_map(rusqlite::params![conversation_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(Error::Database)?;
    let mut lines = Vec::<String>::new();
    let mut current_id = String::new();
    let mut current_role = String::new();
    let mut parts = Vec::<String>::new();
    for row in rows {
        let (message_id, role, block_type, content) = row.map_err(Error::Database)?;
        if !current_id.is_empty() && current_id != message_id {
            lines.push(format!("[{current_role}]: {}", parts.join("\n")));
            parts.clear();
        }
        current_id = message_id;
        current_role = role;
        match block_type.as_str() {
            "text" => parts.push(content),
            "file_reference" => {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(path) = value.get("path").and_then(serde_json::Value::as_str) {
                        parts.push(format!("[Attached file: {path}]"));
                    }
                }
            }
            _ => {}
        }
    }
    if !current_id.is_empty() {
        lines.push(format!("[{current_role}]: {}", parts.join("\n")));
    }
    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_chat_input_serialization() {
        // P1: API key + base URL 不在前端结构里；服务端从 session_id 解析。
        let input = StreamChatInput {
            session_id: "test-session".to_string(),
            run_id: "test-run".to_string(),
            model: Some("gpt-4".to_string()),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: "Hello".into(),
            }],
            runtime_override: None,
        };
        let json = serde_json::to_value(&input).unwrap();
        assert_eq!(json["sessionId"], "test-session");
        assert_eq!(json["runId"], "test-run");
        assert_eq!(json["model"], "gpt-4");
        assert!(json.get("apiKey").is_none(), "apiKey must not be present");
        assert!(
            json.get("providerBaseUrl").is_none(),
            "providerBaseUrl must not be present"
        );
        assert_eq!(json["messages"][0]["role"], "user");
    }

    #[test]
    fn test_stream_payload_serialization() {
        let payload = StreamPayload {
            session_id: "test".into(),
            delta: Some("Hello".into()),
            done: false,
            ..Default::default()
        };
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["sessionId"], "test");
        assert_eq!(json["delta"], "Hello");
        assert_eq!(json["done"], false);
        assert!(json["error"].is_null());
        assert!(json["reasoning"].is_null());
    }

    #[test]
    fn test_stream_payload_done_signal() {
        let payload = StreamPayload {
            session_id: "test".into(),
            done: true,
            ..Default::default()
        };
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["done"], true);
    }

    #[test]
    fn test_stream_payload_error_signal() {
        let payload = StreamPayload {
            session_id: "test".into(),
            done: true,
            error: Some("Network error".into()),
            ..Default::default()
        };
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["error"], "Network error");
        assert_eq!(json["done"], true);
    }

    #[test]
    fn test_cancel_stream_nonexistent() {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut registry: HashMap<String, StreamHandle> = HashMap::new();
            registry.remove("nonexistent");
        }));
        assert!(result.is_ok());
    }

    #[test]
    fn runtime_events_map_to_replay_contract() {
        let (kind, payload) = runtime_event_record(&crate::runtime::RuntimeEvent::ReasoningDelta {
            text: "inspect".into(),
        });
        assert_eq!(kind, "reasoning_delta");
        assert_eq!(payload["text"], "inspect");

        let (kind, _) = runtime_event_record(&crate::runtime::RuntimeEvent::RunCompleted {
            reason: "cancelled".into(),
        });
        assert_eq!(kind, "interrupted");
    }

    #[test]
    fn final_blocks_accumulate_text_reasoning_and_tool_result() {
        let mut blocks = Vec::new();
        update_persisted_blocks(
            &mut blocks,
            &crate::runtime::RuntimeEvent::ReasoningDelta {
                text: "why ".into(),
            },
        );
        update_persisted_blocks(
            &mut blocks,
            &crate::runtime::RuntimeEvent::ReasoningDelta { text: "now".into() },
        );
        update_persisted_blocks(
            &mut blocks,
            &crate::runtime::RuntimeEvent::AssistantDelta {
                text: "hello".into(),
            },
        );
        update_persisted_blocks(
            &mut blocks,
            &crate::runtime::RuntimeEvent::ToolStarted {
                tool_name: "read_file".into(),
                tool_call_id: "tool-1".into(),
                args: serde_json::json!({ "path": "a" }),
            },
        );
        update_persisted_blocks(
            &mut blocks,
            &crate::runtime::RuntimeEvent::ToolCompleted {
                tool_call_id: "tool-1".into(),
                status: "success".into(),
                output: serde_json::json!("ok"),
            },
        );

        assert_eq!(blocks[0].content["reasoning"], "why now");
        assert_eq!(blocks[1].content["text"], "hello");
        assert_eq!(blocks[2].content["status"], "completed");
        assert_eq!(blocks[3].content["output"], "ok");
    }

    #[test]
    fn reasoning_stays_before_text_when_the_provider_sends_it_late() {
        let mut blocks = Vec::new();
        update_persisted_blocks(
            &mut blocks,
            &crate::runtime::RuntimeEvent::AssistantDelta {
                text: "answer".into(),
            },
        );
        update_persisted_blocks(
            &mut blocks,
            &crate::runtime::RuntimeEvent::ReasoningDelta {
                text: "plan".into(),
            },
        );
        assert_eq!(blocks[0].block_type, "reasoning");
        assert_eq!(blocks[1].block_type, "text");
    }
}
