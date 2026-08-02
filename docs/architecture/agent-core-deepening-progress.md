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
| C. Context 与持久历史 | 已接生产 | 工作区变更 | typed roundtrip 1 | typed message 分组持久化；ContextSnapshotCommitted 写入完整 snapshot_json；生产启动按 input_message_ids 合并 snapshot 与压缩后新消息 |
| D. Tool Scheduler 与 Progress | 已接生产 | 工作区变更 | cargo check | Gateway side-effect 驱动模式、整批顺序降级、真实 call-id/turn/message progress、settled late-drop/rate-limit 已接线 |
| E. Steering 与 Next Turn | 已接生产 | 工作区变更 | cargo check | SQLite lease/recovery 与 queue-message 单事务 ack 已接线 |
| F. Event Fail Closed | 已接生产（关键事实） | 工作区变更 | cargo check | Turn/Message/Tool prepared/started/completed、attempt committed、checkpoint open/commit、snapshot、permission 使用 checked；delta 仍可丢弃 |
| G. Lineage 与 Resume | 已接生产（部分） | 工作区变更 | side-effect guard | checkpoint cursor/uncertain retry guard/resume_plan retry 记录已接线；continue/fork/replay RPC 未扩展 |
| H. Projection 与权限清理 | 已接生产 | 工作区变更 | TypeScript source | projection recovery 类型与无伪造终态路径已接入 adapter |
| I. 最终验证 | 部分完成 | 待提交 | controlled check + precise test + protocol | agent-core compaction 5、gateway 91、daemon typed 1、目标 check、fmt、protocol 通过；workspace/native live/frontend 仍受既有环境限制 |

## 阻塞项

- 资源策略限制 Cargo 次数：共享 target `/Users/ldh/Downloads/project/AiNative/Natives/.cargo-target-shared`，jobs=2、incremental=0；本轮未重复 workspace test。
- 前端校验与 live provider/shell/MCP fixture 尚未在本 Worktree 验证。
- `start_with_seams_loads_daemon_conversation_history` 独立运行无输出超过两分钟后中止，保持未验证状态。

## 设计偏差

- 新 Worktree 基于 P0 最新提交；本轮只做 additive 生产接线，不改变 RPC/UI/RunManager terminal authority。
