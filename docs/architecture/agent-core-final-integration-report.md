# Agent Core 最终深化与生产闭环报告

## 1. 基线

- Worktree：`/Users/ldh/Downloads/project/AiNative/Natives-agent-core-deepening`
- 分支：`feat/agent-core-deepening`
- 起始 Commit：`0aadad5316f844fe6312d70472bff478049f9088`
- 结束 Commit：工作区增量待提交（基于 `50e8822ee1702868d6a8d086a786e1cf9018aec9`）
- Pi 参考 Commit：`583f153d502aa8e958eefdb9af0fbd3344e68f95`
- 原工作区：`/Users/ldh/Downloads/project/AiNative/Natives` 保持 dirty，未 reset/stash。

## 2. 能力与生产证据

| 能力 | 当前生产证据 | 状态 |
|---|---|---|
| Typed Message / Turn | `agent-core/src/engine.rs` 发送 `ProviderTurnRequest`；daemon `load_agent_messages` 可从 `message_block` 回放稳定 ID | 已接生产，兼容边界仍保留 `EngineMessage` |
| Context Snapshot | migration 028 + `persist_context_snapshots_from_events` 写入 conversation/turn/source revision 与完整 typed `snapshot_json`；生产启动以最新 snapshot 为基线，并按 `input_message_ids` 追加压缩后新写入的完整消息 | 已接生产 |
| Capability Scheduler | `PermissionGatedTools::list_tool_capabilities` 从 Gateway side-effect 映射模式，Core 保持调度 | 已接生产 |
| Progress | `execute_tool_with_progress_for_call` 使用真实 ToolCall/Turn/Message ID；daemon sink 有序号、50ms 限流、settled late-drop | 已接生产；底层 MCP/网络实时 chunk 仍依赖 handler 合作 |
| Steering / Follow-up | `DurableInputReceiver` SQLite lease/ack/recovery；ack 同事务写入 user message + queue sent | 已接生产 |
| Critical Events | Turn/Message/Tool prepared/started/completed/GenerationAttemptCommitted/Checkpoint/Snapshot/Permission 使用 `append_checked`；checkpoint open/commit 分离 | 已接生产关键事实；delta 仍允许 best-effort |
| Ledger / Checkpoint | side-effect 状态记录；checkpoint 写入 turn/ledger cursor | 部分完成；外部副作用仍不可回滚 |
| Retry / Resume | RunManager retry；uncertain side-effect fail closed；resume_plan 记录 blocked/executed retry | 已接生产，continue/fork/replay 产品语义未扩展 |
| Renderer Projection | 新增 `ProjectionState`、`ProjectionRecovery`、`AuthoritativeEventMissing`；禁止伪造终态序列 | 已接生产 |

## 3. 关键改动

### Typed Message / Turn

`AgentEngine` 在生产入口一次性把兼容历史转换为 typed transcript；Provider 调用统一走 `EngineProvider::stream_turn(ProviderTurnRequest, CancellationToken)`，每次 provider attempt 只在提交后形成一个 Turn。`TurnStarted/Completed`、`MessageStarted/Delta/Completed` 和 typed Assistant/ToolResult 事实由 Core 产生，旧 `EngineMessage` 仅保留在 provider/fixture 兼容边界。

### Typed Message / Conversation

`src-agent-daemon/src/conversation_store.rs` 增加 typed message 读取、按 Turn 分组持久化与 JSON block 解析，工具结果保留 `tool_call_id`、错误码和结果 block。生产启动以最新 `ActiveContextSnapshot` 为基线，按 `input_message_ids` 追加压缩后新写入的 typed 消息；完整历史不删除。旧 `engine_history` 仅作为空 typed history 的兼容回退；Core 是唯一 mutable typed transcript。ContextSnapshotCommitted 携带并持久化 active typed JSON，可通过 replay API 解释重放。

### Scheduler / Progress

Core 新增带 call identity 的 progress seam，daemon 使用 Core 生成的 ID；Gateway 仍是 Schema、权限、路径、超时和执行安全边界。Side-effect ledger 在成功/失败/timeout/cancel 分别记录 `completed`、`failed` 或 `uncertain`。

### Recovery / Projection

Checkpoint 可绑定最近 turn、context snapshot 和 event cursor；run start 发 `CheckpointCreated`，完成提交发独立 `CheckpointCommitted`；retry 会写入 `resume_plan`，不确定副作用时记录 blocked。PermissionRequested/Responded 也通过 `append_checked`，持久化失败即拒绝后续执行。渲染器发现 terminal DB 状态但缺失权威 terminal event 时抛出 `AuthoritativeEventMissing`，不重写 sequence、不合成 Run Event。

## 4. 数据库与协议

- migration 028 维持 additive：turn、typed message 元数据、context snapshot lineage、prompt lease、checkpoint cursor、side-effect status、resume_plan。
- 未修改外部 RPC surface；RunManager 仍是唯一 terminal fact authority。
- 未将 Gateway、Provider Adapter 或 Daemon 重新归类为 Core；未集成 Pi Runtime。

## 5. 精确验证

| 命令 | 结果 |
|---|---|
| `rtk cargo check -p agent-core -p capability-gateway -p provider-adapters -p natives-agent-daemon` | 通过（本轮最新） |
| `rtk cargo test -p agent-core --lib compaction_ -- --nocapture` | 通过：5 |
| `rtk cargo test -p capability-gateway --lib` | 通过：91 |
| `rtk cargo test -p natives-agent-daemon typed_message_round_trip_preserves_tool_call_identity -- --nocapture` | 通过：1 |
| `rtk cargo test -p natives-agent-daemon conversation_round_trip_uses_daemon_tables --lib -- --nocapture` | 通过：1（本轮兼容修复后） |
| `rtk cargo fmt --check` | 通过 |
| `rtk npm run protocol:check` | 通过 |
| `rtk cargo check -p agent-core -p capability-gateway -p provider-adapters -p natives-agent-daemon` | 通过（本轮接线后） |
| `rtk cargo fmt --check` | 通过（本轮格式化后） |
| `rtk npm run verify:native-engine` | 失败：静态 audit OK；daemon lib 326 passed/2 failed，live e2e 0 passed/1 failed；frontend commands因缺少 `tsx`/`tsc` 失败 |
| `rtk npm run typecheck` | 未执行：worktree 无 `node_modules/.bin/tsc`，命令退出 127 |
| `rtk cargo test --workspace` | 未运行，受共享资源策略限制 |
| frontend lint/test/perf | 未运行，依赖未安装 |

直接执行的 Cargo 命令使用共享 target、jobs=2、incremental=0；未执行 clean/rm -rf，未终止其他任务进程。仓库自带 `verify:native-engine` 内部未继承 target 环境，使用了 worktree `target/`，已在本报告明确记录。前三次精确测试失败分别暴露 migration 028 错误索引、测试外键和 block 解码路径，均已修复后通过。

## 6. 未完成与风险

- Active Context 已在生产启动接线：snapshot 的 `input_message_ids` 作为去重边界，压缩后新增消息追加到 active context；完整历史仍保留在 message 表。
- resume_plan 已记录 retry blocked/executed，但 continue/fork/replay 尚未扩展 RPC。
- MCP/网络 handler 的底层取消清理依赖其自身 future/token 合作，缺少 live integration fixture；Shell supervisor 已 kill+wait。
- `start_with_seams_loads_daemon_conversation_history` 在本轮独立运行超过两分钟无输出后中止，未计为通过；native-engine 脚本此前同族测试仍报告 QueryReturnedNoRows，需后续单独诊断。
- Renderer projection 已拒绝伪造事件，但上层 UI 仍需展示 `ProjectionRecovery` 状态。

## 7. 合并顺序与回滚

建议先合并 typed conversation/checkpoint/ledger，再合并 renderer projection；若出现兼容问题，可整体回滚本提交，保留 migration 028 的 nullable/default 字段，不影响既有 RPC、UDS、RunManager、Gateway 和 Provider Adapter。

## 8. 资源使用

- 共享 `CARGO_TARGET_DIR`：`/Users/ldh/Downloads/project/AiNative/Natives/.cargo-target-shared`
- 本轮 Cargo check：1；本轮精准 Cargo Test：0；Workspace Test：0。
- 最大 target 大小：共享 target 约 1.7 GiB；native verifier 产生的本地 target 约 5 GiB。
- 最低可用磁盘：约 73 GiB。
- 主动终止的卡死进程：前序 turn 的 `start_with_seams_loads_daemon_conversation_history` 精确测试无输出超过两分钟后以 stdin Ctrl-C 中止；本轮无 Cargo 进程被终止。
- 清理的临时目录：无；`cargo clean`：否。
