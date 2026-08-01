# Agent Core P0 复核记录

基线：`1b4b1792932e2e24160091c7700a2c092d01e5f2`（当前 Worktree HEAD）。复核基于生产调用路径和当前源码，不改变生产代码。

## Credential Run 绑定

- 当前是否仍存在：是。
- 当前代码证据：`src-agent-daemon/src/production.rs` 的 `RealProvider::stream_with_controls` 调用 `resolve_credential_for_run(..., "provider-stream")`；`agent-core::EngineProvider` 没有请求上下文。
- 与审计基线相比的变化：未见修复。
- 本轮处理方式：为 Provider 调用增加最小 `EngineProviderContext`，由 `EngineRunConfig.run_id` 和 Attempt 序号传入；Daemon 使用真实 Run ID。

## Provider API 模式

- 当前是否仍存在：是。
- 当前代码证据：`src-agent-daemon/src/production.rs::resolve_adapter` 对 Responses 路由调用 `std::env::set_var("NATIVES_OPENAI_API", "responses")`；`provider-adapters/src/providers/openai.rs::prefers_responses_api` 读取该进程全局变量。
- 与审计基线相比的变化：未见修复。
- 本轮处理方式：把 OpenAI API Mode 作为 Adapter 的不可变字段；请求期间不再修改环境变量。

## Tool 参数校验

- 当前是否仍存在：是。
- 当前代码证据：`crates/agent-core/src/engine.rs` 使用 `serde_json::from_str(&args).unwrap_or(json!({"raw": args}))`；`CapabilityGateway::execute` 只执行路径策略、超时和输出限制，没有按 `Tool.schema` 做最终 Schema 校验。
- 与审计基线相比的变化：未见修复。
- 本轮处理方式：Core 对参数做严格 JSON framing；Gateway 在 Handler 前执行注册 Schema 的最终校验。

## Stop Reason

- 当前是否仍存在：是。
- 当前代码证据：`EngineProviderEvent::Completed` 无原因；Provider Adapter 的完成事件没有统一携带 `stop`、`tool_use`、`length`、`cancelled`、`error` 或未知原因；Provider Stream 缺少可靠终态时仍可能结束为普通 Completed。
- 与审计基线相比的变化：未见修复。
- 本轮处理方式：引入统一 Stop Reason，Adapter 映射原始原因；Core 对 `length`、未知原因和无 Final Event 的 Tool Call fail closed，并生成配对错误 Result。

## Hook Deny 配对

- 当前是否仍存在：是。
- 当前代码证据：`crates/agent-core/src/engine.rs` 的 PreToolUse Deny 只加入 `denied` 项并发出 `ToolCallCompleted`，后续组装 `assistant_tool_calls` / `tool_results` 时跳过该项。
- 与审计基线相比的变化：未见修复。
- 本轮处理方式：拒绝项保留 Assistant Tool Call，并以相同 ID 生成错误 Tool Result；不调用 Handler。

## Gateway Cancel

- 当前是否仍存在：是（部分 Handler 自行监听，通用 Wrapper 未监听）。
- 当前代码证据：`crates/capability-gateway/src/lib.rs::CapabilityGateway::execute` 只使用 `tokio::time::timeout`；`ToolCallContext` 已携带 `CancellationToken`，但通用路径未同时 select Handler、Timeout 和 Cancel。
- 与审计基线相比的变化：未见修复。
- 本轮处理方式：通用 Wrapper 同时监听 Handler、Timeout、CancellationToken，并返回可区分的错误码；保留现有 Shell/MCP Handler 的资源清理逻辑。
