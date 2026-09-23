# DeepChat Agent 工程复核

> 复核日期：2026-08-10
> 仓库：`/Volumes/UNTITLED/本人材料/project/deepchat`
> 固定版本：`4461a7f9b76f46ac98753f88c09a821c6df7a6ac`
> 研究范围：Agent Harness、Tool Runtime、权限与沙箱、Prompt/Context、压缩、Hook、Session、Pending Input、Subagent、Tape/Event/Replay 与模块设计。
> 结论性质：固定 `HEAD` 的源码、测试合同、架构文档与 Git 历史静态复核；未启动应用、未执行全仓测试。外置盘 `._*` AppleDouble 文件及工作树改动不纳入评价。

## 1. 结论

DeepChat 是前三个项目中，最接近“可审计 Agent 执行系统”的一个。它最成熟的部分不是 UI，也不是工具数量，而是把一次模型执行拆成了可追踪的逻辑轮次、Provider 请求和物理尝试；再用 Tape、ViewManifest、Run terminal、Tool batch settlement 和 durable pending input 把恢复语义补齐。它还拥有目前已经切换为生产面的持久 child Session 协作系统，具备权限重验、配额公平性、等待时释放 lease、结果摘要与完整结果引用。

它的主要问题也很明确：DeepChat loop 与 Direct ACP 是两套能力不对称的 backend；权限控制仍以应用层 broker、正则命令分析和用户确认缓存为主，并不是 OS sandbox；Tape 的部分 append/telemetry 路径为了可用性 fail-open，不能保证所有执行事实完整；多个 coordinator 和 dispatch 文件过大，行为知识虽然分层，却仍集中在少数认知热点。

综合评价：

| 维度 | 评价 | 依据 |
| --- | --- | --- |
| Kernel loop | 强 | logical round、request sequence、physical attempt 分离；retry/recovery/partial output 合同清楚 |
| Tool runtime | 强 | 保守并发、ordered settlement、truncated call 拒绝执行、batch state 持久化 |
| 权限与 sandbox | 中偏强 / 中偏弱 | exact action hash 与 execution-bound approval 很强；命令分析和隔离层仍偏弱 |
| Prompt/context | 强 | typed contribution、trust framing、稳定前缀、完整 turn 裁剪、ViewManifest |
| Compaction | 强 | summary/anchor 同事务 CAS、stale writer 防护、Tool pair 与 retained tail 合同 |
| Hook | 中 | typed observer、串行顺序、超时与输出上限清楚；fail-open 且继承完整环境 |
| Session/recovery | 强 | durable Queue/Steer claim、disposition fence、restart reconciliation |
| Subagent | 强 | 持久 child Session、mailbox、follow-up、resultRef、权限漂移重验、公平 admission |
| Event/replay | 强 | append-only Tape、ViewManifest、attempt provenance、frozen child head |
| 模块深度 | 中偏强 | 核心 policy module 较深；dispatch/coordinator/orchestration service 过宽 |

## 2. 证据口径与版本校正

本报告采用以下证据等级：

| 标记 | 含义 |
| --- | --- |
| **当前默认面** | 从固定版本的入口、装配和测试可追踪到的现行实现 |
| **并行 backend** | 当前存在，但不经过同一 Harness/Tool Runtime 的执行路径 |
| **兼容墓碑** | 只为历史数据解析、渲染或迁移保留，不是当前 model-facing 执行面 |
| **目标/历史设计** | 文档或 Git 历史中的旧方案，不作为当前能力结论 |

复核时发现一项需要优先纠正的文档漂移：`docs/FLOWS.md`、`docs/architecture/agent-system.md` 和 `docs/architecture/tool-system.md` 仍描述旧 `subagent_orchestrator`；但提交 `710af0d5a feat(orchestration): add proactive collaboration (#2082)` 已删除其 1,308 行执行器和 1,426 行测试。当前唯一 model-facing Subagent 工具是 `deepchat_subagents`，旧名称只保留在历史 transcript 渲染、待处理交互清理和兼容类型中。

因此，本报告不采用旧资料中的“默认 3 个 child、300 秒超时”结论。固定版本的当前实现是：每父 Session 最多 5 个 active live delegations，全局 Agent invocation admission 默认容量 6、pending 上限 256；未发现 child 总 deadline 合同。

## 3. 默认执行链与 Direct ACP 分叉

DeepChat 默认 Agent 路径可还原为：

```mermaid
flowchart LR
    Turn["SessionTurn"] --> Manager["AgentManager.resolveSessionHandle"]
    Manager --> Backend["DeepChatAgentBackend.open"]
    Backend --> Harness["DeepChatAgentHarness"]
    Harness --> Coordinator["TurnCoordinator"]
    Coordinator --> Runner["DeepChatLoopRunner"]
    Runner --> Process["processStream"]
    Process --> Engine["DeepChatLoopEngine"]
    Engine --> Round["consumeLogicalRound"]
    Round --> Tools["settleToolBatch"]
    Tools --> Terminal["settleTurn"]
```

`DeepChatAgentHarness` 对上提供 Session、生成、retry、pending input、permission interaction、compaction 和 lifecycle 操作；`createDeepChatAgentHarness.ts` 负责把 SQLite、Provider、Tool、Tape、Hook、Memory 与 Session 服务装配起来。真正的循环不变量集中在 `loop/deepChatLoopEngine.ts`，Provider request/retry/recovery 集中在 `loop/contextCoordinator.ts`，工具执行和 transcript/tape settlement 则由 runtime coordinators 完成。

Direct ACP 是一条独立 backend：它使用 ACP process/protocol runtime，不经过 DeepChat loop、ToolService、Tape view assembly 和同一套 Permission/Tool settlement。优点是能忠实接入外部 Agent；缺点是产品存在两套可观察能力、恢复能力和权限能力不完全等价的执行面。对 Natives 而言，这种分叉只能作为外部 Runtime Bridge，不能绕过 `Renderer -> Tauri Host -> UDS -> Agent Daemon` 的生产权威链。

## 4. Agent Loop 与重试不变量

### 4.1 三层执行身份

`loop/loopRun.ts` 和 `loop/contextCoordinator.ts` 明确区分：

- `logicalRound`：一次 Provider 语义轮次；工具结算后继续才进入下一轮。
- `requestSeq`：本逻辑轮中组装出的 Provider-visible request 版本。
- `physicalAttempt`：相同 request 的实际传输尝试次数。

transient retry 复用 payload、tool manifest、ViewManifest 和 `requestSeq`，只推进 `physicalAttempt`。context recovery 会改变 Provider-visible payload，因此推进 `requestSeq` 并把物理 attempt 重新从 1 开始。AI SDK 的隐式重试被关闭，`maxRetries: 0`，重试预算只由 `ContextCoordinator` 负责。

这个模型很有价值，因为“网络重放”和“语义上下文改变”不再混成一个 retry 计数。Tape 可以准确回答模型到底看到了哪个 view、同一个 view 发了几次，以及哪次返回了结果。

### 4.2 终止与 partial output

`DeepChatLoopEngine` 约 132 行，只负责：

- provider round fuse；
- 每 turn 最多 128 个 attempted tool calls；
- provider outcome 更新；
- tool batch settlement；
- terminal settlement。

首次出现语义输出后，不再对当前请求做透明重放，避免用户已经看到 partial answer 后又收到一份重复生成。stream 失败时已接收内容被保留，错误通过 terminal outcome 显式结算。tool batch、provider round 和 terminal 都走统一协调路径，测试覆盖 single terminal、round fuse、tool-call fuse、retry manifest reuse 和 partial-output no replay。

### 4.3 局限

- loop 核心很深，但 retry、manifest、context recovery 和 telemetry 仍主要聚集在约 960 行的 `contextCoordinator.ts`。
- 透明 retry 能证明“相同 payload 被重放”，不能证明 Provider 端没有已经产生但未返回的外部副作用；这一点必须保守记录。
- Tape append/attempt telemetry 有 best-effort 路径，执行成功并不总能推出审计事实完整。

## 5. Tool Runtime、并发与结算

### 5.1 Tool surface 与来源

DeepChat 的工具来自内建 Tool、MCP、Plugin、Skill 和 Agent tool。Session 解析后的 mapping 会冻结，并保留 source/ownership 信息。MCP/Plugin 工具若没有更可信的本地声明，默认按 `write + sequential` 处理；MCP `readOnlyHint` 只是一条外部 annotation，不会自行授予并发权。

Tool definition 的执行属性区分 effect 与 mode。当前并发条件非常保守：只有同一批至少两个调用、全部显式标记 `effect=read`、`mode=parallel`，且 Session 处于 `full_access` 时才并行；missing、malformed、duplicate 或 mixed contract 一律降为 sequential。

### 5.2 Ordered settlement

并行批次使用 `Promise.allSettled` 执行，但结果按模型 tool-call 顺序提交到 transcript、Tape 和下一轮 Provider payload。这样既获得只读工具吞吐，又保持 tool call/result pairing 的确定性。

对 streaming 中被截断或参数不完整的 Tool call，runtime 不尝试猜测执行，而是写入结构化失败并结算整批。全局 128 次上限统计 attempted call，防止 invalid/repeated call 绕过 fuse。需要用户 permission/question 的调用会先把已完成 batch 状态持久化，再终止当前 Run；交互完成后创建新的 resume Run，而不是把已经 terminal 的 Run 重新打开。

`PersistedToolBatchState` 保存调用、已提交结果和待交互 call id，使恢复不依赖 UI 当前是否仍保有某个弹窗。

### 5.3 Tool output

`ToolOutputGuard` 会对大输出生成有界 preview，并把完整内容外置。优点是上下文预算与原始结果分开；缺点是协议暴露本地文件路径，没有稳定 artifact id、SHA-256、TTL、ACL 和 lineage。Natives 已有的 `artifact_store.rs` 具备 run-scoped ID、SHA-256、25 MiB 上限、atomic write、fsync/rename，设计基线高于这里，不应退化成裸路径。

### 5.4 局限

- `runtime/dispatch.ts` 约 2,327 行，聚集工具解析、permission、interaction、并发、settlement、历史兼容和多种 Tool 来源分支，是显著认知热点。
- “全批纯读才并行”安全但偏保守，无法并行两个作用于不相交资源的写操作。
- 进程内 effect policy 不是文件锁、事务隔离或 OS sandbox；多 Session 仍可能竞争相同资源。
- `ToolService` 与 `AgentToolManager` 同时知道目录、权限、来源、设置和执行细节，接口面偏宽。

## 6. Permission、Approval 与 Sandbox

### 6.1 LLM reviewer 的 exact action envelope

`runtime/toolPermissionReviewer.ts` 把待审动作归一化为稳定 JSON，并对完整 action envelope 计算 SHA-256 `actionHash`。review model 必须在严格 JSON 中回显同一 hash：非法 JSON、字段缺失、hash mismatch、timeout 或 Provider failure 全部降为 `ask_user`；`high` 风险强制 ask，`critical` 强制 block。

这比“让模型自由判断一个命令是否安全”可靠得多，因为决策至少绑定到了被审动作，而不是绑定工具名或自然语言摘要。

### 6.2 execution-bound permission

`src/main/tool/permission/toolPermissionBroker.ts` 的批准上下文绑定：

- server/config generation；
- binding hash；
- tool name；
- execution id；
- arguments hash；
- source 与 effect。

执行前会重新解析 Subagent/MCP authority，避免 approval 后 Agent slot、MCP server 或 permission mode 已变化。`full_access` 可以跳过普通确认，但不能跳过工具声明的 `explicit_user` confirmation。live delegation 的 explicit spawn/follow-up 进一步使用 execution-bound one-shot receipt；proactive mode 是 Session 级 standing authorization，但不会提升 child 的文件、Shell 或 MCP 权限。

### 6.3 缺点与边界

- command risk 主要依赖正则、命令签名和参数模式，不能覆盖 shell AST、动态 expansion、解释器内部行为或 symlink/namespace 逃逸。
- 没有可证实的通用 OS/process sandbox。应用层 allow/ask/block 不等于系统级隔离。
- file/command/settings approval 主要保存在内存 Session 状态，不能视为统一 durable permission authority。
- LLM reviewer 是风险分类辅助，最终安全性仍取决于 broker 和 dispatch 前重验；不能让模型 review 替代 deterministic policy。

## 7. Prompt、Context 与 Compaction

### 7.1 Context contribution 与 trust framing

DeepChat 将 stable system prefix、checkpoint、完整历史 turn、active user input 按固定顺序组装。mutable summary、handoff 与 Memory 被作为 fenced、untrusted 的 user-role contribution 注入，不被提升为 system role。这样可以明确表达“这是历史材料，不是新的系统指令”。

上下文不足时，先移除可选 Memory，再从最旧完整 turn 开始裁剪；Tool call/result pair 不拆开。retained tail 同时满足配置的 turn floor 与约 25% input-token target，并设置 20k token 上限。它不是简单的“从尾部截字符串”，而是对 turn 结构做选择。

### 7.2 Compaction CAS

compaction 生成 mutable summary 后，会进行 secret-pattern redaction；随后 summary state 与 Tape anchor 在同一 SQLite transaction 内 CAS 提交。若旧 compaction 结果在新 anchor 已经提交后返回，CAS 失败，runtime 读取新的 persisted summary，而不是用 stale summary 覆盖新状态。

这条 `prepare -> summarize -> CAS anchor commit` 语义值得 Natives 直接吸收。它解决的是并发/崩溃下 summary 与执行事实的对应关系，而不只是 token 变少。

### 7.3 ViewManifest

每次 Provider-visible payload 都对应 `view/assembled` ViewManifest，包含：

- policy/builder version；
- included/excluded Tape refs；
- synthetic contribution provenance；
- compaction anchor；
- token budget；
- prompt/tool schema hash；
- `requestSeq`。

Manifest 不复制 raw prompt，hash 也排除 wall-clock 字段，避免不必要地存储敏感全文并提升确定性。transient retry 复用同一 manifest；context recovery 产生新 manifest。

不足是 secret redaction 基于模式，不是完整的凭据分类/扫描器；Memory、summary 或 Tool output 仍可能携带未命中模式的敏感数据。另一个不足是约 2,124 行的 `contextBuilder.ts` 同时承担 turn 识别、pairing、resume、预算、synthetic contribution 和多 Provider shape，认知半径过大。

## 8. Hook：观察面强，安全决策面弱

Runtime Hook 事件包括 `PreToolUse`、`PostToolUse`、`PostToolUseFailure`、`PermissionRequest`、`Stop` 与 `SessionEnd`。terminal 统一投影为 `Stop -> SessionEnd`；同一 Session 顺序执行，不同 Session 可以并行。event acceptance 时会冻结 Hook identity/command，实际执行前重新检查 eligibility。

command Hook 有 30 秒超时，stdout/stderr 有界。它适合审计、通知和外部自动化，但当前是 observer：不能阻断执行、改写 Tool 参数或参与 permission 决策；dispatch 失败 fail-open。

需要明确拒绝的细节：

- Hook 子进程继承完整 `process.env`，可能读取 Provider key、token 或其他宿主秘密。
- timeout 主要终止 shell process，grandchildren 可能继续存活。
- fail-open 合理地保护非关键观察面可用性，但 security-critical Hook 若照搬会在配置错误、命令缺失或超时时静默失守。

Natives 应把 Hook 分成 observer 与 decision 两类。observer 可以采用 DeepChat 的 typed event、同 Session 有序和 bounded execution；decision Hook 必须进入 `PermissionGatedTools` 的 fail-closed 权威链，并使用最小环境、进程组终止和结构化结果。

## 9. Session、Pending Input 与恢复

### 9.1 Queue 与 Steer

Queue 与 Steer 共用 durable pending store，但语义严格分开：

- Queue 等待 safe point 后开始新的输入处理。
- Steer 在当前执行可接纳时进入更高优先级通道，并和可见 user message 绑定。

Steer admission 在事务内写 user message 与 pending row；claim 时在事务内设置 `readAt`、创建 assistant message 并保存 `assistantMessageId`。claim object 必须以 `consume`、`release-after-rollback`、`block` 等 disposition 结算；缺少 disposition 时保持 claimed fence，不允许后续输入越过，从而避免 duplicate settlement。

### 9.2 restart reconciliation

`src/main/session/data/pendingInputs.ts` 的恢复规则覆盖中间状态：

- claimed Queue 已产生对应 queue message：视为已消费；
- claimed Queue 尚无 message：释放回 pending；
- claimed Steer 已有 message：结算并消费；
- blocked Steer：降为 Queue；
- Steer 缺失可见 message：补建 user message，避免隐藏输入。

这是很成熟的恢复思路：持久表不只是保存“有一条消息”，还保存 claim、可见 transcript 事实和 assistant continuation 的对应关系。

### 9.3 局限

- Session status 本身主要是 runtime projection，未 hydrate 时推导为 idle；active Run、lease 和部分执行中状态仍依赖内存与启动 reconciliation。
- Tape、transcript、pending input 和 Session status 虽然有协调事务，但不是所有 UI projection/search 更新都强一致。
- resume Run 是新 Run，语义正确；但跨多种 interaction 的状态机分散在 `turnCoordinator.ts`、`pendingInputPump.ts` 和 Session data coordinator 中，维护成本较高。

## 10. Tape、Event 与 Replay

DeepChat 明确区分：

- Tape：append-only execution facts；
- transcript：面向 UI 的对话投影；
- ViewManifest：某次 Provider 实际看到的上下文选择；
- attempt outcome：一次物理请求的结果；
- replay：根据 Tape、manifest 和 anchor 重建模型输入及执行关系。

attempt event 使用幂等 provenance key，replay 保留 `requestSeq/physicalAttempt`、Tool pair、anchor 和 synthetic provenance。Subagent lineage 记录 child 的 frozen Tape head，父 Session 不复制 child 全部 entries；这既保留可审计边界，也避免父上下文被 child 事实淹没。

优点是“模型看见什么”和“UI 显示什么”不再被当成同一个事实源。缺点是部分 Tape append、projection 和 search 路径仍为 best-effort：生成可以成功，但 attempt telemetry 或投影事实可能缺失。因此当前更准确的说法是“强 replay 设计 + 局部审计缺口”，而不是完整 event-sourced exactly-once runtime。

## 11. 当前 Live Delegation / Subagent

### 11.1 单一执行面

当前 `deepchat_subagents` 支持：

`spawn`、`send`、`follow_up`、`list`、`inspect`、`read_result`、`wait`、`interrupt`。

child 是持久 Session，一个 delegation 可以跨多个 follow-up turn。`send` 只写 mailbox context，不触发新 turn；`follow_up` 明确启动下一次 child turn。父只接收有界 handoff、result hash/resultRef 和 frozen Tape head；完整回答通过 `read_result` 按引用读取。

提交 `710af0d5a` 同时退役了未发布的 QuickJS Workflow runtime，选择持久 child Session 作为唯一执行面。这一取舍正确：对代码 Agent 而言，协作核心是可恢复的会话、权限、mailbox 和结果引用，不是再造一套通用 DAG/脚本虚拟机。

### 11.2 权限与资源治理

- `explicit` 模式的 spawn/follow-up 需要 execution-bound one-shot receipt。
- `proactive` 模式需要 Session standing authorization，但不提升 child 原有 Tool/文件/Shell/MCP 权限。
- 每次 protected action 前重查 parent capability、slot target、lineage、workdir、permission 与 MCP allowlist。
- workdir 改变时先把 permission 降为 `default`，再切 workdir，最后只恢复新旧策略交集。
- generation/model settings 按 turn 冻结；安全状态持续重新验证，避免长生命周期 child 使用过期权限。
- 每父 Session active child 上限 5，运行 lease 上限同为 5。
- 全局 `AgentInvocationAdmission` 默认运行容量 6、pending 256，并按 owner round-robin，避免一个父 Session 独占所有运行槽。
- child 等待 permission/question 时 suspend lease，继续执行前重新 acquire，提高有限推理配额利用率。

### 11.3 持久化与恢复

live delegation repository 分开保存 delegation、turn 和 event。启动恢复会协调 child Session、当前 turn、terminal result 与 parent delivery；测试覆盖 restart reconciliation、permission drift、waiting lease、mailbox backpressure、result hash 和 deletion races。

这套模型比“一个 Tool 内临时 spawn 多个 Agent”更适合 Natives，因为它承认 child 是长生命周期执行主体，而不是一段不可恢复的异步函数。

### 11.4 局限

- 没有 worktree 或资源 ownership 隔离，多个 child 仍可能并行写同一工作区。
- 没有 child 总 deadline；turn fuse 不等价于 delegation 生命周期预算。
- 不能承诺外部 Tool side effect exactly-once。crash 后只能按 durable facts 保守恢复，不能自动证明远端动作未发生。
- `liveDelegationService.ts` 约 1,851 行，repository 约 848 行；权限、恢复、mailbox、delivery、lease 与结果校验集中度较高。
- 旧文档仍描述已退役的 orchestrator，增加 Agent 和维护者误判当前执行面的风险。

## 12. 模块深度、测试面与 Git 演进

### 12.1 较深模块

- `DeepChatLoopEngine`：约 132 行接口承载 round/fuse/terminal 核心不变量。
- `ToolExecutionPolicy`：把并发资格集中成保守、可测试的 policy。
- `ToolPermissionReviewer`：stable envelope、action hash 与降级规则集中。
- `ToolOutputGuard`：模型可见预览与完整输出分离。
- Tape domain/ports：事实、manifest、projection 和 replay 职责清晰。
- `AgentInvocationAdmission`：全局容量、owner fairness 和可 suspend lease 封装在约 411 行中。

### 12.2 认知热点

| 文件 | 约行数 | 主要问题 |
| --- | ---: | --- |
| `runtime/contextCoordinator.ts` | 960 | request、retry、recovery、manifest、attempt telemetry 集中 |
| `runtime/turnCoordinator.ts` | 1,594 | Session turn、claim、transcript、terminal 与 rollback 路径集中 |
| `runtime/contextBuilder.ts` | 2,124 | 多来源上下文、pairing、resume 与预算策略集中 |
| `runtime/dispatch.ts` | 2,327 | 多 Tool 来源、权限、并发、交互和兼容逻辑集中 |
| `orchestration/liveDelegationService.ts` | 1,851 | child lifecycle、权限、mailbox、lease、delivery、恢复集中 |
| `orchestration/liveDelegationRepository.ts` | 848 | 多表状态转换与恢复查询集中 |
| `harness/createDeepChatAgentHarness.ts` | 404 | composition root 依赖面很宽 |

`DeepChatAgentHarness` 是宽 facade，大部分方法只是把调用转发到内部 coordinator。这能稳定上层入口，但没有显著降低理解整个 Session 行为所需的上下文。

### 12.3 测试面

固定版本静态统计：

- `test/main/agent/deepchat`：49 个文件，约 833 个 `it/test`；
- `test/main/tool`：21 个文件，约 162 个测试；
- Session、Tape 与 Orchestration 相关测试合计约 598 个。

高价值测试合同包括 loop single terminal、retry manifest reuse、partial output no replay、parallel-read ordered settlement、permission hash、one-shot approval、execution identity、compaction CAS、完整 turn retained tail、pending claim duplicate-settlement fence、Tape manifest integrity/replay/frozen child head，以及 live delegation 的恢复、permission drift、waiting lease、mailbox backpressure 与 result hash。

本轮没有执行测试，因此以上是“测试合同覆盖面”，不是测试通过声明。

### 12.4 Git 演进判断

`710af0d5a feat(orchestration): add proactive collaboration (#2082)` 一次性改动约 210 个文件，新增约 21k 行、删除约 7k 行，核心动作包括：

- 新增 live delegation 三表、repository、service、consent/safety、routes 与 UI；
- 新增全局 `AgentInvocationAdmission`；
- 新增 `deepchat_subagents` Tool；
- 删除旧 `SubagentOrchestratorTool` 及其测试；
- 退役未发布 QuickJS Workflow 数据；
- 增加权限 broker、effect classification、Session deletion/recovery 合同。

这是方向正确但 blast radius 很大的演进。优点是明确收敛到一个 child Session 执行面；风险是实现、文档、历史渲染和迁移合同跨越 Agent、Session、Tool、DB、Renderer 多层，旧架构文档已经出现漂移。

## 13. Natives Native 执行引擎吸收方案

### 13.1 应吸收，并落入现有权威

| DeepChat 机制 | Natives 落点 | 吸收方式 |
| --- | --- | --- |
| logical round / requestSeq / physical attempt | `src-agent-daemon/src/event_log.rs` +现有 RunEvent | 增加 provider attempt/view provenance；不新增 Tape 数据库 |
| ViewManifest | Agent Daemon context assembly + `event_log` | 记录 source refs、anchor、budget、prompt/tool schema hash，不复制敏感 raw prompt |
| typed context contribution / trust role | `crates/agent-core/src/context.rs` | 统一 system、trusted instruction、untrusted memory/summary/handoff 的角色与来源 |
| compaction CAS anchor | `crates/agent-core/src/compaction.rs` + Daemon persistence | 升级为 `prepare -> summarize -> CAS anchor commit`，保留完整 turn/tool pair |
| Queue/Steer durable claim | `src-agent-daemon/src/prompt_queue_store.rs` | 增加 claim、assistant continuation id、disposition fence 与 restart reconciliation |
| exact action hash | `PermissionGatedTools` | approval 绑定参数 hash、execution identity、capability binding revision，dispatch 前重查 |
| effect/resource-aware scheduling | `src-agent-daemon/src/runtime/tool_policy.rs` | 从“全批纯读”演进到资源读写冲突图；结果仍按模型调用顺序结算 |
| bounded output + resultRef | `src-agent-daemon/src/artifact_store.rs` | 所有大 Tool output 统一返回 artifact id/hash/preview/provenance，禁止裸路径成为身份 |
| persistent child Session/delegation | `child_run_orchestrator.rs` + `subagent_store.rs` | 增加 delegation、turn、mailbox、resultRef 与 frozen child event head |
| owner-fair admission | 现有 child run 调度层 | 全局运行 lease、每 parent 上限、permission wait 时释放、恢复前重新 acquire |
| typed observer Hook | 现有 Hook/Event 投影 | 同 Session 有序、跨 Session 并行、有界输出/超时；明确标记 fail-open observer |

这里最关键的约束是：DeepChat 的 Tape 概念要映射进 Natives 现有 persist-first `event_log`，不能新建第二套 Run/Event 权威；live delegation 要扩展现有 `child_run_orchestrator/subagent_store`，不能复制一套 Orchestration Session authority。

### 13.2 Natives 当前优于 DeepChat 的基线

- `artifact_store.rs` 已有 run-scoped id、SHA-256、25 MiB 限制和 atomic write/fsync/rename，优于裸文件路径结果。
- `event_log.rs` 已执行 persist-first RunEvent，并把 Tool terminal 与 side-effect ledger 放在同一事务，不能为了复刻 Tape 而降级成 best-effort append。
- 生产权威链已冻结为 `Renderer -> Tauri Host -> UDS -> Agent Daemon`，Direct ACP 只能通过外部 Runtime Bridge 进入 Daemon，不能旁路 Tool/Permission/Event。
- security-critical Hook 已被规范要求 fail-closed，不能照搬 DeepChat observer sink 的失败策略。

### 13.3 建议优先级

**P0：先补执行事实和权限身份。**

1. 为 RunEvent 增加 provider request/view/attempt identity 与 manifest digest。
2. approval 绑定 exact action hash、execution id、capability binding revision，并在 dispatch 前重验。
3. Prompt Queue 增加 durable claim/disposition/restart fence。

**P1：再补 context 与 child recovery。**

1. typed context contribution + trust framing。
2. compaction anchor CAS 与 stale writer 防护。
3. child delegation/turn/mailbox/resultRef，等待交互时释放 admission lease。

**P2：最后优化吞吐与生态。**

1. resource-aware Tool scheduler 与 ordered settlement。
2. observer Hook 的 typed delivery、最小环境和 process-group termination。
3. 由现有 event log 构建可丢弃 projection/replay 工具。

所有优先级都受 Natives 当前 `production_blocked` 和既有 P0 缺口约束。本报告只提供设计输入，不能把候选机制写成已实现能力。

## 14. 明确拒绝项

Natives 不应吸收：

- 第二套 Tape DB、Run、Session、Permission、Provider、Capability 或 Trace 权威；
- Direct ACP 绕过 Agent Daemon Native engine 的生产捷径；
- 以裸本地路径作为 artifact identity；
- 把 regex 命令检测宣传为 shell sandbox；
- 让 observer Hook 参与 security gate，或让 security Hook fail-open；
- 再造 QuickJS/通用 Workflow DAG 作为 Subagent 第二执行面；
- 宣称 child 外部副作用 exactly-once；
- 在没有 worktree/resource ownership 的情况下默认允许多个 child 并行写同一工作区；
- 把 mutable summary、Memory 或 child handoff 提升为 system instruction。

## 15. 总结

DeepChat 的核心长处可以概括为三点：执行身份足够精确，恢复状态足够显式，父子 Agent 协作被建模成持久会话而不是临时函数。它给 Natives 的最佳启发不是复制 Tape 或 Harness facade，而是把 `RunEvent` 补成“能够说明模型看见了什么、尝试了几次、基于哪个权限身份执行、从哪个持久锚点恢复”的单一事实权威。

同时必须保留边界判断：DeepChat 还没有通用 OS sandbox，部分审计写入会 fail-open，Direct ACP 与主 loop 能力不对称，协调器和 orchestration service 过宽。Natives 应吸收其不变量和恢复合同，落入现有 `event_log`、`PermissionGatedTools`、`prompt_queue_store`、`artifact_store`、`child_run_orchestrator` 与 `subagent_store`，而不是复制另一套执行平台。
