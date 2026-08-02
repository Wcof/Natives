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
| I. 最终验证 | 部分完成 | 9c066af5 | strict check + controlled frontend verification | strict `-Dwarnings` check、typed seam、legacy reasoning、stop-reason fixture、ledger/permission/cancel、child cancel、prompt requeue、projection recovery、fork/continue persistence 精测通过；typecheck/lint/perf 通过；workspace/native verifier 与完整 frontend test 仍未全绿 |

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

## 设计偏差

- 新 Worktree 基于 P0 最新提交；本轮新增一个 `run.continue` RPC 方法及 renderer wire 字段，但未改变 RunManager terminal authority、UI 行为或数据库既有列语义。
