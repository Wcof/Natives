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
| C. Context 与持久历史 | 部分完成 | 工作区变更 | typed roundtrip 1 | typed message 已从生产 DB 回放；Active Context 无损 artifact 重放仍待验证 |
| D. Tool Scheduler 与 Progress | 已接生产 | 工作区变更 | cargo check | Gateway side-effect 驱动模式与真实 call-id progress 已接线；MCP 全量 sink/rate-limit 未完成 |
| E. Steering 与 Next Turn | 已接生产 | 工作区变更 | cargo check | SQLite lease/recovery 已装配；ack 与 message persistence 的事务屏障仍待补齐 |
| F. Event Fail Closed | 已接生产 | 工作区变更 | cargo check | Tool lifecycle 与 attempt committed 改为 checked；delta 仍按低价值可丢弃处理 |
| G. Lineage 与 Resume | 部分完成 | 工作区变更 | side-effect guard | checkpoint cursor/uncertain retry guard 已接线；resume_plan/fork RPC 未完成 |
| H. Projection 与权限清理 | 已接生产 | 工作区变更 | TypeScript source | projection recovery 类型与无伪造终态路径已接入 adapter |
| I. 最终验证 | 部分完成 | 待提交 | controlled check + precise test + protocol | protocol 通过；native-engine 静态 audit OK 但 daemon lib 2 项/live e2e 1 项失败；frontend 依赖缺失 |

## 阻塞项

- 资源策略限制 Cargo 次数：共享 target `/Users/ldh/Downloads/project/AiNative/Natives/.cargo-target-shared`，jobs=2、incremental=0；完整 workspace test 仍未运行。
- 前端校验与 live provider/shell/MCP fixture 尚未在本 Worktree 验证。

## 设计偏差

- 新 Worktree 基于 P0 最新提交；本轮只做 additive 生产接线，不改变 RPC/UI/RunManager terminal authority。
