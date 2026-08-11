# Kimi Code Agent 工程复核

> 复核日期：2026-08-10
> 仓库：`/Volumes/UNTITLED/本人材料/project/kimi-code`
> 固定版本：`d1ded01b7c50c9847440f4645fe13f588becdc66`
> 研究范围：Harness、Tool Runtime 与并发、Permission/Sandbox、Hook、Prompt/Context、Compaction、Session、Subagent/Swarm、Task、Event/Replay、恢复与副作用重试。
> 结论性质：固定提交的生产源码、测试合同、配置和文档静态复核；未启动 Kimi Code，未执行其全仓测试，未把工作树中的 `._*` 文件纳入证据。

## 1. 结论

Kimi Code 当前 CLI 默认进入 v2；只有显式 legacy 开关才走 v1，`kimi web` 也直接进入 v2 kap-server。v2 最有价值的不是 DI 数量，而是四个可拆取机制：每 Agent 的 Wire reducer/journal、基于资源读写集合的 ToolScheduler、可恢复且有预算的 Goal、以及独立 Agent scope + child lineage。Context replay、tool-pair 修复、compaction stale-prefix 防护、后台 Task 终态通知和 WebSocket cursor 也都有生产实现与专项测试。

但 Wire 不能被称为严格 Event Store。`dispatch()` 先改内存 model，再异步 append；append 失败只记录错误，不回滚。普通 close/archive 没有显式等待 `wire.flush()`，而底层 append log release 启动异步 retirement，调用方不等待。kap-server 也先分配 seq、加入 live tail 并 fan-out，JSONL write-behind 失败后事件成为 live-only；重启后还可能从旧磁盘 watermark 复用该 seq。两处都适合“恢复友好的本地应用”，不满足 Natives 已有 persist-first RunEvent 权威。

安全边界也必须降级评价：Permission 链清晰且可解释，但 approval broker 缺失时自动批准；Bash 没有资源声明而被调度器视为 `all`；系统提示明确环境没有 sandbox；内建文件工具仅做 lexical path 归一化，未证明 symlink/physical identity；external Hook 的 spawn/timeout/abort/普通非零/malformed JSON 多数 fail-open，并继承完整环境。Subagent 复制父 permission mode 和 user tools，未证明权限规则做 parent∩task∩profile 收缩；Swarm 并发上限默认可空。

综合评价：

| 维度 | 评价 | 固定树依据 |
| --- | --- | --- |
| Kernel/loop | 强（Goal）/中（普通 Run） | Goal 有 turn/token/wall-clock budget；普通 loop 的 `maxStepsPerTurn=0` 表示无上限，缺统一 Run 总预算 |
| Tool runtime | 强 | resolve/preflight/Hook/permission/scheduler/execute/truncate/result 集中编排 |
| Tool 并发 | 中偏强 | 非冲突并发、queued-before fairness；完成顺序结算，路径身份和跨 Run lease 不完整 |
| Permission | 中 | ordered policy 可解释；broker 缺失 fail-open，配置规则接入固定树未证明 |
| OS sandbox | 弱 | 系统提示明确无 sandbox；主要防线是 permission 与工具自身路径检查 |
| Hook | 弱到中 | 事件面广；PreToolUse 只能阻断，runner 多数失败放行，输出无读取上限 |
| Prompt/context | 强 | prompt/tool hash、tool snapshot、同 turn prompt/model freeze、Wire fold/replay |
| Compaction | 强（算法）/中（提交） | tool-pair 与 stale-prefix 防护完整；apply/inject/complete 非事务且无统一 flush ack |
| Session/recovery | 中偏强 | Wire restore/migrate/rehydrate、fork 前 flush；常规关闭与持久失败语义不足 |
| Subagent/Swarm | 中偏强 | child 独立 Agent/Wire、所有权检查、批量限速；权限收缩、递归深度和默认总并发不足 |
| Task | 中 | 双轨持久、输出限额、lost reconciliation；持久错误可被吞掉，不能作强事实账本 |
| Event/replay | 强（连接）/中（执行） | `{seq, epoch}` reconnect 完整；Activity/EventBus 非 durable，WS journal write-behind |
| 模块深度 | 中 | DI scope 清楚，但核心协调服务 848–1,457 行，调用链跨大量 Service/Ops/Types |

## 2. 证据口径

本报告按以下标签约束措辞：

- **源码已证实**：固定提交的生产调用链直接成立。
- **测试已证实**：固定提交的测试明确锁定该合同；本轮只静态阅读，没有执行测试。
- **文档意图**：README、`AGENTS.md`、prompt 或注释声明的目标，不单独当作生产事实。
- **固定树未证明**：全树检索未找到调用方、事务、隔离或约束，不能反向断言它绝不存在于外部系统。
- **推断风险**：由多个已证实机制组合得出的工程风险，不冒充已复现故障。

关键证据集中在：

- 默认入口：`apps/kimi-code/src/cli/experimental-v2.ts:1-35`。
- Wire 与日志：`packages/agent-core-v2/src/wire/wireService.ts:50-313`、`src/persistence/backends/node-fs/appendLogStore.ts:49-258`。
- Session 生命周期：`src/workspace/sessionLifecycle/sessionLifecycleService.ts:424-650`。
- Tool：`src/agent/toolExecutor/toolExecutorService.ts:187-682`、`toolScheduler.ts:1-109`、`src/tool/toolContract.ts:1-150`。
- Permission：`src/agent/permissionPolicy/permissionPolicyService.ts:32-68`、`permissionGateService.ts:31-99`、`toolApprovalService.ts:101-215`。
- Hook：`src/agent/externalHooks/runner.ts:58-233`、`externalHooksService.ts:145-399`、`src/app/externalHooksRunner/runner.ts:50-115`。
- Context/Compaction：`src/agent/fullCompaction/fullCompactionService.ts:86-827`、`strategy.ts:248`、`contextMemory/loopEventFold.ts`。
- Goal/Subagent/Swarm/Task：`goalService.ts`、`tools/agent/agentTool.ts`、`session/swarm/agentRunBatch.ts`、`task/taskService.ts`。
- Web replay：`packages/kap-server/src/transport/ws/v1/sessionEventJournal.ts`、`sessionEventBroadcaster.ts`、`wsConnectionV1.ts`。

## 3. 默认执行链与权威边界

**源码已证实。** 默认入口路径为：

```text
kimi / kimi -p / doctor
  -> experimental-v2 CLI
  -> Workspace + Session scope
  -> per-Agent scope
  -> LoopService
  -> LLMRequester
  -> ToolExecutor
       resolveExecution
       preflight + permission
       before/will/started hooks/events
       ToolScheduler
       execute + post hook + output truncation
       tool.result
  -> ContextMemory + Wire models/journal
```

v2 以 DI Scope 划分 App、Workspace、Session、Agent。每个 Agent 拥有自己的 Wire、Context、Profile、Tool registry、permission mode 和 Task；Session 管理 Agent lifecycle、swarm 与交互。`packages/agent-core-v2/AGENTS.md` 将这一结构描述为 v2 移植目标，属于**文档意图**；上面的默认入口和生产注册才是**源码已证实**。

Wire 是同一 Agent 内的 replayable model state 聚合器，不是跨 Session 的全局 Run 事实权威。Op reducer 先更新 model，journal 用于恢复；duplicate Op 注册 fail-fast，未知/损坏记录在 replay 时跳过并报告，restore 还能 migrate、rewrite 和 rehydrate。

**推断风险。** 大量 Agent state 进入 Wire 有利于本地恢复，但如果把同样模式直接放到 Natives，会与现有 `RunManager + EventSequencer + assistant.db` 形成第二套 Run/Trace 权威。可吸收的是 reducer/projection 技术，不是 Wire 存储边界。

## 4. Wire、Session 与恢复

### 4.1 Wire 的真实提交语义

**源码已证实。** `WireService.execute()` 在 `wireService.ts:244-257` 先执行 reducer 改内存，再调用 append，随后发布 signal。append 进入 promise queue，错误交给 handler；不会回滚已更新 model。`flush()` 能等待 persist queue 和 append log，但普通 session close/archive 在 `sessionLifecycleService.ts:424-490` 只 drain Agent 工作再 dispose，没有显式 `wire.flush()`。

底层 `AppendLogStore.release()` 启动 retirement promise；真正 durable flush 在异步 settle 中完成，release 本身不返回该 promise。与之相对，session fork 在复制活跃 Agent journal 前会显式 `await wire.flush()`，再读取并 atomic rewrite 目标日志（`sessionLifecycleService.ts:623-649`）。

**测试已证实。** Wire restore 测试覆盖未知/损坏记录跳过、迁移、fork boundary 和 active Goal 的恢复降级；这些证明“可打开并对账”，不证明宕机点上的 exactly-once。

**推断风险。** 进程在 reducer 生效、append 未落盘或 release retirement 未完成之间退出，会使本次内存事实消失。Kimi 选择 availability/best-effort，本身合理；Natives 的 durable RunEvent、permission、tool settlement 和 terminal 不能采用该顺序。

### 4.2 Context replay 与中断修复

**源码已证实。** loop 将 `step.begin/content/tool.call/tool.result/step.end` 写入 Wire；live 与 replay 共用 fold。恢复遇到未闭合 tool exchange 时会补 synthetic interrupted result，deferred message 保持 tool call/result 邻接。Undo 使用 checkpointed conversation time，compaction summary 形成边界，不能跨 summary 任意回退。

这是值得吸收的“历史卫生”合同：恢复不能留下 orphan tool call，也不能把新消息插入 call/result 之间。但 synthetic result 只能说明“记录中断”，不能证明外部副作用未发生。

## 5. Tool Runtime、并发与输出

### 5.1 中央执行管线

**源码已证实。** `ToolExecutorService` 统一完成参数/schema preflight、tool materialize、access/approval/display 解析、PreToolUse veto、will/started event、scheduler、执行、按序 post hook、输出截断和 result event。PreToolUse 只能 deny，不能改写参数；参数 identity 在 resolve 后不再被 Hook 替换。

`ToolRegistry.register()` 会先删除同名工具再写新 entry。**固定树未证明** registry 具有 revision/digest 或 tool call 对旧 definition 的 stale binding。LLM request 每次从 live tool surface 构建 schema；同一 turn 只冻结模型参数和 system prompt，不冻结工具实例。

**推断风险。** Tool 注册热替换若发生在模型看到 schema 与真正 dispatch 之间，调用可能落到同名新实现。Natives 应继续把 tool materialization digest 固定在 Run snapshot，并由 Capability Gateway 执行前复核。

### 5.2 并发与结算顺序

**源码已证实。** Scheduler 允许不冲突 access 并发，并对更早排队任务实施 queued-before fairness。调用方通过 completion race 逐个收取已完成任务，post hook、truncate、result event 和 context settlement 因此按**完成顺序**发生，不是模型 call order。

内建 Read 等文件工具会在 resolve 时计算 lexical normalized path，并把同一字符串交给 access、approval、display 与 execute；这比各阶段重复解析强。但 **固定树未证明** symlink/realpath/inode identity。Bash 不声明 accesses，executor 将其默认为 `all`，所以会与其它调用冲突，偏保守但并发收益低。

**测试已证实。** Scheduler 测试覆盖非冲突重叠和更早冲突队列的公平性；ToolExecutor 测试覆盖完成顺序交付与 malformed args。

**对 Natives。** 吸收 `read/write/all` 冲突矩阵和 queued-before fairness；结算应由现有 RunEvent 记录明确 call index 与 completion index。资源 key 必须由 Capability Gateway 物化成 canonical identity，并扩展为跨 batch/child Run lease。

### 5.3 大输出

**源码已证实。** 纯文本结果超过 50,000 字符时，完整内容 atomic 写入 Agent scope，模型只收 2,000 字符 preview、绝对路径、字符/字节数。media 或已标记 truncated 的结果不再次处理；保存失败直接返回原大结果。

这是有界 context 的好做法，但绝对路径不是稳定 Artifact identity，保存失败回退又会重新撑大 context。Natives 应复用既有 run-scoped ArtifactStore，返回 artifact id/hash/provenance + bounded preview；不得复制裸路径协议。

## 6. Permission、Sandbox 与 Hook

### 6.1 Permission 顺序

**源码已证实。** policy 顺序固定为：auto + AskUserQuestion deny → 用户 deny → auto approve → session approval history → 用户 ask → 用户 allow → sensitive-file ask → Git control path ask → yolo approve → tool default approve → Git cwd write approve → fallback ask。决策包含 policy name 和 reason，便于解释。

但 `ToolApprovalService` 在没有 approval broker 时返回 approved。**测试已证实** `toolApproval.test.ts:243-269` 明确锁定这一 fail-open 行为。内建文件工具还可能在 resolve 阶段先拒绝敏感路径，使后面的 sensitive-file ask 分支不可达。

配置 schema 存在 `[permission]`，但固定树检索不到生产 `rulesService.addRules` 调用。这里必须写成**固定树未证明配置规则已接线**，不能写成“配置一定无效”。

### 6.2 Sandbox

**文档意图。** `system.md:81` 明确告诉模型运行环境不在 sandbox，动作会直接影响用户系统。**源码已证实** Bash 直接在 Session cwd 启动宿主 shell；固定树没有 Kimi 自有的 OS sandbox 执行层。因此 permission 是主要安全门，不能把模型提示当隔离。

文件 access 使用 lexical path，Hook/approval/resource scheduler 也未共同绑定 physical identity。**推断风险。** symlink 替换、路径别名和 TOCTOU 会让“审查对象”与“执行对象”偏离。Natives 现有 Schema → PathScope → Permission → sandbox 顺序不得降级。

### 6.3 External Hook

**源码已证实。** v2 Hook 事件覆盖 Tool、Prompt、Stop、Compact、Permission、Session、Subagent、Task 和 Heartbeat。相同 matcher 的命令并行 `Promise.all`；任一结构化 block 可以阻断。Prompt Hook 可 block 或追加 context，Stop 最多允许一次 continuation，PreToolUse 只 block、不改参数。

runner 对 spawn error、timeout、abort、普通非零退出和 malformed JSON 多数返回 allow；只有退出码 2 或有效结构化 deny/block 才阻断。child 继承完整环境，stdout/stderr 先完整收集，无读取级上限；超时先 SIGTERM，100 ms 后 SIGKILL，固定树未证明清理完整进程树。

**推断风险。** v2 external Hook 只能作为 observer/体验扩展，不能进入 Natives security decision path。安全 Hook 必须继续由冻结 Run Hook plan + Capability Gateway fail-closed 执行；observer Hook 的放行也要产生明确 RunEvent，不能伪装为未执行。

## 7. Prompt、Context 与 Compaction

### 7.1 Prompt 可追踪性

**源码已证实。** Profile/Wire 保存渲染后 prompt、AGENTS 路径、tool allow/deny、subagent allowlist 和 render generation。每次 LLM request 计算 SHA-256 `systemPromptHash` 与 `toolsHash`，首次见到的 tool schema 保存 snapshot。一个 turn 内 model context、request params 和 system prompt 冻结；工具表仍按请求实时获取。

这一 trace 设计可吸收，但 hash 只解决“看到了什么”，不自动解决“执行了什么”。Natives 应把相同 digest 写进现有 Run snapshot、ContextSnapshot 和 Provider/Tool RunEvent，而不是新增 prompt trace 表。

### 7.2 Compaction

**源码已证实。** full compaction 使用 0.7/0.5/0.35 多轮收缩；策略不会在 open tool exchange 中切断。提交前要求原历史是当前 history 的严格 prefix，tail 只能新增真实 user input；前缀内容变化则取消。测试覆盖新追加 user message 可接受、prefix 改写取消和 tool-pair 边界。

风险在提交边界：apply compaction、post-compaction injection、complete/event 是多次独立 dispatch，没有共同事务或强制 flush。Undo 也不会跨 summary。**推断风险。** crash 可能留下“summary 已应用但 completion/injection 未完整记录”等中间形态。

**对 Natives。** 吸收 stale-prefix CAS、tool-pair 原子分段和递减目标；提交必须落到现有 `ActiveContextSnapshot` + `ContextSnapshotCommitted/ContextCompressed` RunEvent，并遵守 persist-first。

## 8. Goal、普通 Run 与重试预算

**源码已证实。** Goal 是 main Agent 专属状态，包含 active/paused/blocked/complete、turn/token/wall-clock budget、continuation 和终态原因。active 恢复后降为 paused；fork 清除 Goal 并注入提醒。wall-clock 使用 monotonic live deadline，恢复时以 epoch anchor 计入离线时间。达到 token/turn budget 后最多给一次“无工具收尾” grace step；grace 中的工具调用被拒绝。

**测试已证实。** `goal.test.ts` 覆盖 hard deadline、budget grace、grace tool rejection；`resume.test.ts:673` 覆盖恢复 active interval 到 paused/budget-reached；`index.test.ts:389-439` 覆盖 fork 清 Goal 和提醒。

普通 loop 不具备同等级总预算。`maxStepsPerTurn` 只有大于 0 才生效，持久默认值为 0；Provider retries 另有 attempt budget。**固定树未证明**普通 Run 总是存在统一 token/tool/wall-clock/Provider attempt 联合预算。

Kimi Goal 的状态语义值得进入 Natives Run metadata/RunEvent；预算执行仍应由现有 RunManager/AgentEngine 统一，而不是增加 Goal runner。

## 9. Subagent、Swarm 与 Task

### 9.1 Subagent

**源码已证实。** child 是独立 Agent scope，拥有独立 Wire、Context、Profile、model binding 和 task state；Session 维护 flat registry，父级保存 label/parent tool call，并把终态摘要返回父 context。resume 要求 child 属于当前父 Agent 且 idle。

新 child 会复制父 `permissionMode`，并继承 user tools。profile allowlist 可以限制可选类型，但**固定树未证明**每次 assignment 都生成 parent∩task∩profile∩host 权限快照。auto/yolo 因 mode copy 可向下传播。内建 coder profile 可继续使用 Agent/AgentSwarm，explore profile 不含 Agent；固定树未见统一 recursion depth 或 subtree 总预算。

过短的 child 结果会触发一次 continuation。父只接收最终摘要，不自动导入 child 全历史，这是良好隔离；文件系统和外部副作用仍可能共享，不能称为安全隔离。

### 9.2 Swarm

**源码已证实。** 正常阶段首批立即启动 5 个，此后每 700 ms 启动一个；rate limit 模式动态收缩 capacity、指数 backoff 并逐步恢复。`KIMI_CODE_AGENT_SWARM_MAX_CONCURRENCY` 可设正整数上限，但未设置时返回 `undefined`。

**测试已证实。** `sessionSwarm.test.ts:86-113` 锁定“首批五个 + 每 700 ms 一个”，并覆盖显式 maxConcurrency 和 rate-limit 恢复。

**推断风险。** launch pacing 不是运行并发上限；环境变量未设时，长任务可积累大量 active child。Natives 应把这种 rate-limit adaptation 合入现有 child Run admission/budget ledger，默认必须有有界全局与父级配额。

### 9.3 Background Task

**源码已证实。** Task 支持可选 `maxRunningTasks`；process 输出超过 16 MiB 会停止，内存保留 1 MiB ring，terminal Wire record 保留 4 KiB tail。detached task 以 atomic JSON + append output 保存，started/terminated 同时写 Wire。恢复时未终态任务统一标记 `lost`，并生成 terminal notification；notification 进入 context，且 checkpoint/undo 有去重合同。

**测试已证实。** reconcile 测试覆盖 previously-running → lost，task manager 测试覆盖 `maxRunningTasks`，持久化测试覆盖目录权限和兼容读取。

但 `persistLive()` 与 output append queue 的 catch 为空，持久错误会被吞掉。**推断风险。** disk task 与 Wire 生命周期可能分叉，不能把 Task 双轨当强 operation ledger；对副作用任务也不能由 `lost` 推断“未执行”。

## 10. Event、ActivityView 与 WebSocket Replay

### 10.1 ActivityView

**源码已证实。** Agent EventBus 是同步、进程内发布。ActivityView fold turn/step/delta/tool/retry/permission/task/compaction 事件，并从 loop、task、compaction 当前态 seed；Wire restore 只补 last turn。它是只读、可丢弃重建的 UI projection，不是 pending approval/tool/stream 的完整 durable aggregate。

这种 projection 分层值得吸收：Natives Renderer 也应只消费 RunEvent/Snapshot 的投影，不反推“已执行”。但 seed/publish 逻辑应留在 Daemon/Host projection，不新增 Renderer 状态权威。

### 10.2 kap-server cursor

**源码已证实。** per-session JSONL 保存 header `{epoch}` 与 durable `{seq,envelope}`；客户端按 `{seq,epoch}` reconnect，buffer overflow/epoch mismatch 要求 resync。assistant/thinking/tool-call delta、tool progress、shell output/start/complete、agent status 等为 volatile，不进 journal。journal 能容忍 torn/malformed line，readSince 先 flush。

持久模型仍是 write-behind：`nextSeq()` 先增长，`append()` 排队，broadcaster 同步把 envelope 放入 tail 并 fan-out；`flushOnce()` 写失败只 warning，已取出的 pending lines 不回队。事件会“本次仅 live”，重启扫描磁盘后 seq 可回到旧 watermark。

**推断风险。** 这是连接恢复协议，不是执行事实协议。Natives 已有 EventSequencer 在 disk 成功后才 memory/broadcast，且明确分离 live-only delta；只能吸收 epoch/cursor/resync 和 bounded tail，不能复制 kap journal 为第二事件轨。

## 11. 副作用重试与取消

**源码已证实。** Tool executor abort 后只等待 2 秒 grace；Future 结束不等于外部进程/网络副作用已停止。MCP 连接错误可以 reconnect 后重调一次；固定树没有通用 operation identity、started/unknown/completed journal，也没有把 retry 限制为已证明无副作用的请求。

**固定树未证明** Kimi 对任意 Tool 提供 exactly-once 或 unknown-outcome recovery。Task recovery 的 `lost`、Context recovery 的 interrupted result都只是状态对账。

**对 Natives。** 保留现有 `mark_tool_call_uncertain`/recovery-blocked 语义：Tool started 先形成 durable fact，settlement 落盘失败则标 uncertain，任何 write/external effect 不透明重试。Kimi 的 reconnect 只可用于 transport 恢复，重调必须由 Capability Gateway 根据 effect contract 决定。

## 12. 模块深度与成熟度信号

固定树热点包括 `taskService.ts` 1,457 行、`goalService.ts` 1,328 行、`loopService.ts` 1,219 行、`profileService.ts` 1,022 行、`toolExecutorService.ts` 984 行、`fullCompactionService.ts` 901 行、`llmRequesterService.ts` 848 行。2026 年以来 v2/kap/default-entry 相关路径约 50 个提交。

**源码已证实。** Service/Ops/Types、Scope 和 generated manifest 给依赖边界提供一致命名；测试面覆盖 Wire、Goal、Tool、Permission、Compaction、Task、Swarm 和 kap replay。

**推断风险。** 核心行为仍集中在若干千行协调器，同时需要跨多个小 service 追踪真实顺序；继续拆“只有一个实现的接口”不会自然降低复杂度。Natives 应吸收有独立不变量的深模块，拒绝复制装饰器/DI 微模块数量。

## 13. 对 Natives 的映射

| Kimi 机制 | 是否吸收 | Natives 既有落点 | 必须修正的边界 |
| --- | --- | --- | --- |
| Wire reducer + replay/rehydrate | 部分吸收 | Daemon `RunManager`、`EventSequencer`、RunEvent projector | 仍须 persist-first；不新建 Wire/Trace DB |
| ToolScheduler access conflict | 吸收 | `PermissionGatedTools` + Capability Gateway | canonical resource identity、跨 child Run lease、稳定结算序号 |
| Tool output offload | 吸收目标，不复制协议 | Daemon ArtifactStore + ToolCall RunEvent | stable artifact id/hash/TTL/quota，不暴露裸路径 |
| Ordered permission explanation | 吸收 | Capability Gateway decision/finding | broker 缺失必须 fail-closed；配置冻结到 Run snapshot |
| External Hook | 仅观察型可参考 | 冻结 Run Hook Dispatcher + HookInvocation RunEvent | security Hook timeout/parse/spawn 必须 deny；最小环境与进程树清理 |
| Prompt/tools hash | 吸收 | Harness Run snapshot + ContextSnapshot + Provider/Tool RunEvent | 同一 revision/digest 贯穿 preview、provider、execute、trace |
| Compaction stale-prefix/tool-pair | 吸收 | `ActiveContextSnapshot`、`ContextSnapshotCommitted`、`ContextCompressed` | 单一 CAS/事务提交，不做多 dispatch best-effort |
| Goal lifecycle/budget | 吸收 | Run metadata + RunEvent + AgentEngine budget | 不建第二 Goal runner；普通 Run/child Run 共享总预算 |
| child Agent scope/lineage | 吸收语义 | 现有 child Run authority、`subagent_store`、`task_store` | parent∩task∩profile∩host、深度/树预算、独立 credential lease |
| Swarm pacing/rate-limit | 部分吸收 | child Run admission/budget ledger | 默认有界并发；waiting/approval 资源回收；不建第二 scheduler |
| ActivityView | 吸收 | Daemon/Host RunEvent projection | Renderer 不推断 executed；projection 可重建 |
| `{seq,epoch}` reconnect | 吸收协议思想 | `run.watch` + durable cursor/live cursor | authoritative sequence 仍由 EventSequencer；不新建 kap journal |
| Task lost reconciliation | 部分吸收 | task store + child Run recovery | `lost` 不等于无副作用；持久失败不可吞掉 |

优先级建议：

1. **P0：Tool materialization + resource identity。** 在现有 Capability Gateway 固定 tool definition digest、canonical access set、call index/completion index，并让 child Run 共用 resource lease。
2. **P0：副作用 unknown outcome。** 复用现有 `mark_tool_call_uncertain`，补 Tool started/settled/uncertain 的 persist-first 验收；禁止透明重调 write/external effect。
3. **P1：Context CAS。** 将 stale-prefix、tool-pair sanitation 和递减压缩目标并入现有 ContextSnapshot/RunEvent 事务。
4. **P1：统一预算。** 把 Kimi Goal 的 turn/token/wall-clock/grace 语义扩展到普通 Run 与 child subtree，由 RunManager 唯一执行。
5. **P1：可解释权限与 Hook。** policy/finding/revision 进入冻结 Run snapshot；observer/security Hook 分轨且都产生 durable HookInvocation。
6. **P2：projection reconnect。** 为 `run.watch` 吸收 epoch/resync/bounded tail，但 durable/live cursor 继续使用 Natives 现有双轨合同。

## 14. 明确拒绝清单

- 不复制 Wire 或 kap JSONL 为第二套 Run、Session、Trace 或 Event authority。
- 不采用“先 reducer/先 fan-out、后 best-effort append”的 durable 事实顺序。
- 不采用 approval broker 缺失自动批准、security Hook fail-open 或无 sandbox 静默执行。
- 不把 lexical path、绝对输出路径或 Tool 名称当稳定 resource/artifact/definition identity。
- 不让 child 直接复制 auto/yolo 后扩大权限；不允许无默认上限的 Swarm 或递归树。
- 不把 `lost`、interrupted synthetic result 或 cancelled Future解释为副作用未发生。
- 不复制大量一实现接口、decorator 和 Service/Ops/Types 样板；只有独立不变量才形成新模块。
- 不新建 Provider、Credential 或 Prompt authority；child 继续使用 Natives Host Broker 的 run-bound lease 与现有 Provider route。

## 15. 残余空白

- 本轮未启动 Kimi Code、未运行测试，也未做统一模型/任务/硬件基准；“强/中/弱”不是性能排名。
- 未证明外部 Hook child 的完整进程树清理、文件路径的 physical identity、permission config 的生产接线或任意 Tool 的 exactly-once。
- 默认入口切到 v2 是源码事实；“v2 已成熟稳定”不是固定树可证明结论。
- `._*` AppleDouble 文件未删除、未读取为证据；外部仓库无任何修改。
- Natives 当前应用未启动；建议只基于现有 Daemon、RunEvent、Capability Gateway、ContextSnapshot 与 child Run 权威映射。
