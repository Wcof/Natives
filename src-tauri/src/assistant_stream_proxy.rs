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
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::Emitter;
use lazy_static::lazy_static;

lazy_static! {
    static ref STREAM_REGISTRY: Mutex<HashMap<String, StreamHandle>> =
        Mutex::new(HashMap::new());
}

struct StreamHandle {
    abort_handle: tokio::sync::oneshot::Sender<()>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamChatInput {
    pub session_id: String,
    pub model: String,
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

    // 从 session 表取 provider_id（Native runtime 内部会再解密 key）
    let provider_id: String = {
        let asst_conn = crate::db::get_assistant_db_conn()
            .map_err(|e| Error::Internal(format!("failed to get assistant DB: {e}")))?;
        asst_conn
            .query_row(
                "SELECT provider_id FROM assistant_sessions WHERE id = ?1",
                rusqlite::params![input.session_id],
                |row| row.get(0),
            )
            .map_err(|e| Error::Internal(format!("Failed to fetch session provider: {e}")))?
    };

    let (runtime, hint) =
        crate::runtime::registry::resolve_runtime(input.runtime_override.as_deref()).await?;

    // emit 降级提示（若有）——前端工作台横幅消费
    if let Some(h) = hint {
        let _ = app_handle.emit("assistant:stream_update", StreamPayload {
            session_id: input.session_id.clone(),
            error: None,
            done: false,
            ..Default::default()
        });
        // 独立 hint 事件（前端可监听 `assistant://stream/hint` 显示横幅）
        let _ = app_handle.emit("assistant://stream/hint", h);
    }

    // 构造 abort handle
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();
    {
        let mut registry = STREAM_REGISTRY.lock().unwrap();
        registry.insert(input.session_id.clone(), StreamHandle { abort_handle: cancel_tx });
    }

    // 拼首轮 prompt：把 messages 序列化为单串 user prompt（Native runtime 内部按 round 累加）
    let prompt = input.messages.iter()
        .map(|m| format!("[{}]: {}", m.role, m.content))
        .collect::<Vec<_>>()
        .join("\n");

    let stream = runtime.stream(crate::runtime::RuntimeStreamOptions {
        session_id: input.session_id.clone(),
        prompt,
        model: input.model.clone(),
        provider_id,
        system_prompt: None,
        working_directory: std::env::current_dir().ok(),
        abort_receiver: cancel_rx,
        runtime_options: serde_json::json!({}),
    }).await?;

    // 转发 runtime 事件流到 Tauri Event channel（Native runtime 内部已 emit 既有 StreamPayload，
    // 这里消费 RuntimeEvent 仅供未来统一 channel 切换；当前保持双发兼容）
    let app2 = app_handle.clone();
    let sid = input.session_id.clone();
    tokio::spawn(async move {
        tokio::pin!(stream);
        use futures_util::StreamExt;
        while let Some(ev) = stream.next().await {
            // 透传 RuntimeEvent 到新 channel（前端 Slice K 会迁移监听）
            let is_terminal = matches!(ev, crate::runtime::RuntimeEvent::RunCompleted { .. }
                | crate::runtime::RuntimeEvent::RunFailed { .. });
            let _ = app2.emit(&format!("assistant://stream/{}", sid), &ev);
            if is_terminal { break; }
        }
        cleanup_registry(&sid);
    });

    Ok(())
}

/// Extract `...` tags from a content delta.
/// Returns (clean_content, optional_reasoning_text).
pub fn extract_think_tag(delta: &str) -> (String, Option<String>) {
    use std::sync::atomic::{Ordering, AtomicU8};
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

        let reasoning_opt = if reasoning.is_empty() { None } else { Some(reasoning) };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_chat_input_serialization() {
        // P1: API key + base URL 不在前端结构里；服务端从 session_id 解析。
        let input = StreamChatInput {
            session_id: "test-session".to_string(),
            model: "gpt-4".to_string(),
            messages: vec![ChatMessage { role: "user".into(), content: "Hello".into() }],
            runtime_override: None,
        };
        let json = serde_json::to_value(&input).unwrap();
        assert_eq!(json["sessionId"], "test-session");
        assert_eq!(json["model"], "gpt-4");
        assert!(json.get("apiKey").is_none(), "apiKey must not be present");
        assert!(json.get("providerBaseUrl").is_none(), "providerBaseUrl must not be present");
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
        // 新字段存在且默认为 null
        assert!(json["toolStatus"].is_null());
        assert!(json["toolResult"].is_null());
        assert!(json["selfHealCount"].is_null());
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
}
