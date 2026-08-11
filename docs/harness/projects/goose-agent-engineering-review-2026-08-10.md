# Goose Agent 工程复核

> 复核日期：2026-08-10
> 仓库：`/Volumes/UNTITLED/本人材料/project/goose`
> 固定版本：`021b0db8dbee8d6c7e9ffbab580a4143598a3560`
> 研究范围：Agent Harness、Tool Runtime、权限与沙箱、Prompt/Context、压缩、Hook、Session、Subagent、MCP/ACP、Event/Replay、进程边界与模块设计。
> 结论性质：固定提交的源码、测试合同、配置/架构资料与 Git 历史静态复核；未启动 Goose、未执行 Goose 全仓测试。外置盘 `._*` AppleDouble 文件及工作树改动不纳入评价。

## 1. 结论

Goose 是一个生态完成度很高的 Rust Agent 应用：内建 Developer、MCP、Platform Extension、Recipe、Hook、ACP 和 Subagent 都能被统一装配到 Agent 上下文。它最值得研究的不是“工具很多”，而是把工具请求先经过一条有顺序的 Inspector/Permission 流水线，把 compaction 做成工具请求-响应配对的渐进压缩，并为 Recipe、扩展和外部 Agent 保留了可组合的入口。

但 Goose 的安全语义不能按生产级 fail-closed 引擎理解。Inspector 出错继续执行，Adversary Provider 失败明确 Allow，Egress 只记录不阻断，阻断 Hook 的 spawn/timeout/序列化/普通非零退出也按 Allow 处理；`Auto` 模式更是直接允许所有本地工具。Subagent 虽然创建了 durable `SessionType::SubAgent`，但当前为避免权限消息挂起而强制 `GooseMode::Auto`，没有统一的 wall-clock deadline，任务句柄、状态和结果等待表主要在进程内。

综合评价：

| 维度 | 评价 | 事实依据 |
| --- | --- | --- |
| Kernel loop | 中偏强 | 1000 turns fuse、空响应有限重试、Stop Hook 阻断上限、safe-point steer；但 loop 与持久 Run/event log 未分离 |
| Tool runtime | 强 | typed tool request/result、Inspector 顺序、批次内工具并发、按请求顺序回填历史 |
| 权限与风险解释 | 中 | finding 结构和 Allow 不覆盖 deny/ask 很好；失败策略多处 fail-open，Auto 绕过普通确认 |
| OS sandbox | 弱到中 | Flatpak/Docker 可选隔离；内建 Developer shell 不由 `--container` 统一保护，未见完整进程组/资源隔离 |
| Prompt/context | 中偏强 | 稳定扩展排序、时间按小时冻结、动态 hint 触发安全点重建；没有完整 Run Prompt 快照 |
| Compaction | 强 | 0/10/20/50/100% 渐进移除工具结果、summary visibility、结构化摘要回退、工具 pair 保护 |
| Hook | 中 | 事件覆盖面广，Pre/Stop 可表达阻断；decision Hook 失败按 Allow，环境和子进程边界偏宽 |
| Session durability | 中偏强 | SQLite v15、`BEGIN IMMEDIATE`、copy/truncate 语义、100 Agent LRU 与创建锁；没有独立 Run/attempt/replay 事实 |
| Subagent | 中 | child Session、父子链、目录 containment、25 turns/5 background/600 秒结果 TTL；权限、取消、deadline 仍不完整 |
| Event/replay | 弱到中 | `AgentEvent` 覆盖消息/usage/MCP/history replacement；不是 durable append-only Run event log |
| 模块深度 | 中 | 测试合同广，但 Agent、Session、Extension、Summon 都是 3k–4k 行认知热点 |

## 2. 证据口径

本报告只把固定提交中能从装配、执行入口和测试追到的实现称为“当前能力”。README、旧架构稿或 Recipe 示例只用于解释意图，不替代代码。遇到“有接口但失败策略放行”的功能，评价按失败语义而不是按接口数量给分。

关键固定版本证据：

- Agent 循环常量和状态：`crates/goose/src/agents/agent.rs:69-73,1930-2025,2740-2929`。
- 工具检查与合并：`crates/goose/src/tool_inspection.rs:10-118,149-252`，装配顺序 `agents/agent.rs:659-687`。
- Egress/Adversary 失败策略：`security/egress_inspector.rs:344-386`、`security/adversary_inspector.rs:470-488`。
- Hook 事件、阻断和子进程：`hooks/mod.rs:42-62,292-364,385-425,528-577`。
- Prompt 构建与缓存前缀：`agents/prompt_manager.rs:117-199,202-240`。
- Context/compaction：`context_mgmt/mod.rs:26-30,106-170,320-419,500-663`。
- Session/SQLite：`session/session_manager.rs:26-96,1800-1847,2299-2380`；Agent LRU `execution/manager.rs:18-61,121-145`。
- Subagent：`agents/subagent_task_config.rs:8-62`、`platform_extensions/summon.rs:449-527,1064-1125,1271-1320,1708-1776,1787-1850,2047-2073`。
- Recipe：`recipe/mod.rs:41-86,97-206`、`recipe/validate_recipe.rs:10-20,39-171`、`recipe/manifest.rs:11-70`。
- ACP/MCP：`acp/provider.rs:410-490,555-580`、`agents/reply_parts.rs:1108-1163`、`providers/codex_acp.rs:119-145`、`agents/extension_manager.rs:1322-1389,1391-1445,1849-1898`。

## 3. 默认执行链

默认 Goose Agent 可以还原为：

```text
AgentManager
  -> per-session Agent
  -> reply / reply_internal
  -> prepare_reply_context
  -> Provider stream
  -> categorize tool requests
  -> ToolInspectionManager
  -> Permission aggregation
  -> concurrent Tool dispatch
  -> ordered history settlement
  -> next Provider turn / Stop Hook
```

`AgentManager` 用最多 100 个 Agent 的 LRU 缓存，并用 per-session creation lock 避免并发恢复同一 Session 时重复初始化 Provider/MCP。`Agent` 同时持有 Prompt、Extension、Permission、Hook、Retry、Goal/Grind 和 pending steer 状态，因此整条执行链可用，但协调职责集中。

模型流返回后，内建 frontend tool 与普通 tool 先分桶。普通请求先完整经过 Inspector，再由 Permission Inspector 聚合为 approved/needs approval/denied。批准的工具以带 request id 的 stream 并发执行，完成事件可能乱序；response map 最终按原始请求顺序结算到会话历史，再进入下一 Provider 轮次。

## 4. Agent Loop 与终止

### 4.1 有界循环

- 默认 `GOOSE_MAX_TURNS` 为 1000；超过上限产生可见终止消息。
- Provider 返回空文本、无工具、无错误时最多重试 3 次，之后写入可见空响应错误，避免静默结束。
- Stop Hook 可以阻止结束；连续阻断默认上限 8，超过后强制结束并写入警告。
- pending steer 存在 Agent 进程内 `HashMap<session_id, VecDeque<Message>>`。只有到安全点才 drain；取消时明确 discard，未形成 durable queue/claim/replay 合同。

### 4.2 优点与缺点

优点是 fuse、空响应重试和 Stop Hook 上限都在中心 loop 中，而不是散落在工具实现里；safe-point steer 也避免在 Provider 不可中断期间修改请求。

缺点是 `logical round`、Provider request、physical attempt 没有独立持久身份；`AgentEvent` 不是完整的 Run event log。Provider 已发生但客户端未收到的外部副作用，也没有 unknown-outcome journal 可以约束透明重试。

## 5. Tool Runtime、并发与输出

### 5.1 Inspector 流水线

默认装配顺序为 Security → Egress → Adversary → Permission → Repetition。每个 finding 统一携带 `Allow/Deny/RequireApproval`、reason、confidence、inspector name 和可选 finding id。`Allow` 只表示当前 Inspector 不反对，不会覆盖其它 Inspector 已产生的 deny 或 ask。

这是 Goose 很好的可解释性设计：用户可以知道风险来自哪个 Inspector，而不是只看到“工具被拒绝”。但 `ToolInspectionManager::inspect_tools` 捕获 Inspector 错误后记录日志并继续，最后仍返回当前已收集结果；它不是 fail-closed policy gate。

### 5.2 并发和历史顺序

同一 Provider 回复中的多个普通工具可同时启动，使用 `stream::select_all` 收集通知/结果；`request_to_response_map` 保存每个 request id 的响应对象，最终按 Provider 请求顺序写回历史。这种“执行可乱序、事实结算有序”兼顾了读取吞吐和上下文确定性。

当前未见基于资源读写集合的冲突 barrier。写工具只要进入 approved 集合，就可能与其它工具并发；多 Session 之间也没有统一资源锁。因此不能把这种并发模型当作可证明的文件事务调度器。

### 5.3 输出预算

- 通用 MCP 文本超过默认 200,000 字符时写入持久临时文件，模型得到裸路径。
- Developer shell 另按 2,000 行或 50,000 bytes 截断，并保留 stdout/stderr/interleaved 三种结构化结果。
- shell 默认扩展超时 300 秒；超时/取消只 `start_kill` 直接 child，未建立跨平台进程组终止。

工具输出有上限、截断有可见提示，这是优点；裸路径没有稳定 Artifact ID、hash、TTL、ACL 和 lineage，且大文件写入失败时会把完整内容重新放回上下文，可能冲击预算。

## 6. Permission、Hook 与 Sandbox

### 6.1 Permission

Approve/SmartApprove 模式先查用户对工具的 AlwaysAllow/NeverAllow/AskBefore；SmartApprove 可对 read-only annotation 放行，未知工具默认 ask。`Auto` 分支直接生成 Allow，原因就是“Auto mode - all tools approved”。因此 Auto 是产品模式开关，不是经过独立 Capability Gateway 的证明。

### 6.2 Egress 与 Adversary 的真实边界

Egress 能识别 HTTP、Git SSH、S3/GCS、SCP/SSH、容器仓库和包发布目标，并写出站日志；但当前检测到目标后仍返回 `InspectionAction::Allow`，confidence 为 0，没有阻断或用户确认。Adversary Provider 错误则显式返回 Allow、confidence 0。两者适合作为解释事实，不足以作为安全裁决。

### 6.3 Hook

Hook 覆盖 Pre/Post Tool、Session Start/End、UserPromptSubmit、文件读写、Shell 前后和 Stop。普通 `emit` 是 observer；`emit_blocking` 可识别退出码 2 或 `{"decision":"block"}`，Stop/PreToolUse 因此能表达阻断。

风险在于实现合同明确写着：spawn、timeout、序列化和普通非零退出均记录后按 Allow 继续。命令以 `sh -c` 启动，继承宿主环境，只覆盖 `PLUGIN_ROOT`/可选 PATH；`kill_on_drop` 只约束直接 child，`wait_with_output` 未见 stdout/stderr 上限。安全关键 Hook 若采用该语义，会在配置错误时静默放行。

### 6.4 Sandbox

Flatpak 路径可把 shell 放到 host spawn；Docker container 选项主要包裹扩展进程，内建 Developer shell 仍由 Agent 直接启动。代码中的 Permission Inspector、命令模式匹配和这些可选容器都不能等价宣称为 OS sandbox。尤其要区分“需要确认”与“即使 Agent 被攻破也不能越界”。

## 7. Prompt、Context 与 Compaction

PromptManager 对 extension/tool 信息做稳定名称排序、Unicode tag 清理，并把当前时间按小时冻结，以提高跨 Session prompt cache 命中。system override、extra、frontend instructions 和 hints 会在每次安全点重建 system prompt；tool 更新或工作区新 hint 产生后，旧 prompt 并非完整 Run 不可变快照。

默认 compaction 阈值为上下文上限的 0.8。模型摘要遇到 context overflow 时，依次移除中部 0%、10%、20%、50%、100% 的 Tool response 后重试；结构化摘要解析失败保留 raw output。原始消息变成 user-visible/agent-invisible，summary 和 continuation 变成 agent-only user-role，防止摘要伪装成 system 指令。

工具 pair 压缩每批最多 10 个，且保护当前 turn 的最近工具调用；在压缩前要求 request/response 都存在，避免只留下孤立 tool result。这是 Goose 最值得迁移的上下文工程细节之一。

局限是 summary、anchor、Prompt view 和 tool schema 没有同一事务的 CAS 版本；未见 `requestSeq`、ViewManifest 或 stale compaction writer 防护。模型总结仍可能携带未被模式识别的秘密，不能当作完整脱敏系统。

## 8. Session、恢复与 Subagent

Session SQLite 当前 schema version 为 15，Session、消息、usage ledger 分开存储。重要写入使用 `BEGIN IMMEDIATE`；`replace_conversation` 在一个事务中删除后重新插入，copy/truncate 等操作也有明确入口。运行时只缓存最多 100 个 Agent，按 Session 维度加创建锁。

然而没有独立 durable Run、attempt identity、permission fact、event replay 或资源租约。`AgentEvent` 只有 Message、Usage、MessageUsage、MCP notification、HistoryReplaced 五类，适合 UI 流式更新，不足以在崩溃后证明每个工具的 started/unknown/terminal 状态。

### 8.1 Subagent 当前路径

- 默认公开面是 `summon`，内部提供同步 `delegate`、异步 `delegate` 和 `load`；隐藏的 `orchestrator` 默认关闭。
- child 使用 `SessionType::SubAgent`，设置 `parent_session_id`，工作目录必须 canonicalize 到 parent 目录内。
- 默认最多 25 turns；后台运行最多 5 个；已完成结果在进程内保留默认 600 秒。
- `load` 一次等待窗口为 300 秒，超时后把正在运行的 task 放回内存表；这不是任务 wall-clock deadline。
- cancel 最多等待 5 秒，随后 abort。
- child 被强制设为 `GooseMode::Auto`，源码说明原因是父 Session 尚未转发 `ActionRequired`，否则 child 会挂起等待确认。
- child 继承 parent extensions，可按参数缩减；child 禁止递归 delegate。

该设计的优点是 child 不是一次性字符串函数，而是有真实 Session、历史和父子链；缺点是权限无法自然回到父权威，Auto 直接扩大了自主执行面，长时间运行没有统一 deadline/lease/恢复协议。

## 9. Recipe、MCP 与 ACP

### 9.1 Recipe

Recipe 将 instructions/prompt、provider/model/temperature/max turns、MCP/Builtin/Platform/Frontend/stdio/HTTP/inline Python extension、参数、JSON response schema、sub-recipes、success checks、on-failure shell 和 retry 组合成复用入口。参数类型包括 string/number/boolean/date/file/select；验证器检查 duplicate key、模板引用、optional default、file default、JSON schema 和 retry 配置。

这是很好的“输入包”构思，但 Recipe 同时拥有工作流、扩展装配、Provider 参数和 shell 验证，若直接接入 Natives 就会和 Harness Blueprint、RunTemplate、Capability Gateway 形成第二套权威。Recipe manifest 的 id 是路径的 `DefaultHasher`，并非内容 hash 或不可变版本快照，文件内容变化可以保留同一个 id。

### 9.2 MCP 与工具 ownership

ExtensionManager 统一物化 MCP tools，普通名称为 `extension__tool`，并把 extension owner 写入 metadata；tool list 有 cache version，扩展变化时递增版本并清空缓存。MCP App 返回值会先移除不可信 metadata，再注入宿主可信 attachment。这些稳定命名、owner 和 cache invalidation 语义可以直接参考。

### 9.3 ACP external dispatch

ACP Provider 声明 `manages_own_context() = true`，会忽略 Goose 的 system prompt/tool list，由外部 Agent 自己维护上下文和工具。外部 ToolCall 携带 `goose.external_dispatch` 标记，保留在历史中但跳过本地 Inspector/dispatch；权限主要由外部 runtime 的 ActionRequired 或 Goose mode 映射决定。Codex ACP 的 Auto 映射是 `never + danger-full-access`。

优点是对外部 Agent 的边界诚实，避免同一个调用被 Goose 重派发两次；缺点是外部 runtime 成为了第二执行权威，能力、审计、sandbox 和恢复不再与 Goose 本地路径等价。

## 10. 模块深度、测试与 Git 演进

固定树约有 274 个 Rust 源文件，171 个文件含测试，约 1,836 处 `#[test]`/`#[tokio::test]` 标注。行为合同覆盖工具 pairing、permission 混合、compaction overflow、Hook deny、Recipe validation、Subagent working-dir containment、ACP marker 等关键边界。

主要认知热点：`agents/agent.rs` 4,413 行、`session/session_manager.rs` 4,238 行、`agents/extension_manager.rs` 3,318 行、`summon.rs` 3,052 行，此外 ACP server/provider 也超过 2,600 行。测试广不等于模块深；当前仍需要读者在巨型协调器中穿越权限、持久化、Provider、UI event 和兼容分支。

Git 最近演进集中在结构化 compaction、权限修复、Hook parity、Subagent/Recipe 和 ACP audience 边界，说明项目持续修复真实边界而非只有生态扩展。另一方面，同一协调文件持续累积功能，未来应优先把 policy、settlement、lease 和 persistence seam 下沉成深模块。

## 11. Natives 可吸收设计

### P0：只进入既有 Daemon 权威

1. **`RiskFinding` 解释层**：借鉴 `action/reason/confidence/inspector/finding_id`，加入现有 Capability Gateway 的输入事实。finding 只能解释或收紧，不能成为第二个 Allow 权威；安全关键检查异常、超时、解析失败必须 fail-closed。
2. **工具 ownership 与版本化注册表**：采用 `extension__tool` 这类稳定命名、owner/source metadata、cache version 和重复名的显式拒绝/确定性处理，仍由 Natives Gateway 物化最终 Tool schema。
3. **External dispatch 诚实事件**：保留“由外部 runtime 执行”的事实标记、runtime 名称、权限映射和 bypass 原因；只能落在 `claude_cli_harness` 等 External Runtime Bridge，不能伪称已通过 Native Capability Gateway。

### P1：增强现有合同

4. **工具 pair 渐进压缩**：先核对 Natives `agent-core/context.rs`/`compaction.rs` 当前是否已有等价的 pair 保留和 overflow 策略；若缺失，在现有 compaction 权威中补“最多 N 对、保护当前 turn、请求/响应必须成对”的阶段，不新建 Goose 式 Context Manager。
5. **Recipe → RunTemplate 输入包**：吸收 Recipe 的参数校验、file 参数禁止 default、JSON response schema 和 success check 思路；解析后生成不可变 RunTemplate/Capability Hub 输入，所有扩展和 shell 检查仍必须经过现有 Gateway、ArtifactStore、budget 和事件事务。
6. **Observer/Decision Hook 分层**：借鉴 HookEvent 覆盖面、顺序执行和 bounded timeout；observer 只做审计通知，decision Hook 进入 `PermissionGatedTools`，最小环境、stdout/stderr 上限、进程组终止，失败 fail-closed。
7. **Safe-point steer**：保留 Goose 在 Provider turn 结束后 drain steer 的边界，但实现必须接入 Natives 已有 durable prompt queue/RunEvent/claim/disposition，不能复制进程内 `pending_steers`。

### P2：长期演进

8. **子 Agent 长生命周期**：借鉴 child Session、parent lineage、目录 containment 和结果引用；权限、Token/tool/depth/wall-clock budget 继续由 Natives `child_run_orchestrator` 与 Capability Gateway 统一管理，禁止强制 Auto。
9. **结构化执行结果**：保留 Goose shell 的 stdout/stderr/exit_code/timed_out/truncated 字段，但完整输出只能进入 Natives run-scoped ArtifactStore，模型看到 stable artifact reference、bounded preview 与 hash。

## 12. 明确拒绝

- 不采用 Inspector/Adversary/Egress/decision Hook 的 fail-open 作为安全默认。
- 不把 egress“检测并记录”描述成 egress gate。
- 不把 `Auto` 模式或 ACP `danger-full-access` 当作 Native capability grant。
- 不把大响应裸临时路径当 Artifact identity。
- 不让写工具在没有资源冲突调度或事务语义时盲目并发。
- 不让 Prompt、Tool、Permission、Hook 或 Provider 在活动 Run 中热漂移；改动必须产生新 revision/snapshot。
- 不把 Recipe 直接升格为第二套 Run/Permission/Provider/Capability 权威。
- 不把 ACP external dispatch 的历史标记宣传为经过 Native Gateway。
- 不把 turn 上限、300 秒 `load` 等待窗口或 5 秒 cancel grace 当作统一任务 deadline。

## 13. 复核后的单句评价

Goose 适合借鉴“生态装配、风险事实、工具配对压缩、稳定工具 ownership 和外部 dispatch 的诚实标注”；它的 fail-open 安全策略、Auto 子 Agent、裸路径输出、无资源冲突并发和非 durable Event 流不适合作为 Natives Native 执行引擎的底座。
