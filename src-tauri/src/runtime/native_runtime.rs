//! runtime/native_runtime.rs — Native Runtime（无 CLI 降级方案）
//!
//! 把现有 `assistant_stream_proxy.rs` 的 agentic loop 平移过来，实现 `AgentRuntime`。
//! 含 Protocol Adapter（OpenAI 兼容）+ Agent Loop + 自愈熔断。
//! Context Assembler / 步限 / doom loop 在后续切片补齐。

use super::{AgentRuntime, EventStream, RuntimeEvent, RuntimeStreamOptions};
use crate::assistant_executor;
use crate::commands::executor_settings::load_executor_settings;
use crate::module_manager::modules_root;
use crate::Result;
use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::json;
use tauri::Emitter;

pub struct NativeRuntime {
    app_handle: tauri::AppHandle,
}

impl NativeRuntime {
    pub fn new(app_handle: tauri::AppHandle) -> Self {
        Self { app_handle }
    }
}

#[async_trait]
impl AgentRuntime for NativeRuntime {
    fn id(&self) -> &'static str { "native" }
    fn display_name(&self) -> &'static str { "Native Runtime" }
    fn is_available(&self) -> bool { true } // 永远兜底可用

    async fn stream(&self, options: RuntimeStreamOptions) -> Result<EventStream> {
        // 解密 API key + base URL（P1 安全：全程 Rust 内存）
        let (api_key, base_url) = resolve_provider_credentials(&options.provider_id).await?;

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|e| crate::Error::Internal(format!("Failed to build HTTP client: {e}")))?;

        let app = self.app_handle.clone();
        let session_id = options.session_id.clone();
        let mut cancel_rx = options.abort_receiver;

        // 把 prompt 转为 messages（首轮 = user prompt；后续轮由 loop 内部累加）
        // Context Assembler 注入 system prompt（静态摘要 + 模块生成规约）
        let system_prompt = if let Some(wd) = options.working_directory.as_ref() {
            let ctx = crate::runtime::native::context_assembler::assemble(
                Some(wd), &options.prompt, 8000,
            ).await.ok();
            ctx.map(|c| c.system_prompt).unwrap_or_default()
        } else {
            String::new()
        };
        let mut round_messages: Vec<serde_json::Value> = vec![];
        if !system_prompt.is_empty() {
            round_messages.push(json!({ "role": "system", "content": system_prompt }));
        }
        round_messages.push(json!({ "role": "user", "content": options.prompt }));

        // 用 channel 把事件转成 Stream
        let (tx, rx) = tokio::sync::mpsc::channel::<RuntimeEvent>(64);

        tokio::spawn(async move {
            let exec_settings = load_executor_settings();
            let enabled_tools = exec_settings.enabled_tools;
            let max_self_heal: u32 = exec_settings.max_self_heal;
            let max_steps: u32 = exec_settings.max_steps.unwrap_or(50);
            let mut self_heal_count: u32 = 0;
            let mut step: u32 = 0;
            let mut last_tool_signature: Option<String> = None;
            let mut doom_count: u32 = 0;
            let modules_dir = modules_root();

            loop {
                // ── 步数上限（Q17 S1）──
                step += 1;
                if step > max_steps {
                    let _ = app.emit("assistant:stream_update", crate::assistant_stream_proxy::StreamPayload {
                        session_id: session_id.clone(),
                        done: true,
                        error: Some(format!("Max steps ({}) exceeded", max_steps)),
                        ..Default::default()
                    });
                    let _ = tx.send(RuntimeEvent::RunFailed { error: format!("max steps ({max_steps}) exceeded") }).await;
                    return;
                }
                let body = json!({
                    "model": options.model,
                    "messages": round_messages,
                    "stream": true,
                    "tools": assistant_executor::list_tools().iter().map(|t| json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.parameters,
                        }
                    })).collect::<Vec<_>>(),
                });

                let request = client
                    .post(format!("{}/v1/chat/completions", base_url.trim_end_matches('/')))
                    .header("Content-Type", "application/json")
                    .header("Authorization", format!("Bearer {}", api_key))
                    .json(&body);

                let response = tokio::select! {
                    biased;
                    _ = &mut cancel_rx => {
                        let _ = app.emit("assistant:stream_update", crate::assistant_stream_proxy::StreamPayload {
                            session_id: session_id.clone(),
                            done: true,
                            error: Some("Cancelled".into()),
                            ..Default::default()
                        });
                        let _ = tx.send(RuntimeEvent::RunCompleted { reason: "cancelled".into() }).await;
                        return;
                    }
                    result = request.send() => result
                };

                let response = match response {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = app.emit("assistant:stream_update", crate::assistant_stream_proxy::StreamPayload {
                            session_id: session_id.clone(),
                            done: true,
                            error: Some(format!("HTTP request failed: {e}")),
                            ..Default::default()
                        });
                        let _ = tx.send(RuntimeEvent::RunFailed { error: format!("HTTP: {e}") }).await;
                        return;
                    }
                };

                if !response.status().is_success() {
                    let status = response.status();
                    let body_text = response.text().await.unwrap_or_default();
                    let _ = app.emit("assistant:stream_update", crate::assistant_stream_proxy::StreamPayload {
                        session_id: session_id.clone(),
                        done: true,
                        error: Some(format!("API error {}: {}", status, body_text)),
                        ..Default::default()
                    });
                    let _ = tx.send(RuntimeEvent::RunFailed { error: format!("API {status}: {body_text}") }).await;
                    return;
                }

                let mut stream = response.bytes_stream();
                let mut buffer = String::new();
                let mut assistant_text = String::new();
                let mut accumulated_tool_calls: Vec<serde_json::Value> = Vec::new();

                while let Some(chunk_result) = stream.next().await {
                    if cancel_rx.try_recv().is_ok() {
                        let _ = app.emit("assistant:stream_update", crate::assistant_stream_proxy::StreamPayload {
                            session_id: session_id.clone(),
                            done: true,
                            error: Some("Cancelled".into()),
                            ..Default::default()
                        });
                        let _ = tx.send(RuntimeEvent::RunCompleted { reason: "cancelled".into() }).await;
                        return;
                    }

                    let chunk = match chunk_result {
                        Ok(c) => c,
                        Err(e) => {
                            let _ = app.emit("assistant:stream_update", crate::assistant_stream_proxy::StreamPayload {
                                session_id: session_id.clone(),
                                done: true,
                                error: Some(format!("Stream error: {e}")),
                                ..Default::default()
                            });
                            let _ = tx.send(RuntimeEvent::RunFailed { error: format!("stream: {e}") }).await;
                            return;
                        }
                    };

                    let chunk_str = String::from_utf8_lossy(&chunk);
                    buffer.push_str(&chunk_str);

                    while let Some(line_end) = buffer.find('\n') {
                        let line = buffer[..line_end].trim().to_string();
                        buffer = buffer[line_end + 1..].to_string();
                        if line.is_empty() || line.starts_with(':') { continue; }
                        let Some(data) = line.strip_prefix("data: ") else { continue; };
                        if data == "[DONE]" { continue; }
                        let Ok(parsed) = serde_json::from_str::<serde_json::Value>(data) else { continue; };

                        let reasoning = parsed["choices"][0]["delta"]["reasoning_content"]
                            .as_str().map(|s| s.to_string());

                        if let Some(delta) = parsed["choices"][0]["delta"]["content"].as_str() {
                            assistant_text.push_str(delta);
                            let (clean_delta, think_part) = crate::assistant_stream_proxy::extract_think_tag(delta);
                            let combined_reasoning = match (&reasoning, think_part) {
                                (Some(r), Some(t)) => Some(format!("{}{}", r, t)),
                                (Some(r), None) => Some(r.clone()),
                                (None, Some(t)) => Some(t),
                                (None, None) => None,
                            };
                            let _ = app.emit("assistant:stream_update", crate::assistant_stream_proxy::StreamPayload {
                                session_id: session_id.clone(),
                                delta: Some(clean_delta.clone()),
                                reasoning: combined_reasoning,
                                ..Default::default()
                            });
                            let _ = tx.send(RuntimeEvent::AssistantDelta { text: clean_delta }).await;
                        } else if let Some(r) = &reasoning {
                            let _ = app.emit("assistant:stream_update", crate::assistant_stream_proxy::StreamPayload {
                                session_id: session_id.clone(),
                                reasoning: Some(r.clone()),
                                ..Default::default()
                            });
                        }

                        // 累积 tool_calls
                        if let Some(tc_arr) = parsed["choices"][0]["delta"]["tool_calls"].as_array() {
                            for tc in tc_arr {
                                let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                                while accumulated_tool_calls.len() <= idx {
                                    accumulated_tool_calls.push(json!({
                                        "id": "", "function": { "name": "", "arguments": "" }
                                    }));
                                }
                                let slot = &mut accumulated_tool_calls[idx];
                                if let Some(id) = tc["id"].as_str() { slot["id"] = json!(id); }
                                if let Some(fn_obj) = tc.get("function") {
                                    if let Some(n) = fn_obj["name"].as_str() {
                                        slot["function"]["name"] = json!(n);
                                    }
                                    if let Some(a) = fn_obj["arguments"].as_str() {
                                        let prev = slot["function"]["arguments"].as_str().unwrap_or("").to_string();
                                        slot["function"]["arguments"] = json!(format!("{}{}", prev, a));
                                    }
                                }
                                let _ = app.emit("assistant:stream_update", crate::assistant_stream_proxy::StreamPayload {
                                    session_id: session_id.clone(),
                                    tool_call: Some(tc.to_string()),
                                    ..Default::default()
                                });
                            }
                        }
                    }
                }

                // 本轮 SSE 收完
                if accumulated_tool_calls.is_empty() {
                    let _ = app.emit("assistant:stream_update", crate::assistant_stream_proxy::StreamPayload {
                        session_id: session_id.clone(),
                        done: true,
                        ..Default::default()
                    });
                    let _ = tx.send(RuntimeEvent::RunCompleted { reason: "done".into() }).await;
                    return;
                }

                round_messages.push(json!({
                    "role": "assistant",
                    "content": assistant_text.clone(),
                    "tool_calls": accumulated_tool_calls
                }));

                let _ = app.emit("assistant:stream_update", crate::assistant_stream_proxy::StreamPayload {
                    session_id: session_id.clone(),
                    tool_status: Some("pending".into()),
                    tool_result: Some(json!({ "count": accumulated_tool_calls.len() })),
                    ..Default::default()
                });
                for tc in &accumulated_tool_calls {
                    let tool_name = tc["function"]["name"].as_str().unwrap_or("").to_string();
                    let tool_call_id = tc["id"].as_str().unwrap_or("").to_string();
                    let args = tc["function"]["arguments"].as_str().unwrap_or("{}").to_string();
                    let args_json = serde_json::from_str(&args).unwrap_or(json!({}));
                    let _ = tx.send(RuntimeEvent::ToolStarted {
                        tool_name,
                        tool_call_id,
                        args: args_json,
                    }).await;
                }

                let invocations: Vec<assistant_executor::ToolInvocation> = accumulated_tool_calls
                    .iter()
                    .filter_map(|tc| {
                        let id = tc["id"].as_str().unwrap_or("").to_string();
                        let name = tc["function"]["name"].as_str().unwrap_or("").to_string();
                        let args_str = tc["function"]["arguments"].as_str().unwrap_or("{}");
                        let arguments = serde_json::from_str(args_str).unwrap_or(json!({}));
                        if name.is_empty() { None } else {
                            Some(assistant_executor::ToolInvocation { id, name, arguments })
                        }
                    })
                    .collect();

                let results = assistant_executor::execute_batch(&invocations, &modules_dir, &enabled_tools);
                let failed_count = results.iter().filter(|r| r.status == "error").count() as u32;

                // ── Doom Loop 检测（Q17 D1：相同工具签名连续 3 次中断）──
                let sig = invocations.iter()
                    .map(|i| i.name.clone())
                    .collect::<Vec<_>>()
                    .join(",");
                if Some(&sig) == last_tool_signature.as_ref() {
                    doom_count += 1;
                    if doom_count >= 3 {
                        let _ = app.emit("assistant:stream_update", crate::assistant_stream_proxy::StreamPayload {
                            session_id: session_id.clone(),
                            done: true,
                            error: Some("Doom loop detected: same tool combination repeated 3 times".into()),
                            ..Default::default()
                        });
                        let _ = tx.send(RuntimeEvent::RunFailed { error: "doom loop detected".into() }).await;
                        return;
                    }
                } else {
                    doom_count = 0;
                    last_tool_signature = Some(sig);
                }

                for tr in &results {
                    let _ = app.emit("assistant:stream_update", crate::assistant_stream_proxy::StreamPayload {
                        session_id: session_id.clone(),
                        tool_status: Some(tr.status.clone()),
                        tool_result: Some(tr.output.clone()),
                        self_heal_count: if tr.status == "error" { Some(self_heal_count + failed_count) } else { Some(self_heal_count) },
                        ..Default::default()
                    });
                    let _ = tx.send(RuntimeEvent::ToolCompleted {
                        tool_call_id: tr.tool_call_id.clone(),
                        status: tr.status.clone(),
                        output: tr.output.clone(),
                    }).await;
                    round_messages.push(json!({
                        "role": "tool",
                        "tool_call_id": tr.tool_call_id,
                        "content": serde_json::to_string(&tr.output).unwrap_or_else(|_| "null".into())
                    }));
                }

                if failed_count > 0 {
                    self_heal_count += failed_count;
                    if assistant_executor::should_circuit_break(self_heal_count, max_self_heal) {
                        let _ = app.emit("assistant:stream_update", crate::assistant_stream_proxy::StreamPayload {
                            session_id: session_id.clone(),
                            done: true,
                            tool_status: Some("circuit_broken".into()),
                            self_heal_count: Some(self_heal_count),
                            error: Some(format!("Circuit broken after {} failed attempts", self_heal_count)),
                            ..Default::default()
                        });
                        let _ = tx.send(RuntimeEvent::RunFailed { error: format!("circuit broken after {self_heal_count}") }).await;
                        return;
                    }
                }
            }
            // tx dropped here ends the stream
        });

        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }

    fn interrupt(&self, session_id: &str) {
        // cancel_stream 是 async tauri command，这里同步触发——直接操作 STREAM_REGISTRY
        crate::assistant_stream_proxy::cancel_stream_sync(session_id);
    }

    fn dispose(&self) {}
}

/// 从 provider_id 解析出明文 API key + base URL（服务端解密，不下发前端）
async fn resolve_provider_credentials(provider_id: &str) -> Result<(String, String)> {
    let pool_conn = crate::db::get_main_conn()?;
    let conn: &rusqlite::Connection = &*pool_conn;

    let (api_key_encrypted, dek_encrypted, base_url): (String, Option<String>, String) = conn
        .query_row(
            "SELECT k.api_key_encrypted, k.dek_encrypted, p.base_url
             FROM provider_api_keys k
             JOIN user_providers p ON k.provider_id = p.id
             WHERE k.provider_id = ?1
             ORDER BY k.created_at ASC
             LIMIT 1",
            rusqlite::params![provider_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|e| crate::Error::Internal(format!("Failed to fetch provider key: {e}")))?;

    let api_key = if let Some(dek) = &dek_encrypted {
        if !dek.is_empty() {
            crate::provider_key_manager::envelope_decrypt(&api_key_encrypted, dek, conn)?
        } else {
            let encryption_key = crate::env_manager::get_encryption_key(conn)?;
            crate::env_manager::decrypt(&api_key_encrypted, &encryption_key)?
        }
    } else {
        let encryption_key = crate::env_manager::get_encryption_key(conn)?;
        crate::env_manager::decrypt(&api_key_encrypted, &encryption_key)?
    };

    // 消除未使用警告（asst_conn 持有 assistant DB 连接供后续扩展使用）
    Ok((api_key, base_url))
}
