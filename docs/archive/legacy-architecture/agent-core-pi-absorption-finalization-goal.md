# Natives Agent Core：Pi 设计吸收最终闭环 Goal

下面内容可以整份复制给另一个开发 Agent。不要重新执行全仓架构调研；只验证并关闭当前 HEAD 剩余的生产和验收缺口。

## Goal Mode

创建并持续执行以下 Goal：

> 基于当前 Natives Agent Core 深化分支，完成对 Pi Agent Core 行为设计的 Rust 化吸收闭环：保持 Natives 单一 Run Authority、Provider Adapter、Capability Gateway、Daemon、UDS 和持久化边界不变，补齐恢复 fail-closed、真实资源取消、Permission/Subagent 隔离、Next Turn 行为和 Renderer 重连证据，并在最终 HEAD 完成一次真实全量验收。

只有以下条件全部成立时，Goal 才能标记完成：

1. 本文“最终完成矩阵”全部为“完成”；
2. 每项同时具备生产调用路径、持久化/恢复、异常/取消和测试证据；
3. `uncertain` 副作用未确认时 Provider/Tool invocation 为 0；
4. RunManager 仍是唯一 Run terminal authority；
5. Renderer 不生成、补写或重编号权威 Run Event；
6. 最终 HEAD 的全部要求命令有真实退出码；
7. 未运行、环境失败和精准复验不能写成全量通过；
8. 没有用类型、Trait、Migration、字段或 No-op 冒充完成。

不要因为测试耗时、代码量大或某个外部 fixture 不方便而提前完成 Goal。遇到阻塞时继续完成其他不依赖项，并在报告中保留真实阻塞证据。

---

## 一、当前真实基线

- 基线 Worktree（只读）：`/Users/ldh/Downloads/project/AiNative/Natives-agent-core-deepening`
- 当前 HEAD：`7686b20dae28869395e00add051c91afd296ce49`
- 当前状态：detached HEAD；审计时 Working Tree 干净。
- P0/深化起点：`1b78e34104b1a7a5bbd600b689d8bbd1fe33b927`
- 已完成提交：
  - `631da17f`：typed Hook Inject + exact checkpoint Continue
  - `ca901dfa`：Tool Result identity + typed codec
  - `5f2d6dc4`：Snapshot/Resume/Subagent recovery
  - `01a235b3`：Gateway per-tool mode + unified progress
  - `7686b20d`：`run.resume` TS method + reconnect exactly-once test
- 当前实施报告：`docs/architecture/agent-core-sol-remediation-implementation-report.md`
- Pi 参考 Worktree：`/Users/ldh/Downloads/project/AiNative/References/pi`
- Pi Commit：`583f153d502aa8e958eefdb9af0fbd3344e68f95`
- 原始用户 Worktree：`/Users/ldh/Downloads/project/AiNative/Natives`；不得 reset、stash、覆盖或在其中开发。
- 开发要求：必须从基线 Commit 新建唯一分支和独立 Worktree；禁止在上述两个现有 Worktree 中写代码。
- 当前资源记录：可用磁盘约 38 GiB；共享 Target 约 7.6 GiB；未发现任务遗留 Cargo/Rustc/Node 测试进程。

开始时必须重新确认，不得直接相信上述快照：

```bash
pwd
git status --short
git branch --show-current
git rev-parse HEAD
git log --oneline -12
git worktree list
df -h .
du -sh /Users/ldh/Downloads/project/AiNative/Natives/.cargo-target-shared 2>/dev/null || true
pgrep -afil 'cargo|rustc|rustdoc|vitest|jest|tsx|next|vite' || true
```

### 分支与 Worktree 隔离协议

本 Goal 不允许直接占用 `deploy`、旧 `feat/agent-core-deepening`、detached baseline 或其他 Agent 已检出的分支。每个会写代码的 Agent 必须拥有唯一分支和唯一 Worktree。

从任意现有 Natives Worktree 只读执行以下命令。分支与目录使用时间戳和进程号避免多个 Agent 同时抢占：

```bash
set -euo pipefail

export NATIVES_FINAL_BASE="7686b20dae28869395e00add051c91afd296ce49"
export NATIVES_AGENT_TAG="pi-final-$(date +%Y%m%d-%H%M%S)-$$"
export NATIVES_FINAL_BRANCH="codex/agent-core-${NATIVES_AGENT_TAG}"
export NATIVES_FINAL_WORKTREE="/Users/ldh/Downloads/project/AiNative/Natives-${NATIVES_AGENT_TAG}"

git cat-file -e "${NATIVES_FINAL_BASE}^{commit}"
git worktree list --porcelain
git show-ref --verify --quiet "refs/heads/${NATIVES_FINAL_BRANCH}" && exit 2 || true
test ! -e "${NATIVES_FINAL_WORKTREE}"
git worktree add -b "${NATIVES_FINAL_BRANCH}" "${NATIVES_FINAL_WORKTREE}" "${NATIVES_FINAL_BASE}"
cd "${NATIVES_FINAL_WORKTREE}"
git status --short
git branch --show-current
git rev-parse HEAD
```

创建完成后必须满足：

- 当前分支以 `codex/agent-core-pi-final-` 开头；
- 当前 HEAD 是 `7686b20d`；
- `git status --short` 为空；
- `git worktree list --porcelain` 中该分支只出现一次；
- 后续所有读写、测试和提交都在新 Worktree 中执行。

若分支名或目录在检查与创建之间发生竞争，`git worktree add` 失败后不得删除或接管对方分支；重新生成 `NATIVES_AGENT_TAG`，换一个全新分支和目录。不得使用 `git checkout -B`、`git branch -f`、`git worktree remove` 清理其他 Agent 的环境。不得 Push。

### 多 Agent 写入规则

1. 一个分支只能有一个写入 Agent；主 Agent 是最终分支唯一提交者。
2. 只读审计 Agent 可以读取同一仓库，但不得修改最终 Worktree。
3. 需要并行写代码时，每个子 Agent 必须另外创建自己的唯一 `codex/agent-core-*` 分支和 Worktree。
4. 子 Agent 完成且测试通过后，主 Agent只通过 `git cherry-pick <commit>` 集成；不得让两个 Agent同时修改同一分支或 Working Tree。
5. Cherry-pick 前确认子 Agent 已停止写入；发生冲突由主 Agent在最终 Worktree一次性解决。
6. 不得切换、重置、删除、重命名或提交到 `git worktree list` 中属于其他 Agent 的分支。
7. 每次 commentary/实施报告都记录当前 Branch、Worktree、HEAD，防止命令跑到错误项目目录。

---

## 二、已经完成且必须保留的实现

以下能力已有生产代码和精准测试。除非新测试证明存在根因缺陷，不要重写：

1. `AgentEngine::run_with_typed_messages` 使用 typed `AgentMessage` 驱动 Native Agent 主循环。
2. `RealProvider`、`RoutedProvider`、`Sub2ApiPoolProvider` 显式使用 `ProviderTurnRequest`。
3. Hook Inject 已进入 typed transcript。
4. Provider retry 留在同一 Turn；下一次 Provider 调用创建下一 Turn。
5. Tool Result Message ID 经 Core Event → SQLite → reload 稳定。
6. Text、Thinking、Image、ToolCall、ToolResult、Custom typed codec 已有 round-trip；附件在唯一边界显式降级为文本 marker。
7. Stop Reason、截断 Tool Call fail-closed、JSON/Schema validation、同 ID Tool Result pairing。
8. Gateway capability 显式声明 execution mode；Core 不按工具名决定并行；结果保持源顺序。
9. Shell/MCP/Subagent progress 统一进入 `DaemonToolProgressSink`。
10. Durable Steering/Follow-up lease、ack、safe point 和基本 rehydrate。
11. Checkpoint exact snapshot；Continue 不再回退 latest snapshot。
12. `run.resume`、SafeToContinue/ConfirmationRequired/Blocked 和 uncertain guard。
13. Subagent durable scope 字段和 route restart 严格恢复。
14. Renderer `AuthoritativeEventMissing`、local ProjectionRecovery 和重连单次消息/终态结算。
15. RunManager 单一终态权威、EventSequencer persist-first、Gateway Schema/Permission/Cancel 边界。

不允许为了模仿 Pi 而：

- 嵌入 Pi TypeScript Runtime；
- 新建第二套 Agent Loop；
- 用 Pi Session 替换 Natives Run；
- 把 Gateway、Provider Adapter、Daemon 或 UI 塞进 `agent-core`；
- 增加 CLI/TUI/RPC Mode；
- 放弃 Rust、Tauri、UDS、Run Event 或 Capability Gateway。

---

## 三、当前剩余问题

### P0：Goal 完成阻断项

1. **最终全量验收尚未完成**：workspace、native verifier、frontend full suite 没有在当前最终 HEAD 全部绿色退出。
2. **测试隔离不可信**：实施报告记录 `run_manager` 仍有 7 个失败；部分 Agent Core 测试依赖 `NATIVES_EVENT_LOG_DISABLE=1`，固定 run_id 会读取 `~/.natives/events` 陈旧日志。最终测试不能依赖用户 Home 或历史事件。
3. **Snapshot backfill 不是 fail-closed**：`RunManager::try_new_with_store` 捕获 `backfill_context_snapshots` 错误后只 `eprintln!`；`backfill_context_snapshots` 还会静默跳过无法反序列化的 event payload。关键恢复失败时 Daemon 仍继续启动。

### P1：生产证据缺口

1. 实际 Shell handler 子进程 kill + wait/reap 没有本地可控集成测试。
2. MCP pending request cancel/abort、无迟到 success/Completed 没有本地可控 transport 测试。
3. Permission event/interaction 持久化失败的 fail-closed 路径缺系统 fault-injection 证据。
4. Subagent `switchRoute` 的完整生产路径缺测试：scope round-trip 有 Store 测试，但没有 route switch → new child Run → project/permission/allowlist/profile/parent cancel 一致性测试。
5. Next Turn 行为散落在 `run_inner`，需通过 Pi 行为矩阵证明，而不是新增一个只有单实现的 `TurnPolicy` Trait。

### P2：收口问题

1. 当前实施报告仍写“批次 3 待提交、批次 4 未开始”，与 `01a235b3`、`7686b20d` 不一致。
2. Renderer local `projectionRecoveryByRun` 没有产品消费者；必须决定最小行为：复用现有 recovery/connection UI 显示不完整投影，或明确该状态只供诊断并写测试。不要新增权威事件。
3. deprecated/no-op 或 legacy seam 需要最后核查：`ProductionRuntime::set_permission_profile`、默认 Noop receiver/sink、旧 task batch seam。只删除已确认没有生产调用者的内容。

---

## 四、执行批次

一次只做一个批次。每批先写失败测试，再修共享根因；每批形成一个可独立回滚的本地提交。

## 批次 0：基线保护与报告校准

- 目标：确保后续提交不会丢失，并把文档与当前代码状态对齐。
- 涉及文件：
  - `docs/architecture/agent-core-sol-remediation-implementation-report.md`
  - 本 Goal 文件
- 操作：
  1. 按“分支与 Worktree 隔离协议”从 `7686b20d` 创建唯一分支和独立 Worktree；不改已有提交。
  2. 只读检查 `631da17f..HEAD` 的生产调用路径和测试名称是否仍存在。
  3. 更新实施报告基线，记录 `01a235b3`、`7686b20d`；批次 4 改为“已开始，未完成”。
  4. 不运行 Cargo。
- 验收标准：唯一分支只绑定唯一新 Worktree；Working Tree 只包含预期文档变化；当前五个提交均为祖先；报告记录新 Branch/Worktree/Start HEAD，且不再使用过时状态。
- 回滚：单独 docs commit 可直接 revert。
- 是否依赖前一批：否。

## 批次 1：恢复 fail-closed 与测试可信度

- 目标：Daemon 不在关键恢复损坏时继续服务；测试不读取用户 Home 或陈旧 event log。
- 涉及文件：
  - `src-agent-daemon/src/run_manager.rs`
  - `src-agent-daemon/src/conversation_store.rs`
  - `src-agent-daemon/src/event_log.rs` 或测试构造 helper
  - `src-agent-daemon/src/run_manager.rs` 内测试及必要的 test support
  - `crates/agent-core/src/engine.rs` 测试构造
- 生产调用链变化：
  1. `try_new_with_store` 必须传播 snapshot backfill 错误；`new_with_store` 继续 fail-closed panic/abort construction。
  2. `backfill_context_snapshots` 对相关 `run_event` payload 解码错误返回错误，不静默跳过。
  3. 不相关 event type 可忽略；相关 committed snapshot event 损坏必须阻止恢复。
- 测试修复：
  1. 所有固定 run_id 测试改用 UUID 或显式 temp EventLog。
  2. RunManager tests 使用完整 temp `DataStore` migration，不读取 `~/.natives`，不依赖进程全局残留 env。
  3. 不用 `NATIVES_EVENT_LOG_DISABLE=1` 作为最终通过条件；它只可用于单元定位。
  4. 逐一定位报告中的 7 个 `run_manager` 失败：若是 fixture 错误，修 fixture；若是生产错误，修共享根因。不得 ignore。
- 必须新增或强化的测试：
  - committed snapshot event payload 损坏 → `try_new_with_store` 返回 Err；Daemon 不构造成功。
  - event 正常但 row 缺失 → startup backfill 成功且幂等。
  - temp store 连续构造两次，无用户 Home 文件、无跨测试事件污染。
  - `run_manager` 模块在独立 temp store 下全绿。
- 验收标准：相关精测全绿；`cargo test -p natives-agent-daemon run_manager` 不再有已知 7 failures；Agent Core test 不依赖 Home event history。
- 回滚：一个提交；不得回滚现有 Snapshot/Resume 功能。
- 是否依赖前一批：是。

## 批次 2：真实 Cancel、Permission 与 Subagent 闭环

- 目标：用本地可控 fixture 证明资源真的结束，而不是只返回 cancelled。
- 涉及文件：
  - `crates/capability-gateway/src/lib.rs`
  - 已有 Shell handler/process supervisor 文件
  - `src-agent-daemon/src/mcp_runtime.rs`
  - `src-agent-daemon/src/runtime/mcp_invocation.rs`
  - `src-agent-daemon/src/runtime/execution_registry.rs`
  - `src-agent-daemon/src/production_tools.rs`
  - `src-agent-daemon/src/interaction_store.rs`
  - `src-agent-daemon/src/subagent_store.rs`
  - 对应 tests；优先复用 `cancellation_tree_e2e.rs`
- 最小实现原则：优先补测试；只有测试暴露根因时才改生产。不要新建 Cancellation framework。
- 测试：
  1. 启动一个本地长时间 Shell child，取消后验证 child 被 kill、wait/reap，stdout/stderr reader退出，ExecutionRegistry quiet，无 success Tool Result 或迟到 Completed。
  2. 启动本地 fake MCP server 或 controllable pending future，取消后 transport future释放、pending map清空、无 success/late progress。
  3. PermissionRequested/interaction/session actor/PermissionResponded 任一写入注入失败时，Handler invocation=0、waiter被清理、Run fail closed。
  4. Cancel 与 timeout 竞争仍区分 `cancelled`、`timeout`、`cleanup_failed`。
  5. `subagent.switchRoute` 走真实 production dispatch，创建的新 child Run 保留原 project identity、permission ceiling、allowlist、agent profile、max steps和parent run；parent cancel级联；缺任一 durable scope 时不创建 Run。
  6. 多并行 Tool cancel 后所有 future退出，registry quiet。
- 验收标准：所有 fixture 本地运行，不需要公网或真实凭证；真实资源状态可观察；Cancel 后没有成功结果和迟到事件。
- 回滚：测试和最小根因修复同提交；不修改 RPC/DB，除非测试证明现有 additive schema确实缺字段。
- 是否依赖前一批：是。

## 批次 3：Pi 行为一致性闭环

- 目标：证明已经吸收 Pi 的行为设计，而不是复制其类名或新增空 Trait。
- Pi 只读参考：
  - `packages/agent/src/agent.ts`
  - `packages/agent/src/agent-loop.ts`
  - `packages/agent/src/types.ts`
  - `packages/agent/src/stream-fn.ts`
  - `packages/ai/src/types.ts`
  - `packages/ai/src/utils/validation.ts`
- 涉及文件：优先仅测试 `crates/agent-core/src/engine.rs`、`src-agent-daemon/src/prompt_queue_store.rs`；测试失败才修改最小生产路径。
- 行为矩阵与测试：

| Pi 行为 | Natives 必须证明的行为 |
|---|---|
| Agent Loop / Turn | 一次 Provider response 是一个 Turn；retry仍在同 Turn；Tool Result 后自动下一 Turn |
| Typed Message | Context 主链只用 AgentMessage；Provider boundary 才转换；Tool/Thinking/Image identity不丢 |
| Partial Assistant | MessageStarted 后 delta累积；可靠 Final 后才 MessageCompleted；无 Final 的 Tool Call不执行 |
| Tool execution | schema/permission/hook/cancel/timeout所有终点恰好一个同 ID Tool Result |
| Tool scheduler | 明确 ParallelSafe 才并行；Sequential/Exclusive降级；结果源序 |
| Steering | 只在完整 Tool batch/Turn 关闭后的安全点消费；绝不插入半个 Assistant Message |
| Follow-up | Agent 准备结束、上一 Turn已提交后消费，并触发新 Turn |
| One / All | durable FIFO、one/all稳定，多 Run不串 lease |
| Context transform | compaction发生在 Provider boundary前；完整历史保留；active snapshot可解释重放 |
| prepare-next-turn 行为 | Turn 后可以消费队列、触发 compaction、决定继续/停止；不得要求新增单实现 Trait |
| should-stop 行为 | no tool/no follow-up正常结束；cancel/error/unknown stop reason结算明确 |
| Abort / Agent End | Provider、Tool、Permission、Subagent取消全部结算；RunManager唯一终态 |

- 新增一个表驱动 conformance fixture，覆盖上述行为；可复用已有 Provider/Tool fake，不复制既有几十个单测。
- `prepareNextTurn`/`TurnPolicy` 判断：
  - 若现有 safe point + compaction + stop logic 已满足矩阵，只补测试和一个局部 helper，不新增 Trait。
  - 只有确实需要两个生产策略实现时才允许引入 Trait。
- 验收标准：矩阵每行都有生产函数和可运行测试证据；不存在只靠文档或类型声明的能力。
- 回滚：测试提交和必要的最小 helper 可独立回滚。
- 是否依赖前一批：是。

## 批次 4：Renderer / Protocol 最终化

- 目标：完成当前已开始的 Renderer 批次，不引入第二权威。
- 已有提交：`7686b20d` 已加入 TS `run.resume` method 和 reconnect exactly-once test，保留。
- 涉及文件：
  - `src/lib/assistant-protocol/types.ts`
  - `src/lib/assistant-protocol/wire.ts`
  - `src/lib/assistant-protocol/projection.ts`
  - `src/lib/assistant-gateway/daemon-adapter.ts`
  - `src/lib/assistant-workspace/controller.ts`
  - `src/lib/assistant-workspace/reducer.ts`
  - 现有 recovery/connection UI（仅在确有消费者缺口时）
- 检查：
  1. Rust `run.resume` request/response 与 TS method/protocol check 一致。
  2. gap replay、terminal missing、reconnect 从旧 sequence开始时不重复 assistant、Tool、terminal或analytics结算。
  3. `projectionRecoveryByRun` 不写回 Daemon；若当前 UI 完全不可见，复用现有 recovery banner显示“投影不完整”，不新建页面或持久状态。
  4. 缺权威终态继续抛 `AuthoritativeEventMissing`，禁止 synthetic terminal。
- 测试：
  - 保留 `reconnect replay settles assistant message and terminal exactly once`。
  - 增加/确认 Tool block 和 permission card 重放不重复。
  - `run.resume` method contract/protocol generation测试。
- 验收标准：Renderer 只做 projection；所有 local recovery状态可清理；协议检查通过。
- 回滚：Renderer local changes独立回滚；禁止恢复伪造终态逻辑。
- 是否依赖前一批：批次 1；可与批次 2/3 的测试编写并行，但最终提交应独立。

## 批次 5：最终全量验收与事实报告

- 目标：只对最终 HEAD运行一次完整验收，并把真实结果写入报告。
- 前置条件：批次 0–4 全部提交，Working Tree 干净。
- 先运行小范围：

```bash
rtk cargo fmt --check
rtk cargo check -p agent-core -p capability-gateway -p provider-adapters -p assistant-protocol -p natives-agent-daemon
rtk cargo test -p agent-core -- --test-threads=2
rtk cargo test -p capability-gateway -- --test-threads=2
rtk cargo test -p natives-agent-daemon run_manager -- --test-threads=2
rtk cargo test -p natives-agent-daemon cancellation_tree -- --test-threads=2
rtk cargo test -p assistant-protocol -- --test-threads=2
```

精准/Crate检查全绿后，最终命令每项只运行一次：

```bash
rtk cargo test --workspace -- --test-threads=2
rtk npm run protocol:check
rtk npm run verify:native-engine
rtk npm run typecheck
rtk npm run lint
rtk npm run test
rtk npm run perf:check
```

- 不允许用 `NATIVES_EVENT_LOG_DISABLE=1` 隐藏最终 workspace失败。
- 若某命令失败：记录退出码和首个根因；修复后只重跑该命令一次。不得循环全量构建。
- 真实外部 Provider不是 Goal完成必要条件；本地 deterministic wire fixture必须通过。没有凭证时写“live provider环境未验证”，不能写通过。
- 更新 `docs/architecture/agent-core-sol-remediation-implementation-report.md`，包含每个命令是否在最终 HEAD运行。
- 更新独立审计结论时保留原始 54/100，不删除历史；新增“整改后评分”和证据。
- 验收标准：workspace、protocol、native engine、frontend四类必需命令真实绿色；live外部项单独标环境状态。
- 回滚：报告独立提交；测试失败时不回滚安全修复来换绿。
- 是否依赖前一批：依赖全部。

---

## 五、资源与进程限制

所有命令使用 `rtk`。统一环境：

```bash
export NATIVES_REPO_ROOT="/Users/ldh/Downloads/project/AiNative/Natives"
export CARGO_TARGET_DIR="$NATIVES_REPO_ROOT/.cargo-target-shared"
export CARGO_BUILD_JOBS=2
export RUST_TEST_THREADS=2
export CARGO_INCREMENTAL=0
```

硬限制：

- 可用磁盘低于 20 GiB：不运行测试。
- 可用磁盘低于 15 GiB：不运行 Cargo。
- Shared Target 超过 35 GiB：不运行 Cargo Test。
- Shared Target 超过 45 GiB：停止 Cargo。
- 不创建新 Target，不运行 `cargo clean`，不删除 Target。
- 不反复运行 workspace、native verifier或frontend全量。
- 不主动终止不属于本 Goal 的进程。
- 不删除用户源码、数据库、事件日志或未提交文件来让测试通过；测试必须改为 temp隔离。

---

## 六、架构不变量

1. `crates/agent-core` 是唯一 Agent执行循环主体。
2. `provider-adapters` 只处理 Provider wire。
3. `capability-gateway` 只负责 Tool Registry、Schema、安全和执行。
4. `src-agent-daemon` 负责装配、Credential、持久化和 RPC，不建立第二套 Loop。
5. RunManager 是唯一 Run terminal fact authority。
6. Gateway 不修改模型 Context，Renderer 不写持久 Run Event。
7. Credential明文不进入 Core；child使用child run_id。
8. `uncertain` 不自动恢复；确认也只能创建新 Run。
9. Child project root、permission、allowlist、profile只能保持或缩小。
10. 不重写 Conversation DB，不做会话树扩张，不实现Pi Session/CLI/TUI。
11. 不新增完整 TurnPolicy/Scheduler/Progress框架，除非测试证明现有共享边界无法满足行为。

---

## 七、提交纪律

- 每批一个本地提交；测试和最小根因修复放在同一提交。
- 不 Push，不开 PR。
- 只使用本 Goal 创建的唯一开发 Worktree；不得回到基线、原始或其他 Agent Worktree 开发。
- 不与其他 Agent 共用、抢占或强制移动分支；并行写入必须使用独立分支/Worktree和完成后的 cherry-pick。
- 不 reset/stash原始用户Worktree。
- 不混入 Settings、i18n、无关 UI、依赖升级、批量格式化或历史清理。
- 不删除旧审计事实；用新的状态表覆盖“当前状态”。

建议提交边界：

```text
docs(agent-core): anchor finalization baseline
fix(daemon): fail closed on snapshot recovery and isolate tests
test(agent-core): prove cancel permission and subagent cleanup
test(agent-core): lock pi behavior conformance
fix(renderer): close projection and resume protocol verification
docs(agent-core): record final acceptance evidence
```

---

## 八、最终完成矩阵

| 能力 | 完成条件 |
|---|---|
| Typed Message | 生产唯一typed transcript；codec无静默丢失；Hook Inject进入Provider |
| ProviderTurnRequest | Native Agent生产Provider入口唯一，wire adapter边界明确 |
| Turn | retry/next turn/tool result身份和事件/DB一致 |
| Tool Scheduler | Gateway显式能力驱动；安全并行；结果源序 |
| Progress | Shell/MCP/Subagent统一sink；late-drop；取消后无迟到成功 |
| Steering/Follow-up | durable lease/ack；safe point；one/all/FIFO；restart不丢不重 |
| Context | compaction→snapshot→restart→Provider闭环；恢复错误fail closed |
| Next Turn Policy | queue/compaction/continue/stop行为有测试；不要求空Trait |
| Cancel | Provider/Tool/Shell/MCP/Permission/Subagent/parallel全部quiet |
| Ledger/Checkpoint/Resume | exact checkpoint；Safe/Confirm/Blocked；uncertain不自动运行 |
| Permission | Run scoped；持久失败handler=0；无global mutable profile |
| Sub Agent | 独立run/credential/snapshot/ledger；scope不扩大；parent cancel级联 |
| Events | 关键事实persist-first；失败fail closed；唯一terminal authority |
| Renderer | 不伪造sequence/terminal；重连exactly-once；local recovery非权威 |
| 验收 | 最终HEAD全部必需命令真实绿色，环境项如实标记 |

---

## 九、最终报告格式

更新：

```text
docs/architecture/agent-core-sol-remediation-implementation-report.md
```

必须包含：

```markdown
# Agent Core Pi 吸收最终闭环报告

## 1. 基线
- Branch：
- Start HEAD：
- End HEAD：
- Worktree：
- Shared Target：
- Disk：

## 2. 已保留能力

## 3. 本轮修复
| 问题 | 根因 | 修改 | 生产证据 | 测试 |
|---|---|---|---|---|

## 4. Pi 行为一致性
| Pi 行为 | Natives 实现 | 状态 | 证据 |
|---|---|---|---|

## 5. 恢复与安全不变量

## 6. 修改文件
| 文件 | 原因 |
|---|---|

## 7. 测试真实结果
| 命令 | 退出码 | 结果 | 是否最终 HEAD |
|---|---:|---|---:|

## 8. 未完成与环境项

## 9. 最终完成矩阵
| 能力 | 完成/部分/未完成 | 证据 |
|---|---|---|

## 10. Commit 与回滚
```

如果最终矩阵仍有“部分”或“未完成”，Goal保持未完成。不要用“基本完成”“大体完成”规避判定。

现在开始：从 `7686b20d` 创建唯一 `codex/agent-core-pi-final-*` 分支和独立 Worktree，确认没有分支/Worktree竞争后，读取当前实施报告，只执行批次 0。不要在基线或原始 Worktree 开发，不要重新研究整个 Pi 或重写 Agent Core。
