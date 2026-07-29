# Native 执行引擎全量整改 — 契约冻结与进度

> **唯一进度与契约源**（2026-07-23 文档清理后）：其它 `NATIVE_ENGINE_*` 快照 / task pack / linkage 状态文档已删除，请只更新本文件 + [`NATIVE-DAEMON-CAPABILITY-MAP.md`](./NATIVE-DAEMON-CAPABILITY-MAP.md) + [`NATIVE_ENGINE_ENV.md`](./NATIVE_ENGINE_ENV.md)。  
> 冻结日期：2026-07-17（契约）；进度随代码更新  
> **最近一次增量核对**：2026-07-29，Native Harness 证据闭环与 ADR-0015 Job/Scheduler 收敛
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
