# Agent Core Pi 吸收最终闭环报告

## 1. 基线

- Branch：`codex/agent-core-pi-final-20260803-211737-19930`
- Start HEAD：`7686b20dae28869395e00add051c91afd296ce49`
- End HEAD：`bfd7001268fd5a7731af49d77e36d4d79b6d6c70`
- Worktree：`/Users/ldh/Downloads/project/AiNative/Natives-pi-final-20260803-211737-19930`
- Shared Target：`/Users/ldh/Downloads/project/AiNative/Natives/.cargo-target-shared` 7.8 GiB（<35 GiB Cargo Test 门槛）
- Disk：可用 **19.5 GiB**（`df`：Avail 20429392 KB）。**低于 20 GiB 测试门槛**（见第 8 节）。

历史基线（实施期间记录，保留）：`feat/agent-core-deepening` @ `Natives-agent-core-deepening`，Start `1b78e341`；批次 0-3 提交 `631da17f`/`ca901dfa`/`5f2d6dc4`/`01a235b3`/`7686b20d`。

## 2. 已保留能力

批次 0-3 的全部能力在本闭环中保留，未重写：

1. `AgentEngine::run_with_typed_messages` typed `AgentMessage` 主链。
2. `RealProvider`/`RoutedProvider`/`Sub2ApiPoolProvider` 显式 `ProviderTurnRequest`。
3. Hook Inject 进入 typed transcript；Provider retry 留在同 Turn。
4. Tool Result MessageId 经 Core Event → SQLite → reload 稳定；Text/Thinking/Image/ToolCall/ToolResult/Custom 无损 codec；附件唯一边界降级。
5. Stop Reason、截断 Tool Call fail-closed、JSON/Schema 校验、同 ID Tool Result pairing。
6. Gateway 显式 execution mode；Core 不按工具名推断并行；结果源序。
7. Shell/MCP/Subagent progress 统一 `DaemonToolProgressSink`。
8. Durable Steering/Follow-up lease、ack、safe point、rehydrate。
9. Checkpoint exact snapshot；Continue 不再回退 latest。
10. `run.resume`、SafeToContinue/ConfirmationRequired/Blocked、uncertain guard。
11. Subagent durable scope（migration 029）+ route restart 严格恢复。
12. Renderer `AuthoritativeEventMissing`、local ProjectionRecovery、重连 exactly-once。
13. RunManager 单一终态权威、EventSequencer persist-first、Gateway Schema/Permission/Cancel 边界。

## 3. 本轮修复

| 问题 | 根因 | 修改 | 生产证据 | 测试 |
|---|---|---|---|---|
| Snapshot backfill 不是 fail-closed | `try_new_with_store` 吞掉 `backfill_context_snapshots` 错误（只 eprintln）；backfill 静默跳过无法反序列化的相关 event | `try_new_with_store` 改为 `backfill_context_snapshots_with_store(&data_store)?` 传播；backfill 对 `context_snapshot_committed` 行 payload 解码失败返回 Err；不相关 event 类型仍忽略 | `run_manager.rs::try_new_with_store`；`conversation_store::backfill_context_snapshots_with_store` | `recovery_fails_closed_on_corrupt_committed_snapshot_event`；`backfill_with_store_fails_closed_on_corrupt_committed_snapshot_payload`；`backfill_ignores_corrupt_unrelated_event_payload`；`temp_store_constructed_twice_no_home_files_no_cross_test_pollution` |
| 测试隔离不可信 | 固定 run_id 读取 `~/.natives/events` 陈旧日志；`store_from_env` 读进程全局 env；`store()` 缺 env 报错 | agent-core 测试用内存 `EventPersistence` sequencer（不再写 home）；`store_from_env` cfg(test) 只认 thread-local override；daemon `store()` 访问器经 `resolve_test_store`（override→env→per-process 隔离 store），永不读 `~/.natives`；`ledger_store`/`global_checkpoint_manager`/`route_health_connection` 用隔离 test store | 175 agent-core 测试在无 `NATIVES_EVENT_LOG_DISABLE=1` 下全绿 | `run_manager` 模块 39 全绿；`mcp_call_through_permission_gate_emits_events`、`permission_gate_emits_request_and_respond` 等通过隔离 store + 进程稳定 global manager 运行 |
| 真实资源取消无证据 | Shell/MCP/Subagent/Permission 的本地可控取消测试缺失 | 补本地 fixture 测试（无外部服务）：真实 shell child kill+reap、本地 fake HTTP MCP server 取消、permission 写失败 fail-closed、subagent switchRoute scope、并行取消、Clean/CleanupFailed | 生产代码未改（测试确认既有 fail-closed 行为） | `cancellation_tree_e2e.rs` 9 测试 + `run_manager` 新增 2 测试 |
| Pi 行为一致性无表驱动证明 | 行为散落在既有单测，无矩阵性覆盖 | 新增 `pi_conformance` 表驱动 fixture | 生产代码未改；无新增 TurnPolicy Trait | `pi_behavior_matrix` + 5 个 bespoke conformance 测试 |
| `projectionRecoveryByRun` 无产品消费者 | Renderer 只投影；该状态只写不读 | 明确为诊断-only 状态并补测试；不新增权威事件、不伪造终态 | 生产 TS 未改；`AuthoritativeEventMissing` 仍抛 | `projection.test.ts`（dedup/gap/recovered/missing-terminal/reducer 诊断状态） |
| `run.resume` TS 契约无测试 | 仅方法名在 union | 补静态契约测试 + 逐字段核对 | protocol:check 通过 | `frontend-backend-contract.test.ts` `run.resume is catalogued...` |
| P2.3 deprecated seam | `set_permission_profile` 无生产调用者 | 删除该方法 + 6 个测试调用点 | 零生产调用者 | `cargo check --tests` 通过（删除后编译干净） |

## 4. Pi 行为一致性

| Pi 行为 | Natives 实现 | 状态 | 证据 |
|---|---|---|---|
| Agent Loop / Turn | 一次 Provider response 一个 Turn；retry 同 Turn；Tool Result 后自动下一 Turn | 完成 | `pi_behavior_matrix`（retry attempts=2、TurnStarted/Completed 各 1）；`tool_turn_then_text_turn_are_distinct_turns` |
| Typed Message | Context 主链只用 AgentMessage；Provider boundary 才转换 | 完成 | `typed_boundary_provider_receives_agent_messages`（ProviderTurnRequest.messages 是 AgentMessage，tool result 回传） |
| Partial Assistant | MessageStarted 后 delta 累积；可靠 Final 后 MessageCompleted；无 Final 的 Tool Call 不执行 | 完成 | `pi_behavior_matrix`（partial assistant）；`no_final_tool_call_never_executes`（handler=0，fail-closed error） |
| Tool execution | schema/permission/hook/cancel/timeout 全终点恰好一个同 ID ToolResult | 完成 | `pi_behavior_matrix`（exactly one same-id ToolCallCompleted + result_message_id）；`permission_write_failure_fails_closed_handler_not_invoked` |
| Tool scheduler | 显式 ParallelSafe 才并行；Sequential/Exclusive 降级；结果源序 | 完成 | `scheduler_parallel_only_when_explicit_and_source_order`（max_running≥2、源序 a1/b1/c1）；`sequential_tool_serializes_the_batch` |
| Steering | 只在完整 Tool batch/Turn 关闭后的 safe point 消费 | 完成 | `steering_and_follow_up_consumed_at_safe_points`（tool turn + steering turn，无半条 Assistant 插入） |
| Follow-up | Agent 准备结束、上一 Turn 已提交后消费并触发新 Turn | 完成 | 同上 + `follow-up-turn-boundary` 既有测试 |
| One / All | durable FIFO、one/all 稳定，多 Run 不串 lease | 完成（既有批次 3） | `steering_lease_excludes_a_second_run`、`durable_steering_ack_removes_live_queue_item`、`steering_queue_survives_coordinator_rehydrate` |
| Context transform | compaction 在 Provider boundary 前；完整历史保留；active snapshot 可解释重放 | 完成（既有批次 2/3） | compaction 测试族 |
| prepare-next-turn | Turn 后可消费队列、触发 compaction、决定继续/停止；未新增单实现 Trait | 完成 | `pi_behavior_matrix` + steering/follow-up 测试；无 `TurnPolicy` Trait |
| should-stop | no tool/no follow-up 正常结束；cancel/error/unknown stop reason 结算明确 | 完成 | `pi_behavior_matrix`（should-stop）；`abort_mid_provider_settles_as_cancelled` |
| Abort / Agent End | Provider、Tool、Permission、Subagent 取消全部结算；RunManager 唯一终态 | 完成 | `cancel_mid_run_marks_interrupted`、`production_cancel_wakes_permission_waiter_fast`、`shell_child_is_killed_reaped_and_registry_quiet_on_cancel`、`subagent_switch_route_restarts...` |

## 5. 恢复与安全不变量

1. `crates/agent-core` 是唯一 Agent 执行循环主体；`provider-adapters` 只处理 Provider wire；`capability-gateway` 只负责 Tool Registry/Schema/安全/执行；`src-agent-daemon` 装配 + 持久化 + RPC。
2. RunManager 唯一 Run terminal authority（批次 0-3 未触碰）。
3. Gateway 不修改模型 Context；Renderer 不写持久 Run Event。
4. Credential 明文不进入 Core；child 使用 child run_id。
5. `uncertain` 不自动恢复；确认也只创建新 Run（`run.resume`）。
6. Child project root/permission/allowlist/profile 只保持或缩小（subagent switchRoute 严格恢复，缺字段 fail-closed）。
7. 关键恢复失败 fail-closed：snapshot backfill 损坏、`context_snapshot_committed` payload 解码失败 → daemon 不构造成功（批次 1 修复）。
8. 测试永不读/写 `~/.natives`：agent-core 用内存 sequencer；daemon 用隔离 test store。
9. Renderer 投影 dedup（run_id+sequence）、`AuthoritativeEventMissing` 不伪造终态、`projectionRecoveryByRun` 不写回 Daemon。

## 6. 修改文件

| 文件 | 原因 |
|---|---|
| `crates/agent-core/src/event_seq.rs` | 新增 `test_support::memory_sequencer`（内存 EventPersistence） |
| `crates/agent-core/src/engine.rs` | 测试用 memory_sequencer；新增 `pi_conformance` 模块（6 测试） |
| `src-agent-daemon/src/conversation_store.rs` | backfill fail-closed + `resolve_test_store` 隔离 |
| `src-agent-daemon/src/run_manager.rs` | backfill 传播、`store_from_env` override-only、test global manager、权限写失败/switchRoute 测试、`with_global_manager_lock` |
| `src-agent-daemon/src/interaction_store.rs` / `prompt_queue_store.rs` / `subagent_store.rs` / `task_store.rs` / `capability/mod.rs` | `store()` 经 `resolve_test_store` 隔离 |
| `src-agent-daemon/src/side_effect_ledger.rs` | cfg(test) 用隔离 TEST_STORE |
| `src-agent-daemon/src/checkpoint.rs` | cfg(test) global checkpoint 用隔离 store |
| `src-agent-daemon/src/routing.rs` | cfg(test) route health 用隔离 store（修 env race flake） |
| `src-agent-daemon/src/production.rs` | `respond_permission` 先验 owner（mismatch 不消费 interaction）；删除 `set_permission_profile` seam |
| `src-agent-daemon/src/runtime/interaction_hub.rs` | 新增 `verify_permission_owner` |
| `src-agent-daemon/src/production_tools.rs` | plan-mode / tools_for fixture 补 conversation+run+checkpoint 种子 |
| `src-agent-daemon/tests/cancellation_tree_e2e.rs` | shell kill+reap、MCP HTTP cancel、并行取消、Clean/CleanupFailed |
| `src-agent-daemon/tests/failure_injection.rs` | corrupt replay 断言改为 fail-closed |
| `src/lib/assistant-protocol/projection.test.ts` | 新增（投影 dedup/gap/recovered/missing-terminal） |
| `src/lib/assistant-protocol/frontend-backend-contract.test.ts` | `run.resume` 契约测试 |
| `src/lib/assistant-workspace/controller.test.ts` | reconnect replay 不重复 tool/permission/usage |
| `src-agent-daemon/tests/live_engine_e2e.rs` | 删除 no-op `set_permission_profile` 调用 |
| `docs/architecture/agent-core-sol-remediation-implementation-report.md` | 本报告 |
| `docs/architecture/agent-core-pi-absorption-finalization-goal.md` | 锚定（batch 0） |

## 7. 测试真实结果

| 命令 | 退出码 | 结果 | 是否最终 HEAD |
|---|---:|---|---:|
| `cargo fmt --check` | 0 | 干净 | 是 |
| `cargo check -p agent-core -p capability-gateway -p provider-adapters -p assistant-protocol -p natives-agent-daemon` | 0 | 5 crates 编译通过 | 是 |
| `cargo check --tests -p natives-agent-daemon` | 0 | daemon 测试目标编译通过（seam 删除后） | 是 |
| `cargo test -p agent-core -- --test-threads=2` | 0 | 175 passed（lib，含 6 个 pi_conformance）+ 6（run_state 集成） | agent-core 自批次 3 未再变更，等价最终 HEAD |
| `cargo test -p capability-gateway -- --test-threads=2` | 0 | 111 passed（93 lib + 5 plan_mode + 13 security） | 是（gateway 未变更） |
| `cargo test -p provider-adapters -- --test-threads=2` | 0 | 6 passed | 是 |
| `cargo test -p assistant-protocol -- --test-threads=2` | 0 | 56 passed | 是 |
| `cargo test -p natives-agent-daemon -- --test-threads=2` | 0 | 360 lib passed + 全部集成 suite（cancellation_tree 9、event_replay、failure_injection 8、handshake、harness、mcp_protocol_surface、rpc_dispatch_contract、uds_run_lifecycle 等） | 批次 2 HEAD（ec8af271）；daemon 此后仅 seam 删除（编译已验） |
| `cargo test -p natives-agent-daemon run_manager -- --test-threads=2` | 0 | 39 passed（批次 0-2 修复后） | 等价最终 HEAD |
| `cargo test -p natives-agent-daemon cancellation_tree -- --test-threads=2` | 0 | 9 passed | 等价最终 HEAD |
| `npm run protocol:check` | 0 | TS types 与 Rust surface 对齐 | 是 |
| `npm run typecheck` | 0 | tsc --noEmit 干净 | 是 |
| `npm run lint` | 0 | eslint + i18n（2347/2347）+ hardcoded colors 通过 | 是 |
| `npx tsx ... src/lib/assistant-protocol/*.test.ts src/lib/assistant-workspace/*.test.ts` | 0 | 134 passed（含 reconnect exactly-once、tool/permission/usage replay no-dup、projection、run.resume 契约） | 是 |
| `cargo test --workspace -- --test-threads=2` | **未运行** | 磁盘 19.5 GiB < 20 GiB 硬门槛（见第 8 节）；workspace 含 `src-tauri` | 否 |
| `npm run verify:native-engine` | **未运行** | 该脚本运行 cargo 测试，磁盘门槛阻挡 | 否 |
| `npm run test`（全量 frontend） | **未运行（全量）** | 已运行的 assistant 子集 134 全绿；全量 `src/**/*.test.ts` 未跑 | 否 |
| `npm run perf:check` | **未运行** | 含 `npm run build`（磁盘/构建成本），磁盘门槛阻挡 | 否 |

注：`cargo test --workspace` 中 agent-core/capability-gateway/provider-adapters/assistant-protocol/natives-agent-daemon 已在最终 HEAD 等价代码上全绿（见上）；唯一未覆盖的是 `src-tauri`（Tauri Host）的测试与一次性的 `--workspace` 统一调用。未使用 `NATIVES_EVENT_LOG_DISABLE=1` 作为任何通过条件。

## 8. 未完成与环境项

**真实环境阻塞：可用磁盘低于 20 GiB 硬门槛。**

- 当前 `df`：`Avail 20429392 KB ≈ 19.5 GiB`，系统级 91% 占用。
- Goal 硬限制：`可用磁盘低于 20 GiB：不运行测试`、`低于 15 GiB：不运行 Cargo`、`不删除 Target`、`不删除用户事件日志`。
- 已清理本会话产生的测试工件（`natives-daemon-test-*`、`natives-side-effect-ledger-test-*`、106 个 leaked `.tmp*` 测试目录）；剩余占用来自共享 target 7.8 GiB + 其他 worktree 的 target（`Natives/target` 11G、`Natives-luna-personal-creative-236eae/target` 12G、`Natives-agent-core-deepening/target` 6.2G）与系统占用，均不可删除（Goal 禁止删除 Target / 他 worktree / 用户事件日志）。
- 因此 `cargo test --workspace`、`npm run verify:native-engine`、`npm run test`（全量）、`npm run perf:check` 未在最终 HEAD 运行。**不把"未运行"写成通过。**
- 真实外部 Provider/Shell/MCP 凭据 fixture：无凭证环境，本地 deterministic wire fixture 已全绿；live provider 项标「环境未验证」，未伪装为通过。
- `projectionRecoveryByRun`：已明确为 Renderer 诊断-only 状态（决策点选择），无新增权威事件、无 synthetic terminal。

## 9. 最终完成矩阵

| 能力 | 完成/部分/未完成 | 证据 |
|---|---|---|
| Typed Message | 完成 | `typed_boundary_provider_receives_agent_messages` + 既有 typed transcript 测试 |
| ProviderTurnRequest | 完成 | provider 显式 `stream_turn(ProviderTurnRequest)`；conformance |
| Turn | 完成 | retry 同 Turn、Tool Result 下一 Turn、事件/DB 一致 |
| Tool Scheduler | 完成 | 显式模式驱动；并行 max_running≥2；结果源序 |
| Progress | 完成 | 统一 sink；late-drop；取消后无迟到成功 |
| Steering/Follow-up | 完成 | durable lease/ack；safe point；restart 不丢不重 |
| Context | 完成 | compaction→snapshot→restart→Provider 闭环；恢复损坏 fail-closed |
| Next Turn Policy | 完成 | queue/compaction/continue/stop 有测试；无空 Trait |
| Cancel | 完成 | Shell/MCP/Permission/Subagent/parallel 全部 quiet 且有本地可控证据 |
| Ledger/Checkpoint/Resume | 完成 | exact checkpoint；Safe/Confirm/Blocked；uncertain 不自动运行 |
| Permission | 完成 | Run scoped；持久失败 handler=0；无 global mutable profile（seam 已删） |
| Sub Agent | 完成 | 独立 run/credential/snapshot/ledger；scope 不扩大；parent cancel 级联 |
| Events | 完成 | 关键事实 persist-first；失败 fail-closed；唯一 terminal authority |
| Renderer | 完成 | 不伪造 sequence/terminal；重连 exactly-once；local recovery 非权威、诊断-only |
| 验收 | **部分** | workspace/native-verifier/frontend 全量四类中，Rust 各 crate、protocol、typecheck、lint、assistant 测试子集已真实绿色；`cargo test --workspace`（含 src-tauri）、`verify:native-engine`、全量 `npm run test`、`perf:check` 因磁盘 <20 GiB 未运行（如实记录，不写成通过） |

## 10. Commit 与回滚

- 批次 0：`3e33ede6`（`docs(agent-core): anchor finalization baseline`）
- 批次 1：`8aaff857`（`fix(daemon): fail closed on snapshot recovery and isolate tests`）
- 批次 2：`ec8af271`（`test(agent-core): prove cancel permission and subagent cleanup`）
- 批次 3：`6e6fa807`（`test(agent-core): lock pi behavior conformance`）
- 批次 4：`85ff8ce3`（`test(renderer): close projection and resume protocol verification`）
- P2.3 seam：`bfd70012`（`fix(daemon): remove deprecated no-op set_permission_profile seam`）
- 未 Push，未开 PR。
- 回滚：每批一个提交，`git revert <提交>` 或 reset 到 Start HEAD `7686b20d` 可整体回滚；批次 1 无新增 DB/Protocol 迁移（migration 029 为批次 2 既有）；批次 4 仅测试，无生产 TS 改动；`bfd70012` 只删除无生产调用者的方法。
