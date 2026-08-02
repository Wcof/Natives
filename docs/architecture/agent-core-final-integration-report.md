# Agent Core 最终深化与生产闭环报告

## 1. 基线

- Worktree：`/Users/ldh/Downloads/project/AiNative/Natives-agent-core-deepening`
- 分支：`feat/agent-core-deepening`
- 起始 Commit：`0aadad5316f844fe6312d70472bff478049f9088`
- 结束 Commit：`fefc8ff0`（当前 Worktree）；本报告随后同步更新
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
| `rtk cargo check -p agent-core -p capability-gateway -p provider-adapters -p natives-agent-daemon -p natives` | 通过（本轮最新，0 errors） |
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
| `rtk npm run typecheck` | 未执行：worktree 无 `node_modules/.bin/tsc`，命令退出 127 |
| `CARGO_TARGET_DIR=... cargo test --workspace -- --test-threads=2` | 运行：agent-core 首次 160/162 通过；两个 legacy fixture 因仍发送无授权 `Completed` 失败；改为显式 `CompletedWithReason::ToolUse` 后两个精准回归通过，未重复 workspace 全量 |
| frontend lint/test/perf | 未运行，依赖未安装 |

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
- 本轮 Cargo check：4（含 2 次修复后重跑）；本轮精准 Cargo Test：1；Workspace Test：0。
- 最大 target 大小：共享 target 约 2.5 GiB；native verifier 产生的本地 target 约 5 GiB。
- 最低可用磁盘：约 70 GiB。
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
