# Agent Core 深化开发进度

## 开发基线

- 分支：feat/agent-core-deepening
- Worktree：/Users/ldh/Downloads/project/AiNative/Natives-agent-core-deepening
- 开始 Commit：0aadad5316f844fe6312d70472bff478049f9088
- P0 Commit：a601636、bd3f88c
- Pi Commit：583f153d502aa8e958eefdb9af0fbd3344e68f95

## 阶段状态

| 阶段 | 状态 | Commit | 精准测试 | 备注 |
|---|---|---|---|---|
| A. P0 强化验证 | 已完成 | 81aad0f | agent-core 161；provider reason 1 | 复核文档、截断/拒绝回归测试复用 P0 基线并补齐跨 Provider reason 映射 |
| B. Turn 与类型化消息 | 已完成 | 81aad0f | agent-core 161 | opaque IDs、Turn/Message 事件、ProviderTurnRequest；保留 EngineMessage 兼容转换 |
| C. Context 与持久历史 | 已接生产 | 5734ef5f | typed roundtrip 1（前序） | typed transcript 是唯一生产入口；旧 EngineMessage 只做一次兼容转换；snapshot 记录 provider window 与替换范围 |
| D. Tool Scheduler 与 Progress | 已接生产 | 8bed49a2 | Gateway cancel 精测 1 | Gateway capability 驱动模式、conflict key、Shell stdout/stderr、MCP stdio/HTTP/SSE、Sub Agent progress、8KiB/250ms 合并、settled late-drop 已接线；真实 SSE fixture 未运行 |
| E. Steering 与 Next Turn | 已接生产 | 5734ef5f | cargo check | SQLite lease/recovery 与 queue-message 单事务 ack 已接线；ack 失败 fail closed |
| F. Event Fail Closed | 已接生产（关键事实） | 5734ef5f | cargo check | 关键事实使用 checked；ledger start/complete persistence failure 生成配对错误结果并停止 Engine；delta/progress 仍可丢弃 |
| G. Lineage 与 Resume | 已接生产 | 5734ef5f | side-effect guard | retry/continue 记录 source/checkpoint/turn lineage；resume_plan 在 detached start 后结算；continue 只从 durable checkpoint 启动独立 Run |
| H. Projection 与权限清理 | 已接生产 | 工作区变更 | TypeScript source | projection recovery 类型与无伪造终态路径已接入 adapter |
| I. 最终验证 | 部分完成 | HEAD | strict check + controlled frontend verification | strict `-Dwarnings` check、typed seam、legacy reasoning、stop-reason fixture、ledger/permission/cancel、child cancel、prompt requeue、projection recovery、fork/continue persistence 精测通过；typecheck/lint/perf/protocol 通过；workspace/native verifier 未重新运行，frontend 全量首跑仅有过期 projection 断言失败，已精准复验修正 |

## 阻塞项

- 资源策略限制 Cargo 次数：共享 target `/Users/ldh/Downloads/project/AiNative/Natives/.cargo-target-shared`，jobs=2、incremental=0；本轮未重复 workspace test。
- 前端 typecheck/lint/perf 已在安装锁文件依赖后通过；完整 frontend test 首次 751/752，旧合成终态断言已改正并精准复验，未按资源规则重复全套。live provider/shell/HTTP-MCP fixture 仍无真实外部验证。
- `start_with_seams_loads_daemon_conversation_history` 曾因测试 reaper 与环境锁死锁；已禁用单元测试中的后台 reaper 并精准复验通过。
- `b2ab95d3` 将 `ProviderTurnRequest` 直接接入三个生产 provider 实现；旧 `EngineMessage` 转换保留为 legacy/fixture 兼容边界，并补 typed block identity 精测。
- `0a2542a0` 修正 strict warning 与 legacy reasoning 兼容回归；workspace 初次运行的两个 fail-closed fixture 已精准复验通过。完整 workspace/native verifier 仍未重新宣称通过。
- `3bc18472` 让测试 ledger 使用真实临时 SQLite，并让 ToolUse fixture 明确声明 `CompletedWithReason::ToolUse`；没有放宽 Core 的 fail-closed 规则。
- `eaf54ac7` 将真实 `parent_run_id` 传入 ProductionRuntime，修复 child cancel tree 注册；测试构造器不再启动会争用环境锁的 reaper，并补齐 fixture project root。
- `1d235cd5` 让 Prompt Queue 在 RunManager 同步启动失败时保留 SQLite 行并恢复 actor 队列，补充 requeue 回归测试。
- `9c066af5` 将 Renderer 旧的“从 run.list 合成终态”测试改为权威事件缺失恢复测试；不改变生产投影的 fail-closed 行为。

## 当前 Worktree 新增收口

- Durable queue lease 现在贯穿 `PendingInput`、SQLite drain 和 ack；token 不匹配、lease UPDATE 非单行和事务错误均 fail closed。
- 生产启动/恢复的 history、actor snapshot、checkpoint snapshot、branch lookup 以及 typed MessageCompleted 解码错误不再被当作空上下文。
- Permission response 先持久化再唤醒 waiter；Retry/Continue 的 resume plan 写入错误不再被忽略。
- Tool progress 首条非终态更新增加 250ms bounded flush；settled call 的迟到更新继续丢弃。
- 本轮新增精确测试 `progress_flushes_after_batch_window_without_next_update` 通过；严格检查已通过。完整 workspace/native verifier、前端全量套件、真实外部 Provider/Shell/MCP fixture 和 permission fault-injection 仍未验证。

## 当前收口状态（当前 Worktree）

- Follow-up/Steering：**已接生产**；上一 Turn 的 `MessageCompleted/TurnCompleted` 先于 queue lease/ack 消费，ack 失败 fail closed；精准 Follow-up Turn 边界测试通过。
- Permission 持久化：**已接生产**；Broker、interaction、actor snapshot、resolved response 和 PermissionResponded 形成 fail-closed 链路；尚缺 fault-injection 精测。
- Retry/Continue Resume Plan：**已接生产**；RPC 不再在 `Preparing` 返回时提前标记 `executed`，由 RunManager 在执行计划持久化后结算。
- Checkpoint/Conversation authority：**已接生产**；Runtime 实例的 EventLog/Checkpoint 共用 DataStore；失败/取消完整 typed turn 会持久化，partial typed turn 跳过。
- 最终验证：**部分完成**；最新严格 Cargo check、fmt、git diff 和精准 Follow-up/Progress 测试通过；workspace test、native verifier 和真实外部 Provider/Shell/MCP fixture 仍无新的全量证据。

## 设计偏差

- 新 Worktree 基于 P0 最新提交；本轮新增一个 `run.continue` RPC 方法及 renderer wire 字段，但未改变 RunManager terminal authority、UI 行为或数据库既有列语义。

## 23. 恢复读取与启动资源收口（当前 Worktree）

- typed transcript、Active Context snapshot、EventLog replay 和 Checkpoint file snapshot 均严格校验身份、结构、JSON 和时间戳；损坏记录 fail closed。
- `ProductionRuntime` 仅在启动前置持久化/恢复读取成功后注册 engine；Checkpoint skeleton 写入失败会回滚 live map，运行结束先移除 handle 再做 post-processing。
- Gateway handler 已完成但 ledger settle 写入失败时追加 conservative `uncertain` 状态并返回 `PERSISTENCE_FAILED`。
- 受控 `RUSTFLAGS=-Dwarnings cargo check` 通过；workspace/native verifier、前端全量套件、真实 Provider/Shell/MCP fixture 与 permission fault-injection 仍未重新运行。

## 24. 生产闭环再审计收口（当前 Worktree）

- Core scheduler 已移除 `task` 工具名特判；生产批次只读取 Gateway 广告的 `ToolCapability.execution_mode`、`parallel_safe` 与冲突键。Sub Agent progress 改由通用调用路径按运行结果启动，不再依赖 Core 的工具名分支。
- `ContextSnapshotCommitted` 的模型摘要现在同时写入 `message`/`message_block`；旧 `ContextCompressed` 事件也生成稳定的系统摘要消息 ID。Runtime 合并 snapshot 与 durable history 时按 snapshot message ID 去重，避免压缩后的摘要尾部重复。
- Prompt Queue 启动恢复、actor snapshot、lease/status 行与 malformed queue item 均改为严格错误传播；RPC/Authority 的 event replay 使用 checked API，不再把坏事件流当成空数组。
- MCP side-effect ledger 在实际 transport 前记录 `started`，完成/失败/取消后必须 settle；settle 失败尝试 `uncertain` 并返回 `PERSISTENCE_FAILED`。Continue 的 branch parent message 查询也不再 `.ok()` 静默降级。
- 精确验证：`tool_name_does_not_select_batch_execution`、Gateway schema rejection、Gateway cancellation、`malformed_queue_snapshot_is_rejected`、`context_compression_events_persist_snapshot_and_reenter_history` 通过；受控 `RUSTFLAGS=-Dwarnings cargo check` 与 `cargo fmt` 通过。workspace/native verifier、前端全量套件、真实外部 Provider/Shell/MCP fixture 与 permission fault-injection 仍未按资源规则重复运行。
