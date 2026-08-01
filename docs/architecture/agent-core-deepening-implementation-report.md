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
- `ToolProgressSink` 和 `execute_tool_with_progress` 为 no-op 兼容 seam，未接 UI 或完整批处理。
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
| Tool Progress | trait/no-op seam | `ToolProgressSink` | 完整 batching 留后续 |
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
| 完整 workspace/frontend 验证 | 待最终阶段 | 避免重复全量消耗 |

## 5. 未完成或偏差

- 未修改 Conversation 数据库、UI projection、Prompt Queue 产品语义。
- 未实现完整 Tool Progress batching、TurnPolicy 多策略、Checkpoint DB migration、事件 fault-injection 全链路。
- Provider live HTTP fixture、Shell/MCP kill+wait 和并行整体 cancel 仍需阶段性集成测试。

## 6. 阶段 1 前置条件

- 为 `ActiveContextSnapshot` 建立 daemon-owned artifact/migration 与旧记录兼容 fixture。
- 为 `EngineInputReceiver` 接入现有 Queue lease/ack，并定义 daemon 重启恢复规则。
- 将所有关键 Tool/Permission/terminal 事实迁移到 checked event sink 后再开放 projection reducer。

## 7. 风险与回滚

- Additive protocol 变体需要旧客户端忽略未知 event type；不改变既有 RunEvent terminal payload。
- capability 未声明时顺序执行，性能保守但安全；回滚只需移除 production capability override。
- 新类型均为内部/兼容 seam；若 migration fixture 不通过，不合并 DB 或 UI 接线。
