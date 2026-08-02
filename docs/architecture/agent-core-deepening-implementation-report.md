# Agent Core 深化实施报告

## 1. 开发基线

- 分支：`feat/agent-core-deepening`
- Worktree：`/Users/ldh/Downloads/project/AiNative/Natives-agent-core-deepening`
- 开始 Commit：`8bfade4babd443267ef59299516580cd7d83569e`
- P0 Commit：`a601636`、`bd3f88c`
- Pi 参考 Commit：`583f153d502aa8e958eefdb9af0fbd3344e68f95`
- 原工作区：保持 dirty，未 reset、stash 或覆盖。

## 2. 实施内容

### 2.1 P0 复核与测试

`agent-core-p0-revalidation.md` 逐项复核 Credential、API mode、Stop Reason、Schema、
Hook Deny 和 Cancel。P0 已有的截断/拒绝/Schema/Cancel 测试继续作为基线；本轮增加
Provider stop reason family 映射测试。

### 2.2 Turn、Message 与 Provider seam

- `message.rs` 增加 `EngineRunId`、`TurnId`、`MessageId`、`ToolCallId` 和 typed message/content/result 类型。
- `turn.rs` 增加 `TurnOutcome`、`Usage`。
- `EngineProvider::stream_turn(ProviderTurnRequest, ...)` 是兼容默认方法，生产仍由 Daemon 装配 Provider。
- Protocol v2 增加 additive `turn_*` 与 `message_*` 事件；Run terminal 仍由 RunManager 提交。

### 2.3 Context、Lineage 与恢复模型

- `context_snapshot.rs` 分离 `FullHistory`、`ActiveContext`、`ActiveContextSnapshot`，包含 source revision、provider window 和 compaction artifact ref。
- `lineage.rs` 增加 lineage fields、side-effect status、`ResumePlan`；未知副作用会使 resume 不安全。
- 本轮不修改 Conversation DB，不声称已完成无损迁移。

### 2.4 Tool Capability、Progress 与输入安全点

- `ToolCapability` 和 `ToolExecutionMode` 由 Daemon/Gateway 广告；生产 ReadOnly→ParallelSafe，Process/Destructive→Exclusive，其余 Sequential；未知默认 Sequential。
- 删除 Engine 生产路径对工具名称的并行分类依赖；保留旧 helper 供兼容代码。
- `DaemonToolProgressSink` 已接入 Shell、MCP stdio/HTTP/SSE 和 Sub Agent；按 8KiB/250ms 合并并在 tool settled 后丢弃迟到更新，尚未接入完整 UI Progress Sink。
- `EngineInputReceiver` 定义 Steering/FollowUp、DrainMode、safe point、ack；现有 Prompt Queue 仍是持久化权威。

### 2.5 Event Fail Closed 与权限边界

- Turn/Message lifecycle 使用 `EventSequencer::append_checked`；delta 仍可 best-effort，避免把展示流误当状态事实。
- `ProductionRuntime::set_permission_profile` 仅在测试编译保留；生产 PermissionGatedTools 使用 RunStartContext 的 profile-bound 请求，避免跨 Run 全局 profile 变更。

## 3. 能力矩阵

| 能力 | 当前状态 | 证据 | 结论 |
|---|---|---|---|
| Turn 生命周期 | 已接入 additive event | `engine.rs`、`run_event.rs` | 可重放 lifecycle，仍保留兼容主循环 |
| 类型化消息 | Core 内部类型已创建 | `message.rs` | UI/RPC 尚未迁移 |
| Tool Schema | P0 已 fail closed | Gateway `validate_schema` | Core 不复制 registry |
| 并行工具 | Gateway capability 驱动 | `production_tools.rs` | 默认顺序，未知不并行 |
| Tool Progress | 生产 sink 已接线 | `DaemonToolProgressSink`、`production_tools.rs` | UI 展示与外部 SSE fixture 留后续 |
| Steering/Follow-up | Core safe point receiver | `input.rs` | durable lease 留后续 |
| Context Snapshot | 可序列化运行期模型 | `context_snapshot.rs` | DB/artifact 留后续 |
| Resume | safety model | `lineage.rs` | 不自动重放未知副作用 |
| Permission | 主路径 Run-bound | `production.rs` | global setter 仅 test |

## 4. 验证结果

| 命令 | 结果 | 说明 |
|---|---|---|
| `CARGO_BUILD_JOBS=4 RUST_TEST_THREADS=4 cargo test -p agent-core --lib` | 通过 | 161 passed |
| `cargo test -p provider-adapters ...normalizes_all_provider_stop_reason_families` | 通过 | 1 passed |
| `cargo check -p agent-core -p natives-agent-daemon` | 通过 | Provider/Daemon seam 编译 |
| `cargo check -p assistant-protocol` | 通过 | additive event variant 编译 |
| `cargo fmt --check` | 通过 | 无格式差异 |
| `cargo test --workspace -- --test-threads=4` | 环境失败 | 编译阶段因 `No space left on device`，未发现本次代码断言失败 |
| `npm run protocol:check` | 通过 | TS/Rust protocol surface aligned |
| `npm run typecheck` / `lint` / `test` / `perf:check` | 环境未运行 | worktree 未安装 `tsc`/`eslint`/`tsx` |
| `npm run verify:native-engine` | 环境失败 | 临时目录因 `No space left on device` |
| `git diff --check` | 通过 | 无 whitespace 错误 |

## 5. 未完成或偏差

- 未修改 Conversation 数据库、UI projection、Prompt Queue 产品语义。
- 未实现完整 UI Tool Progress 展示、TurnPolicy 多策略、事件 fault-injection 全链路。
- Provider live HTTP fixture、Shell/MCP kill+wait 和并行整体 cancel 仍需阶段性集成测试。

## 6. 阶段 1 前置条件

- 为 `ActiveContextSnapshot` 建立 daemon-owned artifact/migration 与旧记录兼容 fixture。
- 为 `EngineInputReceiver` 接入现有 Queue lease/ack，并定义 daemon 重启恢复规则。
- 将所有关键 Tool/Permission/terminal 事实迁移到 checked event sink 后再开放 projection reducer。

## 7. 风险与回滚

- Additive protocol 变体需要旧客户端忽略未知 event type；不改变既有 RunEvent terminal payload。
- capability 未声明时顺序执行，性能保守但安全；回滚只需移除 production capability override。
- 新类型均为内部/兼容 seam；若 migration fixture 不通过，不合并 DB 或 UI 接线。

## 8. 本轮闭环补充（基于 `c6495f06`）

- 生产 Daemon 现在优先调用 `AgentEngine::run_with_typed_messages`；只有旧数据库没有 typed rows 时才走 `EngineMessage` 兼容读取。
- `MessageCompleted` 携带完整 provider-neutral content；Context Snapshot 持久化 branch、turn、source revision 与 token estimate，Tool Result artifact 保留引用和错误码。
- Gateway capability metadata 直接驱动 Core scheduler；Shell stdout/stderr、MCP stdio progress notification、Sub Agent child event 进入稳定 call/turn/message progress sink，并在 settled/cancel 后丢弃迟到更新。
- Gateway 通用执行包装器在 timeout/cancel 后等待 250ms 清理；无法 quiet 返回 `cleanup_failed`。HTTP/SSE MCP 的 curl 子进程可被取消杀掉并逐行转发中间 progress。
- Tool completion 关键事实持久化失败会取消 Engine，并由 `mark_tool_call_uncertain` 写入 side-effect ledger，阻止后续自动恢复。

## 9. 生产收口补充（`8bed49a2`，含 `5734ef5f` 与 `39d6514f`）

- 旧消息只在生产入口转换一次为 `AgentMessage`，Core 主执行路径统一为 typed transcript；空历史不再切换 legacy Agent Loop。
- Durable prompt queue 的 ack 现在可返回错误；SQLite 消息写入、turn 绑定与 queue consumed 在同一事务中完成，失败即停止输入消费。
- Compaction 的 dangling repair 保留 assistant Tool Call，并按调用源顺序补齐 `DANGLING_TOOL_CALL` 错误 Tool Result；实际 Gateway handler 的 Side-effect ledger start/complete persistence failure 统一为 `PERSISTENCE_FAILED`，Core 生成配对结果后停止，编排工具不留下孤立 started 行。
- HTTP/SSE MCP 逐行读取 JSON/SSE 帧并转发 progress，cancel/timeout 会 kill、wait、join；Retry/Continue 的 resume plan 在 detached start 成功后才结算。

本轮仍未声称完成真实外部 fixture、Renderer recovery 的专用 UI 文案、workspace/frontend 全量测试或后续 TurnPolicy/Scheduler 重构。

## 10. Provider typed seam 收口（`b2ab95d3`）

- 生产 `RealProvider`、`RoutedProvider`、`Sub2ApiPoolProvider` 显式实现 `stream_turn`，直接接收 Core 的 `ProviderTurnRequest`。
- typed transcript 只在 provider-neutral `HistoryMessage` 边界转换；`EngineMessage` 仅保留旧数据库/fixture 兼容路径。
- 增加 Thinking/Image/ToolCall/ToolResult identity 回归测试，并修正 assistant fallback Thinking 内容。

## 11. 最终受控验收（`0a2542a0`）

- strict `RUSTFLAGS=-Dwarnings cargo check` 覆盖 natives、daemon、protocol、Core、provider-adapters、capability-gateway，通过。
- legacy delta-only conversation fixture 保留 duration-bearing reasoning compatibility block；带 `MessageCompleted` 的 typed path 只持久化 canonical Thinking。
- workspace 首次运行中两个旧 tool fixture 因未声明 `ToolUse` stop reason 失败，已改 fixture 并用两个精准测试复验通过；workspace 未因资源策略重复运行。

## 12. Fixture 与 Ledger 收口（`3bc18472`）

- 测试环境的 side-effect ledger fallback 改为真实临时 SQLite `DataStore`，仅在测试且未装配全局 store 时启用；生产路径仍在无 store 时 fail closed。
- ToolThenText、权限请求和取消 fixture 显式携带 `CompletedWithReason::ToolUse`，保留阶段 0 对未知/不完整 stop reason 的拒绝语义。
- 该提交未引入 Turn/Message/Scheduler/Projection 的新生产架构，只修正验证装配和 fixture 事实。

## 13. 本轮剩余生产接线收口（`eaf54ac7`、`1d235cd5`）

- `RunStartContext` 携带真实 `parent_run_id`，ProductionRuntime 将 child token 注册到父取消树；fixture gateway 也按 Run 的 project path 设置 project root。
- 测试构造器跳过后台 subagent reaper，避免环境锁与 Tokio runtime drop 的互等；非测试构建仍启动生产 reaper。
- Prompt Queue 在 `send_now` 与 terminal drain 中延迟删除 durable row，RunManager 同步启动失败时恢复 actor item；`SessionCoordinator::requeue` 同时覆盖已存在的 claimed item，防止 Running 状态残留。
- 精准回归已通过：child cancellation、live subagent fixture、daemon history seam、send-now drain、prompt requeue；strict warning check、fmt 和 diff check 也通过。

## 14. Renderer 验证收口（`9c066af5`）

- 修正与当前 Projection 设计相反的旧测试：终态数据库状态没有权威事件时，adapter 必须抛出 `AuthoritativeEventMissing`，而不是合成 `failed` 事件。
- `npm run typecheck`、`npm run lint`、`npm run perf:check` 通过；前端全量测试首次为 751/752，修复后的失败用例已单独精准通过，未重复完整套件。

## 15. Durable 输入、恢复与进度收口（当前 Worktree）

- `PendingInput` 增加 durable lease token；SQLite drain/ack 绑定 token 并检查单行更新，避免并发或重试跨 lease 消费。
- 生产 history、actor snapshot、checkpoint snapshot 和 branch lookup 的数据库错误不再回退为空；损坏的 typed `MessageCompleted` 直接停止恢复。
- Permission response 先持久化 resolved interaction，再唤醒 waiter；Retry/Continue 的 `resume_plan` 写入失败不再被忽略。
- Progress sink 增加 250ms bounded idle flush，并保留 settled late-drop；未引入完整 Progress Sink 或 Scheduler 重构。
- 验证：严格 `RUSTFLAGS=-Dwarnings cargo check` 与 `progress_flushes_after_batch_window_without_next_update` 通过；完整 workspace/native verifier、前端全量测试、真实外部 Provider/Shell/MCP fixture、permission fault-injection 未按资源策略重跑。
