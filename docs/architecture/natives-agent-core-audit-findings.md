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
- 实现状态：已修复主链（Cancel 已接生产；Progress 已接 Shell/MCP stdio/HTTP/SSE/Sub Agent，并由 sink 合并与丢弃迟到更新；真实外部 fixture 未运行）
- 原始代码证据：EngineToolRuntime；CapabilityGateway::execute；Shell/MCP Handler。
- 原始行为：Gateway 通用路径只 race Timeout，实际取消依赖各 Handler；没有完整 Progress Sink。
- 修复后行为：Gateway 为每次调用创建 child CancellationToken，同时 select Handler、Timeout、父 Cancel；Timeout/Cancel 返回不同错误码，给予 Handler 250ms 清理窗口，无法 quiet 时返回 `cleanup_failed` 并 abort。Shell/MCP stdio/HTTP/SSE/Sub Agent progress 通过 Core sink 发送，按 8KiB/250ms 合并并在 settled/cancel 后丢弃迟到更新。
- 影响/触发条件：普通 Handler 取消边界统一；第三方/网络 Handler quiet 仍需集成验证。
- 测试方式：`capability-gateway::p0_tests::cancellation_wins_over_blocking_handler`（本轮 1 passed）；仍需真实 shell kill+wait、HTTP/MCP pending 和并行 quiet fixture。

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
- 实现状态：已接生产关键事实（非关键 delta/progress 仍保持兼容的 best-effort）
- 原始代码证据：event_seq.rs；多处 events.append 忽略返回值。
- 当前行为：`EventSequencer::append_checked` 已存在；Turn/Message/Tool/Permission/Snapshot/Checkpoint 生命周期事实使用 checked seam；Tool completion 持久化失败会取消 Engine 并写 side-effect `uncertain`，旧 delta/progress 调用仍为 best-effort。
- 新代码证据：`crates/agent-core/src/engine.rs::append_critical`。
- 建议：为 checked sink 增加 fault-injection persistence fixture，并补齐 HTTP/MCP/并行取消 quiet 证据。

## UI 会合成或重标终态展示事件

- 严重等级：P1
- 所属模块：Renderer DaemonAssistantAdapter
- 实现状态：已修复投影接线（adapter 已拒绝伪造终态，并将缺失权威事件作为本地 `ProjectionRecovery` 写入 workspace state；专用 UI 文案仍待接线）
- 原始代码证据：src/lib/assistant-gateway/daemon-adapter.ts。
- 当前行为：`DaemonAssistantAdapter` 缺失权威终态时抛出 `AuthoritativeEventMissing`，不再重标 sequence；controller 将 incomplete 状态写入 workspace state，不写回 Daemon Event Store。
- 新代码证据：`src/lib/assistant-protocol/projection.ts`、`daemon-adapter.ts::recoverTerminalEvent`。
- 建议：将 `ProjectionRecovery` 状态接入工作区 reducer 的恢复提示。
- 新增验证：`subscribe rejects terminal status without an authoritative event` 通过；测试确认不会从 `run.list` 伪造 `failed` 事件，并保留 `ProjectionIncomplete(authoritative_event_missing)`。

## Run Lineage 与恢复动作

- 严重等级：P1
- 所属模块：assistant-protocol / RunManager / Conversation Store
- 实现状态：已接生产（Continue/Fork/Replay 语义分离）
- 代码证据：`run_manager.rs::retry`、`continue_run`；`conversation_store.rs::fork`；`assistant_protocol::v2::RunV2` lineage 字段；`run.continue` RPC dispatch。
- 当前行为：Retry/Continue 都创建独立 Run 并记录 source/checkpoint/turn lineage；Continue 拒绝无 durable checkpoint、缺 snapshot 或 uncertain side effect；Fork 复制 typed message/block、重映射 parent IDs 并重置权限 profile 为 `ask`；Replay 只读事件，不重跑工具。
- 风险：Continue/Fork 已有真实 SQLite seam 验证，但尚未在完整 daemon/UDS/native-engine verifier 中重新运行；`resume_plan` 已改为先 `approved`，detached start 成功后再结算 `executed`。
- 新增精准证据：`fork_copies_typed_transcript_with_new_message_ids_and_ask_profile` 与 `continue_creates_lineage_from_durable_checkpoint` 通过；前者验证新分支 transcript/权限隔离，后者验证 checkpoint/snapshot lineage 与 resume plan 结算。

## Prompt Queue Ack 与 typed transcript 一致性

- 严重等级：P1
- 所属模块：`crates/agent-core/src/input.rs`、`src-agent-daemon/src/prompt_queue_store.rs`、`conversation_store.rs`
- 实现状态：已修复（`5734ef5f`）
- 代码证据：`EngineInputReceiver::ack` 返回 `Result`；`DurableInputReceiver::ack` 调用 `persist_queued_input_and_ack(..., turn_id)`；`AgentEngine::drain_inputs` 在修改 typed transcript 前等待 ack 成功。
- 当前行为：queue lease、typed user message 与 consumed 状态在同一事务完成；数据库故障不会静默丢失 steering/follow-up。

## Provider typed transcript 生产边界

- 修复状态：已接生产；旧 `EngineMessage` 只保留 legacy/fixture 兼容边界。
- 代码证据：`crates/agent-core/src/engine.rs` 的 `ProviderTurnRequest`；`src-agent-daemon/src/production.rs` 的 `RealProvider::stream_turn` 与 `agent_message_to_history`；`src-agent-daemon/src/routing.rs` 的 `RoutedProvider::stream_turn`、`Sub2ApiPoolProvider::stream_turn`。
- 当前行为：生产 Core 主循环调用 `stream_turn`，daemon 在 provider-neutral `HistoryMessage` 边界保留 Thinking、Image、ToolCall、ToolResult identity；不再通过 trait 默认实现重建 `EngineMessage`。
- 验证：`typed_boundary_preserves_blocks_and_tool_identity` 通过；完整 workspace/live provider 验证仍按报告标记未完成。

## Legacy delta-only reasoning compatibility

- 修复状态：已修复兼容回归。
- 代码证据：`src-agent-daemon/src/conversation_store.rs::append_single_assistant_turn`。
- 当前行为：没有 `MessageCompleted` 的旧 delta 批次仍保留带 duration 的 `reasoning` block；生产 typed `MessageCompleted` 批次使用 canonical `Thinking`，不会重复写旧块。
- 验证：`conversation_round_trip_uses_daemon_tables` 通过。

## Side-effect Ledger 开始/完成事实

- 严重等级：P1
- 所属模块：`src-agent-daemon/src/production_tools.rs`、`side_effect_ledger.rs`
- 实现状态：已修复（`5734ef5f`）
- 代码证据：实际 Gateway handler 前 `record_tool_effect_state(..., "started", ...)` 失败返回 `PERSISTENCE_FAILED`；不经过 Gateway 的 skill/task 编排路径不写孤立 started 行；完成记录失败也返回同码；Core 仍生成同 ID Tool Result 后停止后续 Provider turn。
- 当前行为：外部副作用没有 ledger start 事实时不执行；完成事实无法落库时不伪装成功或继续自动恢复。
- 追加证据（`3bc18472`）：测试未装配全局 DataStore 时使用真实临时 SQLite，而非 no-op store；因此 Plan Mode/ledger 回归断言仍验证持久事实，生产无 store 继续 fail closed。

## Fail-closed ToolUse Fixture 事实

- 修复状态：测试装配已修复；生产规则未放宽。
- 代码证据：`src-agent-daemon/src/production.rs`、`src-agent-daemon/src/run_manager.rs` 中 ToolThenText、权限请求和取消 fixture 使用 `ProviderStopReason::ToolUse`。
- 当前行为：需要执行工具的 fixture 明确声明 `ToolUse`；Core 对裸 `Completed`、`Length`、Unknown 或无 Final Event 仍拒绝执行并生成配对错误结果。

## Resume Plan 结算时机

- 严重等级：P1
- 所属模块：`src-agent-daemon/src/run_manager.rs`、`rpc.rs`
- 实现状态：已修复（`5734ef5f`）
- 代码证据：`retry`/`continue_run` 插入 `approved`；`mark_resume_plan_executed` 仅在 detached start 成功后更新 `executed`。
- 当前行为：计划批准、Run 创建和 Run 真正启动三个事实不再混为一个预先写入的终态。

## Child Run 未继承父取消 Token

- 严重等级：P1
- 所属模块：`src-agent-daemon/src/production.rs`、`run_manager.rs`
- 修复状态：已修复（`eaf54ac7`）
- 代码证据：`RunStartContext.parent_run_id`；`ProductionRuntime::start_run` 调用 `ensure_execution_token(&run_id, parent_run_id.as_deref())`；RunManager 构造上下文时传入 `run.parent_run_id`。
- 当前行为：Child Run 使用自己的 Run ID 注册到父取消树，取消父 Run 会取消 child；child Credential/Permission/Project Root 仍按 child scope 装配。
- 验证：`production_runtime_registers_child_under_parent_token`、`dual_provider_engine_fixture_subagent` 精准通过。

## Prompt Queue 启动失败会丢失已认领消息

- 严重等级：P1
- 所属模块：`src-agent-daemon/src/prompt_queue_store.rs`、`crates/harness-core/src/session_actor.rs`
- 修复状态：已修复（`1d235cd5`）
- 代码证据：`send_now` 与 `on_run_terminal` 在 `start_detached_global` 成功后才删除 `prompt_queue` 行；失败路径调用 `SessionCoordinator::requeue` 并持久化 actor snapshot。
- 当前行为：SQLite durable row 在 Run 尚未被 RunManager 接受前保留；重试不会静默丢失 queued/steering 输入，已 claimed item 也会恢复为 queued。
- 验证：`send_now_cancels_active_and_starts_new_run`、`requeue_restores_claimed_item_at_front` 精准通过。

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

## Checkpoint 与 Event Store 使用了不同的持久化权威

- 严重等级：P1
- 所属模块：`src-agent-daemon/src/production.rs`、`checkpoint.rs`、`rpc.rs`
- 修复状态：已修复（当前 Worktree）
- 原始行为：`RunManager::new_with_store` 虽注入了事件 `DataStore`，Checkpoint 和 rewind 仍可能通过进程级 `global_checkpoint_manager()` 打开默认数据库。
- 修复后行为：`ProductionRuntime` 持有实例级 `Arc<CheckpointManager>`；事件、checkpoint、生产工具捕获和 rewind 使用同一 `DataStore`，无 store 的 fixture 才保留显式兼容 fallback。
- 影响：避免多实例/测试数据库中 Run、Event、Checkpoint 不一致，降低恢复和 rewind 读取错误权威的风险。
- 测试方式：严格 `cargo check -Dwarnings` 通过；需后续补多 DataStore 集成 fixture。

## 完整 typed turn 在失败/取消时可能未写入 Conversation

- 严重等级：P1
- 所属模块：`src-agent-daemon/src/production.rs`、`conversation_store.rs`
- 修复状态：已修复（当前 Worktree）
- 原始行为：生产只在 `EngineOutcome::Completed` 时扫描事件写入 assistant turn，Provider error/cancelled 会丢失已经完整结算的 typed turn。
- 修复后行为：所有 outcome 都调用 typed event persistence；Conversation Store 对包含 `TurnStarted` 但没有 `TurnCompleted` 的部分 turn fail closed，保持完整失败 turn 并跳过真正未结算的 stream。
- 影响：Full History 与执行事件的一致性提高，且不会把半个 assistant message 当成可恢复对话。
- 测试方式：`incomplete_typed_turn_is_not_committed_to_conversation`；旧 delta-only round-trip 仍保持兼容。

## Tool Progress 结算与迟到更新存在竞态

- 严重等级：P1
- 所属模块：`src-agent-daemon/src/production_tools.rs`
- 修复状态：已修复（当前 Worktree）
- 原始行为：`publish` 和 `mark_tool_call_settled` 分别获取 settled/pending 锁，结算后仍可能追加迟到 `ToolOutputDelta`。
- 修复后行为：两条路径按相同锁序持有 pending 与 settled；事件追加和结算形成互斥顺序，settled call 的更新被丢弃。
- 影响：避免 ToolCallCompleted 后出现可见的伪进度；不改变 progress sink 的限流策略。
- 测试方式：`settled_tool_drops_late_progress` 精测通过。

## 动态 MCP Schema 未覆盖未知/外部工具执行边界

- 严重等级：P1
- 所属模块：`crates/capability-gateway/src/lib.rs`、`src-agent-daemon/src/production_tools.rs`
- 修复状态：已修复（当前 Worktree）
- 原始行为：Gateway 的本地注册工具校验不能覆盖 namespaced MCP schema，未知 namespaced 名称可能继续进入 transport。
- 修复后行为：Gateway 暴露单一 `validate_external_input` 校验边界；生产工具先拒绝未知 namespaced MCP，再按 MCP 广告 schema 校验动态参数，之后才进入 permission/transport。
- 影响：Schema 不再只是模型提示，动态外部工具也必须通过最终校验；generic `mcp_call` 保持既有 envelope 兼容。
- 测试方式：严格 `cargo check -Dwarnings` 通过；真实 MCP fixture 仍待受控集成验证。

## Follow-up/Steering 在上一 Turn 结算前消费

- 严重等级：P1
- 所属模块：`crates/agent-core/src/engine.rs`
- 修复状态：已修复（当前 Worktree）
- 原始行为：Follow-up 在 `MessageCompleted`/`TurnCompleted` 写入前被 ack 并注入内存 transcript；Steering 也在工具结果组装后、当前 Assistant Turn 结算前消费。
- 修复后行为：两类输入都在当前 Turn 的完整权威结算事实之后消费；ack 失败直接结束当前 Run，不重复补发失败 Turn 事件。
- 影响：Provider Partial/未结算 Assistant Message 之间不会插入下一条用户输入，Conversation Store 可按事件完整重放。
- 测试方式：`engine::tests::follow_up_closes_previous_turn_before_next_provider_call` 通过；严格编译通过。

## Permission 持久化错误被吞掉或被随机 ID 替换

- 严重等级：P1
- 所属模块：`src-agent-daemon/src/production_tools.rs`、`interaction_store.rs`
- 修复状态：已修复（当前 Worktree）
- 原始行为：Permission Broker 错误被替换为随机 UUID；interaction/session actor/响应写入失败仍可继续等待或授予 grant。
- 修复后行为：Broker、interaction row、actor snapshot、resolved response 或权威 PermissionResponded 任一持久化失败均返回 `PERMISSION_PERSISTENCE_FAILED`/拒绝；grant 在 PermissionResponded 成功后才记录。
- 影响：没有可恢复权限事实时不会执行副作用工具，也不会留下可误用的权限授权。
- 测试方式：严格 `cargo check -Dwarnings` 通过；应补充 DataStore fault-injection 精测。

## Resume Plan 在 detached Preparing 阶段被提前结算

- 严重等级：P1
- 所属模块：`src-agent-daemon/src/rpc.rs`、`run_manager.rs`
- 修复状态：已修复（当前 Worktree）
- 原始行为：Retry/Continue RPC 在 `start_detached` 仅返回 `Preparing` 后立即将 `resume_plan` 标记为 `executed`。
- 修复后行为：RPC 不再提前结算；RunManager 在 capability/harness/effective prompt 运行计划成功持久化后，进入真实执行前才更新 approved plan。
- 影响：后台启动失败不会伪装为已恢复；Source Run、New Run 和 Resume Plan 状态可区分。
- 测试方式：既有 Continue lineage 精测验证 approved/executed 转换；本轮严格编译通过。

## Durable Steering/Follow-up drain 错误被当作空队列

- 严重等级：P1
- 所属模块：`src-agent-daemon/src/prompt_queue_store.rs`、`crates/agent-core/src/input.rs`
- 修复状态：已修复（当前 Worktree）
- 原始行为：SQLite store、事务、查询或提交失败时，`DurableInputReceiver::drain` 返回空列表，Core 会继续 Provider 调用并可能静默丢失用户插话。
- 修复后行为：`drain` 返回 `Result`；任一持久化错误都传播为 Engine 错误，队列不会被伪装成空队列，下一轮 Provider 调用不会继续。
- 影响：Steering/Follow-up 的 lease/ack 失败保持可审计并 fail closed，避免输入事实丢失。
- 测试方式：严格 `RUSTFLAGS=-Dwarnings cargo check` 通过；Follow-up Turn 边界精测通过。
