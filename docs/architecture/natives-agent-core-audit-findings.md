# Natives Agent Core 审计问题

> 原始审计基线：1b4b1792932e2e24160091c7700a2c092d01e5f2。本文件保留原问题判断，并追加 P0 Worktree 的修复状态与新证据。

## Provider Credential 未绑定真实 Run

- 严重等级：P0
- 所属模块：src-agent-daemon Provider 装配 / Credential Broker
- 实现状态：已修复（当前 worktree）
- 原始代码证据：production.rs 的 RealProvider::stream_with_controls；production_credentials.rs 的 resolve_credential_for_run。
- 原始行为：Provider 使用固定 provider-stream，EngineProvider 没有 Run identity。
- 修复后行为：agent-core 的 EngineProviderContext 携带 run_id 与 attempt；AgentEngine 调用 stream_with_context；RoutedProvider 将上下文传给 RealProvider::stream_with_context_controls；Broker 现在收到真实 Run ID。无上下文 helper 仅保留 legacy-unbound，不在 Engine 生产主链使用。
- 影响/触发条件：原 Lease、审计、并发 Run 与 child Run 归属错误已从主链消除；仍需集成验证真实 Tauri Broker 的撤销行为。
- 测试方式：production_credentials::run_identity_tests::broker_receives_distinct_run_ids。

## Provider API 模式使用进程全局环境变量

- 严重等级：P0
- 所属模块：src-agent-daemon / provider-adapters
- 实现状态：已修复（当前 worktree）
- 原始代码证据：production.rs::resolve_adapter；provider-adapters/providers/openai.rs::prefers_responses_api。
- 原始行为：Responses 路由通过 std::env::set_var("NATIVES_OPENAI_API", "responses") 改写进程状态。
- 修复后行为：OpenAiAdapter 增加不可变 OpenAiApiMode；Daemon 直接构造 Responses 模式；prefers_responses_api 不再读取环境变量。
- 影响/触发条件：Responses/Chat Completions 并发串模式风险已移除；未显式指定模式的旧构造路径仍按模型名推断。
- 测试方式：provider-adapters::providers::openai::api_mode_tests::explicit_api_mode_is_request_independent。

## Tool 参数未做 Schema 最终校验，非法 JSON 仍可能执行

- 严重等级：P0
- 所属模块：agent-core / capability-gateway
- 实现状态：已修复（当前 worktree）
- 原始代码证据：agent-core engine 参数累积处；CapabilityGateway::execute。
- 原始行为：解析失败后包装 { "raw": ... }，Gateway 不验证注册 Schema。
- 修复后行为：Core 严格 JSON 解析，失败生成 INVALID_TOOL_ARGUMENTS；Gateway 在 Handler 前执行 type、properties、required、additionalProperties、items、enum 和 numeric bounds 校验。全部内置 Schema 已通过清点。
- 影响/触发条件：malformed JSON、根类型、required、type、enum、additional property 错误现在 fail closed。
- 测试方式：capability-gateway::p0_tests::schema_failure_never_reaches_handler；all_builtin_schemas_are_supported_by_validator。

## Provider Stop Reason 丢失，截断 Tool Call 可能执行

- 严重等级：P0
- 所属模块：provider-adapters → src-agent-daemon → agent-core
- 实现状态：已修复（当前 worktree；缺少完整 live provider 验证）
- 原始代码证据：各 Adapter 的无参数 ProviderEvent::Completed；agent-core::EngineProviderEvent。
- 原始行为：Core 无法区分 stop、tool_use、length。
- 修复后行为：Provider Adapter 统一携带 ProviderStopReason；OpenAI SSE、Responses、Anthropic、Gemini 与兼容路径均映射完成原因。Core 对 Length、Unknown 或无 Final Event 的 Tool Call 构造配对错误结果，Handler 不执行。
- 影响/触发条件：潜在截断副作用已 fail closed；未知终态宁可拒绝工具调用。
- 测试方式：agent-core::tests::length_stop_never_executes_collected_tool_call；OpenAI parser length reason 测试；仍需每个 live adapter 的 length/unknown/no-final fixture。

## Hook Deny 没有形成模型上下文中的配对 Tool Result

- 严重等级：P0
- 所属模块：crates/agent-core
- 实现状态：已修复（当前 worktree）
- 原始代码证据：PreToolUse Deny 分支及后续 denied 跳过逻辑。
- 原始行为：拒绝项只发事件，Assistant Tool Call / Tool Result 被跳过。
- 修复后行为：PreparedToolCall.rejected 统一承载 Hook Deny、Invalid JSON 和 fail-closed 错误；execute_prepared_tools 跳过 Handler 但保留 ExecutedToolCall；后续始终加入 Assistant Tool Call 与同 ID Tool Result，并发出一个 ToolCallCompleted。
- 影响/触发条件：Provider 可看到拒绝原因并重发；同批兄弟调用继续；本轮没有引入完整 Tool Scheduler。
- 测试方式：agent-core::tests::hook_deny_keeps_tool_call_and_emits_one_error_result。

## Tool Runtime 的取消和 Progress 契约不完整

- 严重等级：P1
- 所属模块：agent-core / capability-gateway / src-agent-daemon
- 实现状态：部分修复（Cancel 已修复；Progress 明确不在本轮）
- 原始代码证据：EngineToolRuntime；CapabilityGateway::execute；Shell/MCP Handler。
- 原始行为：Gateway 通用路径只 race Timeout，实际取消依赖各 Handler；没有完整 Progress Sink。
- 修复后行为：Gateway 为每次调用创建 child CancellationToken，同时 select Handler、Timeout、父 Cancel；Timeout/Cancel 返回不同错误码，并先取消 child token 让资源型 Handler 清理。Shell/MCP 既有逻辑保持不变。
- 影响/触发条件：普通 Handler 取消边界统一；第三方/网络 Handler quiet 仍需集成验证。
- 测试方式：capability-gateway::p0_tests::cancellation_wins_over_blocking_handler；后续补 shell kill+wait、MCP pending、并行整体取消和 late completion。

## Conversation Active History 不能无损恢复执行上下文

- 严重等级：P1
- 所属模块：Conversation Store / agent-core message model
- 实现状态：部分修复（当前 worktree；已保存 snapshot_json 并提供 replay API）
- 原始代码证据：conversation_store.rs；agent-core EngineMessage。
- 当前行为：typed message 已写入/读取 `message_block` 并保留 ToolCall/ToolResult identity；ContextSnapshotCommitted 持久化完整 typed `snapshot_json`，生产启动优先使用 checkpoint 绑定 snapshot，再按 `input_message_ids` 合并压缩后新增消息；完整历史仍保留。
- 新代码证据：`src-agent-daemon/src/conversation_store.rs::load_agent_messages`、`load_active_context_messages`、`persist_context_snapshots_from_events`；`crates/agent-core/src/engine.rs::agent_messages_from_json`；migration 028 的 context_snapshot 字段。
- 新增代码证据：`conversation_store.rs::load_active_context_snapshot_for_checkpoint`、`production.rs` 的 `run.checkpoint_id` 选择路径；`run_manager.rs::continue_run` 对 checkpoint/snapshot 存在性与 uncertain side effect 的 fail-closed 检查。
- 建议：补齐 artifact 内容、source message ids 和 provider window 的可重放 fixture，并为 Continue/Fork 增加持久化集成测试。

## Event 持久化失败未在所有执行接缝 fail closed

- 严重等级：P1
- 所属模块：agent-core event_seq 及调用者
- 实现状态：部分修复（深化阶段；非关键 delta 仍保持兼容的 best-effort）
- 原始代码证据：event_seq.rs；多处 events.append 忽略返回值。
- 当前行为：`EventSequencer::append_checked` 已存在；本轮 Turn/Message 生命周期事实使用 checked seam，旧 delta 调用仍为 best-effort。
- 新代码证据：`crates/agent-core/src/engine.rs::append_critical`。
- 建议：下一阶段将 Tool/Run terminal 等关键事实统一迁移到 checked sink，并加入 fault-injection persistence。

## UI 会合成或重标终态展示事件

- 严重等级：P1
- 所属模块：Renderer DaemonAssistantAdapter
- 实现状态：部分修复（adapter 已拒绝伪造终态；UI 状态展示仍待接线）
- 原始代码证据：src/lib/assistant-gateway/daemon-adapter.ts。
- 当前行为：`DaemonAssistantAdapter` 缺失权威终态时抛出 `AuthoritativeEventMissing`，不再重标 sequence；上层仍需展示 incomplete 状态。
- 新代码证据：`src/lib/assistant-protocol/projection.ts`、`daemon-adapter.ts::recoverTerminalEvent`。
- 建议：将 `ProjectionRecovery` 状态接入工作区 reducer 的恢复提示。

## Run Lineage 与恢复动作

- 严重等级：P1
- 所属模块：assistant-protocol / RunManager / Conversation Store
- 实现状态：已接生产（Continue/Fork/Replay 语义分离）
- 代码证据：`run_manager.rs::retry`、`continue_run`；`conversation_store.rs::fork`；`assistant_protocol::v2::RunV2` lineage 字段；`run.continue` RPC dispatch。
- 当前行为：Retry/Continue 都创建独立 Run 并记录 source/checkpoint/turn lineage；Continue 拒绝无 durable checkpoint、缺 snapshot 或 uncertain side effect；Fork 复制 typed message/block、重映射 parent IDs 并重置权限 profile 为 `ask`；Replay 只读事件，不重跑工具。
- 风险：Continue/Fork 的端到端 daemon fixture 尚未在本轮运行；resume_plan 的 `executed` 写入发生在 detached start 之前，后续可改为 start 成功后结算。

## PermissionManager 共享实例存在遗留全局 Profile 接口

- 严重等级：P2
- 所属模块：src-agent-daemon / agent-core permissions
- 实现状态：主生产路径已收紧；测试/legacy setter 仅保留在 `cfg(test)`
- 原始代码证据：共享 Arc<PermissionManager> 与 set_permission_profile；生产 PermissionGatedTools 使用 per-run profile。
- 当前行为：`PermissionGatedTools` 使用 `RunStartContext.permission_profile` 调用 profile-bound API；`ProductionRuntime::set_permission_profile` 仅在测试编译保留，不能改变生产进程全局状态。
- 新代码证据：`src-agent-daemon/src/production.rs::set_permission_profile` 的 `cfg(test)`；`production_tools.rs::request_permission_for_profile`。
- 建议：后续限制 global setter 为测试/deprecated，并加入 profile isolation 静态 gate。

## 已验证未发现的问题（原始结论保留）

- 重复持久终态：未发现。AgentEngine 返回 EngineOutcome，RunManager CAS/事务保持唯一 terminal fact authority。
- 子 Agent Project Root 放大：未发现。Child 使用父 Gateway project root，权限与 allowlist 只收窄。
- 协议广告无 dispatch：未发现。protocol:check 与 rpc_dispatch_contract 在基线通过。
- 多 Tool Call 只能顺序：不成立。Core 已有受限并行与源序结果回灌，本轮没有重写 Scheduler。
- 压缩只有字符裁剪：不成立。已有模型摘要、budget 与 dangling repair；token estimate/replay 仍不精确。
