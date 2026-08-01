# Natives Agent Core 演进计划（深化执行状态）

## 阶段 0：执行正确性（已完成于 P0 基线）

真实 Run Credential、显式 Provider API mode、统一 stop reason、严格 JSON/Schema、
Tool Result 配对和 Gateway Cancel 已落地。证据见
`docs/architecture/agent-core-p0-implementation-report.md` 与
`docs/architecture/agent-core-p0-revalidation.md`。

## 阶段 1：Turn 与消息（本轮完成最小兼容层）

改动：`agent-core/message.rs`、`turn.rs`、`engine.rs`、Protocol v2 event。
保留 `EngineMessage` 作为 provider adapter 兼容类型；新 opaque ID 和 Turn/Message
事件先旁路接入，避免 UI/RPC/数据库破坏性迁移。

验收：161 个 agent-core 单测通过；Provider request 每次携带 run/attempt；关键
生命周期事件 persist-first。

## 阶段 2：Context/History Snapshot（本轮部分完成）

`context_snapshot.rs` 定义 FullHistory、ActiveContext 和可序列化快照，保留现有
compaction 与 provider window 估算。下一批增加 daemon-owned artifact 表和 migration，
并为旧 transcript 保留 legacy marker。

## 阶段 3：Tool Capability/Progress（本轮部分完成）

Gateway/Daemon 广告 `ParallelSafe/Sequential/Exclusive`；Core 未收到声明时顺序执行。
Progress Sink 作为 no-op 默认 seam，后续再接入 batched RunEvent，规定取消后丢弃 late
update，不增加本轮 UI 语义。

## 阶段 4：Steering/FollowUp/Policy（本轮部分完成）

`EngineInputReceiver` 只在 `AfterToolBatch`、`BeforeProvider`、`BeforeRunEnd` 等 safe
point drain；每项 input 显式 ack。现有 Prompt Queue 仍负责持久化，下一批实现 lease/
retry/ack 与恢复策略。

## 阶段 5：Event 与恢复（下一批）

把 ToolCall、Permission、Run terminal 等关键事实统一切到 checked sink；注入失败 persistence
测试；增加 `Conversation → Branch → Run → Turn → Message` additive lineage 和 side-effect
ledger。未知副作用状态必须阻止自动 replay。

## 阶段 6：Projection 与子 Agent（下一批）

增加 UI-only projection/recovery 类型，reducer 以 `event_id` 幂等；子 Agent 继续复用同一
AgentEngine，但只继承收窄后的 project root、allowlist、profile 和独立 Run ID。

## 提交与回滚

每阶段保持独立本地提交；不 push。回滚顺序为先移除 additive protocol consumers，再移除
Engine builder seam，最后回退具体 scheduler/DB migration。任何 schema migration 必须
先有 forward/backward fixture。
