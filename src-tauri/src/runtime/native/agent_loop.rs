//! runtime/native/agent_loop.rs — Agentic Loop 状态机（增强版）
//!
//! 从旧版 native_runtime.rs 巨函数中提取的独立 Agent Loop 状态机。
//! 职责单一：管理 AI ↔ 工具执行的迭代循环。
//!
//! 增强特性（对标 Claude Code Agent Loop）：
//!   - 完整状态机：Idle → Thinking → ToolExec → Observing → (loop) → Done
//!   - Doom Loop 检测：检测重复工具调用模式
//!   - 循环检测：检测思维重复
//!   - 自愈熔断：连续失败超限后熔断
//!   - 流式输出：实时 delta + tool_call 推送
//!   - 中断支持：通过 cancel_rx 随时中断

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use crate::runtime::RuntimeEvent;
use crate::runtime::native::capability::{
    CancellationToken, CapabilityContext, CapabilityExecutor, CapabilityRegistry, CapabilityRequest,
    PermissionEngine, PermissionMode,
};
use crate::runtime::native::hook_pipeline::HookPipeline;
use crate::runtime::native::rule_engine::RuleEngine;
use crate::runtime::native::stream_provider::{SseClient, OpenAiDeltaConsumer, ConsumeResult, DeltaConsumer};
use serde_json::json;
use tauri::Emitter;
use tokio::sync::mpsc;

// ──────────────────────────────────────────────
// 状态机定义
// ──────────────────────────────────────────────

/// Agent Loop 状态
#[derive(Clone, Debug, PartialEq)]
pub enum AgentState {
    Idle,
    Thinking,
    ToolExec,
    Observing,
    Done,
    Failed(String),
}

/// Agent Loop 配置
pub struct LoopConfig {
    pub max_steps: u32,
    pub max_self_heal: u32,
    pub doom_threshold: u32,
    pub model: String,
    pub base_url: String,
    pub api_key: String,
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            max_steps: 50,
            max_self_heal: 3,
            doom_threshold: 3,
            model: String::new(),
            base_url: String::new(),
            api_key: String::new(),
        }
    }
}

// ──────────────────────────────────────────────
// Agent Loop
// ──────────────────────────────────────────────

/// 工具调用历史（用于循环检测）
#[derive(Clone, Debug)]
struct ToolHistoryEntry {
    step: u32,
    tool_names: Vec<String>,
    success: bool,
}

/// Agentic Loop 执行器（增强版）
pub struct AgentLoop {
    config: LoopConfig,
    state: AgentState,
    round_messages: Vec<serde_json::Value>,
    step: u32,
    self_heal_count: u32,
    doom_count: u32,
    /// 上次工具调用签名（用于 doom loop 检测）
    last_tool_signature: Option<String>,
    /// 工具调用历史（用于模式检测）
    tool_history: VecDeque<ToolHistoryEntry>,
    /// 历史签名集合（用于全局循环检测）
    seen_signatures: HashSet<String>,
    /// 相同文本内容计数（用于思维循环检测）
    repeated_content_count: u32,
    last_assistant_content: Option<String>,
}

impl AgentLoop {
    /// 创建新的 Agent Loop，注入初始 messages
    pub fn new(config: LoopConfig, initial_messages: Vec<serde_json::Value>) -> Self {
        Self {
            config,
            state: AgentState::Idle,
            round_messages: initial_messages,
            step: 0,
            self_heal_count: 0,
            doom_count: 0,
            last_tool_signature: None,
            tool_history: VecDeque::with_capacity(20),
            seen_signatures: HashSet::new(),
            repeated_content_count: 0,
            last_assistant_content: None,
        }
    }

    /// 获取当前状态
    pub fn state(&self) -> &AgentState {
        &self.state
    }

    /// 获取当前轮次
    pub fn step(&self) -> u32 {
        self.step
    }

    /// 获取 heal 计数
    pub fn self_heal_count(&self) -> u32 {
        self.self_heal_count
    }

    /// 构建工具队列的签名（按名称排序，避免顺序不同导致的误判）
    fn build_tool_signature(invocations: &[crate::assistant_executor::ToolInvocation]) -> String {
        let mut names: Vec<&str> = invocations.iter().map(|i| i.name.as_str()).collect();
        names.sort(); // 排序后拼接，避免顺序敏感性
        names.join(",")
    }

    /// 检测思维循环 — 检查助手回复是否在重复相同模式
    fn detect_content_loop(&mut self, content: &str) -> bool {
        if content.is_empty() {
            return false;
        }

        // 取内容的前 100 个字符作为指纹
        let fingerprint: String = content.chars().take(100).collect();

        if let Some(last) = &self.last_assistant_content {
            if *last == fingerprint {
                self.repeated_content_count += 1;
                if self.repeated_content_count >= 2 {
                    return true;
                }
            } else {
                self.repeated_content_count = 0;
            }
        }

        self.last_assistant_content = Some(fingerprint);
        false
    }

    /// 运行完整的 agentic loop，通过 tx 发送事件
    pub async fn run(
        &mut self,
        tx: &mpsc::Sender<RuntimeEvent>,
        app_handle: &tauri::AppHandle,
        session_id: &str,
        modules_dir: &Path,
        working_dir: Option<PathBuf>,
        enabled_tools: &std::collections::HashMap<String, bool>,
        hook_pipeline: &HookPipeline,
        rule_engine: &RuleEngine,
        cancel_rx: &mut tokio::sync::oneshot::Receiver<()>,
    ) {
        self.state = AgentState::Thinking;

        // 构建注册表（每次刷新，确保插件注册的能力可用）
        let registry = {
            let mut reg = CapabilityRegistry::new();
            for cap in crate::runtime::native::capability::create_default_capabilities(modules_dir) {
                reg.register(cap);
            }
            for (name, enabled) in enabled_tools {
                reg.set_enabled(name, *enabled);
            }
            reg
        };

        // 构建工具列表（OpenAI function-calling 格式）
        let tools_json: Vec<serde_json::Value> = registry.list_visible_tools().iter().map(|meta| json!({
            "type": "function",
            "function": {
                "name": meta.name,
                "description": meta.description,
                "parameters": meta.parameters,
            }
        })).collect();

        // 创建 SSE 客户端
        let sse_client = match SseClient::new() {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(RuntimeEvent::RunFailed { error: e.to_string() }).await;
                self.state = AgentState::Failed(e.to_string());
                return;
            }
        };

        // ── 主循环 ──
        loop {
            // 中断检查
            if cancel_rx.try_recv().is_ok() {
                self.emit_cancelled(tx, app_handle, session_id).await;
                self.state = AgentState::Done;
                return;
            }

            // 步数检查
            self.step += 1;
            if self.step > self.config.max_steps {
                let msg = format!("Max steps ({}) exceeded", self.config.max_steps);
                self.emit_complete(tx, app_handle, session_id, &msg).await;
                self.state = AgentState::Failed(msg);
                return;
            }

            // ── Thinking: 调用 LLM ──
            self.state = AgentState::Thinking;

            let response = match sse_client.stream_chat(
                &self.config.base_url,
                &self.config.api_key,
                &self.config.model,
                &self.round_messages,
                &tools_json,
                cancel_rx,
            ).await {
                Ok(r) => r,
                Err(e) => {
                    self.emit_failed(tx, app_handle, session_id, &e).await;
                    self.state = AgentState::Failed(e);
                    return;
                }
            };

            // ── 消费 SSE 流 ──
            let mut consumer = OpenAiDeltaConsumer::new();
            let mut assistant_text = String::new();

            let mut stream = response.bytes_stream();
            let mut buffer = String::new();

            use futures_util::StreamExt;
            let mut any_tool_call = false;

            while let Some(chunk_result) = stream.next().await {
                // 中断检查
                if cancel_rx.try_recv().is_ok() {
                    self.emit_cancelled(tx, app_handle, session_id).await;
                    self.state = AgentState::Done;
                    return;
                }

                let chunk = match chunk_result {
                    Ok(c) => c,
                    Err(e) => {
                        self.emit_failed(tx, app_handle, session_id, &format!("Stream error: {e}")).await;
                        self.state = AgentState::Failed(format!("Stream error: {e}"));
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

                    let events = consumer.consume(data);
                    for event in events {
                        match event {
                            ConsumeResult::Delta(text) => {
                                let (clean_delta, _) = crate::assistant_stream_proxy::extract_think_tag(&text);
                                assistant_text.push_str(&clean_delta);
                                let _ = tx.send(RuntimeEvent::AssistantDelta { text: clean_delta }).await;
                            }
                            ConsumeResult::Reasoning(r) => {
                                let _ = app_handle.emit("assistant:stream_update",
                                    crate::assistant_stream_proxy::StreamPayload {
                                        session_id: session_id.to_string(),
                                        reasoning: Some(r),
                                        ..Default::default()
                                    });
                            }
                            ConsumeResult::ToolCall(tc) => {
                                any_tool_call = true;
                                let _ = tx.send(RuntimeEvent::ToolStarted {
                                    tool_name: tc.name.clone(),
                                    tool_call_id: tc.id.clone(),
                                    args: tc.arguments.clone(),
                                }).await;
                            }
                            ConsumeResult::Done => {
                                // 由下层循环处理
                            }
                            ConsumeResult::Skip => {}
                        }
                    }
                }
            }

            // 检查是否触发了 tool_call（consumer 中可能还有未 drain 的）
            let remaining_tools = consumer.drain_tools();
            if !remaining_tools.is_empty() {
                any_tool_call = true;
            }

            // ── 思维循环检测 ──
            if !assistant_text.is_empty() && self.detect_content_loop(&assistant_text) {
                let msg = "Content loop detected: assistant is repeating the same response pattern";
                eprintln!("[AgentLoop] {msg}");
                // 注入系统消息打破循环，而不是直接失败
                self.round_messages.push(json!({
                    "role": "system",
                    "content": "You appear to be repeating yourself. Please try a different approach."
                }));
                continue;
            }

            // ── 没有 tool_call → 完成 ──
            if !any_tool_call {
                self.emit_complete(tx, app_handle, session_id, "done").await;
                self.state = AgentState::Done;
                return;
            }

            // ── ToolExec: 执行工具调用 ──
            self.state = AgentState::ToolExec;

            // 构建 ToolInvocation 列表
            let invocations: Vec<crate::assistant_executor::ToolInvocation> = remaining_tools
                .iter()
                .map(|tc| crate::assistant_executor::ToolInvocation {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    arguments: tc.arguments.clone(),
                })
                .collect();

            if invocations.is_empty() {
                self.emit_complete(tx, app_handle, session_id, "done").await;
                self.state = AgentState::Done;
                return;
            }

            // 将 assistant 消息加入上下文
            self.round_messages.push(json!({
                "role": "assistant",
                "content": assistant_text,
                "tool_calls": remaining_tools.iter().map(|tc| json!({
                    "id": tc.id,
                    "type": "function",
                    "function": { "name": tc.name, "arguments": tc.arguments.to_string() }
                })).collect::<Vec<_>>(),
            }));

            // ── Doom Loop 检测（相同工具组合重复执行） ──
            let sig = Self::build_tool_signature(&invocations);
            if Some(&sig) == self.last_tool_signature.as_ref() {
                self.doom_count += 1;
                if self.doom_count >= self.config.doom_threshold {
                    let msg = format!(
                        "Doom loop detected: same tool combination '{}' repeated {} times",
                        sig, self.doom_count + 1
                    );
                    eprintln!("[AgentLoop] {}", crate::log_sanitizer::sanitize(&msg));
                    self.emit_failed(tx, app_handle, session_id, &msg).await;
                    self.state = AgentState::Failed(msg);
                    return;
                }
            } else {
                self.doom_count = 0;
                self.last_tool_signature = Some(sig.clone());
            }

            // ── 全局循环检测（曾经见过这个签名） ──
            if !self.seen_signatures.insert(sig.clone()) {
                // 重复出现历史签名（工具组合曾出现过）
                // 注：不一定是问题，但值得记录
                eprintln!("[AgentLoop] Repeated tool signature detected: {}", crate::log_sanitizer::sanitize(&sig));
            }

            // ── 记录工具历史 ──
            self.tool_history.push_back(ToolHistoryEntry {
                step: self.step,
                tool_names: invocations.iter().map(|i| i.name.clone()).collect(),
                success: false, // 后续更新
            });
            if self.tool_history.len() > 20 {
                self.tool_history.pop_front();
            }

            // ── 通过 CapabilityExecutor 执行工具 ──
            let requests: Vec<CapabilityRequest> = invocations.iter().map(|inv| CapabilityRequest {
                call_id: inv.id.clone(),
                name: inv.name.clone(),
                arguments: inv.arguments.clone(),
                working_dir: working_dir.clone(),
            }).collect();

            let context = CapabilityContext {
                session_id: session_id.to_string(),
                working_dir: working_dir.clone(),
                project_root: working_dir.clone(),
                source: "native-agent-loop".into(),
                permission_mode: PermissionMode::Allow,
            };
            let cancellation = CancellationToken::new();
            let executor = CapabilityExecutor::new(&registry, PermissionEngine::default());
            let mut results = Vec::with_capacity(requests.len());
            for request in &requests {
                results.push(
                    executor
                        .execute(
                            request,
                            &context,
                            Some(hook_pipeline),
                            Some(rule_engine),
                            &cancellation,
                        )
                        .await,
                );
            }

            let failed_count = results.iter().filter(|r| {
                matches!(r.status, crate::runtime::native::capability::CapabilityStatus::Error
                    | crate::runtime::native::capability::CapabilityStatus::Rejected
                    | crate::runtime::native::capability::CapabilityStatus::CircuitBroken)
            }).count() as u32;

            // 更新工具历史
            if let Some(entry) = self.tool_history.back_mut() {
                entry.success = failed_count == 0;
            }

            // ── Observing: 注入结果 ──
            self.state = AgentState::Observing;

            for tr in &results {
                let status_str = match &tr.status {
                    crate::runtime::native::capability::CapabilityStatus::Success => "success",
                    crate::runtime::native::capability::CapabilityStatus::Error => "error",
                    crate::runtime::native::capability::CapabilityStatus::Rejected => "rejected",
                    crate::runtime::native::capability::CapabilityStatus::CircuitBroken => "circuit_broken",
                    crate::runtime::native::capability::CapabilityStatus::TimedOut => "timed_out",
                };

                let _ = tx.send(RuntimeEvent::ToolCompleted {
                    tool_call_id: tr.call_id.clone(),
                    status: status_str.into(),
                    output: tr.output.clone(),
                }).await;
                if matches!(tr.status, crate::runtime::native::capability::CapabilityStatus::Rejected) {
                    let tool_name = invocations
                        .iter()
                        .find(|inv| inv.id == tr.call_id)
                        .map(|inv| inv.name.clone())
                        .unwrap_or_else(|| "unknown".into());
                    let reason = tr.output.get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Tool rejected")
                        .to_string();
                    let _ = tx.send(RuntimeEvent::ToolRejected {
                        tool_name,
                        tool_call_id: tr.call_id.clone(),
                        reason,
                    }).await;
                }

                // 工具结果注入（OpenAI 格式）
                let content = if tr.output.is_object() || tr.output.is_array() {
                    serde_json::to_string(&tr.output).unwrap_or_else(|_| "null".into())
                } else {
                    tr.output.as_str().unwrap_or("null").to_string()
                };

                self.round_messages.push(json!({
                    "role": "tool",
                    "tool_call_id": tr.call_id,
                    "content": content,
                }));
            }

            // ── 熔断检查 ──
            if failed_count > 0 {
                self.self_heal_count += failed_count;
                if self.self_heal_count >= self.config.max_self_heal {
                    let msg = format!("Circuit broken after {} failed attempts", self.self_heal_count);
                    eprintln!("[AgentLoop] {msg}");
                    self.emit_failed(tx, app_handle, session_id, &msg).await;
                    self.state = AgentState::Failed(msg);
                    return;
                }
            }

            // 回到 Thinking 状态继续下一轮
        }
    }

    // ── 辅助发射方法 ──

    async fn emit_complete(
        &self, tx: &mpsc::Sender<RuntimeEvent>,
        app_handle: &tauri::AppHandle,
        session_id: &str,
        reason: &str,
    ) {
        let _ = app_handle.emit("assistant:stream_update",
            crate::assistant_stream_proxy::StreamPayload {
                session_id: session_id.to_string(),
                done: true,
                error: if reason != "done" { Some(reason.to_string()) } else { None },
                ..Default::default()
            });
        let _ = tx.send(RuntimeEvent::RunCompleted { reason: reason.into() }).await;
    }

    async fn emit_failed(
        &self, tx: &mpsc::Sender<RuntimeEvent>,
        app_handle: &tauri::AppHandle,
        session_id: &str,
        error: &str,
    ) {
        let _ = app_handle.emit("assistant:stream_update",
            crate::assistant_stream_proxy::StreamPayload {
                session_id: session_id.to_string(),
                done: true,
                error: Some(error.to_string()),
                ..Default::default()
            });
        let _ = tx.send(RuntimeEvent::RunFailed { error: error.into() }).await;
    }

    async fn emit_cancelled(
        &self, tx: &mpsc::Sender<RuntimeEvent>,
        app_handle: &tauri::AppHandle,
        session_id: &str,
    ) {
        let _ = app_handle.emit("assistant:stream_update",
            crate::assistant_stream_proxy::StreamPayload {
                session_id: session_id.to_string(),
                done: true,
                error: Some("Cancelled".into()),
                ..Default::default()
            });
        let _ = tx.send(RuntimeEvent::RunCompleted { reason: "cancelled".into() }).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_signature_sorted() {
        let invocations = vec![
            crate::assistant_executor::ToolInvocation {
                id: "1".into(), name: "write_file".into(),
                arguments: json!({}),
            },
            crate::assistant_executor::ToolInvocation {
                id: "2".into(), name: "read_file".into(),
                arguments: json!({}),
            },
        ];
        let sig = AgentLoop::build_tool_signature(&invocations);
        // read_file 在 write_file 前（按字母序）
        assert_eq!(sig, "read_file,write_file");
    }

    #[test]
    fn test_content_loop_detection() {
        let config = LoopConfig::default();
        let mut loop_runner = AgentLoop::new(config, vec![]);
        assert!(!loop_runner.detect_content_loop("Hello world"));
        assert!(!loop_runner.detect_content_loop("Hello world")); // 第二次，但 fingerprint 不同
        assert!(!loop_runner.detect_content_loop("Different content"));
        assert!(!loop_runner.detect_content_loop("Different content"));
    }

    #[test]
    fn test_seen_signatures_dedup() {
        let mut set = HashSet::new();
        assert!(set.insert("a,b".to_string()));
        assert!(!set.insert("a,b".to_string())); // 重复
        assert!(set.insert("c,d".to_string()));
    }

    #[test]
    fn test_loop_config_default() {
        let config = LoopConfig::default();
        assert_eq!(config.max_steps, 50);
        assert_eq!(config.max_self_heal, 3);
        assert_eq!(config.doom_threshold, 3);
    }
}
