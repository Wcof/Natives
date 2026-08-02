# Agent Core 最终深化与生产闭环报告

## 1. 基线

- Worktree：`/Users/ldh/Downloads/project/AiNative/Natives-agent-core-deepening`
- 分支：`feat/agent-core-deepening`
- 起始 Commit：`0aadad5316f844fe6312d70472bff478049f9088`
- 结束 Commit：当前 Worktree `HEAD`（本轮最新受控代码）
- Pi 参考 Commit：`583f153d502aa8e958eefdb9af0fbd3344e68f95`
- 原工作区：`/Users/ldh/Downloads/project/AiNative/Natives` 保持 dirty，未 reset/stash。

## 2. 能力与生产证据

| 能力 | 当前生产证据 | 状态 |
|---|---|---|
| Typed Message / Turn | `agent-core/src/engine.rs` 的 `run_with_typed_messages` 直接驱动生产 transcript；`RealProvider`、`RoutedProvider`、`Sub2ApiPoolProvider` 显式实现 `stream_turn(ProviderTurnRequest, ...)`，只在 provider-neutral `HistoryMessage` 边界转换；旧 `EngineMessage` 只在 legacy/fixture 入口使用 | 已接生产；Core 主链不再按历史是否为空切换第二套 Loop |
| Context Snapshot | migration 028 + `persist_context_snapshots_from_events` 写入 conversation/turn/source revision 与完整 typed `snapshot_json`；生产启动以最新 snapshot 为基线，并按 `input_message_ids` 追加压缩后新写入的完整消息 | 已接生产 |
| Capability Scheduler | `PermissionGatedTools::list_tool_capabilities` 从 Gateway side-effect 映射模式，Core 保持调度 | 已接生产 |
| Progress | Shell `ToolCallContext.progress`、MCP stdio/HTTP/SSE progress notification、Sub Agent child event 都进入 `DaemonToolProgressSink`；sink 有序号、8KiB/250ms 合并、settled late-drop | 已接生产；真实 SSE fixture 仍未运行 |
| Steering / Follow-up | `DurableInputReceiver` SQLite lease/ack/recovery；ack 同事务写入 user message + queue sent，失败不消费 | 已接生产 |
| Critical Events | Turn/Message/Tool prepared/started/completed/GenerationAttemptCommitted/Checkpoint/Snapshot/Permission 使用 `append_checked`；Tool completion 持久化失败会取消运行并写 side-effect `uncertain` | 已接生产关键事实；delta/progress 仍允许 best-effort |
| Ledger / Checkpoint | side-effect 状态记录；checkpoint 写入 turn/ledger cursor | 部分完成；外部副作用仍不可回滚 |
| Retry / Resume | RunManager retry/continue 均创建独立 Run，记录 checkpoint、lineage 与 resume_plan；plan 在 detached start 成功后才结算；不确定副作用 fail closed | 已接生产；Replay 保持只读事件重放，Fork 复制 typed transcript 并重置权限 |
| Renderer Projection | 新增 `ProjectionState`、`ProjectionRecovery`、`AuthoritativeEventMissing`；禁止伪造终态序列 | 已接生产 |

## 3. 关键改动

### Typed Message / Turn

`AgentEngine::run_with_typed_messages` 接收 daemon 已加载的 typed transcript；主循环只在 provider 边界做一次 provider-neutral 转换，Provider 调用统一走 `EngineProvider::stream_turn(ProviderTurnRequest, CancellationToken)`。`MessageCompleted` 现在携带完整 provider-neutral content，事件丢失时仍可独立恢复 Assistant blocks。旧 `EngineMessage` 仅保留在旧数据库/fixture 兼容边界。每次 provider attempt 只在提交后形成一个 Turn。

### Typed Message / Conversation

`src-agent-daemon/src/conversation_store.rs` 增加 typed message 读取、按 Turn 分组持久化与 JSON block 解析，工具结果保留 `tool_call_id`、错误码和结果 block。生产启动以最新 `ActiveContextSnapshot` 为基线，按 `input_message_ids` 追加压缩后新写入的 typed 消息；完整历史不删除。旧 `engine_history` 仅作为空 typed history 的兼容回退；Core 是唯一 mutable typed transcript。ContextSnapshotCommitted 携带并持久化 active typed JSON，可通过 replay API 解释重放。

### Scheduler / Progress

Gateway 的 `list_capabilities()` 是调度元数据唯一来源；Core 只消费 `ExecutionMode`/`conflict_key`，未知能力按 Sequential 处理。Shell stdout/stderr、MCP stdio progress notification、Sub Agent 子 Run 事件通过稳定 call/turn/message ID 进入 sink；Gateway 通用执行包装器在取消/超时后保留 Handler 清理窗口，无法 quiet 时返回 `cleanup_failed`。Side-effect ledger 在成功/失败/timeout/cancel 以及 completion 事实持久化失败时分别记录状态。

### Recovery / Projection

Checkpoint 可绑定最近 turn、context snapshot 和 event cursor；run start 发 `CheckpointCreated`，完成提交发独立 `CheckpointCommitted`；retry 会写入 `resume_plan`，不确定副作用时记录 blocked。PermissionRequested/Responded 也通过 `append_checked`，持久化失败即拒绝后续执行。渲染器发现 terminal DB 状态但缺失权威 terminal event 时抛出 `AuthoritativeEventMissing`，不重写 sequence、不合成 Run Event。

## 4. 数据库与协议

- migration 028 维持 additive：turn、typed message 元数据、context snapshot lineage、prompt lease、checkpoint cursor、side-effect status、resume_plan。
- 新增 `run.continue`（仍由 RunManager 负责创建/启动独立 Run）；其他既有 RPC 与数据库表保持兼容，RunManager 仍是唯一 terminal fact authority。
- 未将 Gateway、Provider Adapter 或 Daemon 重新归类为 Core；未集成 Pi Runtime。

## 5. 精确验证

| 命令 | 结果 |
|---|---|
| `RUSTFLAGS=-Dwarnings CARGO_TARGET_DIR=/Users/ldh/Downloads/project/AiNative/Natives/.cargo-target-shared CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 cargo check -p agent-core -p capability-gateway -p provider-adapters -p assistant-protocol -p natives-agent-daemon` | 通过（strict，0 errors；代码验证基线 `9c40bc49`） |
| `rtk cargo test -p agent-core --lib compaction_ -- --nocapture` | 通过：5 |
| `rtk cargo test -p capability-gateway --lib` | 通过：91 |
| `rtk cargo test -p natives-agent-daemon typed_message_round_trip_preserves_tool_call_identity -- --nocapture` | 通过：1 |
| `rtk cargo test -p natives-agent-daemon conversation_round_trip_uses_daemon_tables --lib -- --nocapture` | 通过：1（本轮兼容修复后） |
| `rtk cargo fmt --check` | 通过 |
| `rtk npm run protocol:check` | 通过 |
| `rtk cargo check -p agent-core -p capability-gateway -p provider-adapters -p natives-agent-daemon` | 通过（本轮接线后） |
| `rtk cargo fmt --check` | 通过（本轮格式化后） |
| `rtk cargo test -p capability-gateway cancellation_wins_over_blocking_handler -- --test-threads=2` | 通过：1 |
| `CARGO_TARGET_DIR=... cargo test -p natives-agent-daemon typed_boundary_preserves_blocks_and_tool_identity -- --nocapture --test-threads=2` | 通过：1；验证 typed provider seam 保留 Thinking/Image/ToolCall/ToolResult identity |
| `npm run verify:native-engine` | 失败：静态 audit/protocol OK；严格 warning check 已单独通过；daemon lib 323 passed/6 failed，live fixture 0/1 failed；frontend commands 因缺少 `tsx`/`tsc` 失败 |
| `npm run typecheck` | 通过 |
| `npm run lint` | 通过 |
| `npm run perf:check` | 通过（含 typecheck/lint/build/perf budget） |
| `npm run protocol:check` | 通过 |
| `npm run test` | 首次集中运行 751/752 通过；唯一失败为过期的“缺事件时合成终态”断言，已改为 fail-closed 断言并精准复验通过；按资源规则未重复全量 |
| `cargo test --workspace -- --test-threads=2` | 已运行一次；agent-core 首次 160/162，两个 legacy fixture 已修正并精准复验；未重复 workspace 全量 |
| `npm run verify:native-engine` | 已运行一次；静态/protocol 通过，但 daemon/live 历史 fixture 与当时缺失 frontend 工具导致失败；修复后受资源上限未重复运行 |

直接执行的 Cargo 命令使用共享 target、jobs=2、incremental=0；未执行 clean/rm -rf，未终止其他任务进程。仓库自带 `verify:native-engine` 内部未继承 target 环境，使用了 worktree `target/`，已在本报告明确记录。前三次精确测试失败分别暴露 migration 028 错误索引、测试外键和 block 解码路径，均已修复后通过。

## 6. 未完成与风险

- Active Context 已在生产启动接线：snapshot 的 `input_message_ids` 作为去重边界，压缩后新增消息追加到 active context；完整历史仍保留在 message 表。
- `run.continue` 已接入 daemon RPC/authority，要求 source terminal、durable checkpoint、snapshot 存在且无 uncertain side effect；Fork 已复制 source conversation 的 typed message/block，并为新分支重新使用 `ask` 权限 profile。Replay 仍是只读事件回放，不重跑工具。
- HTTP/SSE MCP 逐行解析 JSON/SSE 帧并转成 ToolOutputDelta；取消可杀掉并 wait curl；stdio MCP 同样支持 notifications/progress。
- Handler 若不响应 Gateway child token，在 250ms 清理窗口后会返回 `cleanup_failed` 并 abort task；这保证不伪装成功，但真实外部资源仍需专项 quiet fixture。
- `start_with_seams_loads_daemon_conversation_history` 曾因单元测试后台 reaper 与环境锁互等而无输出；关闭该测试构造器中的 reaper 后已精准通过。`verify:native-engine` 的历史 live/native 失败仍不能据此宣称全量通过。
- Renderer projection 已拒绝伪造事件；adapter 将 `ProjectionRecovery` 暴露给 workspace state，专用 UI 文案仍可后续补齐。

## 7. 合并顺序与回滚

建议先合并 typed conversation/checkpoint/ledger，再合并 renderer projection；若出现兼容问题，可整体回滚本提交，保留 migration 028 的 nullable/default 字段，不影响既有 RPC、UDS、RunManager、Gateway 和 Provider Adapter。

## 8. 资源使用

- 共享 `CARGO_TARGET_DIR`：`/Users/ldh/Downloads/project/AiNative/Natives/.cargo-target-shared`
- 共享 target：`/Users/ldh/Downloads/project/AiNative/Natives/.cargo-target-shared`；jobs=2、incremental=0。
- 本轮严格 Cargo check 与精准 Cargo test 均使用共享 target；workspace test 与 native verifier 各只运行一次，未重复全量。
- 观测到 shared target 约 7.5 GiB、worktree target 约 5.6 GiB；最低可用磁盘约 60 GiB。
- 主动终止的卡死进程：前序 turn 的 `start_with_seams_loads_daemon_conversation_history` 测试进程按资源规则中止；本轮没有终止 Cargo/Rustc。
- 清理的临时目录：无；`cargo clean`：否。

## 9. 后续闭环提交（`5734ef5f`，含 `39d6514f`）

- `EngineInputReceiver::ack` 改为可失败；Daemon 的 SQLite queue ack 先在同一事务写入 typed user message，再标记 queue consumed，并记录真实 `turn_id`。任何持久化失败都停止 Engine，避免“已消费但未进入上下文”。
- 旧消息行只在入口转换为 typed message；生产执行统一走 `run_with_typed_messages`。Provider context window 进入 snapshot，机械 compaction 记录替换范围。
- dangling tool call 不再删除 assistant call；按源调用顺序保留真实结果并补 `DANGLING_TOOL_CALL` 错误结果，避免无配对或结果乱序。
- side-effect ledger 的 `started` 记录在实际 Gateway handler 前必须成功；skill/task 等不经过 Gateway 的编排工具不创建孤立 ledger 行；完成记录失败返回 `PERSISTENCE_FAILED`，Core 在生成配对 Tool Result 后 fail closed。重复状态更新保留原 started 时间。
- MCP HTTP/SSE transport 逐行读取 `data:` 帧，转发 `notifications/progress`，并在 cancel/timeout 时 kill + wait + join reader；不改变 Gateway 的安全边界。
- Retry/Continue 的 `resume_plan` 从 `approved` 开始，只有 detached run 启动成功后才转为 `executed`。

本提交没有实现完整 TurnPolicy、Tool Scheduler 重写、UI ProjectionRecovery 展示、真实 provider/shell/MCP fixture 或 workspace/frontend 全量测试；这些仍是后续验收项，不在本报告中伪称完成。

## 10. 本轮收口补充（`8bed49a2`）

- Provider `ReasoningDelta` 现在进入 typed Assistant `Thinking` block；取消、Provider Error、空响应和 retry backoff 的失败路径补齐 `MessageCompleted`/`TurnCompleted` 事实。
- Tool Call 只有 `tool_use` stop reason 才可执行；`stop`、`length`、`cancelled`、`error`、`unknown` 和无 Final 均 fail closed，并保留同 ID 错误 Tool Result。
- `DaemonToolProgressSink` 由 50ms 丢弃改为 8KiB/250ms 合并；settled call 的迟到更新丢弃。
- `ProjectionRecovery` 由 adapter 暴露并写入 workspace reducer；gap 与缺失权威终态保持 Renderer 本地状态，不产生伪造 Daemon Event。
- 资源记录：共享 target `/Users/ldh/Downloads/project/AiNative/Natives/.cargo-target-shared`；本轮受控 `cargo check` 2 次、精准 `cargo test` 1 次；最大观测 shared target 2.7 GiB、可用磁盘约 70 GiB；未执行 `cargo clean`，未主动终止任务进程。

## 11. Typed Provider 生产接线（`b2ab95d3`）

- `RealProvider`、`RoutedProvider` 和 `Sub2ApiPoolProvider` 现在都显式覆盖 `EngineProvider::stream_turn`；生产 `ProviderTurnRequest` 不再落入 trait 默认的 typed→`EngineMessage` 兼容转换。
- Daemon 只在 provider-neutral `HistoryMessage` 边界把 typed blocks 转成 adapter 输入；旧 `EngineMessage` helper 仍保留给 legacy/fixture 路径。
- 修复 legacy assistant turn fallback 将 Thinking block 错写成普通文本的问题。

## 12. 最终受控验收补充（`0a2542a0`）

- `RUSTFLAGS=-Dwarnings cargo check -p natives -p natives-agent-daemon -p assistant-protocol -p agent-core -p provider-adapters -p capability-gateway` 通过；仅兼容测试转换 helper 使用 `#[allow(dead_code)]`，不进入生产主链。
- legacy delta-only assistant persistence 仍保留 duration-bearing `reasoning` block；生产带 `MessageCompleted` 的 typed Thinking 不重复写兼容块；`conversation_round_trip_uses_daemon_tables` 精准测试通过。
- 全 workspace 首次运行暴露的两个 tool fixture 已改为显式 `ToolUse` stop reason，并各自精准复验通过；未伪称 workspace 已重新全量通过。

## 13. Fixture 与 Ledger 持久化收口（`3bc18472`）

- `side_effect_ledger` 的测试 fallback 使用真实临时 SQLite `DataStore`，不再以 no-op store 掩盖 Plan Mode/ledger 断言；生产路径无 DataStore 时仍 fail closed。
- `ToolThenText`、权限请求、慢工具取消等 fixture 明确发出 `CompletedWithReason::ToolUse`，与 Core 的 fail-closed stop-reason 语义一致；不放宽生产执行条件。
- 这些变更只修正测试/fixture 的事实输入与测试持久化装配，未改变 RunManager 的终态权威或生产安全边界。

## 14. 受控验证边界

- 最新精确验证继续通过：typed provider seam、legacy reasoning round-trip、compaction fallback、tool-call collection、plan/permission/MCP/cancel 关键测试，以及 `RUSTFLAGS=-Dwarnings cargo check`。
- `cargo test --workspace` 未重复运行（共享 target/磁盘策略）；首次运行暴露的两个 fixture 已精准复验。
- 前端依赖已按锁文件在本 Worktree 安装；`npm run typecheck`、`npm run lint`、`npm run perf:check` 通过。首次 `npm run test` 为 751/752：唯一失败是与新投影不变量冲突的旧“合成终态”断言；已改为 `AuthoritativeEventMissing`/本地 recovery 断言，精准用例通过，但按资源规则未重复整个前端套件，因此不宣称全量测试绿。

## 15. 本轮生产收口（`eaf54ac7`、`1d235cd5`）

- `ProductionRuntime::start_run` 现在接收并使用 `RunStartContext.parent_run_id`；父 Run 取消会真实级联到 child token。Native fixture gateway 同样继承已解析的 project root，不扩大路径作用域。
- 单元测试构造器不再启动会访问 SQLite/环境锁的 subagent reaper，避免测试 runtime drop 死锁；生产与集成构建仍保留 reaper。
- Prompt Queue 的 `send_now` 和 terminal drain 只在 `RunManager` 接受新 Run 后删除 durable row；同步启动失败会 requeue actor item 并保留 SQLite 行。`SessionCoordinator::requeue` 对已存在的 claimed item 也恢复为 queued，避免重复或 Running 残留。
- 新增精准证据：`production_runtime_registers_child_under_parent_token`、`dual_provider_engine_fixture_subagent`、`start_with_seams_loads_daemon_conversation_history`、`send_now_cancels_active_and_starts_new_run`、`requeue_restores_claimed_item_at_front` 均通过；strict `cargo check -Dwarnings`、`cargo fmt --check`、`git diff --check` 通过。
- `fork_copies_typed_transcript_with_new_message_ids_and_ask_profile` 通过，验证 Fork 的 SQLite typed transcript 复制、消息 ID 重映射和权限 profile 重置。
- `continue_creates_lineage_from_durable_checkpoint` 通过，验证 Continue 只从 durable checkpoint/snapshot 创建独立 Run，并将 `resume_plan` 从 `approved` 结算为 `executed`。

## 16. Renderer 投影测试校正（`9c066af5`）

- 旧测试曾要求在只有 `run.list` 终态而缺少权威事件时合成 `failed` 事件，这与 Projection 分型设计冲突。
- 现在测试验证 `AuthoritativeEventMissing`，并确认 `ProjectionIncomplete(authoritative_event_missing)` 仅写入 Renderer 本地恢复状态，不伪造 Daemon sequence。
- 精准测试通过；`npm run typecheck`、`npm run lint`、`npm run perf:check` 通过。全量 `npm run test` 的首次退出仍保留为 751/752，未重复全量套件。

## 17. 当前资源与验证记录

- `npm ci --ignore-scripts` 使用 `package-lock.json` 安装 801 个包；`node_modules` 未被 Git 跟踪。
- 当前共享 Cargo target 约 7.5 GiB，Worktree target 约 5.6 GiB，可用磁盘约 60 GiB；没有遗留 Cargo/Rustc/Node 测试进程。
- 未执行 `cargo clean`、未删除共享 target、未 push；原始 Worktree 仍保持用户的 dirty 状态。

## 18. 本轮生产接线补充

- `ProductionRuntime` 现在为每个运行实例持有 `CheckpointManager`；`new_with_event_store` 将 Checkpoint、EventLog 与同一个 `DataStore` 配对，生产工具和 rewind RPC 不再默认打开进程级 Checkpoint 数据库。
- `ProductionRuntime::start_run` 对成功、取消和失败 outcome 都尝试持久化完整 typed assistant turn；`conversation_store` 只跳过带 `TurnStarted` 但缺少 `TurnCompleted` 的部分 typed turn，旧 delta-only 兼容批次仍可加载。
- 写工具的 checkpoint before/after、side-effect ledger start/complete 失败均 fail closed；after-image 或 ledger completion 无法落库时不返回成功输出，并将副作用标记为 `uncertain`。
- 动态 MCP 工具在进入 permission/transport 前按广告 Schema 校验；未注册的 namespaced MCP 名称返回 `UNKNOWN_TOOL`，不进入 transport。
- `DaemonToolProgressSink` 与 settled 状态使用同一锁序，保证结算与迟到进度不会交叉产生事件；`ToolOutputDelta` 增加可选 `tool_name`，保留旧协议反序列化兼容。
- 受控验证：严格 `cargo check -Dwarnings` 通过；`settled_tool_drops_late_progress` 精测通过。此前 workspace/native verifier 和前端全量测试仍按资源上限未重跑，不能宣称全量绿。

## 19. 本轮安全点与恢复计划补充

- Follow-up 现在先提交上一 Assistant Message/Turn 的完整结算事实，再在 `BeforeRunEnd` 消费 durable queue；Steering 也在 ToolCallCompleted、MessageCompleted、TurnCompleted 之后消费，ack 失败不会留下未关闭的 Turn。
- Permission 请求、interaction row、session actor snapshot 和 PermissionResponded 现在均按 fail-closed 处理；Permission Broker 错误不再被随机 UUID 替换，未持久化的权限结果不会创建 grant。
- Retry/Continue 的 `resume_plan` 不再由 RPC 在 `Preparing` 返回时提前标记 `executed`；RunManager 在运行计划持久化完成、真正进入执行前结算 approved plan，失败仍保持可审计的未结算状态。
- 本轮新增精准证据：`engine::tests::follow_up_closes_previous_turn_before_next_provider_call` 通过；严格 `cargo check -Dwarnings` 通过。完整 workspace/native verifier 和真实外部 fixture 仍未重新运行。

## 20. 本轮队列与权限收口补充

- `DurableInputReceiver::drain` 不再把 SQLite store、事务、查询或提交错误折叠为空队列；错误会穿过 `EngineInputReceiver` 传播到 Agent Core，阻止下一次 Provider 调用。
- Permission interaction、actor snapshot、resolved response 和 PermissionResponded 任一关键持久化失败都会返回 `PERMISSION_PERSISTENCE_FAILED` 或拒绝结果；批准 grant 只在权威响应事件持久化后记录。
- Retry/Continue 的 `resume_plan` 由 RunManager 在运行计划持久化后结算，RPC 不再在 Preparing 响应阶段提前结算。
- 最终精确 Follow-up Turn 边界测试通过；因固定测试 Run ID 会读取默认持久化事件日志，本轮将该 fixture 改为 UUID Run ID，避免跨次测试污染。
- 本轮最终 strict Cargo check、fmt/diff 检查和精确测试均通过；workspace/native verifier、完整前端套件和真实外部 Provider/Shell/MCP fixture 仍未按资源上限重跑。

## 21. Durable lease、恢复读取与进度窗口收口

- Durable Steering/Follow-up 的 `PendingInput` 现在携带 SQLite lease token；drain 的 UPDATE 必须恰改一行，ack 同时校验 token，并在单事务内写入 typed user message 与 consumed 状态。
- 生产启动不再把 actor snapshot、typed/legacy history 或 checkpoint snapshot 的读取错误折叠为空上下文；branch lookup 与损坏的 `MessageCompleted` content 也 fail closed。
- Permission response 先写 resolved interaction，再唤醒绑定 Run 的 waiter；Retry/Continue 的 blocked/approved `resume_plan` 写入失败均拒绝继续，approved 失败会结算新 Run 为失败。
- `DaemonToolProgressSink` 为非终态首条更新安排 250ms bounded flush，保留 8KiB/age flush 与 settled late-drop，不引入完整 Progress Sink 架构。
- 本轮精确验证：严格 `RUSTFLAGS=-Dwarnings cargo check`、`progress_flushes_after_batch_window_without_next_update`、`cargo fmt --check` 与 `git diff --check`。完整 workspace/native verifier、前端全量套件、真实外部 Provider/Shell/MCP fixture 与 permission fault-injection 仍未重新运行，不能宣称全量绿。

## 22. 当前 Worktree 最终生产接缝

- 恢复读取不再依赖 `unwrap_or_default`/`filter_map`：typed message、tool-result block、Active Context、EventLog 和 Checkpoint snapshot 在结构损坏时停止。
- 启动阶段的 Checkpoint 与 engine registry 具备回滚/延迟注册语义；引擎运行结束后先移除 handle，再执行事件重放与后续持久化。
- 工具完成后的 ledger 结算写失败会记录 conservative `uncertain` 状态并返回 `PERSISTENCE_FAILED`，避免副作用已发生却向模型报告成功。
- 本节未扩大 UI/RPC/DB surface，也未引入完整 TurnLoop、Scheduler 重写、Progress Sink 新协议或 Pi Runtime。
- 严格 warning-as-error Cargo check、fmt 和 diff check 通过；完整 workspace/native verifier、真实 provider/shell/MCP 与 permission fault injection 仍未验证。

## 23. 生产闭环与恢复边界再收口

- 调度器不再按 `task` 名称决定批处理；Gateway capability 是唯一并发依据。通用 tool-call wrapper 仍负责 Sub Agent progress，避免移除特判后丢失进度。
- Compaction summary 现在可在 `message`/`message_block` 重放；snapshot 与 durable history 合并按 message identity 去重。普通会话的 nullable `branch_id` 读取已修复并有压缩回归证据。
- Queue recovery 与 event replay 的 SQL/JSON 错误向上返回；RPC 以错误响应结束，不再发送空事件数组。MCP ledger settle 失败转为 uncertain/PERSISTENCE_FAILED；child event replay 损坏转为 SubagentFailed。
- 本轮未扩大 UI、RPC 或数据库 schema surface，也未引入 TurnLoop/Scheduler/Progress Sink 的后续架构重写。
- 精确测试和受控 strict check 通过；真实外部 fixture 与资源受限的全量命令仍保持未验证状态。
