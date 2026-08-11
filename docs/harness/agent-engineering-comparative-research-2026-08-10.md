# 八仓 Agent 工程能力对标研究

> 研究日期：2026-08-10
> 研究对象：AtomCode、Claude Code、DeepChat、Goose、Grok Build、Kimi Code、Kun、OpenCode
> 研究范围：Agent Harness、Tool Runtime、Permission/Sandbox、Context/Compaction、Hook/Extension、Session durability、Multi-agent、Event/Replay、模块深度与可移植性。
> 详细原始证据：[agent-engineering-benchmark-2026-08-10.md](/Users/ldh/Downloads/project/AiNative/Natives/docs/pm-context/collect/agent-engineering-benchmark-2026-08-10.md)
> 单仓深度复核：[AtomCode](/Users/ldh/Downloads/project/AiNative/Natives/docs/harness/projects/atomcode-agent-engineering-review-2026-08-10.md)、[Claude Code](/Users/ldh/Downloads/project/AiNative/Natives/docs/harness/projects/claude-code-agent-engineering-review-2026-08-10.md)、[DeepChat](/Users/ldh/Downloads/project/AiNative/Natives/docs/harness/projects/deepchat-agent-engineering-review-2026-08-10.md)、[Goose](/Users/ldh/Downloads/project/AiNative/Natives/docs/harness/projects/goose-agent-engineering-review-2026-08-10.md)、[Grok Build](/Users/ldh/Downloads/project/AiNative/Natives/docs/harness/projects/grok-build-agent-engineering-review-2026-08-10.md)、[Kimi Code](/Users/ldh/Downloads/project/AiNative/Natives/docs/harness/projects/kimi-code-agent-engineering-review-2026-08-10.md)；Kun、OpenCode 按同一模板复核中，未完成单仓复核的横向评价仍是初步结论。

## 1. 结论先行

八个项目没有一个可以整体复制。它们的优势集中在不同的“深模块”：

| 项目 | 最强的工程构思 | 主要限制 |
| --- | --- | --- |
| AtomCode | 中立 Kernel、阶段化 Hook 合同、中央 Tool 输出/并发/fuse | v1/v2/bridge 并存；权限与 Hook 有启发式或 fail-open 边界 |
| Claude Code | 生产成熟度、插件分发、后台 Agent 恢复与隔离、版本能力演进 | 核心闭源；只能做文档/黑盒研究，行为强版本依赖 |
| DeepChat | Session/Run/Tape/projector/replay/lineage；逻辑轮次与物理尝试分离 | 两套 backend；总协调层过宽，部分 Hook 与 artifact 协议不够权威 |
| Goose | typed risk finding、egress 检查、渐进 tool-pair compaction、Recipe/Hook 生态 | 多个安全检查 fail-open；临时路径输出；Recipe 容易越界成为第二权威 |
| Grok Build | typed streaming Tool、TypedExtensions、safe-point interjection、两阶段 compaction、cursor replay 与 child lineage | Hook/sandbox fail-open；资源 key 未 canonicalize；完成顺序入史；多文件提交非事务；快照演进不可审计 |
| Kimi Code | Wire fold/replay、资源冲突 Tool scheduler、Goal 预算、child Agent scope、`{seq,epoch}` reconnect | Wire/WS 都是 write-behind；无 OS sandbox；approval/Hook 有 fail-open；child/Swarm 默认边界不足 |
| Kun | Tool Host 安全顺序、least-authority Graph assignment、Lead-approved handoff、content-addressed ArtifactStore | Graph/runtime 状态空间很重；composition root 巨大；非商业许可 |
| OpenCode | durable prompt admission + coalesced wake、Session 串行 runner、Context Epoch、typed Tool registry | v2 仍迁移中；durable continuation recovery、MCP/Task/完整 Plugin Tool 尚未完成 |

对 Natives 的总判断：应组合少数机制，而不是引入某个项目的整套运行时。

1. 最高优先级是把“可重放事实、不可变 Run 快照、权限与 Hook 结果、工具结果 Artifact”绑定到现有 Daemon 权威。
2. Tool Runtime 应采用 typed input/output + progress/terminal 合同，并由单一 Gateway 统一输出上限、并发与重试 fuse。
3. Prompt/Context 应按 source algebra 生成版本化快照；活动 Run 不随配置或外部 Runtime 变化。
4. 多 Agent 先吸收权限收缩、命名 handoff 和独立生命周期，不引入完整 Graph 第二权威。
5. 所有 security-critical Hook 保留 Natives 的 fail-closed；对标项目的 fail-open 只能记录为风险反例。

## 2. 研究边界与判定方法

### 2.1 版本与证据

| 项目 | 固定 commit | 研究可信度 |
| --- | --- | --- |
| AtomCode | `4677ddfa68a84897a0154fe56af3e3e3b173410f` | 源码和设计文档可核查；目标架构与当前并存实现分开评价 |
| Claude Code | `c39cb0f14bfe8bb519bae5bfc55add6867c5e2ab` | 仅公开 README/CHANGELOG/插件样例；核心内部不可证实 |
| DeepChat | `4461a7f9b76f46ac98753f88c09a821c6df7a6ac` | 源码/架构文档可核查 |
| Goose | `021b0db8dbee8d6c7e9ffbab580a4143598a3560` | 源码、测试合同与独立报告可核查 |
| Grok Build | `c68e39f60462f28d9be5e683d9cbe2c57b1a5027` | 导出快照；演进历史不可审计 |
| Kimi Code | `d1ded01b7c50c9847440f4645fe13f588becdc66` | v2 已是 CLI/web 默认；v1 仍保留，源码与测试合同可核查 |
| Kun | `9db7d4f34f26bd2faba8900fb84eaf7ce29f7661` | 源码/设计文档可核查；许可证限制直接采用 |
| OpenCode | `b8bd88901a4870ef3a5752840f4e23e11d54e24e` | v2 源码可核查；v1/v2 双轨且 v2 TODO 明确 |

能力标记采用“强 / 中 / 弱 / 不可证实”：强表示调用链和持久化边界均可追踪；中表示存在实现但边界或迁移状态有限；弱表示仅局部实现或目标文档；不可证实表示仓库证据不足。它们不是性能百分比或质量分数。

### 2.2 Natives 权威约束

本报告只提出可嵌入现有 `Renderer → Tauri Host → UDS → Agent Daemon` 的机制。不得新建第二套 Run、Capability、Provider、Credential、Permission 或 Trace 权威；不得放松 `docs/standards/technical/02-security.md` 的五道安全防线。

## 3. 逐仓结论

### 3.1 AtomCode

> 完整证据与 Natives seam 映射见 [AtomCode Agent 工程复核](/Users/ldh/Downloads/project/AiNative/Natives/docs/harness/projects/atomcode-agent-engineering-review-2026-08-10.md)。

**优点。** AtomCode 对职责边界的表述清楚：Kernel 只持有 Agent/Tool/Provider/Hook 的中立契约，approval、persona、code intelligence 被排除在外。工具执行采用 classify/gate、execute、ordered apply 三阶段；mounted surface、64 KiB 输出上限、读写 barrier、并发上限和按调用顺序结算集中在 Kernel。`LifecycleHooks` 有统一 `turn_complete`，compaction 则由策略提案、Kernel 重校验，并在手动压缩时执行 durable checkpoint 后提交。

**缺点。** CLI 默认 v2 仍经 bridge 使用 core 协议，目标架构不等于当前单一生产路径。Kernel 没有 OS sandbox，shell 权限依赖启发式扫描，Tool 参数仍是 raw JSON string。Claude-compatible Hook 的启动失败/超时 fail-open，且 `PreToolUse allow` 会短路其后的工作区写、Bash 范围与通用审批；worker 子 Agent 又在 `AutoRespond::AllowAll` 下运行，形成授权放大风险。snapshot、raw JSONL transcript 和 live event 的一致性不同，不能视为完整 durable replay。

**对 Natives 的取舍。** 吸收冻结 mounted surface、三阶段 Tool loop、ordered settlement、统一 terminal funnel、cache epoch 与 compaction prepare/checkpoint/commit。不复制 bridge、三文件 session 权威、Hook allow 短路和 Tool 内 `AllowAll` 子 Agent；security Hook 继续遵守 Natives fail-closed。

### 3.2 Claude Code

> 完整证据、可证实边界与 Natives seam 映射见 [Claude Code Agent 工程复核](/Users/ldh/Downloads/project/AiNative/Natives/docs/harness/projects/claude-code-agent-engineering-review-2026-08-10.md)。

**优点。** 从版本化行为可确认其生产工程重点：Subagent 默认后台且通过通知回流，长任务/worker 可跨进程停止、升级和 daemon 重启恢复，失败回复会保存后递送；child permission prompt 回传主会话，managed deny 高于 Hook allow/ask/updated input，catastrophic removal 在 bypass/auto 仍有兜底。worktree isolation、MCP reconnect/OAuth/tool search、50K 大输出外置、prompt cache 稳定化和 transcript/checkpoint 有界化都有持续演进。Plugin 又把 commands/agents/skills/hooks/MCP、marketplace policy 和 persistent data 组合为分发单元；`asyncRewake` 展示了后台结果事件驱动回会话的模式。

**缺点。** 核心闭源，不能审计真实 loop、Tool registry、完整权限排序、恢复事务或内部 module seam。CHANGELOG 反复修复 blank reopen、stale prompt respawn、wrong terminal status、Git mutation 落入 main checkout、symlink escape、managed deny 被 Hook 降级和 orphan tool result，说明后台 Agent + worktree + auto permission 的组合复杂。plugin-dev 文档与当前 Agent 字段、Hook timeout/事件有漂移；公开 Hookify/security-guidance 示例在 import、JSON、API 或状态失败时通常 fail-open。磁盘外置只证明 path + preview 行为，不证明稳定 Artifact identity。

**对 Natives 的取舍。** 把这些行为转成现有 Run/child Run 的验收合同：worker generation + durable safe point、pending input delivery/ack、worktree resource lease、tool-pair 跨 resume/compact 完整、permission ceiling、approval preview 规范化和 event-driven continuation。只读 Inspector/Bridge 先做 capability probe；Extension 仍经 Extension Host，安全 Hook fail-closed，大输出仍进入 Daemon ArtifactStore。不得从 CHANGELOG 虚构 Claude 内部实现。

### 3.3 DeepChat

> 完整调用链、当前 Live Delegation 校正与 Natives seam 映射见 [DeepChat Agent 工程复核](/Users/ldh/Downloads/project/AiNative/Natives/docs/harness/projects/deepchat-agent-engineering-review-2026-08-10.md)。

**优点。** `Session` 长期权威、闭合 `Run`、append-only Tape、ViewManifest 与 UI projection 的组合适合审计和回放。logical round/request sequence/physical attempt 三层身份使 transient retry、上下文恢复和工具循环不混淆；首次语义输出后禁止透明重放。Tool Runtime 只并行整批显式 `read + parallel` 调用，执行可并发但按模型 call order 结算。approval 同时采用 exact action hash 与 execution-bound broker，dispatch 前重查 MCP/Subagent authority。compaction summary 与 Tape anchor 同事务 CAS，Queue/Steer 又有 durable claim、disposition fence 和 restart reconciliation。

当前 `deepchat_subagents` 是唯一 model-facing Subagent 工具，child 是可跨 follow-up turn 的持久 Session。它提供 mailbox、resultRef/frozen Tape head、每动作权限重验、每父 Session 5 个 active child、全局默认 6 个运行槽与 owner-round-robin admission；permission/question wait 时释放 lease，恢复前重新申请。旧 `subagent_orchestrator` 已删除执行器，只保留历史 transcript 兼容墓碑。

**缺点。** DeepChat loop 与 Direct ACP 是两套 backend，存在能力不对称和双测试负担。`contextBuilder`、`dispatch`、`turnCoordinator` 与 `liveDelegationService` 等协调文件过宽。Runtime Hook 是 fail-open observer，继承完整 `process.env`，timeout 也不能保证清理 grandchildren；大输出仍以本地路径作为协议对象。命令风险分析不等于 OS sandbox，部分 Tape/telemetry append 失败也不会阻断生成。当前旧架构文档仍描述已退役 Subagent executor，存在文档漂移。

**对 Natives 的取舍。** 把 ViewManifest/attempt provenance 合并进现有 `event_log`，把 typed context contribution、compaction CAS、durable Queue/Steer、exact-action approval 和 persistent delegation 分别落到现有 context/compaction、`prompt_queue_store`、`PermissionGatedTools`、`child_run_orchestrator/subagent_store`。不引入第二套 Tape/ACP/loop；Artifact 继续由 Daemon ArtifactStore 负责；observer Hook 与 fail-closed security Hook 必须分离。

### 3.4 Goose

> 完整执行链、失败语义、证据行号与 Natives seam 映射见 [Goose Agent 工程复核](/Users/ldh/Downloads/project/AiNative/Natives/docs/harness/projects/goose-agent-engineering-review-2026-08-10.md)。

**优点。** Goose 把 Security、Egress、Adversary、Permission、Repetition 组织成有顺序的 Inspector 流水线，`InspectionResult` 统一记录 action、reason、confidence、inspector 和 finding id；`Allow` 不会覆盖其它 Inspector 的 deny/ask。Context compaction 默认按 0/10/20/50/100% 渐进移除中部 Tool response，保护当前 turn 并要求 request/response 成对。Recipe、Hook、ACP、MCP、Extension 与 Subagent 形成完整的生态装配面，ExtensionManager 还提供稳定 `extension__tool` 命名、owner metadata 和版本化 tool cache。

**缺点。** Inspector 异常、Adversary Provider 错误和 blocking Hook 的 spawn/timeout/序列化/普通非零退出均有 fail-open 路径；Egress 当前只提取并记录目标，随后返回 Allow，不能称为出站阻断。`Auto` 模式允许全部工具，Subagent 又被强制设置为 Auto，权限不会自然回到父 Session。MCP 大响应使用裸临时路径；写工具缺少资源冲突 barrier；pending steer、background task、AgentEvent 主要在进程内，缺少 durable Run/attempt/replay 事实。Recipe 同时承载工作流、扩展装配和运行配置，容易挤压 Harness/RunTemplate 权威。

**对 Natives 的取舍。** 只把 `RiskFinding` 作为 Capability Gateway 的解释/收紧输入；稳定 tool ownership、cache version、file 参数禁止 default 和 tool-pair compaction 可并入现有权威。Recipe 应转换为经过 Gateway 的 RunTemplate 输入包；ACP 的 `external_dispatch` 只能作为诚实的外部 Runtime Bridge 事件。observer Hook 与 security decision Hook 必须分层，后者继续 fail-closed；Artifact 继续使用 Natives run-scoped ArtifactStore，不能退回路径协议。

### 3.5 Grok Build

> 完整执行链、失败语义、证据行号与 Natives seam 映射见 [Grok Build Agent 工程复核](/Users/ldh/Downloads/project/AiNative/Natives/docs/harness/projects/grok-build-agent-engineering-review-2026-08-10.md)。

**优点。** `Tool::Args/Output` 在实现侧保持类型，只在 `ToolDyn` JSON 边界擦除；执行统一为 0..N progress + exactly-one terminal，adapter 能检测 missing terminal，`TypedToolOutput` 又分开结构化 JSON、模型内容和 completion output。`TypedExtensions` 按类型注入 CWD、Cancellation 与 SessionContext。SessionActor 在 safe point FIFO drain interjection；两阶段 compaction 校验 prefix fingerprint、模型、长度和 tool pair，sanitize 失败回退最小历史。`updates.jsonl` 还提供 `eventId` 游标续传、rewind 分支过滤、ToolCall folding 和 unfinished child 对账；Subagent 有独立 Session、lineage、terminal metadata 与 completed-child resume provenance。Unix process group/Windows Job Object 的进程树清理也比只 kill direct child 完整。

**缺点。** approved Tool 用 `FuturesUnordered` 增量执行，每个 Future 完成时立即写 ChatState，所以快工具先回 UI 的同时也按完成顺序进入模型历史，不是稳定 call order。相同文件 mutex 只使用未 normalize/canonicalize 的原始路径，且只覆盖当前 batch。默认 sandbox profile 为 `off`，内建 profile 应用失败会 warning 后无沙箱继续；macOS 网络限制为空操作，Windows 没有对应 OS sandbox。command/client Hook fail-open；HTTP Hook 只校验初始 URL，默认跟随 10 次未经复核的 redirect，body/命令输出在完整读取后才截断。compaction checkpoint、marker 和 chat replacement 只共享 FIFO，不共享事务/commit ack；普通 flush 也不等于 `fsync`。Tool/MCP 401 可重试但没有副作用 operation journal。Subagent 的 600 秒是前台等待预算，不是 deadline；worktree 创建失败会退回共享 workspace，running child 在重启后只会被归为 cancelled。`prompt_context.json` 会持久化，却不是恢复读取的权威，也没有 effective prompt hash。

**对 Natives 的取舍。** 吸收 typed streaming envelope、object-safe dispatch、TypedExtensions、safe-point steering、two-pass sanitation、cursor projection、child lineage 和进程树清理。把原始路径 mutex 升级为 canonical resource identity + 跨 Run lease；compaction 进入现有 ContextSnapshot/RunEvent 事务 CAS；Tool retry 接 side-effect journal；worktree/sandbox 降级必须显式失败。security Hook 继续遵守 Natives fail-closed，每次 HTTP redirect 重做 SSRF 校验。`updates.jsonl`/`events.jsonl` 只可启发 projection 与诊断，不成为第二套执行权威。

### 3.6 Kimi Code

> 完整执行链、持久化失败语义、测试合同与 Natives seam 映射见 [Kimi Code Agent 工程复核](/Users/ldh/Downloads/project/AiNative/Natives/docs/harness/projects/kimi-code-agent-engineering-review-2026-08-10.md)。

**优点。** v2 已是默认入口。每 Agent Wire 统一 reducer、journal、migration 与 rehydrate；Context live/replay 共用 fold，恢复会补 interrupted tool result。ToolScheduler 依据 read/write/all 冲突做非冲突并发和 queued-before fairness。Prompt/Tool schema 有 SHA-256 trace；compaction 做 tool-pair 分段、stale-prefix 校验和 0.7/0.5/0.35 递减。Goal 对 main Agent 提供 active/paused/blocked/complete、turn/token/wall-clock budget、恢复降级、fork 清除和一次无工具收尾。child 是独立 Agent/Wire，Swarm 有首批 5 个、700ms pacing 与 rate-limit capacity；Task 有 16 MiB process output hard cap、1 MiB ring、lost reconciliation。kap-server 的 `{seq,epoch}` 支持 reconnect/resync，ActivityView 保持可丢弃 projection。

**缺点。** Wire `dispatch` 先改内存再异步 append，普通 close/archive 未显式 await flush；kap journal 也先分 seq/tail/fan-out再 write-behind，失败事件成为 live-only，不能称为 persist-first。Tool result 按完成顺序入史；registry 同名静默替换且无 stale-call revision。lexical path 未证明 symlink/physical identity，Bash access 默认为 `all`。approval broker 缺失会 auto-approve，系统 prompt 明示无 sandbox；external Hook 对 spawn/timeout/abort/普通非零/malformed JSON 多数 fail-open，输出无读取上限。compaction 多 dispatch 非事务。child 复制父 permission mode/user tools，未证明 least-authority 交集；递归深度、subtree budget 和 Swarm 默认并发上限不足。Task 持久错误可被空 catch 吞掉；`lost`/interrupted 不等于副作用未发生。

**对 Natives 的取舍。** 吸收 reducer/projection、resource conflict fairness、Goal 状态与预算、tool-pair/stale-prefix、child identity、rate-limit adaptation 和 cursor/resync；全部落入现有 Daemon `RunManager/EventSequencer`、Capability Gateway、ContextSnapshot 与 child Run。Tool materialization 必须带 digest/canonical resource identity，Hook/approval/sandbox fail-closed，副作用接现有 uncertain/recovery-blocked，不复制 Wire/kap/Task 为第二套 Run、Trace 或 operation authority。

### 3.7 Kun

**优点。** Kun 的 Tool Host 先 sandbox、再 Hook、再 policy/approval、再执行，顺序清晰且有大量测试。PreToolUse 参数改写会重新过 policy；外部写审批绑定物理路径 inode 并复核 symlink；operation journal 对 unknown side effect 禁止自动重试。Graph 使用 parent∩graph∩profile∩node∩host 权限、immutable least-authority assignment、Lead-approved named data packet；ArtifactStore 做 content hash、quota、原子写入、权限位和 dedupe；read-only 工具只在安全策略下有限并发。

**缺点。** `runtime-factory.ts`、`delegation-runtime.ts`、`agent-loop.ts` 很大；Graph 的 7 天 run/24 小时 node 生命周期增加清理和恢复成本。产品面横跨 Harness、Workflow、Graph、Extension、Schedule，权威边界容易拥挤。PolyForm Noncommercial 使代码不能直接用于商业集成。

**对 Natives 的取舍。** 吸收 least-authority assignment snapshot、Lead-approved handoff、Artifact content addressing、event-driven supervision 与 unknown outcome 不重试。只实现这些机制的 Natives 版本，不引入 Kun Graph 全状态机或其巨大 composition root。

### 3.8 OpenCode

**优点。** `SessionV2.prompt` 的 durable admission 与 `execution.wake` 分离；`SessionRunCoordinator` 按 Session 串行、跨 Session 并发并合并 wake；steer/queue 在 provider-turn safe boundary 提升。Context Epoch 用 typed source algebra 冻结 Prompt baseline；Tool registry 以 Effect Schema 约束输入输出、物化 identity 和 stale call；Permission 是 ordered last-match 规则，agent deny 高于 saved allow；ToolOutputStore 统一有界化；EventV2/projector 支持 durable history。

**缺点。** runner 注释明确 durable continuation recovery、持久运行状态、集群 ownership、bounded retry、插件/取消/进度等尚未完成。v2 built-in tool 清单把 Task、MCP/plugin tool transform、LSP、background 等列为后续迁移；v1 与 v2 双轨。Tool output 外置路径没有稳定 artifact id/hash/provenance 合同；审批 Deferred 仅内存；v2 Plugin Hook 主要是配置/AI SDK transform，而非完整 Tool/Session 生命周期 Hook。

**对 Natives 的取舍。** 优先吸收 durable inbox/admission、coalesced wake、safe-boundary promotion、Context Epoch、stale identity 与 typed Tool registry。必须由 Natives Daemon 补足崩溃续跑、持久 approval、Artifact authority 与安全 Hook fail-closed。

## 4. 横向能力矩阵

| 项目 | 执行权威与 loop | Tool interface/runtime | Permission/security | Prompt/context/compact | Hook/extension | Session durability/recovery | Multi-agent | Event/observability/replay | 模块深度/可测试性 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| AtomCode | 强（目标/实现并存） | 强 | 中 | 强 | 强 | 中 | 弱-中 | 中 | 中（迁移层复杂） |
| Claude Code | 不可证实（黑盒） | 中-强（公开行为） | 强（产品行为）/内部不可审计 | 强（行为记录） | 强 | 强（行为记录）/事务不可审计 | 强（行为记录） | 中（内部不可见） | 不可证实 |
| DeepChat | 强 | 强 | 强 | 强 | 中 | 强 | 强 | 强 | 中（总协调层过宽） |
| Goose | 中-强 | 强 | 中（finding 强，失败策略弱） | 强 | 强 | 中 | 中-强 | 中-强 | 中 |
| Grok Build | 强（局部 fuse 完整、全局预算可空） | 强 | 中（优先级强，路径/失败策略弱） | 强（算法）/中（提交） | 弱-中 | 中-强（展示 replay 强、执行恢复中） | 中-强 | 强（展示）/中（事实） | 中（config/workspace/actor 大） |
| Kimi Code | 强（Goal）/中（普通 Run） | 强 | 中（链清楚，失败策略弱） | 强（算法）/中（提交） | 弱-中 | 中（恢复强，persist-first 弱） | 中-强 | 强（连接）/中（事实） | 中（协调器大且微模块多） |
| Kun | 强 | 强 | 强 | 强 | 强 | 强 | 强 | 强 | 中（composition root 大） |
| OpenCode | 强（v2 admission） | 强 | 强（执行时） | 强 | 中 | 中（admission 强，continuation 未完成） | 弱（v2）/中（legacy） | 强 | 中（Effect 链和双轨） |

### 4.1 矩阵解读

- **最可移植的事实边界**：DeepChat Tape、Kimi Wire、OpenCode durable inbox 都把“已记录事实”和“当前执行”区分开；Natives 应在现有 RunEvent/EventSequencer 上合并这种语义，而不是增加 transcript 数据源。
- **最可移植的 Tool 边界**：Grok typed streaming、AtomCode central fuse、Kun Tool Host 顺序互补：Tool 自己只描述输入输出和副作用，Gateway 负责 materialize、permission、progress、output budget、replay/unknown outcome。
- **最可移植的权限边界**：Kun 的实际执行顺序最完整；Kimi 的 ordered policy chain 和 Goose typed finding 适合做解释层。Natives 最终裁决仍只在 Capability Gateway。
- **最成熟但最不可审计**：Claude Code 的后台 Agent/worktree/MCP 产品边界；只能转化为 Natives 的验收清单，不能转化为实现事实。
- **共同风险**：八仓中至少 AtomCode、DeepChat、Goose、Grok、Kimi v1、Claude 示例存在 fail-open 或不可验证路径；安全 Hook 的失败策略必须单独核对，不能看生态数量。

## 5. Natives 可吸收清单

### P0：进入现有执行权威的合同

1. **Execution Fact + Projection**：让 Run admission、Prompt/Tool/Permission/Hook decision、Tool progress/result、continuation 与 terminal 都先进入 Daemon authoritative event，再生成 Renderer projection。参考 DeepChat Tape、Kimi Wire、OpenCode `session_input`。
2. **不可变 Run/Context 快照**：启动时固定 Prompt source mapping、Tool definition digest、Permission/Hook revision、Provider route 与 Context Epoch；配置变化只影响未来 Run。参考 DeepChat Run snapshot、OpenCode Context Epoch、AtomCode session freeze。
3. **Typed Tool contract**：统一 `Args` decode、structured output、model output、0..N progress、exactly-one terminal、cancellation 与 output budget。参考 Grok `xai-tool-runtime` 与 OpenCode Effect Schema。
4. **中央 Tool Gateway fuse**：统一并发 lane、重复调用/工具风暴、超时、重试、上下文溢出和最大输出；工具实现不得各自决定全局上限。参考 AtomCode central fuse、Kun dispatch policy、Kimi resource scheduler。
5. **安全 Hook fail-closed**：PreToolUse/Permission/egress/prompt-injection 等阻断性 Hook 的异常、超时、解析失败都必须阻止调用并产生结构化 finding；观察 Hook 可 warning，但必须在事件中明确 `observer`。参考 Kun Tool Host，拒绝 Goose/Grok/示例 Hook 的 fail-open。

### P1：提高恢复与可解释性

1. **Safe-boundary steering/interjection**：当前 provider turn 不透明重放；在下一个安全边界把 steer 提升为独立 synthetic/user event。参考 OpenCode promotion、Grok InterjectionBuffer。
2. **三层计数**：分离 logical round、request sequence、physical provider attempt，并将 retry/compaction/overflow 标注到事件。参考 DeepChat。
3. **Goal 生命周期**：active/paused/blocked/complete，恢复 active 自动降级，fork 不继承，budget 和结构化结束。参考 Kimi Goal。
4. **Structured RiskFinding**：`finding_id/action/resource/effect/reason/confidence/source/revision` 作为 Gateway 输入；Inspector 不拥有最终 Allow/Deny。参考 Goose `ToolInspectionManager`。
5. **ArtifactStore**：大输出、截图、日志使用 content hash id、大小/TTL/quota、原子写入、权限位和 provenance；模型只看到 stable artifact reference + bounded preview。参考 Kun，修正 OpenCode/DeepChat/Goose 的裸路径协议。
6. **Unknown side-effect journal**：工具开始前记录 operation identity；完成结果可重放，started/unknown 状态禁止透明重试。参考 Kun operation journal。

### P2：扩展与多 Agent

1. **Capability probe**：外部 Runtime/Provider/MCP 通过版本/握手报告支持矩阵；未知能力显示不可见，不能静态猜测。参考 Claude Code/Cindy 公开边界。
2. **Least-authority child snapshot**：子 Agent 权限为 parent ∩ task ∩ profile ∩ host，assignment immutable；子 Agent 只写独立 Session/Artifact，父 Agent通过 Lead-approved named handoff 接收。参考 Kun Graph、DeepChat subagent、Kimi fork 规则。
3. **可分发 Extension bundle**：命令、Agent、Skill、Hook、Tool provider、MCP 以声明单元发布，但生命周期注册/卸载仍归 Extension Host，不能直接改 Daemon 权威。参考 Claude Plugin 与 OpenCode Plugin Host。
4. **Event-driven supervision**：子 Agent/后台任务通过状态事件唤醒 supervisor，不用模型轮询普通 progress。参考 Kun Graph supervision、Claude `asyncRewake`。

## 6. 明确拒绝清单

- 不复制任何对标项目的第二套 Run/Session/Permission/Trace runtime。
- 不把 Goose Recipe 或 Kun Graph 直接升格为 Natives Harness Blueprint 权威。
- 不把临时文件路径当长期 Artifact identity；不把 raw effective prompt、secret 或敏感 Hook payload 写入 Renderer。
- 不采用 AtomCode/Grok/Goose/示例 Hook 的 security fail-open；`UserPromptSubmit` 观察/体验 Hook 的 fail-open 也不能用于 Tool Gate。
- 不用 shell 字符串启发式作为最终 sandbox/egress 裁决；最终仍需路径、Host、Capability Gateway 和受控执行器。
- 不复制 Kimi v2 的过度微模块化或 Kun 的超大 composition root；每个新增 seam 必须有清晰权威和独立不变量。
- 不把 Claude Code 的 CHANGELOG 行为写成内部源码事实，不把 OpenCode v2 TODO 写成已完成能力。
- 不在活动 Run 中热修改 Prompt、Tool、Permission、Hook 或 Provider；改动产生新 revision/新 Run snapshot。

## 7. 建议实施顺序

| 优先级 | 交付 | 验收重点 |
| --- | --- | --- |
| P0-A | Run admission + immutable Harness/Tool/Permission/Hook/Provider snapshot | 同一 Run 的预览、快照和 Provider 消费证据使用同一 revision/digest |
| P0-B | Typed Tool Gateway contract 与中央 output/concurrency/retry fuse | 每个调用都有 durable started/settled 事实；terminal 恰好一次；超限可回放 |
| P0-C | Security Hook fail-closed + typed RiskFinding | malformed/timeout/transport/unknown 都阻断并有 reason/source/confidence |
| P1-A | ArtifactStore + unknown side-effect journal | content-address、quota、TTL、provenance；unknown 不透明重试 |
| P1-B | Safe-point steering、三层 attempt 计数、compaction tool-pair invariants | 无 partial output 透明重放；steer 不打断 provider turn |
| P2-A | Least-authority subagent + named handoff | child 无共享上下文写权限；父级显式验收 packet |
| P2-B | Capability probe + Extension bundle | 外部 runtime 能力未知时不伪造；卸载不留注册泄漏 |

## 8. 对 Natives Harness 控制面的直接影响

- 执行地图应把 Session/Run 的事实链作为主对象，显示 admission、Context Epoch、Prompt source、Tool materialization、Permission finding、Hook decision、Tool settlement 和 continuation，而不是展示竞品名称。
- Tool 详情应区分“模型可见定义”“Gateway 最终可执行”“Hook/系统内部调用”，并显示 schema/digest/revision。
- 外部 Runtime 视图只做证据驱动 Inspector：来源文件、解析状态、runtime capability、Native 映射状态；不可观察就是不可观察。
- Artifact、RunEvent、Permission finding 和 Context snapshot 都经 Host/Daemon 边界进入 Renderer，Renderer 不直接打开 SQLite。
- 研究结论属于 `docs/harness/` 对标资料，不复制到 `NATIVE_ENGINE_FULL_REMEDIATION.md` 的进度表；只有确定的 Natives 决策才回灌现有 ADR/架构文档。

## 9. 残余风险与验证空白

- 本轮没有在统一模型、统一任务集和统一硬件上做基准测试，矩阵不是性能排名。
- Claude Code 内部安全与恢复行为不可源码审计；未来若接入其外部 runtime，必须做 capability probe 和黑盒回归。
- OpenCode v2 durable inbox 之后的 provider continuation recovery 尚未设计，不能作为完整 crash-recovery 方案。
- Natives 当前应用无法启动，未跑 UI、IPC、Daemon 端到端测试；本轮仅执行文档/源码证据校验。
- Kun 代码受 PolyForm Noncommercial 许可约束；可吸收概念，不可直接复制商业代码。
