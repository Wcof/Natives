# Agent Core P0 复核（深化基线）

## Credential Run 绑定

- 当前是否仍存在：否（P0 已修复）。
- 当前代码证据：`crates/agent-core/src/engine.rs` 的 `EngineProviderContext`；`src-agent-daemon/src/production.rs` 通过 `run_id` 进入路由；`production_credentials.rs` 按真实 Run 解析。
- 与审计基线相比的变化：固定 `provider-stream` 已移除。
- 本轮处理方式：保留并增加 `ProviderTurnRequest`，确保每次 Attempt 携带 Run ID 和序号。

## Provider API Mode

- 当前是否仍存在：否（P0 已将模式改为 Adapter/Route 显式配置）。
- 当前代码证据：`crates/provider-adapters/src/providers/openai.rs` 的 `OpenAiApiMode`；生产路径未在请求期间写环境变量。
- 与审计基线相比的变化：Responses 与 Chat Completions 不再共享可变进程状态。
- 本轮处理方式：增加 Provider Turn 请求 seam，未重新引入环境变量。

## Stop Reason 与截断 Tool Call

- 当前是否仍存在：截断调用执行风险已关闭；统一原因仍由 Adapter 映射到 Core。
- 当前代码证据：`crates/agent-core/src/engine.rs` 的 `ProviderStopReason`、`TRUNCATED_TOOL_CALL` / `UNKNOWN_PROVIDER_STOP_REASON` 分支；`crates/provider-adapters/src/stream/openai_sse.rs` 的 `from_raw`。
- 与审计基线相比的变化：P0 已禁止 `Length`、未知原因和无 Final Event 的调用进入 Handler。
- 本轮处理方式：增加跨 Provider reason 回归测试和 Turn 完成原因事件。

## JSON 与 Schema 边界

- 当前是否仍存在：P0 已移除全局 `{raw: ...}` 执行回退；Gateway 继续执行最终校验。
- 当前代码证据：`crates/agent-core/src/engine.rs` 严格 `serde_json::from_str`；`crates/capability-gateway/src/lib.rs` 的 Gateway 验证路径。
- 与审计基线相比的变化：解析失败生成配对错误结果而不是调用 Handler。
- 本轮处理方式：保持边界，未把 Schema Registry 复制进 Core。

## Hook Deny 配对

- 当前是否仍存在：否（拒绝终点已产生同 ID `ToolCallCompleted` 错误事实）。
- 当前代码证据：`crates/agent-core/src/engine.rs` 的 `PreparedToolCall.rejected` 和 `HOOK_DENIED`。
- 与审计基线相比的变化：拒绝不再删除 Assistant Tool Call。
- 本轮处理方式：增加生命周期事件并保留既有配对测试。

## Gateway Cancel

- 当前是否仍存在：P0 已在 Gateway 通用路径监听 Handler、Timeout、Cancellation；底层 Handler 的资源清理仍由具体执行器负责。
- 当前代码证据：`crates/capability-gateway/src/lib.rs` 的执行包装器与 Shell/MCP 专项取消测试。
- 与审计基线相比的变化：Cancel 与 Timeout 已区分。
- 本轮处理方式：本阶段不重写 Scheduler；新增 Core `ToolProgressSink` seam 不改变取消语义。
