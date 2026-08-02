# Agent Core 生产接线实施报告

## 1. 基线

- 分支：`feat/agent-core-deepening`
- Worktree：`/Users/ldh/Downloads/project/AiNative/Natives-agent-core-deepening`
- 开始 Commit：`6ef0f10eab2692a030f68fbc9cbf7d8091059e68`
- Pi 参考 Commit：`583f153d502aa8e958eefdb9af0fbd3344e68f95`
- 原工作区：保持 dirty，未 reset、stash 或覆盖。

## 2. 真实接线结果

### 2.1 Typed Message / Turn

- `crates/agent-core/src/engine.rs` 的生产循环以 `ProviderTurnRequest` 发送 typed `AgentMessage`；兼容 Provider 仍在 Trait 默认边界转换为 `EngineMessage`。
- 每次 committed response 生成 `TurnId` 与 assistant `MessageId`，Turn/Message lifecycle 使用 checked event seam。
- `src-agent-daemon/src/conversation_store.rs` 聚合真实 Run Event，写入 `turn`、typed assistant message 和配对 tool-result block；旧 Renderer 继续读取兼容 JSON blocks。

### 2.2 Conversation / Context Snapshot

- `src-agent-daemon/src/storage/migrations.rs` 的 migration 028 additive 增加 turn/message/run/event/context_snapshot/prompt_queue/checkpoint/side-effect/resume 字段。
- ContextCompressed 事件现在可持久化 compaction snapshot；完整 Active Context 无损重放与 artifact 迁移仍未宣称完成。

### 2.3 Tool Capability / Progress

- `PermissionGatedTools::list_tool_capabilities` 从 Gateway 的 side-effect 元数据广告调度模式；Core 不复制工具 Registry。
- `DaemonToolProgressSink` 已接入 AgentEngine；状态进度和已有 run_terminal stdout/stderr 分块事件进入 Run Event。完整可取消 Progress Sink、全工具 rate limit 与 UI reducer 留在后续阶段。

### 2.4 Steering / Follow-up

- `DurableInputReceiver` 实现 Core `EngineInputReceiver`，在 safe point 以 SQLite transaction lease queued prompt，ack 后记录 `consumed_turn_id`。
- Daemon 启动恢复将 `running/leased` 归还 queued，避免租约永久占用；Prompt Queue 的 Send Now 语义仍由既有 RunManager/SessionCoordinator 负责。

### 2.5 Provider / Credential / Stop Reason

- `EngineProviderContext` 的真实 `run_id` 和 attempt 进入 `RoutedProvider` → `RealProvider::stream_with_context_controls` → `resolve_credential_for_run`；`legacy-unbound` 仅保留无上下文兼容 helper。
- OpenAI API mode 是 adapter 实例字段，不再读写 `NATIVES_OPENAI_API`；Responses 与 Chat Completions 不共享可变请求状态。
- OpenAI、Responses、Anthropic、Gemini 及兼容 adapter 的 completion reason 统一映射到 Core `ProviderStopReason`；Length/Unknown/无 Final 的 tool call fail closed，并生成同 ID error Tool Result。

### 2.6 Cancel / Permission / Authority

- Gateway `execute` 同时 race handler、timeout、父 CancellationToken，并向 Handler 传递 child token；timeout 与 cancelled 使用不同错误码。
- RunManager 仍是唯一 terminal fact authority；AgentEngine 只返回 `EngineOutcome` 并发 domain events，不补第二个 Run 终态。

## 3. 修改文件

| 文件 | 目的 |
|---|---|
| `crates/agent-core/src/engine.rs` | typed Provider request、Stop Reason、fail-closed tool parsing、Tool Result pairing、Input/Progress seams |
| `crates/agent-core/src/message.rs` | StopReason 的稳定持久化显示 |
| `src-agent-daemon/src/conversation_store.rs` | turn/message/block typed roundtrip 与 compaction snapshot 持久化 |
| `src-agent-daemon/src/event_log.rs` | Run Event 写入 turn/message 关联 |
| `src-agent-daemon/src/production.rs` | 真实 Input Receiver、Progress Sink 装配；Provider context/adapter mode |
| `src-agent-daemon/src/production_tools.rs` | Gateway capability 与生产进度事件 |
| `src-agent-daemon/src/prompt_queue_store.rs` | SQLite lease/ack/recovery |
| `src-agent-daemon/src/storage/migrations.rs` | migration 028 additive schema |

## 4. 验证

| 命令 | 结果 | 说明 |
|---|---|---|
| `rtk cargo check -p agent-core -p capability-gateway -p provider-adapters -p natives-agent-daemon` | 通过 | shared target；最终运行 5.74s |
| `rtk cargo test -p agent-core --lib completes_simple_text_turn -- --nocapture` | 通过 | 1 passed，161 filtered |
| `rtk cargo fmt --check` | 通过 | 格式无差异 |
| `git diff --check` | 通过 | 无 whitespace 错误 |
| `cargo test --workspace` | 未运行 | 遵守资源策略；避免重复 workspace 编译 |
| 前端 typecheck/lint/test/perf | 未运行 | 当前 Worktree 未安装完整 frontend toolchain |
| live provider / shell kill+wait / MCP pending fixture | 未运行 | 需要外部服务或专门 fixture |

资源记录：所有 Cargo 命令使用 `/Users/ldh/Downloads/project/AiNative/Natives/.cargo-target-shared`、`CARGO_BUILD_JOBS=2`、`RUST_TEST_THREADS=2`、`CARGO_INCREMENTAL=0`；未执行 `cargo clean`、`rm -rf target`，未终止其他任务的 Node 进程。

## 5. 未完成项

- 没有引入完整 TurnLoop、AgentMessage UI/RPC 类型迁移、Steering/Follow-up 产品语义或新的 Tool Scheduler 架构。
- Active Context 无损 artifact/replay、Checkpoint Resume、side-effect ledger 的生产恢复策略仍是阶段 1 前置条件。
- 完整 Tool Progress Sink（每个 Handler 的实时输出、统一限流、迟到事件丢弃）不在本轮完成。

## 6. 下一步验收门

1. 为 durable queue、typed DB roundtrip、Tool Result pairing 和 provider stop reason 增加 daemon-owned fixture。
2. 对 shell/MCP/并行工具做 cancel/timeout/cleanup integration test。
3. 通过一次受控 workspace test 与 protocol/native-engine 检查后，才进入 Renderer projection 与 Checkpoint Resume。

## 7. 回滚

本轮修改均为 additive：回滚 migration 028、AgentEngine 的 Input/Progress 装配和 typed persistence 接线即可；不删除既有 RPC、UI、UDS、RunManager terminal authority 或 P0 安全校验。
