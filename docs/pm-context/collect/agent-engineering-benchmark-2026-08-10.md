# Agent 工程对标原始材料（2026-08-10）

> 类型：源码、项目文档、Git 历史与当前对话的聚合记录。
> 处理原则：保留来源与事实边界，不把目标设计改写成已实现能力。
> 研究主题：Agent Harness、Tool Runtime、权限、安全、上下文、持久化、多 Agent、扩展与可观测性。

## 1. 四源扫描与版本

### 1.1 来源覆盖

| 来源 | 数量 | 结果 |
| --- | ---: | --- |
| 当前对话 | 1 | 用户给出 8 个本地仓库，要求研究 Agent Harness、Tool 等 Agent 工程能力并记录优缺点 |
| 本地项目 | 9 | 8 个对标仓库 + Natives 当前仓库 |
| URL | 0 | 用户未提供 URL；本轮不以网络资料替代固定本地版本 |
| 知识库 | 0 | 项目未配置、扫描中未发现本任务可用的独立知识库 |

### 1.2 固定版本

| 材料 ID | 项目 | 本地路径 | Git commit | 证据类型 |
| --- | --- | --- | --- | --- |
| M1 | AtomCode | `/Volumes/UNTITLED/本人材料/project/atomcode` | `4677ddfa68a84897a0154fe56af3e3e3b173410f` | 源码 + 项目设计文档 + Git 历史 |
| M2 | Claude Code | `/Volumes/UNTITLED/本人材料/project/claude-code` | `c39cb0f14bfe8bb519bae5bfc55add6867c5e2ab` | README/CHANGELOG + 插件与 Hook 示例；不含核心源码 |
| M3 | DeepChat | `/Volumes/UNTITLED/本人材料/project/deepchat` | `4461a7f9b76f46ac98753f88c09a821c6df7a6ac` | 源码 + 架构文档 + Git 历史 |
| M4 | Goose | `/Volumes/UNTITLED/本人材料/project/goose` | `021b0db8dbee8d6c7e9ffbab580a4143598a3560` | 源码 + 文档；与已有 Goose 调研基线相同 |
| M5 | Grok Build | `/Volumes/UNTITLED/本人材料/project/grok-build` | `c68e39f60462f28d9be5e683d9cbe2c57b1a5027` | monorepo 导出源码 + 用户指南；公开历史仅一个同步提交 |
| M6 | Kimi Code | `/Volumes/UNTITLED/本人材料/project/kimi-code` | `d1ded01b7c50c9847440f4645fe13f588becdc66` | v1/v2 源码 + 目标文档 + Git 历史 |
| M7 | Kun | `/Volumes/UNTITLED/本人材料/project/Kun` | `9db7d4f34f26bd2faba8900fb84eaf7ce29f7661` | 源码 + 设计文档 + Git 历史 |
| M8 | OpenCode | `/Volumes/UNTITLED/本人材料/project/opencode` | `b8bd88901a4870ef3a5752840f4e23e11d54e24e` | v1/v2 源码 + 文档 + Git 历史 |
| M9 | Natives | `/Users/ldh/Downloads/project/AiNative/Natives` | `5627e3e43cf7b5113bc8e17f8c813ed43a1a78a4` + 当前工作树 | 权威规范、引擎能力审计、Harness 控制面设计与既有调研 |

## 2. M1 AtomCode

### 2.1 来源索引

- `docs/target-architecture.md`
- `docs/security/permission-model.md`
- `docs/hook-architecture.md`
- `docs/compact-durable-checkpoint-design.md`
- `docs/v5.0.0-retire-bridge-core-progress.md`
- `crates/atomcode-kernel/src/agent.rs`
- `crates/atomcode-kernel/src/tool.rs`
- `crates/atomcode-kernel/src/hook.rs`
- `crates/atomcode-capabilities/src/session/context.rs`

### 2.2 原始事实

- 目标分层为 `protocol <- kernel <- capabilities <- L2 domain agent <- frontend`；Kernel 明确不拥有 approval、persona 与 code intelligence。
- Tool、Provider、Hook 是 Kernel 的公共扩展接缝。
- `LifecycleHooks` 覆盖 session、turn、request、delta、reasoning、tool、continuation 与 terminal 阶段，并区分永久/临时 Hook、注册顺序和短路语义。
- Tool 结果由中央路径实施 64 KiB 上限；Agent loop 还集中处理并发上限、重试、空响应、上下文溢出与 fuse。
- Session context 在启动时冻结，设计显式考虑 Prompt cache 稳定性。
- 文件工具与常见 shell 文件命令复用路径访问策略。
- `docs/v5.0.0-retire-bridge-core-progress.md` 记录 v1 core、v2 kernel/capabilities/coding 和 bridge 仍并存；`atomcode-core` 仍约 76k LOC。
- shell 权限以启发式分析命令，无法证明解释器内部访问路径。
- Claude-compatible command Hook 的配置或执行异常存在 fail-open 行为。
- 工具调用边界仍以 JSON 字符串参数进入，类型化主要发生在实现内部。

### 2.3 关联

- 与 M5 相同：Rust Kernel/Tool deep module、中央 continuation/fuse。
- 与 M6/M8 相同：正处于新旧引擎迁移，目标架构与当前生产路径必须分开评价。
- 与 M9 相关：Hook 阶段合同、中央输出限制和中立 Kernel 可用于校验 Natives 的 Agent Core / Capability Gateway 职责。

## 3. M2 Claude Code

### 3.1 来源索引

- `README.md`
- `CHANGELOG.md`（当前版本记录 `2.1.211`）
- `plugins/README.md`
- `plugins/plugin-dev/skills/`
- `plugins/security-guidance/hooks/hooks.json`
- `plugins/hookify/hooks/pretooluse.py`

### 3.2 原始事实

- 本地仓库未包含核心 Agent loop、权限引擎或 Session runtime 源码，不能作为核心源码审计。
- CHANGELOG 当前顶部版本为 `2.1.211`。Subagent 默认后台运行，父 Agent 继续工作并在完成时收到通知；长命令/工作流可跨进程停止、重启和升级存活，daemon restart 后 worker 可恢复。
- 后台回复递送失败时先保存，session restart 后再递送；产品历史也修复过 blank cold reopen、killed Agent respawn、revived Agent 重跑 stale prompt、wrong completion status 和 version-skew worker。
- background subagent permission prompt 回传主会话；Agent 消息不视为用户批准。foreground/background 都有 5 层深度上限，resume/fork 恢复或计入原 depth。
- Agent 定义已演进到 `tools`、`disallowedTools`、`permissionMode`、`mcpServers`、`effort`、`maxTurns`、`isolation: worktree` 和 agent-scoped hooks；plugin-dev Agent 文档仍只覆盖较旧字段集。
- 当前版本保证 managed deny 高于 PreToolUse `allow`/`ask` 和 PermissionRequest `updatedInput`；unsandboxed Bash 的 Hook `ask` 在 auto mode 中至少保持 prompt。catastrophic removal 即使在 bypass/auto mode 中仍提示。
- permission preview 会中和 bidi override、zero-width 和 look-alike quotes；“always allow” 保存到 repo root，以跨 worktree/session 复用。
- 公开 sandbox 能力以 BashTool 为中心；严格示例默认禁止 unsandboxed command、Unix socket、本地监听和未列域名，不能推断所有 Tool 都在同一 OS sandbox 中。
- `isolation: worktree` 是声明式 Agent 能力，并配套 WorktreeCreate/Remove Hook；Git mutation 落到主仓、stale worktree、cwd 恢复、shared `.git` allowlist、symlink escape、lock cleanup 和 unpushed commit 删除均有历史修复。
- 大 Tool result 与 Hook output 超过 50K characters 后外置到磁盘并返回 path + preview；MCP `_meta["anthropic/maxResultSizeChars"]` 可调整到最高 500K。公开材料没有 stable artifact id/hash/TTL/ACL/provenance 合同。
- MCP tool description/server instructions 有 2KB cap；描述超过上下文 10% 时默认 deferred，通过 Tool Search 按需发现。MCP reconnect、OAuth、dynamic tool list、tool-pool cache、request timeout 和 stderr 64MB 上限都有演进记录。
- Prompt cache 修复覆盖动态日期移出 system prompt、tool schema bytes 稳定、MCP late instructions、resume full miss 和 tool-pool assembly；auto compaction 连续失败 3 次 circuit-break。
- parallel tool calls 独立结算；resume、stream sibling failure 和 compaction 多次修复 orphan/missing `tool_use/tool_result` pairing。
- Plugin 分发单元可组合 commands、agents、skills、hooks 与 MCP；manifest 限制相对路径，marketplace 有 managed host/path policy，`${CLAUDE_PLUGIN_DATA}` 保存跨升级状态。
- Hook 事件覆盖 Tool、Permission、Session、Compact、Subagent、Worktree、Config、Instructions 和多 Agent 生命周期；matching Hook 并行运行。`asyncRewake` 可在后台 Hook 完成后重新唤醒会话。
- security-guidance 展示 Git baseline、touched-path snapshot-and-clear、transient restore、diff cap、Stop fire cap、rate limit、investigate/self-refute 和 reviewed SHA dedup；它是补充审查器，多种故障仍 exit 0。
- `hookify` PreToolUse 示例在 import/运行异常时 allow 且 finally exit 0，只代表示例插件，不能据此推断内核默认；Natives security-critical Hook 不可复制。
- OTel/stream-json/Hook 提供 `agent_id`、`parent_agent_id`、`tool_use_id` 等关联字段，并对敏感内容 opt-in；它们不是可证实的 durable replay authority。
- 仓库只有约 216 个 tracked 文件、没有核心实现或 tracked tests，且使用 Anthropic proprietary license；模块深度和核心测试面不可评价。
- 所有行为结论依赖版本；本地材料无法验证内部权限排序、事务一致性和模块边界。

### 3.3 关联

- 与 M4/M7 相同：拥有较丰富 Hook/插件生态。
- 与 M7 相同：worktree/子 Agent 隔离和恢复被视为产品级工程问题。
- 与 M3/M6/M8 相同：需要把 durable execution fact、恢复 checkpoint 和可丢弃 projection 分开，但 Claude 仓不能证明内部如何实现。
- 与 M5/M7 相同：Tool output 应外置；Claude 的裸 path 行为需由 Natives ArtifactStore 的 id/hash/provenance 补足。
- 与 M9 相关：外部 Runtime Bridge 应进行 capability probe，并把不可见内部状态标为不可证实。

## 4. M3 DeepChat

### 4.1 来源索引

- `docs/architecture/agent-system.md`
- `docs/architecture/tool-system.md`
- `docs/architecture/tape-system.md`
- `src/main/agent/deepchat/loop/deepChatLoopEngine.ts`
- `src/main/agent/deepchat/loop/ports.ts`
- `src/main/agent/deepchat/harness/deepChatAgentHarness.ts`
- `src/main/agent/deepchat/harness/createDeepChatAgentHarness.ts`
- `src/main/agent/deepchat/runtime/toolExecutionPolicy.ts`
- `src/main/agent/deepchat/runtime/toolPermissionReviewer.ts`
- `src/main/agent/deepchat/runtime/toolOutputGuard.ts`
- `src/main/agent/deepchat/runtime/runtimeHookSink.ts`
- `src/main/session/data/pendingInputs.ts`
- `src/main/session/data/tape.ts`
- `src/main/agent/invocationAdmission.ts`
- `src/main/orchestration/liveDelegationRepository.ts`
- `src/main/orchestration/liveDelegationService.ts`
- `src/main/tool/agentTools/liveDelegationTool.ts`
- `docs/architecture/proactive-multi-agent-orchestration/spec.md`

### 4.2 原始事实

- Session 是长期权威，Run 是一次闭合执行快照；Tape 保存 append-only 执行事实，UI transcript 是投影。
- DeepChat loop 与 Direct ACP 分属两套 backend；ACP 路径拥有自己的 process/protocol runtime。
- `DeepChatLoopEngine` 只编排 logical round、tool settlement、128 次工具调用上限与 terminal settlement。
- 计数模型区分 logical round、request sequence 与 physical attempt；透明重试和上下文恢复不会混同为新逻辑轮次。
- Tape 保存 ViewManifest、可回放事实和 subagent lineage。
- Tool mapping 按 Session 冻结；MCP、Skill、Plugin ownership 有独立来源。
- 只有 full access 且整批工具都显式声明 read + parallel 时才并发执行。
- LLM 自动审批使用 action hash；非法/缺失 JSON 或 hash mismatch 退回 ask user，critical 级别强制 block。
- `ToolPermissionBroker` 还把批准绑定到 server/config generation、binding hash、tool、execution、arguments hash、source 与 effect；执行前重新解析 MCP/Subagent authority。
- Queue 与 Steer 使用 durable pending store、claim/disposition fence 和 restart reconciliation；Steer claim 同时绑定可见 user message 与 assistant continuation。
- compaction summary 与 Tape anchor 在同一 SQLite transaction 中 CAS；stale summary 不能覆盖新 anchor。
- 当前唯一 model-facing Subagent 工具是 `deepchat_subagents`；旧 `subagent_orchestrator` 执行器及测试已在 `710af0d5a` 删除，只保留历史 transcript 兼容。
- child 是持久 Session，一个 delegation 可跨多个 follow-up turn；操作包括 spawn/send/follow_up/list/inspect/read_result/wait/interrupt。
- explicit spawn/follow-up 使用 execution-bound one-shot receipt；proactive 是 Session standing authorization，但不提升文件、Shell 或 MCP 权限。
- 每父 Session active child 上限为 5；全局 Agent invocation admission 默认容量 6、pending 256，按 owner round-robin；permission/question waiting 会 suspend lease。
- 父只接收 bounded handoff、result hash/resultRef 与 frozen child Tape head；不承诺外部副作用 exactly-once，也没有 worktree 写隔离或 child 总 deadline。
- 大工具输出在 5k 后外置，并按上下文预算生成模型可见预览。
- 顶层 `DeepChatAgentHarness` 方法面很宽，多数方法转发到内部服务。
- `contextCoordinator`、`contextBuilder`、dispatch、turnCoordinator、liveDelegationService 等协调文件达到约 960–2,327 行，行为知识分散在 dependency bag 与 coordinator。
- Runtime Hook sink 主要用于观察，dispatch 异常 fail-open。
- Hook 继承完整 `process.env`；timeout 终止 shell，但无法保证 grandchildren 全部退出。
- 大输出协议暴露本地文件路径，未形成稳定 artifact id、hash、TTL 与 provenance 合同。
- DeepChat loop 与 Direct ACP 仍是两套能力不对称的 backend；部分 Tape/attempt telemetry append 失败不会阻断生成。

### 4.3 关联

- 与 M6/M8 相同：持久事实与 UI projection 分离。
- 与 M8 相同：输入/事件先持久化，再由执行器消费；M3 的 Tape replay 与 lineage 更完整。
- 与 M9 相关：Tape 的 execution facts、ViewManifest、投影、回放和 lineage 应合并进 Natives 现有 `RunEvent` 与 Session/Run 权威，不能另建 Tape DB。
- 与 M9 相关：durable Queue/Steer、exact-action approval 与 live delegation 分别映射到现有 `prompt_queue_store`、`PermissionGatedTools`、`child_run_orchestrator/subagent_store`。

## 5. M4 Goose

### 5.1 来源索引

- 本地 Goose 固定版本。
- Natives 既有报告：`docs/harness/cindy-goose-harness-research-2026-07-29.md`。
- 本轮独立报告：`docs/harness/projects/goose-agent-engineering-review-2026-08-10.md`。
- 既有报告所用 Goose commit 与本轮完全相同；本轮重新抽查固定提交源码并补充独立证据行号。

### 5.2 原始事实

- `ToolInspectionManager` 将检查结果归一为 Allow、Deny、RequireApproval，并携带 reason、confidence 与 finding id。
- 检查内容包括命令/路径风险、egress 与工具结果中的潜在 Prompt injection。
- 检查器默认顺序是 Security、Egress、Adversary、Permission、Repetition；Inspector 异常会记录后继续，Adversary Provider 错误明确返回 Allow。
- Egress 会提取并记录目标，但当前 finding 的 action 仍是 Allow，不能视为 egress gate。
- 上下文管理默认在 0/10/20/50/100% 级别移除中部 Tool response，工具 pair 每批最多 10 个并保护当前 turn；结构化摘要失败保留 raw output。
- Recipe 将指令、扩展、参数、JSON response schema、success checks、on-failure shell、Provider 配置和 retry 打包为复用入口；file 参数禁止 default。
- Goose 有 Hook、ACP、MCP、Extension 与独立 Subagent；Subagent child 使用 `SessionType::SubAgent`，但当前强制 `GooseMode::Auto`，后台任务与结果等待主要在内存表，300 秒只是一次 load 等待窗口。
- Hook 支持 Pre/Post Tool、Session、Prompt、文件、Shell、Stop；blocking Hook 的 spawn、timeout、序列化和普通非零退出按 Allow 处理，安全关键路径存在 fail-open。
- MCP/Developer 大输出会外置后返回临时文件路径；Developer shell 默认 300 秒，只终止直接 child，未建立进程组。
- ExtensionManager 为普通工具生成 `extension__tool` 名称，维护 owner metadata 与 versioned tool cache；ACP external dispatch 标记会保留历史但跳过本地 Inspector/dispatch。
- Recipe 同时承载工作流与运行配置，若照搬会与 Natives Harness/Capability 权威重叠。

### 5.3 关联

- 与 M9 相关：可吸收 typed risk finding、稳定 tool ownership、versioned cache 和渐进式 tool-pair compaction；最终 Allow/Deny 仍必须由 Natives Gateway 决定。
- Recipe 的归属更接近 Natives RunTemplate/Capability Hub，而不是 Harness Blueprint；ACP external dispatch 只能作为诚实的 External Runtime Bridge 事件。
- 与 M1/M3/M8 相同：Goose 的进程内 steer、AgentEvent 和 background task 表不能替代 Natives durable RunEvent、prompt queue 和 child budget。

## 6. M5 Grok Build

### 6.1 来源索引

- `README.md`
- `crates/codegen/xai-grok-agent/src/agent.rs`
- `crates/codegen/xai-grok-agent/src/builder.rs`
- `crates/codegen/xai-grok-agent/src/prompt/context.rs`
- `crates/common/xai-tool-runtime/src/tool.rs`
- `crates/common/xai-tool-runtime/src/dispatch.rs`
- `crates/common/xai-tool-runtime/src/context.rs`
- `crates/common/xai-tool-protocol/src/turn_hook.rs`
- `crates/common/xai-interjection-core/src/buffer.rs`
- `crates/common/xai-grok-compaction/src/lib.rs`
- `crates/codegen/xai-grok-shell/src/session/acp_session_impl/turn.rs`
- `crates/codegen/xai-grok-shell/src/session/acp_session_impl/tool_calls.rs`
- `crates/codegen/xai-grok-shell/src/session/acp_session_impl/updates.rs`
- `crates/codegen/xai-grok-shell/src/session/compaction.rs`
- `crates/codegen/xai-grok-shell/src/session/storage/mod.rs`
- `crates/codegen/xai-grok-shell/src/agent/subagent/`
- `crates/codegen/xai-grok-hooks/src/dispatcher.rs`
- `crates/codegen/xai-grok-sandbox/src/`
- `crates/codegen/xai-file-utils/src/events/`
- 用户指南第 10、16、17、18、22 章。
- 独立报告：`docs/harness/projects/grok-build-agent-engineering-review-2026-08-10.md`。

### 6.2 原始事实

- `xai-tool-runtime` 的 Tool 使用 typed Args，统一返回 streaming 执行：0..N progress + exactly one terminal。
- blocking 工具可以通过适配器进入相同 Tool contract。
- `ToolDispatch` 是 object-safe 接缝；`call_terminal` 会把缺失 terminal 转成明确错误。
- `TypedToolOutput` 同时保留结构化 JSON、模型内容块和可选 completion output。
- `TypedExtensions` 向 ToolContext 注入 cwd、behavior version、trace、session、cancellation 等按需能力。
- Computer Hub hard cancel 会取消 token 并丢弃执行 Future；terminal 使用 Unix process group/Windows Job Object 清理进程树。
- Tool batch 的 parse、Hook、permission 串行，approved 调用进入 `FuturesUnordered`；完成项立即更新 UI 并写 ChatState，因此 ToolResult 按完成顺序入史。
- 同批相同路径使用 mutex，兼容 `file_path/path/target_file`；key 未 canonicalize，锁不跨 batch/turn/Session。
- managed deny 高于 YOLO、Auto、sandbox auto-allow 和 remembered grant；permission channel 断开 Reject，plan-mode edit gate 不被 YOLO 绕过。
- Tool/MCP auth-shaped failure可恢复后重新 dispatch，但没有 side-effect operation journal。
- `InterjectionBuffer` 定义 FIFO、safe drain，并将每个插入项转换成独立 synthetic user message。
- `max_turns` 可为空；结构化输出、TodoGate、401、Sampler 和 Doom-loop 都有局部 fuse，但没有统一 wall-clock/tool/token 总预算。
- Doom-loop recovery 默认关闭；启用时只对 thinking tail repetition 做有限重采样，默认 2 次。
- 两阶段 compaction 按 prefix fingerprint、model、prefix length 校验 cache，sanitize 并验证孤立 ToolResult，失败回退最小历史。
- compaction checkpoint、marker 和 chat replacement 只通过同一 persistence FIFO 排序，不在同一事务；错误只 warning，内存与磁盘可能分叉。
- `PromptContext`、`system_prompt.txt` 和 chat System 都会持久化；生产恢复不读取 `prompt_context.json`，恢复又会覆盖它，没有 effective prompt hash。
- command/client Hook fail-open；HTTP Hook 只校验初始 HTTPS URL/DNS/IP，默认 redirect 未重复 SSRF 校验，body 与命令输出在完整读取后才截断。
- 默认 sandbox profile 为 `off`；built-in profile 应用失败后无沙箱继续，custom profile 才 fail-closed。Linux 有 Landlock/bwrap/seccomp，macOS child network 限制为空操作，Windows 无对应 OS sandbox。
- `updates.jsonl` 有 `eventId`、cursor delta replay、rewind filtering、ToolCall folding 和 unfinished child reconciliation；`events.jsonl` 是独立诊断轨。二者都不是事务性 operation log。
- child 有独立 Session、lineage、terminal/meta 和 completed-child resume provenance；遗留 running child 在恢复时归为 cancelled，不续跑。
- child 最大递归深度 1；继承父 PermissionHandle，未知 `ToolKind` 的 MCP/custom tool 会保留；600 秒是前台等待预算，不是 deadline。
- worktree 创建失败会退回共享 workspace；`meta.json` 直接写，无 temp+rename/fsync。
- 当前仓库是 monorepo 导出快照，公开 Git 历史只有一个同步提交。
- 固定树约 2,237 个 Rust 文件，约 1,466 个文件含 test attribute，约 25,439 处匹配；测试面极广。
- 主要热点包括 agent config 约 11,284 行、workspace handle 约 9,486 行、permission manager 约 5,633 行、compaction 约 3,321 行、tool calls 约 3,015 行、subagent 约 2,827 行。
- 并发测试中仍有旧 `join_all`/输入顺序注释，和当前增量 completion-order 实现漂移。

### 6.3 关联

- 与 M1 相同：Rust typed Tool deep module 和中央 loop；也同样存在 Hook fail-open 与协调器膨胀风险。
- 与 M3/M4 相同：compaction 对 tool-call/tool-result 配对有显式保护；Grok 的算法强，但提交原子性弱于 DeepChat Tape CAS/Goose SQLite replace。
- 与 M3/M6/M8 相同：展示 projection 和 durable execution fact 必须分开；Grok 的 cursor replay 不能替代 operation journal。
- 与 M7 相反：Grok Tool/MCP 401 可透明重试且缺 unknown-outcome journal，Kun 对 started/unknown 禁止重试。
- 与 M9 相关：typed streaming Tool、TypedExtensions、safe-point interjection、two-pass sanitation、cursor projection、child lineage 和进程树清理可独立吸收；必须改造为 canonical resource lease、事务 Context commit、fail-closed Hook/sandbox 和副作用 journal，不复制大 SessionActor/多文件权威。

## 7. M6 Kimi Code

### 7.1 来源索引

- `README.md`、`apps/kimi-code/README.md`
- `apps/kimi-code/src/cli/experimental-v2.ts`
- `packages/agent-core-v2/AGENTS.md`
- `packages/agent-core-v2/src/agent/loop/loopService.ts`
- `packages/agent-core-v2/src/agent/toolExecutor/toolExecutorService.ts`
- `packages/agent-core-v2/src/agent/toolExecutor/toolScheduler.ts`
- `packages/agent-core-v2/src/agent/toolRegistry/toolRegistryService.ts`
- `packages/agent-core-v2/src/agent/permissionPolicy/permissionPolicyService.ts`
- `packages/agent-core-v2/src/agent/toolApproval/toolApprovalService.ts`
- `packages/agent-core-v2/src/agent/externalHooks/`
- `packages/agent-core-v2/src/agent/contextMemory/`
- `packages/agent-core-v2/src/agent/fullCompaction/`
- `packages/agent-core-v2/src/agent/goal/goalService.ts`
- `packages/agent-core-v2/src/agent/tools/agent/agentTool.ts`
- `packages/agent-core-v2/src/session/swarm/agentRunBatch.ts`
- `packages/agent-core-v2/src/agent/task/taskService.ts`
- `packages/agent-core-v2/src/wire/wireService.ts`
- `packages/agent-core-v2/src/persistence/backends/node-fs/appendLogStore.ts`
- `packages/agent-core-v2/src/workspace/sessionLifecycle/sessionLifecycleService.ts`
- `packages/kap-server/src/transport/ws/v1/`
- 独立报告：`docs/harness/projects/kimi-code-agent-engineering-review-2026-08-10.md`。

### 7.2 原始事实

- CLI、headless prompt 和 doctor 默认进入 v2，truthy legacy 开关才走 v1；`kimi web` 也直接进入 v2 kap-server。
- 每 Agent Wire 统一 reducer、journal、migration、rewrite 与 rehydrate；duplicate Op 注册 fail-fast，replay 跳过未知/损坏记录并报告。
- Wire `dispatch()` 先更新内存 model，再异步 append；失败不回滚。普通 session close/archive 没有显式 await `wire.flush()`，append log release 的 retirement flush 也不由调用方等待；fork 复制活跃 Agent 前会显式 flush。
- Context live/replay 共用 fold；恢复补 interrupted tool result，deferred message 保持 call/result 邻接。
- ToolExecutor 集中执行 preflight、resolve、PreToolUse veto、permission、will/started、scheduler、execute、post hook、truncate 和 result。
- ToolScheduler 依据 read/write/all 冲突允许非冲突并发，并保证 queued-before fairness；调用方按完成顺序 post-process 和入史。
- 内建文件工具复用同一个 lexical normalized path 进行 access/approval/execute，但未证明 symlink/realpath/inode identity；Bash 无 access 声明，默认为 `all`。
- Tool registry 同名注册静默替换；没有 definition revision/digest 或 stale-call binding。每次 LLM request 读取 live tools，同 turn 只冻结 model params/system prompt。
- 纯文本 Tool 结果超过 50,000 字符会原子外置到 Agent scope，模型看 2,000 字符 preview + 绝对路径；保存失败回退原大结果。
- Permission 是有序 policy chain并记录理由；approval broker 缺失时 auto-approve，且有专项测试。`[permission]` schema 存在，但固定树未找到生产 `rulesService.addRules` 调用。
- 系统 prompt 明示运行环境不在 sandbox；Bash 直接作用于宿主系统，主要依赖 permission 与工具路径检查。
- v2 Hook 覆盖 Tool、Prompt、Stop、Compact、Permission、Session、Subagent、Task、Heartbeat；PreToolUse 只阻断不改参数。spawn/timeout/abort/普通非零/malformed JSON 多数 fail-open，继承完整环境，stdout/stderr 无读取级上限。
- Prompt/Wire 保存 rendered prompt、AGENTS 路径、tool allow/deny、subagent allowlist 与 render generation；每次 request 计算 systemPrompt/tools SHA-256 并保存首次 tool schema snapshot。
- compaction 使用 0.7/0.5/0.35 递减，保护 tool pair，提交前校验 history prefix 只允许追加真实 user input；apply、post injection 与 complete 是独立 dispatch，不是事务。
- Goal 支持 active/paused/blocked/complete、turn/token/wall-clock budget、75% 收敛提醒、一次无工具 grace；恢复 active 降 paused，fork 清除。普通 loop `maxStepsPerTurn=0` 表示无上限，未证明普通 Run 具有统一总预算。
- child 是独立 Agent/Wire/Context/model/profile；resume 要求 parent-owned + idle。新 child 复制父 permission mode 并继承 user tools，未证明 parent∩task∩profile∩host 权限收缩；递归深度与 subtree 总预算未见统一约束。
- Swarm 首批启动 5 个、之后每 700ms 一个，并有 rate-limit capacity/backoff；max concurrency 仅在可选环境变量设置后生效。
- Task 有可选 maxRunningTasks、16 MiB process output hard cap、1 MiB ring、atomic JSON + append output、Wire started/terminated 和 lost reconciliation；persist/output queue 的错误可被空 catch 吞掉。
- ActivityView 从进程内 EventBus fold，并只从 cold state/Wire last turn seed；不是 pending approval/tool/stream 的完整 durable projection。
- kap-server JSONL 用 `{seq,epoch}` cursor reconnect/resync，delta/progress/shell/status 为 volatile；durable event 仍先分 seq、入 tail/fan-out，再异步 append。写失败事件成为 live-only，重启后可能复用 seq。
- MCP reconnect 和 Tool abort 没有通用 side-effect operation identity/unknown journal；`lost` 或 interrupted synthetic result 不能证明副作用未发生。
- 主要协调热点为 task 1,457 行、goal 1,328 行、loop 1,219 行、profile 1,022 行、tool executor 984 行、compaction 901 行、LLM requester 848 行。

### 7.3 关联

- 与 M3/M5/M8 相同：execution fact 与 disposable/client projection 必须分开；Kimi Wire/kap 的 write-behind 弱于 Natives 现有 EventSequencer persist-first，不能另建事件轨。
- 与 M1/M5 相同：资源冲突 scheduler 值得吸收；Kimi 的 lexical key 还需升级为 Capability Gateway 的 canonical identity 与跨 child Run lease。
- 与 M5 相同：compaction 算法保护 stale prefix/tool pair，但提交不是事务；应落到 Natives 现有 ContextSnapshot + RunEvent CAS。
- 与 M7 对照：Kimi 缺通用 unknown-side-effect journal；Natives 应保持 existing uncertain/recovery-blocked，禁止透明重试 write/external effect。
- 与 M3/M7 相同：child identity、lineage、独立 context 可吸收；权限必须由现有 child Run 做 parent∩task∩profile∩host 收缩，Provider/Credential 继续使用现有 route/lease。
- Goal 生命周期、预算 grace、Swarm rate-limit adaptation、ActivityView 和 `{seq,epoch}` resync 可分别合入 RunManager/AgentEngine、child admission、RunEvent projection 与 `run.watch`；不复制 Wire、kap journal、Goal runner 或 Task ledger。

## 8. M7 Kun

### 8.1 来源索引

- `README.en.md`
- `DESIGN.md`
- `docs/graph-mode.en.md`
- `kun/src/loop/`
- `kun/src/hooks/hook-engine.ts`
- `kun/src/adapters/tool/local-tool-host.ts`
- `kun/src/adapters/tool/sandbox-policy.ts`
- `kun/src/loop/tool-execution-service.ts`
- `kun/src/loop/tool-dispatch-policy.ts`
- `kun/src/artifacts/artifact-store.ts`
- `kun/src/server/runtime-factory.ts`
- `kun/src/delegation/`

### 8.2 原始事实

- GUI/TUI 共用 `kun serve` runtime，Renderer/Main 不实现 Agent loop；HTTP/SSE 是统一外部边界。
- immutable prefix hash 在每个 step 校验漂移，并使用有界 TTL/LRU 缓存。
- Graph Mode 不是第二套 Agent runtime；它额外定义 plan/run/node/attempt/edge/resource ledger、CAS revision 与 bounded loop。
- Worker 权限为 parent、graph、profile、node 与 host 权限的交集，且 Worker 不能递归委派或控制 Graph。
- Graph 使用 immutable least-authority assignment snapshot；Lead 显式验收后才发布命名 data handoff，child 输出按不可信证据处理。
- `ArtifactStore` 使用 content hash id、quota、原子 temp-write + rename、0600/0700、dedupe 与 summary。
- Agent loop 拆出 model round、tool execution、context compaction、history hygiene、tool storm breaker、turn budget 与 steering。
- `LocalToolHost` 的执行顺序为：resolve/静态 sandbox → PreToolUse → plan/read-before-edit/runtime policy → 外部写目标解析 → approval → operation journal → execute → PostToolUse → 大输出外置。
- PreToolUse 改写后的参数会继续经过后续 policy/approval；Hook auto-allow 不能跳过外部路径写、workspace shell 和显式外部副作用审批。
- Pre/Post Tool Hook 崩溃或超时返回 `hook_failed`；不会继续执行或把未审查结果交给模型。
- `UserPromptSubmit` Hook 崩溃 fail-open；TurnStart/TurnEnd/PreCompact 为观察型，错误转 warning。
- workspace-write 的 host shell 不是真正路径隔离，因此只开放内建 shell 且强制逐命令审批；窄 delegated path scope 下直接禁用 shell。
- 外部文件审批绑定物理路径、device、inode、parent device/inode，并在批准前后重新解析 symlink。
- operation journal 对 completed 结果重放；started 或 unknown side effect 不自动重试。
- 只并发 built-in read-only 工具（默认最多 3）和独立 delegation lane；会触发审批的策略全部串行。
- `runtime-factory.ts` 约 3,635 行，`delegation-runtime.ts` 约 1,629 行，`agent-loop.ts` 约 1,236 行；装配与 Graph 状态空间很大。
- Graph 默认保留 7 天、node 可运行 24 小时，恢复、清理与监督成本高。
- 项目使用 PolyForm Noncommercial 许可，只能借鉴构思，不能直接用于商业集成。

### 8.3 关联

- 与 M3/M6 相同：多 Agent 使用独立生命周期、投影/ledger 和权限收缩。
- 与 M5 相同：safe-point steering/interjection 与中央 Tool contract。
- 与 M9 相关：least-authority assignment、Lead-approved data packet、ArtifactStore、unknown outcome 不重试与 event-driven supervision 可作为独立机制评估。

## 9. M8 OpenCode

### 9.1 来源索引

- 根 `AGENTS.md`
- `packages/core/src/session.ts`
- `packages/core/src/session/input.ts`
- `packages/core/src/session/run-coordinator.ts`
- `packages/core/src/session/execution.ts`
- `packages/core/src/session/execution/local.ts`
- `packages/core/src/session/runner/llm.ts`
- `packages/core/src/session/context-epoch.ts`
- `packages/core/src/session/history.ts`
- `packages/core/src/session/projector.ts`
- `packages/core/src/session/compaction.ts`
- `packages/core/src/system-context/`
- `packages/core/src/tool/tool.ts`
- `packages/core/src/tool/registry.ts`
- `packages/core/src/tool-output-store.ts`
- `packages/core/src/permission.ts`
- `packages/core/src/plugin/host.ts`
- `packages/core/src/aisdk.ts`

### 9.2 原始事实

- `SessionV2.prompt` 先发布 durable admission event 并投影到 `session_input`，完成等价性/幂等冲突检查后才调用 `execution.wake`。
- `wake` 是可合并的进程内提示；`SessionRunCoordinator` 按 Session ID 串行，同一执行可 join，不同 Session 可并发。
- steer 只在 provider turn 边界按 admission cutoff 提升；queue 在当前工作即将 idle 时逐条提升，同时吸收截止点内 steer。
- 每个 provider turn 只调用一次 `llm.stream(request)`；本地工具调用先持久记录，再 eager 启动，全部 settlement 完成后才 continuation。
- runner 源码明确写出 durable continuation recovery、durable busy/retry/idle 状态、集群 ownership 和 bounded provider retry 仍是未来工作。
- Context Epoch 保存模型可见 baseline 和 typed source snapshot；普通 source 变化产生 durable system update，compaction 后才允许不兼容 baseline replacement。
- System Context 使用 namespaced typed source、codec、compare、baseline/update/removed algebra；source 暂时 unavailable 时保留已接纳 snapshot。
- Tool 使用 Effect Schema 校验 typed input/output，并为 model 输出与 structured output 分别编码。
- Tool materialization 冻结注册 identity；运行期注册漂移会返回 stale tool call，不执行新实现。
- Registry 只在某 action 被 `resource: *` 最终 deny 时隐藏定义；每个具体工具仍在执行函数中调用 Permission Service。
- Permission 使用 last matching ordered rule；默认无 agent 权限时全 deny，未匹配默认为 ask。
- 配置 deny 在 saved allow 前单独检查，持久“always allow”不能覆盖 agent 明确 deny。
- 待审批 Deferred 只在内存中；runtime scope 结束时统一 Decline，尚不是 durable approval continuation。
- Tool output 中央限制默认 2,000 行/50 KiB，外置文件保留 7 天并定时清理；协议暴露本地路径且未 content-address。
- compaction 保存旧 summary + 最近原文，工具结果摘要最多保留 2,000 字符；支持预算触发与首次 overflow 恢复。
- v2 Plugin Host 当前主要提供 agent/catalog/command/integration/reference/skill transform 与 AI SDK 初始化 Hook；没有完整 Tool/Session lifecycle Hook 面。
- AI SDK Hook 顺序执行且错误向上传播；未发现 catch 后继续的 fail-open 路径。
- v2 built-in tool 列表明确把 task、MCP/plugin tool transform、LSP、background 等列为后续迁移；subagent agent metadata 已存在，但 v2 Task Tool 未实现。
- `packages/core/src/v1/` 仍保留旧引擎；v2 与 legacy 双轨，能力不等价。

### 9.3 关联

- 与 M3/M6 相同：durable facts + projection；M8 把输入 admission 与执行唤醒拆得最清楚。
- 与 M1/M5 相同：Tool registry/typed contract 是 deep module。
- 与 M9 相关：durable inbox、coalesced wake、safe-boundary promotion、Context Epoch 和 stale tool identity 可独立吸收；崩溃续跑与 v2 迁移缺口必须保留为限制。

## 10. M9 Natives 权威对照材料

### 10.1 来源索引

- `docs/standards/technical/01-layering.md`
- `docs/standards/technical/02-security.md`
- `docs/standards/technical/05-backend.md`
- `docs/architecture/NATIVE_ENGINE_FULL_REMEDIATION.md`
- `docs/architecture/NATIVE-DAEMON-CAPABILITY-MAP.md`
- `docs/architecture/EXECUTION-ENGINE-CAPABILITY-AUDIT.md`
- `docs/superpowers/specs/2026-07-26-native-harness-control-plane-design.md`
- `docs/harness/cindy-goose-harness-research-2026-07-29.md`

### 10.2 对照边界

- Natives 的生产执行权威固定为 Renderer → Tauri Host → UDS → Agent Daemon。
- Run、Provider、Capability execution、RunEvent 与协议权威位于 Daemon/共享 Rust workspace；对标结论不能引入第二套执行或权限权威。
- Capability Gateway、Permission Manager、Plan Mode、安全 Hook 与不可变 Run Snapshot 是已存在基础，不应被竞品同名模块替换。
- Harness 控制面负责解释和配置未来 Run，不拥有活动 Run 热修改。
- Raw effective prompt、凭证、secret 与敏感 Hook 输入输出不得持久化或进入 Renderer。
- `PromptPlanBuilder` 已产出 `CompiledPromptPlan`，Provider 使用同一 `effective_full_text`，snapshot 保存 `effective_prompt_hash`；不再把“Prompt 单一事实源”列为缺口。
- model-backed compaction、`ContextSnapshotCommitted` 与 GenerationAttempt started/failed/discarded/committed 已存在；对标项目只能补强 pair、CAS 或 manifest 细节，不能新建第二套 Context/Tape。
- Tool grant 已结构化持久化；RunEvent persist-first，Tool terminal 与 side-effect ledger 同事务；外部副作用仍需按 unknown outcome 保守处理。
- ArtifactStore 已有 run-scoped ID、SHA-256、25 MiB 上限、atomic write/fsync/rename；竞品裸路径协议只能作为反例。
- Subagent 已有 task/token/tool/depth/wall-clock budgets，`child_timeout_ms` 默认 600,000；不能把 Goose turn 上限或 `load` 等待窗口当作补足 deadline 的方案。
- macOS Seatbelt 失败即拒绝；Windows 无 AppContainer 时禁止 autonomous shell；应用层 Inspector/Permission 不能替代这些 OS 级边界。
- External runtime matrix 已区分 `native_gateway` 与 `claude_cli_harness`；ACP/external dispatch 只能补充诚实 provenance，不能宣传为经过 Native Gateway。

## 11. 材料冲突与缺口

- M2 只可用于黑盒行为和公开扩展面研究，不可用于内部模块深度评分。
- M5 缺乏可审计公开演进历史，不能从单一快照判断长期稳定性。
- M1/M6/M8 都存在新旧引擎并行；报告必须按当前默认路径和迁移缺口分别描述。
- M3 有 DeepChat loop 与 Direct ACP 双 backend，能力不可互相代换。
- M7 的 Graph 能力和普通 Agent loop 共用 runtime，但不等于 Natives 应引入完整 Graph 权威。
- 八个项目均未在相同硬件、模型、任务集上运行；本轮不生成吞吐、成功率或延迟排行。
- 用户说明 Natives 当前因规范整改无法启动，本轮没有进行 UI/端到端运行对比。
