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
| C. Context 与持久历史 | 部分完成 | 81aad0f | context_snapshot 1 | 新增 FullHistory/ActiveContext/Snapshot 类型；Conversation DB 迁移留待下一阶段 |
| D. Tool Scheduler 与 Progress | 部分完成 | 81aad0f | agent-core 161；daemon check | Gateway 能力声明和默认顺序调度；Progress Sink seam 暂不接入 UI |
| E. Steering 与 Next Turn | 部分完成 | 81aad0f | agent-core check | Core 安全点 receiver/ack 已存在；Prompt Queue 持久 lease 适配留待下一阶段 |
| F. Event Fail Closed | 部分完成 | 81aad0f | agent-core 162 | Turn/Message 关键事实使用 append_checked；新增 persistence fault injection；旧非关键 delta 保持可丢弃 |
| G. Lineage 与 Resume | 部分完成 | 81aad0f | lineage 1 | 新增 lineage/resume safety model；Checkpoint/DB 接线留待下一阶段 |
| H. Projection 与权限清理 | 部分完成 | 81aad0f | daemon check | 生产路径禁用全局 profile setter；UI projection reducer 尚未迁移 |
| I. 最终验证 | 待开始 |  |  |  |

## 阻塞项

- 无

## 设计偏差

- 新 Worktree 基于 P0 最新提交；上一轮研究架构文档未进入 P0 Git 提交，本轮会以实际代码和新增报告恢复必要证据索引。
