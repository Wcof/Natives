# Native 执行引擎全量整改 — 契约冻结与进度

> **唯一进度与契约源**（2026-07-23 文档清理后）：其它 `NATIVE_ENGINE_*` 快照 / task pack / linkage 状态文档已删除，请只更新本文件 + [`NATIVE-DAEMON-CAPABILITY-MAP.md`](./NATIVE-DAEMON-CAPABILITY-MAP.md) + [`NATIVE_ENGINE_ENV.md`](./NATIVE_ENGINE_ENV.md)。  
> 冻结日期：2026-07-17（契约）；进度随代码更新  
> **最近一次增量核对**：2026-07-29，Native Harness 证据闭环与 ADR-0015 Job/Scheduler 收敛
> **2026-08-04（B00 可信基线）**：冻结基线 `9584c3c2`；建立 native/compat/test 三类执行入口矩阵与 10 点 crash matrix（`scripts/runtime-crash-matrix.json`）；生成 `natives-runtime/b00-baseline` 稳定标签
> 基座：`agent-core` / `provider-adapters` / `capability-gateway` / `assistant-protocol` / `src-agent-daemon` / `src-tauri`  
> 分层约束：[`standards/technical/01-layering.md`](../standards/technical/01-layering.md)  
> 状态语义：`not_started` | `in_progress` | `partial` | `done` | `blocked`  
> 诚实原则：广告 ⊆ 可调；fixture 通过 ≠ 真桌面 done

## 0. 结论标签（禁止夸大）

| 标签 | 含义 | 当前 |
|------|------|------|
| 核心原型可测 | Fixture/单测 + 主链骨架 | **done** |
| 生产闭环最小集 | P0/P1 阻断项（工具消息、detached start、UDS façade、Gateway 门、Broker、project_path、persist-first） | **partial → 接近 done** |
| **真正全量完成** | 本文件 DoD 全部满足 | **仍未完成：真网 Anthropic 与 GUI headed E2E 待环境验收** |

**2026-07-29 校正**：Daemon 能力广告按真实执行面收敛为 `tools/hooks/subagents/mcp=true`、`extensions/scheduler=false`。`scheduler.*` 已从 Daemon 广告与生产分发移除，调度归 Host Job Module；`extension.list` 只保留发现诊断并返回 `discovered_not_executable`，未接隔离运行时前不宣称 enable/executable。Native Harness 的 Prompt 与 Tool 证据已复用同一生产编译/冻结结果；真正全量完成仍受真实 Provider 桌面验收门禁约束。

生产模式目标：

```text
Tauri → UDS Agent Daemon   （唯一生产路径）
Embedded                   （仅单测与开发诊断；禁止生产静默降级）
```

## 1. 目标架构（唯一执行主链）

```text
UI
  ↓ Protocol v2
Tauri Daemon Authority (ExecutionAuthority)
  ↓ UDS
Native Agent Daemon
  ├─ Agent Engine
  ├─ Provider Adapter
  ├─ Credential Broker (lease; no long-lived key in daemon)
  ├─ Capability Gateway
  ├─ Hook Runtime
  ├─ MCP Runtime
  ├─ Subagent Runtime
  ├─ Memory / Compaction
  ├─ Capability-selected MCP / Skill / Expert / Team
  └─ Persistent Event Store (persist-first)
```

唯一执行接口：`ExecutionAuthority`（见 `src-agent-daemon` / 协议层）。

禁止：UI / Tauri command / 旧 Runtime 直接调 Provider、Stream、CLI Runtime、旧 Agent Loop。

## 2. 能力矩阵（归属 / 入口 / 事件源）

> 状态列于 2026-07-26 逐行核对（基线 `682453e3`），2026-07-27 合并能力库（ADR-0016）/ 任务模块（ADR-0015）在途分支时按当时工作区代码复核。**校正方向是让标签与代码一致，不是让它好看**——本轮修正全部是「低报 → 上调」，但同时补录了每行的**已知缺口**，避免上调后被误读为完成。

| 能力域 | 唯一归属模块 | 唯一入口 | 唯一事件源 | 状态 | 核对证据与已知缺口（2026-07-26） |
|--------|-------------|----------|------------|------|------|
| Run 生命周期 | `RunManager` + `agent-core` | `ExecutionAuthority::*` / `run.*` RPC | `EventSequencer` | `partial` | 维持。`run.subscribe` 为 long-poll（`rpc.rs:933-1020`，`mode=subscribe_push_wait`），全双工 push 未做 |
| Provider 流式/工具 | `provider-adapters` | Engine → ProviderAdapter | RunEvent provider/tool | `partial` | 维持。7 个适配器 + 4 个 SSE 解析器共 3778 行。缺 `cache_control` / `tool_choice`（全仓库零命中）、请求侧 `thinking`；`max_tokens` 生产路径固定 4096（`production.rs:1166`、`routing.rs:376`） |
| 工具执行 | `capability-gateway` | Gateway.execute | ToolCall* events | `partial` | 维持。有 `web_fetch`（`tools/mod.rs:639`）+ SSRF 门；**无 `web_search`** |
| 权限 | `agent-core` + Gateway | permission.respond + PermissionClass | PermissionRequested | `partial` | 维持。`permission.listPending` 已进广告面但 daemon 无 handler（见第 3 节） |
| Hook | `agent-core` hooks | Engine 钩子点 | Hook* events | `partial` | **上调证据**：16 个事件全部有 dispatch 调用点（`harness-core/src/hooks/definition.rs:18-40`）。缺口：`permissionDecision` 只认 `"deny"`（`hook_handlers.rs:219-222`）、`HookFailurePolicy::{Skip,Default}` 从未被读取、`rpc.rs` 对 hook 零引用（GUI 不可见） |
| Subagent | `agent-core` + RunManager child runs | tool `task`（含 `subagent_type`/`system_prompt`/`tool_allowlist`/`max_steps`；`agent` = 专家团成员，ADR-0016） | 子 Run 事件流 | `partial` | **2026-07-27 上调证据**：预算账本齐全（`subagents.rs:69-83`，单 mutex `:156`）；2026-07-26 记的三个缺口已关闭——`agent_profile_id` 由 `task` 入参解析并写入子 Run（`production_tools.rs`，`subagent_type`/`agent_profile_id` 双名解析）、`max_steps` 改为「入参 → profile.max_steps → 15」三级回退、`task` schema 已补 `subagent_type`/`system_prompt`/`tool_allowlist`/`max_steps`（`capability-gateway/src/tools/extra.rs`，`task` 工具定义）。专家团派生（`agent` 必须命中花名册，越界 fail-closed）+ member profile 注入已落地（`capability/experts.rs` 946 行）。缺口：`task` 仍不暴露 `model`（凭据/模型由 host 路由策略指派，属刻意约束） |
| Credential | Tauri Broker / natives.db sidecar | credential.resolve | 无密钥事件 | `partial` | 维持。子 Agent 路由只存 ID（`subagent_store.rs:53-57`） |
| 事件存储 | EventSequencer + 默认持久化 | run.replay / subscribe | sequence | `partial` | 维持 |
| MCP | `src-agent-daemon/mcp_runtime` + 能力库 `capability/mcp.rs`（可信配置源，ADR-0016） | mcp.* RPC + capability.mcp.*（CRUD/导入/Hub） | MCP events | `partial` | **2026-07-27 上调证据**：2026-07-26 记的「只有 tools 面」已作废。协议面现为 `initialize` / `tools/list` / `tools/call` / `resources/list` / `resources/read` / `resources/templates/list` / `prompts/list` / `prompts/get` / `roots/list` / `notifications/*` 摄取（`mcp_runtime.rs` 2494 行，`list_resources:1263`、`list_resource_templates:1299`、`read_resource:1327`、`list_prompts:1481`、`get_prompt:1497`、`set_roots:1529`、`client_roots:1548`、`notifications:1591`），RPC 全部分发（`rpc.rs:1890`–`:2016`），协议面回归 `tests/mcp_protocol_surface.rs`（432 行）。`initialize` 的 `capabilities` 逐字捕获为 `McpServerCapabilities`，区分「未握手 / 不广告 / 广告但为空」三态。stdio 读循环按 `method`+`id` / `method` / `id` 三态解复用，服务端中途插入通知或反问 `roots/list` 不再让会话永久错帧。能力库侧：注册 / 按选可用 / schema 注入已落地（`capability/mcp.rs` 590 行，`enabled_runtime_configs:334`、`env_for_server:382`），`env` 里 secret-like key 必须写成 `secret:<id>` 引用（明文直接拒，`capability/mcp.rs:494`），untrusted MCP 在 resolve 与 invoke 两处双门拒；会话按 run 引用计数回收（`mcp_runtime.rs` `acquire:364` / `release_run:378` / `reap_idle:404`）。缺口：`sampling/*` 与 `elicitation/*` **刻意不实现且不进方法目录**（信任方向反转），收到时回 `-32601`；`mcp.call` 对 RPC 关闭（`direct_mcp_call_disabled`）；OAuth browser 流仅 Host 侧 `mcp_oauth_start`（`src-tauri/src/commands/mcp_oauth.rs:61`），daemon 侧 redirect 仍 unsupported |
| Extension | `src-agent-daemon/extension_store.rs` | `extension.list` 发现诊断 | — | **`not_started`（执行）** | 返回 `discovered_not_executable`；`extension.enable` 不广告并 fail-closed。`extension-host/` 尚未接入隔离执行主链 |
| Skill | `src-agent-daemon/skill_store.rs` + 能力库 `capability/skills.rs`（ADR-0016） | `skill.list` + capability.skill.*；注入 system prompt / 按选注入 `prompt_for_selection` | — | `not_started` → **`partial`** | 276 行真实实现，RPC 已分发（`rpc.rs:1934`），注入点 `production.rs:442`（父）/ `:788`（子）。**2026-07-27 补录**：能力库侧元数据/分类/导入/按选注入已落地（`capability/skills.rs` 599 行，`prompt_for_selection:460`）。缺口：`skill_store` 侧仍不解析 YAML frontmatter、`inject_prompt` 拼全文无渐进披露（`skill_store.rs:178-184`）、`trusted: true` 硬编码（`:125-127`）、无 per-skill `allowed-tools`；能力库 git 导入为 P1 未做 |
| Job / Scheduler | Host `src-tauri/src/jobs/` | `job_*`；派发经 `daemon_authority::create_run/start_run` | Host `task_runs` + Daemon Run | **`done`（自动化）** | ADR-0015 P1 已落地：显式 scheduled_at 幂等、CapabilitySelection、Run 对账、旧 jobs.json 事务迁移；Daemon scheduler store/loop/RPC 已退役。真实 Provider 手动+定时桌面验收待 |
| Memory | `src-agent-daemon/memory_store.rs` | `memory.search` / `memory.add` | — | `not_started` → **`partial`** | 193 行，RPC 已分发（`rpc.rs:1953` / `:1974`）。关键词扫描，无 embedding |
| Compaction | `agent-core` `context.rs` / `compaction.rs` | engine 内 | PreCompact / PostCompact | `not_started` → **`partial`** | 两条独立路径均已接线：`compaction.rs:70` ←`engine.rs:1067`（工具输出截断）、`context.rs:254` ←`production.rs:455`（历史裁剪）。**核心缺口：两者都是纯机械截断，无模型摘要**；且 PostCompact 返回值被 `let _ =` 丢弃（`engine.rs:1088`），hook 无回写通道 |
| Artifact/Attachment | `artifact_store.rs` + `conversation_store` | `artifact.*` | Artifact events | `partial` | 维持。`artifact.list/open` 已分发（`rpc.rs:1630` / `:1830`）；`artifact.reveal` 仅 Host 侧实现（`src-tauri/src/assistant_service.rs:99`），daemon 无 handler。附件在 daemon 侧降级为文本占位（`conversation_store.rs:506`） |
| Sidecar Supervisor | `src-tauri/sidecar_supervisor.rs` | 启动编排 | Daemon health | `not_started` → **`partial`** | 1007 行，已接入 Tauri 启动/退出（`src-tauri/src/lib.rs:183/192/415`），暴露 4 个 command（`:745-748`）。含 `ensure_started` / `poll_child_health` / `ensure_healthy_or_restart` / `shutdown_with_grace` |
| Protocol v2 Envelope | assistant-protocol v2 | UDS wire | — | `in_progress` → **`partial`** | `envelope.rs` 371 行，Host/Daemon 全链使用。缺口是广告面一致性（第 3 节），不是 Envelope 本身 |
| 旧链删除 | G6 | 物理删 / deprecated | — | `done` | 维持 |
| 执行图（DAG 调度） | — | — | — | **`not_started`** | 新增行。全仓库无依赖图调度：`ExecutionRegistry`（`src-agent-daemon/src/runtime/execution_registry.rs`，632 行）是**取消令牌树**不是调度器；`SubagentCreated/Completed` 事件无依赖字段（`v2/run_event.rs:176-188`） |

## 3. 方法清单与实现状态

每条 v2 方法三态：`implemented` | `unsupported` | `invalid_request`。

### 3.1 清单规模（2026-07-26 实测）

| 常量 | 条数 | 位置 |
|------|------|------|
| `ALL_METHODS`（目标目录） | **84** | `crates/assistant-protocol/src/v2/methods.rs:5-84` |
| `IMPLEMENTED_METHODS`（daemon 广告） | **74** | 同文件 `:94-176` |
| `HOST_IMPLEMENTED_METHODS`（host 广告） | **10**（其中 4 条不在 `IMPLEMENTED_METHODS`） | 同文件 `:179-198` |
| 实际广告面 = 两者并集 | **78** | `capabilities.rs:72-76`（`host_mediated`） |
| `rpc.rs` 真实 match 分支覆盖 | **73** | `src-agent-daemon/src/rpc.rs:396-2263` |

> 早期文档曾称「`ALL_METHODS` 与 `IMPLEMENTED_METHODS` 各 84 条且一致」——**该说法不成立**，两表相差 10 条。

### 3.2 广告 ⊆ 可调 违规清单（6 条，MUST 级）

以下方法进入了 `daemon.getCapabilities().methods`，但 **daemon `rpc.rs` 没有对应 match 分支**，落入兜底 `_ =>`（`rpc.rs:2264-2280`）。因 `method_status()` 判定其为 `Implemented`，兜底返回码是 **`internal_error`**（而非诚实的 `unsupported`）。

| 方法 | 广告来源 | 实际结果 | 备注 |
|------|----------|----------|------|
| `conversation.listPage` | `IMPLEMENTED_METHODS` | `internal_error` | `conversation_store.rs:13` 已实现 handler，但 `rpc.rs:615-625` 的分发名单漏列 —— 一行名单遗漏。前端在用（`src/lib/assistant-workspace/controller.ts:42`，有 catch 降级） |
| `conversation.getMessagesPage` | `IMPLEMENTED_METHODS` | `internal_error` | 同上，`conversation_store.rs:17`；前端 `src/lib/assistant-gateway/daemon-adapter.ts:307` 在用 |
| `permission.listPending` | `HOST_IMPLEMENTED_METHODS` | `internal_error` | Host 的 `is_host_owned_method`（`src-tauri/src/assistant_service.rs:135-145`）**不含**它，`permission.` 前缀被转发到 daemon |
| `run.finish` | `HOST_IMPLEMENTED_METHODS` | `internal_error` | 同上，`run.` 前缀转发到 daemon |
| `run.listChildren` | `HOST_IMPLEMENTED_METHODS` | `internal_error` | 同上。前端能力门 `capability-gate.ts:51` 把它当作 `task.list` 的降级路径 —— 门被广告骗开 |
| `artifact.reveal` | `HOST_IMPLEMENTED_METHODS` | Tauri 路径 OK / 直连 daemon 路径 `internal_error` | Host 有实现（`assistant_service.rs:99`）且被 `is_host_owned_method` 拦在本地，故 GUI 实际可用；但对任何直连 daemon 的客户端仍是空头广告 |

### 3.3 目录中未广告、诚实返回 `unsupported` 的方法（5 条，合规）

`agent.list`、`conversation.update`、`run.getActivity`、`mcp.auth.oauthStart`、`mcp.auth.oauthCallback` —— 仅在 `ALL_METHODS`，不在任何广告面，落兜底后 `method_status()` 判定为 `Unsupported`，返回码 `unsupported`。**这符合契约**，不属于第 3.2 节的违规。

另有 `mcp.call`：有 match 分支（`rpc.rs:1430`）但故意返回 `invalid_input: direct_mcp_call_disabled`，且**未**进广告面（`methods.rs:126` 有显式注释）——诚实关闭，合规。

### 3.4 逐域状态（校正后）

| 方法域 | 目标 | 当前 | 备注（含 `rpc.rs` 行号） |
|------|------|------|------|
| daemon.getCapabilities / getStatus / ping | implemented | **implemented** | `:600` / `:401` / `:438` |
| provider.list | implemented | **implemented** | Host 侧 `assistant_service.rs:94`；daemon `:1147` |
| provider.discoverModels | implemented | ~~unsupported~~ → **implemented** | `:1169`，真调适配器 `discover_models`，无凭据时降级 `list_models` |
| provider.test | implemented | ~~unsupported~~ → **implemented** | `:1236`，要求 `model_id`，走 `test_provider_model` 真请求 |
| conversation.*（11 个） | implemented | **implemented** | `:615-625`；已是 daemon-owned（旧备注「多在 Tauri DB」已过期）。`listPage`/`getMessagesPage` 见第 3.2 节；`conversation.update` 未实现且未广告 |
| run.create / start / cancel / retry | implemented | **implemented** | `:648` / `:688` / `:732` / `:833` |
| run.subscribe | implemented | **partial** | `:933`，long-poll（`wait_ms` / `mode=push`）；全双工连续 push 仍未做 |
| run.replay / getEvents / list | implemented | **implemented** | `:908` / `:1021` |
| run.rewind / rewindPreview | implemented | **implemented** | `:2214` |
| workspace.restore / restorePreview | implemented | **implemented** | `:2214` |
| run.listChildren / run.finish / run.getActivity | implemented | **unsupported** | 前两者被 host 广告面误列，见第 3.2 节 |
| permission.respond | implemented | **implemented** | `:772` |
| permission.listPending | implemented | **unsupported** | 见第 3.2 节 |
| interaction.listPending / respond | implemented | **implemented** | `:1070` |
| promptQueue.*（7 个） | implemented | **implemented** | `:1036-1042` |
| tool.list | implemented | **implemented** | `:1124` |
| agent.list | implemented | **unsupported** | 目录内未广告，合规 |
| subagent.list / touch / switchRoute | implemented | ~~unsupported~~ → **implemented** | `:1097` |
| extension.list / enable | list 诊断 / enable 未广告 | **list=discovered_not_executable；enable=unsupported** | 执行隔离未接线前 fail-closed |
| skill.list | implemented | ~~未列~~ → **implemented** | `:1934` |
| memory.search / add | implemented | ~~未列~~ → **implemented** | `:1953` / `:1974` |
| scheduler.*（6 个旧方法） | 未广告 | **unsupported** | 调度已归 Host Job Module；Daemon 分发与 15 秒 loop 已删除 |
| mcp.list / start / stop / liveness / reconnect | implemented | ~~unsupported~~ → **implemented** | `:1334` / `:1350` / `:1398` / `:1450` / `:1482` |
| mcp.auth.set / status / clear | implemented | **implemented** | `:1514` / `:1565` / `:1598` |
| mcp.call | — | **故意关闭** | `:1430`，返回 `direct_mcp_call_disabled`，不进广告面 |
| mcp.auth.oauthStart / oauthCallback | implemented | **unsupported** | 明确未广告，合规 |
| artifact.list / open | implemented | ~~unsupported~~ → **implemented** | `:1630` / `:1830` |
| artifact.reveal | implemented | **host-only** | 见第 3.2 节 |
| task.list / cancel / wait | implemented | ~~未列~~ → **implemented** | `:1642` / `:1714` / `:1761` |
| conversation.getContextUsage | implemented | ~~未列~~ → **implemented** | `:2240` |
| engine.rateLimit.*（4 个） | implemented | ~~未列~~ → **implemented** | `:448` / `:468` / `:522` / `:563` |

**规则**：`ALL_METHODS` 可列目标；`IMPLEMENTED_METHODS` / capabilities 只能列 `implemented`。未实现必须返回明确 `unsupported`，不得假成功。**第 3.2 节的 6 条即为该规则的现存违反项。**

> 说明：另有 agent 正在并行修复第 3.2 节的广告面缺口，本表记录的是 2026-07-26 基线 `682453e3` 的审计时点状态；修复进展见对应 task。

## 4. Run 状态机（冻结）

```text
created
→ queued
→ preparing
→ running
→ waiting_permission | waiting_subagent
→ cancelling
→ completed | failed | cancelled | interrupted
```

终端态：`completed` | `failed` | `cancelled` | `interrupted`。  
完成后禁止再入活动态。  
`project_path` 必须由 UI 显式传入，禁止 daemon cwd 默认。

持久化字段（最低集）：  
`run_id, conversation_id, parent_run_id, project_path, provider_id, key_id, model_id, permission_profile, input, status, retry_count, created_at, started_at, finished_at, last_event_sequence, idempotency_key, error`。

## 5. 事件类型清单（最低集）

- RunStarted / RunCompleted / RunFailed / RunCancelled / RunInterrupted  
- TextDelta / ThinkingDelta  
- ToolCallStarted / ToolCallCompleted / ToolCallFailed  
- PermissionRequested / PermissionResolved  
- SubagentStarted / SubagentCompleted / SubagentFailed  
- HookStarted / HookCompleted / HookFailed  
- CompactStarted / CompactCompleted  
- UsageUpdated  
- Error  

路径：创建 → 分配 sequence → **持久化成功** → 内存索引 → 广播。禁止先广播后落盘。

## 6. Tool Manifest（目标）

`read_file, write_file, edit_file, list_dir, grep, glob/search, terminal/bash, task/subagent, ask_user_question, background task, task output, kill task, artifact, memory search, MCP tools, Web/HTTP`。

统一管线：Schema → Run 身份 → PathScope → PermissionClass → SideEffect → PreToolUse → 执行 → 截断 → PostToolUse → 事件。

## 7. Hook 事件清单（目标）

SessionStart/End, UserPromptSubmit, PreToolUse, PostToolUse, PermissionRequest, SubagentStart/Stop, Stop, CompactStart/End, Notification, Error。

配置源：项目 / 用户 / 内置；`.claude|grok|natives/hooks.json`；command + HTTP；matcher；blocking；fail-closed（安全类）/ fail-open（通知类可配）。

## 8. Provider Capability Matrix（目标）

| Provider | 文本流 | 工具 | 多工具 | 回填 | usage | cancel | discover | 真网 E2E |
|----------|--------|------|--------|------|-------|--------|----------|----------|
| OpenAI Responses/Chat | 目标 | 目标 | 目标 | 目标 | 目标 | 目标 | 目标 | 发布门禁 |
| OpenAI Compatible | partial | partial | partial | partial | partial | partial | partial | SenseNova 已测 |
| Anthropic | partial | partial | partial | partial | partial | partial | partial | 待 |
| Gemini | partial | partial | partial | partial | partial | partial | partial | 待 |
| DeepSeek | partial | partial | partial | partial | partial | partial | partial | 待 |
| Ollama | partial | partial | partial | partial | partial | partial | partial | 待 |
| Custom | partial | partial | partial | partial | partial | partial | partial | 待 |

发布：至少两家真实 Provider Engine E2E（文本 + 工具闭环 + Subagent + 取消 + 失败重试）。

## 9. Subagent 数据模型（冻结）

Child Run 为完整独立 Run：独立 provider/key/model/base_url、permission、tools、hooks、context、event stream；父子关联；级联取消；凭据不可继承；权限不可提升；最大深度与并发可控。

## 10. 环境变量与运维契约

| 变量 | 含义 | 默认 |
|------|------|------|
| `NATIVES_DAEMON_MODE` | `uds` / `embedded` / `auto`(仅开发) | **生产 `uds`** |
| `NATIVES_DAEMON_SOCKET` | UDS 路径 | `$XDG_RUNTIME_DIR/natives-agent.sock` |
| `NATIVES_DAEMON_BOOTSTRAP` | 握手 token | sidecar 安全管道 |
| `NATIVES_RUNTIME_DIR` | runtime 根 | `~/.natives/runtime` |
| `NATIVES_DB_PATH` | 密钥库 | `~/.natives/natives.db` |
| `NATIVES_EVENT_LOG_DIR` | **过渡**可选 JSONL；终态默认 SQLite/JSONL 开启，不依赖该变量 | — |

生产禁止：`UDS 失败 → 静默 Embedded`。必须显式故障态。

## 11. 实施阶段进度

| Phase | 内容 | 状态 |
|-------|------|------|
| 0 | 契约冻结（本文档） | **done** |
| 1 | Protocol v2 Envelope + ExecutionAuthority | **partial**（Envelope + trait + reconnect；UDS lifecycle harness 绿；subscribe long-poll；全双工 push 流待） |
| 2 | Sidecar Supervisor + Credential Broker 硬化 | **partial**（Supervisor 已非骨架：`src-tauri/src/sidecar_supervisor.rs` 1007 行，含 `ensure_started` / `poll_child_health` / `ensure_healthy_or_restart` / `shutdown_with_grace`，接入 `lib.rs:183/192/415` 与 4 个 command；**生产默认 uds**；无静默 Embedded；Lease 绑定 run_id；崩溃后自动恢复的端到端录证待） |
| 3 | Run 生命周期/恢复 + 默认事件持久化 | **partial**（状态机扩展；默认 JSONL 事件；Run 快照恢复为 Interrupted；start 幂等；禁 cwd project_path） |
| 4 | Provider 全量 + Tool Gateway 安全 | **partial**（Gateway 主路径已有；Anthropic 真网门禁待凭据） |
| 5 | Hook 全量 + Subagent 产品化 | **done（fixture 双 Provider 父子 Engine 闭环；真网凭据验收待）** |
| 6 | MCP / Extension / Skill / Memory / Artifact；Job 归 Host | **partial**（MCP 主链与完整协议面 + 能力库按选可用已落地；Skill 按选注入、Memory、Artifact 有最小真实表面。Extension 仅发现诊断、执行仍 `not_started`。Scheduler 已从 Daemon 移除并由 Host Job Module 独立闭环。其余缺口：Skill 渐进披露/能力库 git 导入、Memory embedding、模型摘要 Compaction、MCP OAuth daemon redirect） |
| 7 | UI 全路径联调 | **partial**（project_path 强制 + subscribe push_wait；headed GUI 录证待） |
| 8 | 旧链删除 + 发布门禁 | **done（旧执行文件物理删除；审计脚本 + strict warning + CI runner）** |

## 12. 发布阻断条件（任一即不可标「全量完成」）

1. 生产默认仍可能走 Embedded  
2. UI 可绕过 Daemon  
3. Daemon 无法获取真实 Credential  
4. project_path 使用 daemon cwd  
5. 事件先广播后持久化  
6. retry 可能重复启动  
7. Tool 可绕过 Permission Gate  
8. Hook 仅少数事件  
9. Child 继承父 Key/权限  
10. 能力广告面与 `rpc.rs` 分发不一致（**当前有 6 条违反，见第 3.2 节**）  
11. 旧执行链仍有生产调用方  
12. 测试/日志出现明文 Key  
13. 仅 Adapter 测试、无真实 Engine 工具闭环  

## 13. Definition of Done（真正完成）

- [ ] 生产默认 UDS Daemon；无静默 Embedded  
- [ ] Sidecar 自动启动 / health / 重启 / 恢复  
- [ ] NATIVES_DB_PATH 文档 + 校验 + 迁移 + 自动化  
- [ ] 统一 Protocol v2 Envelope 通信  
- [ ] 工具 / Hook / Permission / Subagent 同一主链  
- [ ] Subagent 独立 Provider/Key/Model  
- [ ] 事件默认持久化 + replay/subscribe  
- [ ] MCP / Extension / Skill / Memory / Compaction / Artifact / Attachment 完成（Scheduler 不属 Daemon；Host Job 自动化已闭环）
- [ ] UI 全路径联调通过  
- [ ] 两家 Provider 真实 Engine E2E  
- [ ] 旧链物理删除或严格 deprecated + CI 禁生产 import  
- [ ] workspace / frontend / security / UI E2E 全过  
- [ ] 能力声明 = 实际实现  
- [ ] 无凭据泄漏 / 权限绕过 / 路径逃逸 / 重复执行  

## 14. 与 ADR-0011 / Gap Checklist 关系

- ADR-0011：阶段性生产闭环缺口（G1–G6）——多数 G 已关闭，**不**等于全量 DoD。  
- ~~`NATIVE_ENGINE_GAP_CHECKLIST.md`~~：已于 2026-07-23 文档清理时删除，勿再引用。  
- [`EXECUTION-ENGINE-CAPABILITY-AUDIT.md`](./EXECUTION-ENGINE-CAPABILITY-AUDIT.md)：执行引擎能力审计（2026-07-26），本文第 2/3 节标签的证据来源。  
- **本文档**：全量整改权威进度与契约；Phase 交付后同步更新本表。  

## 15. 可信基线（B00，2026-08-04）

> 实施批次 B00 / TASK-000 交付。来源：`docs/audit/natives-agent-problem-verification.md` + `docs/audit/natives-agent-issues.json`（审计 worktree）。本批次只冻结契约，**不修改生产代码，不创建 DB 迁移，不实现生产 failpoint**。后续 TASK-001..017 必须从 `natives-runtime/b00-baseline` 解析出的精确 SHA 开始。

### 15.1 基线记录（可追溯）

| 项 | 值 |
|---|---|
| 基线 Commit（GitHub `deploy` 最新） | `9584c3c263c1e83b1066e4208e1ab2d678a9deeb`（`fix(ui): use semantic log tokens`） |
| 任务分支 | `codex/runtime-task-000-baseline-b00a-202608041512` |
| 执行 Worktree | `Natives-agent-runtime-task-000-baseline-202608041512`（主工作区被 Creative OS B1 占用，用户显式授权为此任务创建 Worktree） |
| Rust 工具链 | `stable-aarch64-apple-darwin`，rustc 1.96.0 / cargo 1.96.0 |
| 其他工具链 | git 2.50.1；node v22.23.2；npm 10.9.8 |
| OS | macOS 26.5.2（arm64） |
| 可用磁盘（开始） | 35 GiB |
| 共享 Cargo target（`.cargo-target-shared`） | 7.8 GiB |
| node_modules（主工作区） | 1.0 GiB |
| Worktree 自占 | 18 MiB |
| 同时 Worktree | 3（主 + 审计 + 本任务）；B00 后清理本任务 Worktree 即回 2 |
| 构建环境 | `CARGO_TARGET_DIR=…/.cargo-target-shared`、`CARGO_BUILD_JOBS=2`、`RUST_TEST_THREADS=2`、`CARGO_INCREMENTAL=0` |

### 15.2 三类执行入口矩阵（native / compat / test）

| 入口 | 路径 | 状态 | 权威约束 |
|---|---|---|---|
| **native（唯一生产主链）** | Renderer → Tauri Host → 认证 UDS → Daemon `rpc.rs` → `RunManager`（`runtime_id=native`）→ `ProductionRuntime` → `AgentEngine` → `ProviderTurnRequest` / `PermissionGatedTools` → `CapabilityGateway` | 生产 | Run 终态唯一提交者 = RunManager；事件 persist-first；Renderer 不合成 sequence/terminal；UI/Host 不得旁路 |
| **compat（兼容运行时）** | `cli_runtime_bridge`（`claude_cli` 子进程，`run_manager.rs:1900-2065`），不经 AgentEngine/Gateway | 兼容窗 | 不宣称 Native Core 不变量；CLI capability 明示非 native authority（TASK-013 收口） |
| **test / advanced seam** | `run_with_tool_schemas` EngineMessage compat（`engine.rs:289-420`；`production.rs:688-778`） | 仅测试与开发诊断 | 禁止生产静默降级；嵌入式模式仅单测/诊断，失败必须显式故障态 |

### 15.3 十个可复现 crash points（故障注入契约）

> 机器可读契约：`scripts/runtime-crash-matrix.json`（`contract_version=1`）。每点 = 一个可注入的 kill/DB 故障/资源故障相位 + 恢复契约 + 责任任务。TASK-017 全量验证时必须给出每个点的生产链或 crash test 证据。

| ID | 名称 | 问题 | 优先级 | 相位 | 恢复契约 | 责任任务 |
|---|---|---|---|---|---|---|
| CRASH-01 | Checkpoint 越界读取 | N01 | P0 | `capture_before`（Gateway scope 校验前） | 绝对/`..`/symlink 在任何 read/checkpoint I/O 前拒绝，拒绝不产生快照 | TASK-001 |
| CRASH-02 | Tool 成功后事实写失败 | D04 | P0 | ToolCallCompleted event append | crash 后 Resume Blocked；uncertain 持久化失败使 Run 不可自动恢复 | TASK-004 |
| CRASH-03 | Checkpoint cursor 冒充 Ledger watermark | G01 | P0 | checkpoint cursor 写入 | cursor 可查询为 Ledger 前缀；event sequence 不再写入 ledger cursor | TASK-004 |
| CRASH-04 | Turn 投影 crash-gap | N02/F02 | P0 | run end projection（多事务） | 任一写点 kill 后 transcript 与 reload 等价，收敛到完整或不存在 | TASK-005 |
| CRASH-05 | Shell pipe 死锁 | J02 | P0 | stdout/stderr drain | 10MB 双流不 hang；cancel 后 child 已 kill/wait/reap | TASK-002 |
| CRASH-06 | Shell Unicode 截断 panic | J02 | P0 | output cap/tail | CJK/emoji 任意 cap 边界不 panic | TASK-002 |
| CRASH-07 | UDS 无界帧 / slowloris | N04 | P1 | RPC frame read | 固定上限终止 oversized frame；非法输入/慢连接均回收 session | TASK-003 |
| CRASH-08 | SQLite 同步 Mutex 阻塞 async | F01 | P1 | storage 同步连接 | async 路径无直接 `Mutex<Connection>` 阻塞；BUSY/FULL 不丢 critical fact | TASK-006 |
| CRASH-09 | Progress 无界积压 | H03 | P1 | progress channel | 固定容量 + 100MB 输出 RSS 有界；Result/cancel 后 late update 丢弃 | TASK-007 |
| CRASH-10 | Sub Agent 创建失败留孤儿 | E04/N05 | P1 | child create 补偿 | 每个失败点后无 orphan row；重启后 budget 连续；child 互不越权 | TASK-009 |

### 15.4 failpoint contract 说明

- `scripts/runtime-crash-matrix.json` 是**契约**，不是生产 failpoint 实现。生产 failpoint 由各自任务卡（TASK-001..016）在允许路径内实现，TASK-017 全量验证。
- 每个 crash point 的 `repro_test` 引用对应任务卡的 `targeted_tests`；命令名以任务卡最终实现为准。
- 验收语义统一：启动副作用必须有 terminal 或 durable uncertain；无法证明 safe 时自动 Resume 调用 handler 次数为 0。

### 15.5 验证命令与退出码（TASK-000）

| 命令 | 退出码 | 备注 |
|---|---|---|
| `cargo fmt --check` | **0（2026-08-04 实跑）** | 共享 Target 环境；本批次不修改代码格式 |
| `cargo check --workspace --jobs 2` | **0（2026-08-04 实跑）** | 共享 Target；8 crates；19.56s（缓存热，同 SHA）。执行前等待 Creative OS B1 的 `cargo test -p natives --lib`（独立 `target/`）结束后错峰运行，未并发使用共享 Target |

> 未运行：workspace test、clippy、npm/lint/typecheck、Tauri build（B00 禁止）；前端依赖验证属 TASK-017。

---

## 16. Pi-like 热路径整改（2026-08-08，A0–A8 真实完成状态）

> 本轮整改基线 `4b8193cd`（deploy），已合并进 `main`（单 clone 状态）。
> **2026-08-08 最终集成复核**（HEAD `662f1cca` + 本地未提交收尾）：补齐 A8 run-end 增量投影生产接线（run_manager 两处改调 `project_run_incremental`）、修复 verify:native-engine 并行测试隔离（`EventSequencer::memory_only()`），9/9 Final Gate 与 B1/B2/B3/B5 在复核 HEAD 上重跑全绿（见 16.2/16.3）。
> 只记录真实完成项与证据；**真网 Anthropic 与 GUI headed E2E 仍未验收**，不夸大为全量完成。

### 16.1 完成项（代码 + 测试证据）

| 任务 | 状态 | 证据 |
|---|---|---|
| A0 Baseline harness | done | `src-agent-daemon/tests/perf_baseline.rs`（#[ignore]）；`.runtime-evidence/baseline.json` |
| A1 Live/Durable Event Split | done | `agent-core` 新增 `LiveEventBus`；TextDelta/ReasoningDelta/ToolCallDelta 走 live lane；删除累计 MessageDelta emit（保留 enum decode）；`live_text_delta_never_calls_durable_persistence` 测试（1000 delta → <10 durable calls） |
| A2 Persistent Event Stream | done | daemon `run.watch`（after_sequence 增量 + 持续 push，newline V2EventEnvelope）；`event_stream_v1` 能力位；long-poll `run.subscribe` 保留为兼容；`stream_watch_*` 测试 2 项 |
| A3 Persistent UDS Client | done | `UdsAuthority` 长连接 CommandClient（8 RPC = 1 handshake）+ 独立 EventClient（watch_events）；`DaemonClient::read_event`；`daemon-adapter.ts` 移除固定 poll sleep；`uds_authority_reuses_command_connection` 等测试 3 项。**生产接线复核（2026-08-08）**：Host `daemon_authority::request("run.subscribe", {after_sequence, wait_ms, mode:'push'})` → UdsAuthority CommandClient 长连接（不每 RPC connect）；`run_gateway.rs:345-376` 转发 `run.subscribe` 并在失败时 fallback `replay_events`；UdsAuthority `watch_events`（`authority.rs:454-488`）独立 EventClient 调 `run.watch`；`resolve_run_authority_mode`（`client.rs:428-468`）生产默认 UDS、未知值 fail-closed 到 UDS、缺失 socket 硬失败不静默降级 Embedded（`uds_mode_must_not_silent_fallback_to_embedded` 测试） |
| A6 Settings V2 Backend | done | `execution_engine:settings:v2` 持久化权威 + 迁移旧 `executor:settings`；Runtime snapshot/resolver；Codex fail-closed；disabledTools 只减法；`prepare_for_save` 纯策略层；6 项单测 |
| A4 ReadOnly Tool Fast Path | done | `SideEffect::ReadOnly` 工具跳过 checkpoint/ledger/conflict lease，直达 handler；写类工具严格路径不变；`readonly_tool_does_not_create_side_effect_record` 测试 |
| A5 PreparedAgentSession | done | `prepared_session.rs` 缓存静态 prompt/tool schema/skill 元数据 + digest 失效 + 有界（32 项）；`ContextStats` 增量字符/token 预算；3 项单测 + context 套件 |
| A7 Settings UI | done | 设置 > 执行引擎 = [运行设置][Harness 编排] 双 tab；`ExecutionEngineSettingsPanel` 完全由 backend snapshot 驱动；Harness tab 复用 `NativeHarnessPanel`；tauri-adapter V2 API；i18n zh/en 同步。**生产接线复核（2026-08-08）**：`SettingsPage.tsx:826-827` `case 'runtime'` → `ExecutionEngineSection`；`ExecutionEngineSection`（`SettingsPage.tsx:34-61`）双 tab `tab==='runtime'` → `ExecutionEngineSettingsPanel` / 否则 → `NativeHarnessPanel`（默认 tab = 运行设置）；i18n `executionEngine.tabRuntimeSettings`/`tabHarness` 在 zh.ts:581-583 与 en.ts:581-583 同步；面板注释明示仅暴露 Runtime/fallback/maxSteps/减法型 disabledTools，禁止 UDS poll/durability/checkpoint/doom-loop 开关 |
| A8 Incremental Projector | done | `project_run_incremental` 只重放 watermark 之后事件；幂等/quarantine/partial-turn 安全不变；`projector_incremental_uses_watermark_prefix` 测试；**生产接线已补齐**（2026-08-08 复核）：`run_manager.rs` detached/常规两处 run-end 投影由 `project_run_from_events(replay_after_checked(0))` 改为 `project_run_incremental`，不再从 sequence 0 全量重放；`production.rs` run 收尾仍用本次 run 内存事件切片（非全量历史） |

### 16.2 Barrier / Final Gate 实测

- Wave1 Barrier（合并 A0→A1→A2→A3→A6）：`cargo fmt --check` 0、`cargo check --workspace --all-targets` 0 errors、`protocol:check` OK、`typecheck` OK。
- Wave2 合并 A4→A5→A7→A8 后 Final Gate 全部通过（9/9，2026-08-08 逐项实跑，main HEAD `aba50b06`）：
  `cargo fmt --check` / `cargo check --workspace --all-targets` / `protocol:check` / `verify:native-engine`（447 passed） / `typecheck` / `lint`（含 i18n 2550=2550、hardcoded colors 0 新增） / `test`（795 pass, 0 fail） / `perf:check`（bundle 预算内） / `tauri:build`（.app + .dmg 产出）。
- **2026-08-08 最终集成复核重跑（HEAD `662f1cca` + 本地收尾改动，9/9 全绿）**：
  `cargo fmt --check` 0 / `cargo check --workspace --all-targets` 0 / `protocol:check` OK（156 methods）/ `verify:native-engine` 0（agent-core 207 passed + daemon 452 passed，AUDIT PASS；此前并行 flaky 已通过 `EventSequencer::memory_only()` 测试隔离修复）/ `typecheck` 0 / `lint` 0（i18n 2550=2550）/ `test` 0（796 pass, 0 fail）/ `perf:check` 0（bundle 预算内）/ `tauri:build` 0（.app + .dmg 产出）。
- 退出码与完整输出日志入库：`.runtime-evidence/gate/final-gate-2026-08-08.txt` + `.runtime-evidence/gate/logs/`（提交 `25055cee`、`cd5fa1f3`）。
- **复核 Gate 证据文件**：`.runtime-evidence/gate/review-logs/`（`review-summary.txt` 9/9 全 0 + `01-fmt.log`…`09-tauri-build.log` 完整日志，2026-08-08 集成复核实跑）。

### 16.3 性能证据（A0 baseline → integration，同一 harness）

| 指标 | Before | After |
|---|---:|---:|
| submit→completed（500 chunks） | 1612 ms | 707 ms |
| submit→completed（2000 chunks） | 7852 ms | 2855 ms |
| durable run_event rows（500 chunks） | 1006 | 6（live delta SQLite writes = 0） |
| durable run_event rows（2000 chunks） | 4006 | 6（live delta SQLite writes = 0） |
| run_event rows / 1000 chunks | 2012 | 12（500）/ 3（2000） |
| payload bytes / 1000 chunks | 5.2 MB | 22.3 KB |
| 新 MessageDelta emits | 500 | 0 |

> 数字为 2026-08-08 对 `main` HEAD `aba50b06` 的最终重跑（同一 A0 harness，单 clone 状态）；原始证据已入库：`.runtime-evidence/after/baseline.json` + `delta.md`（提交 `79e6b0ba`、`aba50b06`）。
> **2026-08-08 集成复核重跑（HEAD `662f1cca` + 本地收尾改动，证据文件 `.runtime-evidence/after/review-benchmark.json` + `review-benchmark.md`）**：B1 500 chunks `submit_to_completed = 760 ms`（durable rows 6 / message_delta 0）、2000 chunks `3008 ms`（durable rows 6 / message_delta 0）——对比 Before 1612/7852 ms（−53%/−62%），性能结论不变。B2（`readonly_coding_loop_does_not_create_ledger_or_checkpoint`）、B3（`settle_tool_effect_only_moves_started_rows`）、B5（`stream_watch` 2 项 + `persistent_uds_reuses_handshake`）全部通过。

详细 delta 见 `.runtime-evidence/after/delta.md`。

### 16.4 未完成项（诚实边界）

- 真网 Anthropic / OpenAI 桌面 headed E2E 性能采样（p95 delta→paint、cancel→engine、terminal tail 需真实 GUI 会话）。
- B4 长会话、B5 reconnect 断开恢复、B6 daemon restart、B7 settings migration 的桌面级复测（B5/B6 已有 UDS 测试覆盖，非 headed 验收）。
- A7 尚未移除 Scheduled Tasks 侧栏入口与旧 `RuntimePanel` 死代码（保留兼容周期）。
- Codex app-server 未实现，保持 blocked（fail-closed 不变）。

### 16.5 Rollback commit list

单层可回滚（从 `main` HEAD `aba50b06` 依次 revert）：

1. `738dcb08` fix(gate): hardcoded colors → theme tokens（可安全 revert，仅 UI 样式）
2. `d203d0c6` fix(gate): audit-old-symbols 期望持久 UDS command/event client 拆分（与 A3 同层）
3. `d60d3d7f` test(engine): fixture lifecycle 断言 TextDelta live-only（A1 契约测试调整）
4. `5124882b` perf(projector): watermark-driven incremental projection [A8]
5. `3b1f474e` feat(settings): execution engine settings UI [A7]
6. `888ca5cb` perf(session): prepared agent session cache + incremental context stats [A5]
7. `7ac0e122` perf(tools): read-only fast path [A4]
8. `e5f5f6d8` fix(integration): barrier wiring（TS run.watch 类型同步，A2 依赖，勿单独回退）
9. `fa1be1bb` refactor(settings): execution engine settings v2 backend [A6]
10. `204449ff` perf(uds): reuse command client connection [A3]
11. `72befba6` perf(daemon): add persistent run event stream [A2]
12. `7c525542` perf(agent): split live events from durable facts [A1]
13. `696ff035` perf(baseline): A0 baseline harness（无生产行为，可保留）

回滚策略：A1 为架构分水岭，其上层（A2/A3/A4/A5/A8/A7/A6）依赖 live/durable 拆分；如需整体退回旧架构，从 #12 起 revert 并保留 A0 baseline 即可。旧 `MessageDelta`/`executor:settings`/localStorage runtimePref 读路径全部保留，可安全 downgrade。

---

## 17. 05 验收报告（B1/B2/B3/B5/B7 + §7 模板）

### Performance delta

| Metric | Before | After | Change | Pass? |
|---|---:|---:|---:|---|
| submit→completed（500 chunks） | 1612 ms | 707 ms | −51% | ✅ |
| submit→completed（2000 chunks） | 7852 ms | 2855 ms | −60% | ✅ |
| SQLite writes / 1000 chunks | 2012 | 12（500 chunks）/ 3（2000 chunks） | −99%+ | ✅ |
| live delta SQLite writes | 4006 rows | 6 rows（B1 500/2000 各 6） | **0** | ✅ |
| 新 message_delta emits | 500 | 0 | **0** | ✅ |
| active stream fixed sleep | 400 ms poll | 0 ms（server push） | — | ✅ |
| handshakes / run（B5 稳定连接） | 1 per RPC | command 1 + event 1（8 RPC = 1） | — | ✅ |
| readonly tool→next provider p95 | 受 checkpoint/ledger 拖累 | 快路径（无 ledger/checkpoint） | — | ⏳ headed 实测 |
| delta→paint p95 | — | — | — | ⏳ headed 实测 |
| terminal tail p95 | — | — | — | ⏳ headed 实测 |

### Correctness

- checkpoint invariant：写类工具 capture_before/after 严格保留（A4 仅 ReadOnly 跳过）；`readonly_coding_loop_does_not_create_ledger_or_checkpoint` 断言快照=0。
- ledger invariant：B2 只读循环 side-effect ledger rows = 0；`settle_tool_effect_only_moves_started_rows` 原子 settle 保留。
- resume invariant：checkpoint/ledger/watermark 语义未退化；projector 增量投影保持幂等/quarantine/partial-turn 安全。
- permission invariant：`deny_wins_over_allow`、readonly profile 拒绝副作用、Settings 只收紧不扩权（`settings_disabled_tools_cannot_expand_capabilities`）。
- reconnect invariant：`persistent_uds_reuses_handshake`（8 RPC = 1 handshake）+ `run.watch` after_sequence 恢复（`stream_watch_*`）。
- migration invariant：B7 冲突 fixture —— 已持久化 V2 权威，legacy `executor:settings` 不覆盖（`b7_conflict_v2_is_authority_legacy_not_reapplied`）；无 DB 键时安全默认（`b7_no_db_keys_yields_safe_defaults`）。
- codex fail-closed：`codex_stays_blocked_without_app_server`。
- no embedded fallback：UDS 模式缺失 socket/bootstrap 即硬失败（`resolve_run_authority_mode` 生产默认 uds）。

### 证据文件
- `.runtime-evidence/after/baseline.json` + `delta.md`（最新提交 `aba50b06`，先期 `888a9a29`/`79e6b0ba`）
- 回归测试：`readonly_coding_loop_does_not_create_ledger_or_checkpoint`（production_tools.rs）、`execution_engine_settings_b7.rs`（提交 `36bfc93d`）；§5 精确命名断言 13/13（提交 `ac898128`）
