# Agent Core 深化开发进度

## 开发基线

- 分支：feat/agent-core-deepening
- Worktree：/Users/ldh/Downloads/project/AiNative/Natives-agent-core-deepening
- 开始 Commit：8bfade4babd443267ef59299516580cd7d83569e
- P0 Commit：a601636、bd3f88c
- Pi Commit：583f153d502aa8e958eefdb9af0fbd3344e68f95

## 阶段状态

| 阶段 | 状态 | Commit | 精准测试 | 备注 |
|---|---|---|---|---|
| A. P0 强化验证 | 已完成 | 81aad0f | agent-core 161；provider reason 1 | 复核文档、截断/拒绝回归测试复用 P0 基线并补齐跨 Provider reason 映射 |
| B. Turn 与类型化消息 | 已完成 | 81aad0f | agent-core 161 | opaque IDs、Turn/Message 事件、ProviderTurnRequest；保留 EngineMessage 兼容转换 |
| C. Context 与持久历史 | 部分完成 | 工作区变更 | context_snapshot 1 | Conversation/turn/message/context_snapshot additive migration 与 typed roundtrip 已接入；Active Context 无损快照仍待完整生产重放验证 |
| D. Tool Scheduler 与 Progress | 部分完成 | 工作区变更 | agent-core 161；daemon check | Gateway 能力声明驱动调度；Daemon 已接入状态/终端输出事件，完整 rate-limit/progress sink 仍未实现 |
| E. Steering 与 Next Turn | 部分完成 | 工作区变更 | agent-core check | Core receiver/ack 已接入；Prompt Queue 具备 SQLite lease/recovery，仍需真实 daemon 重启与多 Run 验证 |
| F. Event Fail Closed | 部分完成 | 81aad0f | agent-core 162 | Turn/Message 关键事实使用 append_checked；新增 persistence fault injection；旧非关键 delta 保持可丢弃 |
| G. Lineage 与 Resume | 部分完成 | 81aad0f | lineage 1 | 新增 lineage/resume safety model；Checkpoint/DB 接线留待下一阶段 |
| H. Projection 与权限清理 | 部分完成 | 81aad0f | daemon check | 生产路径禁用全局 profile setter；UI projection reducer 尚未迁移 |
| I. 最终验证 | 部分完成 | 待提交 | 受控 cargo check 已执行；完整 workspace/frontend 验证待最终单次窗口 |

## 阻塞项

- 资源策略限制 Cargo 次数：共享 target `/Users/ldh/Downloads/project/AiNative/Natives/.cargo-target-shared`，jobs=2、incremental=0；完整 workspace test 仍未运行。
- 前端校验依赖与 live provider/shell/MCP fixture 尚未在本 Worktree 验证。

## 设计偏差

- 新 Worktree 基于 P0 最新提交；本轮只做 additive 生产接线，不改变 RPC/UI/RunManager terminal authority。
