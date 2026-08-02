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
| I. 最终验证 | 部分完成 | b2ab95d3 | controlled check + precise test | 目标 crate check、typed provider seam 精测、fmt、git diff 通过；workspace/native live/frontend 仍受既有环境限制 |

## 阻塞项

- 资源策略限制 Cargo 次数：共享 target `/Users/ldh/Downloads/project/AiNative/Natives/.cargo-target-shared`，jobs=2、incremental=0；本轮未重复 workspace test。
- 前端校验与 live provider/shell/HTTP-MCP fixture 尚未在本 Worktree 验证；stdio/HTTP MCP 完成代码级接线但无真实外部 fixture。
- `start_with_seams_loads_daemon_conversation_history` 独立运行无输出超过两分钟后中止，保持未验证状态。
- `b2ab95d3` 将 `ProviderTurnRequest` 直接接入三个生产 provider 实现；旧 `EngineMessage` 转换保留为 legacy/fixture 兼容边界，并补 typed block identity 精测。

## 设计偏差

- 新 Worktree 基于 P0 最新提交；本轮新增一个 `run.continue` RPC 方法及 renderer wire 字段，但未改变 RunManager terminal authority、UI 行为或数据库既有列语义。
