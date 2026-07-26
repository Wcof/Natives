# Native Agent Daemon — 内部能力划分

> **版本**: 1.1.0  
> **日期**: 2026-07-23  
> **范围**: Daemon 进程内模块边界 + Host 迁移/预检边界（无运行期投影）  
> **契约总表（唯一进度源）**: [`NATIVE_ENGINE_FULL_REMEDIATION.md`](./NATIVE_ENGINE_FULL_REMEDIATION.md)  
> **分层规范**: [`standards/technical/01-layering.md`](../standards/technical/01-layering.md)  
> **方法目录**: `crates/assistant-protocol/src/v2/methods.rs`（`ALL_METHODS` / `IMPLEMENTED_METHODS`）  
> **原则**: 广告 ⊆ 可调；能力域有唯一归属、唯一入口、唯一事件源

---

## 0. 一句话

Daemon 是**本地 sidecar 微服务**：会话 / Run / 工具 / 供应商流式 / Hook / 子 Agent / 事件落盘 的**执行权威**。  
外部唯一入口是 **UDS RPC**（`rpc.rs`）；内部唯一 Run 权威是 **`RunManager` + `ProductionRuntime`**。

### Host 边界（v1.1）

| 角色 | 职责 | 禁止 |
|------|------|------|
| **Tauri Host** | Provider/凭据（natives.db）、`run.start` 预检（project_path + provider/model）、OS 动作（artifact.open）、旧 `assistant_*` **只读迁移源** | 运行期写入 `assistant_runs/messages/run_events`；事件投影循环；镜像队列 |
| **Agent Daemon** | Conversation / Message / Run / Event / Permission / Queue / SessionActor / Checkpoint 的**唯一写入权威**（assistant.db） | 读取 Host 密钥明文 |

旧 `assistant_*` 表保留一个版本周期供迁移；确认稳定后再单独删表。

---

## 1. 进程内分层（总览）

```text
┌─────────────────────────────────────────────────────────────────────────┐
│  L0  传输与协议门面                                                       │
│  main.rs · rpc.rs · client.rs · authority.rs                             │
│  握手 / 会话令牌 / 方法分发 / ExecutionAuthority 适配（嵌入 vs UDS 客户端）   │
└────────────────────────────────┬────────────────────────────────────────┘
                                 │ method + params
┌────────────────────────────────▼────────────────────────────────────────┐
│  L1  领域权威（生命周期与编排）                                            │
│  run_manager.rs          Run 唯一权威（create/start/cancel/retry/list）   │
│  production.rs           生产接缝组装：Engine + Provider + 门控工具 + 子Run │
│  session_harness.rs      会话 Actor 骨架（与 agent-core SessionCoordinator）│
│  prompt_queue_store.rs   提示词队列持久化 + 插话 / sendNow / drain          │
└─────┬──────────────────────┬──────────────────────┬─────────────────────┘
      │                      │                      │
┌─────▼──────────┐  ┌────────▼──────────┐  ┌────────▼──────────────────────┐
│ L2a 执行核心    │  │ L2b 安全与能力     │  │ L2c 扩展运行时                 │
│ （库 crate）    │  │                  │  │                              │
│ agent-core     │  │ capability-      │  │ mcp_runtime · skill_store     │
│  AgentEngine   │  │  gateway         │  │ extension_store · scheduler   │
│  状态机·Hook   │  │  PermissionGated │  │ cli_runtime_bridge            │
│  压缩·协调器   │  │  Tools           │  │ codex_runtime_bridge(关)      │
│ provider-      │  │ natives_db_      │  │ memory_store · artifact_store │
│  adapters      │  │  broker(可选)    │  │ subagent_store · task_store   │
└─────┬──────────┘  └────────┬─────────┘  └────────┬──────────────────────┘
      │                      │                      │
┌─────▼──────────────────────▼──────────────────────▼─────────────────────┐
│  L3  持久化与投影                                                         │
│  storage/* · event_log · conversation_store · checkpoint · interaction  │
│  assistant.db（执行库）· 事件先落盘再广播                                   │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## 2. 能力域矩阵（归属 / 入口 / 事件源）

| 能力域 | 归属模块（Daemon 内） | 唯一入口（RPC 或内部） | 唯一事件源 | 备注 |
|--------|----------------------|------------------------|------------|------|
| **进程门面** | `rpc` / `main` | `daemon.*` | — | 握手、能力广告、ping/status |
| **Run 生命周期** | `run_manager` + `agent-core` 状态机 | `run.create/start/cancel/retry/list` | `EventSequencer` / `event_log` | 硬终态；`project_path` 必显式传入 |
| **执行环** | `production` → `AgentEngine` | 仅经 `RunManager.start` | RunEvent* | 禁止 RPC 直调 Engine |
| **供应商流式** | `production::RealProvider` + `provider-adapters` | Engine → `EngineProvider` | Text/Thinking/ToolCall/Usage | 生产路径无 Echo mock |
| **工具执行** | `PermissionGatedTools` + `capability-gateway` | `tool.list`；执行仅 Engine 调 | ToolCall* | Schema→PathScope→Permission |
| **权限 / 交互** | `production` waiters + `interaction_store` | `permission.respond` / `interaction.*` | Permission* | grant 可落 `tool_grant` |
| **Hook** | `production::build_production_hooks*` + `agent-core` | Engine 钩子点 | Hook* | 项目 hooks 默认不信任 |
| **子 Agent** | `production` spawn + `subagent_store` + `task_store` | 工具 `task`；`subagent.*` | 子 Run 事件流 | 独立 provider/key；不继承父 Key |
| **会话 / 消息** | `conversation_store` | `conversation.*` | — | 生产 UDS 下 daemon-owned |
| **提示词队列** | `prompt_queue_store` + harness | `promptQueue.*` | — | 插话仅安全点生效 |
| **检查点 / 回放** | `checkpoint` + `event_log` | `run.rewind*` / `run.replay` / `subscribe` | sequence | persist-first |
| **MCP** | `mcp_runtime` | `mcp.*` | MCP 相关 | OAuth 浏览器流 host 侧 |
| **调度** | `scheduler_store` | `scheduler.*` | 到期 tick → 再走 Run 主链 | 不旁路 Engine |
| **技能** | `skill_store` | `skill.list`；注入 system prompt | — | 仅 trusted+enabled 注入 |
| **扩展** | `extension_store` | `extension.list/enable` | — | 最小表面；完整隔离后续 |
| **记忆** | `memory_store` | `memory.search/add` | — | 关键词扫描（无 embedding） |
| **产物** | `artifact_store` | `artifact.list/open` | — | 路径隔离在 run 目录内 |
| **任务** | `task_store` | `task.list/cancel/wait` | — | 终端任务 + 子 Agent 任务 |
| **CLI 扩展轨** | `cli_runtime_bridge` / `codex_*` | 会话主路径可选 | 映射为 RunEvent | Codex 默认 fail-closed |
| **凭证** | Host Broker 为主；`natives_db_broker` 可选 | `resolve_credential_for_run` | **无密钥事件** | 生产默认不长存 Key |

---

## 3. 核心模块职责（按文件）

### 3.1 L0 传输与协议

| 文件 | 职责 | 不负责 |
|------|------|--------|
| `main.rs` | 读环境变量、缩短 UDS 路径、启动 `RpcServer`、优雅退出 | 业务逻辑 |
| `rpc.rs` | 握手、鉴权、**方法分发**到 RunManager / stores | 执行环内部细节 |
| `client.rs` | 作为 **UDS 客户端**（Host 侧或进程间） | Daemon 内权威 |
| `authority.rs` | `ExecutionAuthority` trait：`EmbeddedAuthority` / `UdsAuthority` | 工具/模型实现 |

### 3.2 L1 领域权威

| 文件 | 职责 |
|------|------|
| **`run_manager.rs`** | 进程内 **Run 唯一权威**：内存 map + SQLite 恢复、幂等键、`project_paths`、挂 `ProductionRuntime`、启动/取消/重试/列表/事件订阅入口 |
| **`production.rs`** | **生产接缝组装**：`ProductionRuntime`（事件序号、权限等待、子 Agent、Hook、引擎表、CLI 取消旗、tool grants）；`RealProvider`；`PermissionGatedTools`；`spawn_child_task`；fixture 仅测试 |
| `session_harness.rs` | 每会话 Actor：队列镜像、插话、cancel-and-send（与 `agent-core::SessionCoordinator` 对齐） |
| `prompt_queue_store.rs` | `prompt_queue` 表 + 与 harness / RunManager 接线 |

**组装关系（启动一次 Native Run）**：

```text
RunManager.start_run
  → 校验状态 / 写 trigger 消息 / 解析 Profile·skills
  → ProductionRuntime 准备 hooks + PermissionGatedTools + RealProvider
  → assemble_context(...)
  → AgentEngine::run(config, provider, tools, events)
  → 终态回写 RunManager + 事件流
```

### 3.3 L2 执行与安全（库 + Daemon 适配）

```text
┌────────────────── agent-core ──────────────────┐
│ AgentEngine          单次 Run 执行环             │
│ run_state            状态转移唯一规则            │
│ SessionCoordinator   安全点 / 队列 / 插话        │
│ hooks / permissions  Hook 与权限类模型           │
│ compaction / context 历史压缩与上下文装配        │
│ subagents            子 Agent 配置与深度模型     │
│ EventSequencer       序号分配（可接持久化）       │
└──────────────────┬─────────────────────────────┘
                   │ trait 接缝
     EngineProvider│              │EngineToolRuntime
                   ▼              ▼
        RealProvider          PermissionGatedTools
     (provider-adapters)    (capability-gateway 执行)
                   │              │
                   ▼              ▼
            真实 HTTP 流式     Schema·PathScope·沙箱·审计
```

| 组件 | 接口语义 |
|------|----------|
| `EngineProvider` | `stream(model, messages, tools, system, cancel)` → 文本/思考/工具增量 |
| `EngineToolRuntime` | `list_tool_schemas` / `execute_tool` / 可选 `execute_task_batch`（子 Agent 批） |
| `PermissionGatedTools` | 在网关外再包 Ask/Allow/Deny、grant、与 `interaction` 等待 |

### 3.4 L2c 扩展运行时（Daemon 模块）

| 模块 | 能力边界 |
|------|----------|
| `mcp_runtime` | 注册表、stdio 会话、HTTP/SSE、tools/call；未信任 stdio 不自动起；SSRF 门 |
| `skill_store` | 发现 `.claude/.grok/.natives/skills`；仅 trusted+enabled 注入提示 |
| `extension_store` | 列表/启用/信任声明（最小真实表面） |
| `scheduler_store` | 作业 JSON + due tick；**触发仍走 Run 主链** |
| `memory_store` | JSONL 关键词检索 / 追加 |
| `artifact_store` | `artifacts/{run_id}/` 下列出/打开，防路径逃逸 |
| `subagent_store` | 路由策略与会话登记（只存 provider/key/model **ID**） |
| `task_store` | 终端任务与子 Agent 任务行 |
| `cli_runtime_bridge` | Claude CLI stream-json → RunEvent |
| `codex_runtime_bridge` | **红线**：未就绪则 fail-closed，不得因二进制存在而广告可执行 |

### 3.5 L3 持久化

| 模块 | 内容 |
|------|------|
| `storage/*` | SQLite `DataStore`、迁移、分库/legacy 迁移工具 |
| `event_log` | 单调 sequence、原子 append、after_sequence 回放 |
| `conversation_store` | 会话与消息、上下文用量等 |
| `checkpoint` | 检查点捕获 / rewind 预览与冲突策略 |
| `interaction_store` | 权限/提问交互行（恢复待决） |
| `natives_db_broker` | **可选** sidecar 读 `natives.db`；生产优先 Host Broker |

**库边界**：

```text
natives.db          → 密钥 SoT（Host / 可选 broker 短解密）
assistant.db        → Run / 会话 / 事件 / 队列 / 交互（Daemon 执行库）
NATIVES_RUNTIME_DIR → socket、scheduler jobs、memory、artifacts
```

---

## 4. RPC 能力面（按域分组）

来源：`IMPLEMENTED_METHODS`（须与 `rpc.rs` 一致；未实现不得进广告）。

> **2026-07-26 审计发现该一致性当前被破坏**：广告面 78 条中有 6 条无 `rpc.rs` 分支，落兜底返回 `internal_error`。清单与根因见 [`NATIVE_ENGINE_FULL_REMEDIATION.md` 第 3.2 节](./NATIVE_ENGINE_FULL_REMEDIATION.md)。下表列的是**应有表面**，不代表当前全部可调。

| 域 | 方法（已实现表面） |
|----|-------------------|
| 守护进程 | `daemon.getCapabilities` `getStatus` `ping` |
| 供应商 | `provider.list` `discoverModels` `test` |
| 会话 | `conversation.create/list/get/fork/getMessages/appendMessage/rename/update_model/update_permission/archive/delete` `getContextUsage` |
| Run | `run.create/start/cancel/retry/subscribe/replay/list/getEvents` `rewind` `rewindPreview` |
| 权限/交互 | `permission.respond` `interaction.listPending/respond` |
| 提示词队列 | `promptQueue.list/enqueue/update/remove/reorder/sendNow/interject` |
| 工具 | `tool.list` |
| 子 Agent | `subagent.list/touch/switchRoute` |
| MCP | `mcp.list/start/stop/liveness/reconnect` + `mcp.auth.set/status/clear`。`mcp.call` **故意不进广告面**（`rpc.rs:1430` 返回 `direct_mcp_call_disabled`，task-06 关闭直连旁路）；`mcp.auth.oauthStart/oauthCallback` 明确 unsupported |
| 调度 | `scheduler.list/create/update/delete/history/tick` |
| 扩展/技能/记忆 | `extension.list/enable` `skill.list` `memory.search/add` |
| 产物/任务 | `artifact.list/open` `task.list/cancel/wait` |

**Host 仍可参与的缝**（`HOST_IMPLEMENTED_METHODS`）：Run 预检/投影、`permission/interaction` UI、`artifact.open/reveal` 等 OS 动作——**不**取代 Daemon 的 Run 权威。

---

## 5. ProductionRuntime 内部结构

```text
ProductionRuntime
├─ events: EventSequencer              事件序号 + 可选持久化
├─ permissions: PermissionManager      权限配置/解析
├─ subagents: SubAgentManager          子 Agent 身份与深度
├─ hooks: HookRegistry                 生命周期 + 项目 hooks
├─ permission_waiters                  permission_id → (run_id, oneshot)
├─ assignment_waiters                  子 Agent 批量分配交互
├─ task_outputs                        task_id → 子 Run 状态/输出
├─ engines                             run_id → 活着的 AgentEngine
├─ cli_cancel_flags                    CLI 轨取消
├─ tool_grants                         会话/项目级工具批准记忆
└─ run_tool_allowlists                 子 Run 启动前硬白名单

接缝实现：
  RealProvider          : EngineProvider
  PermissionGatedTools  : EngineToolRuntime（内嵌 Gateway + 权限等待）
  FixtureProvider       : 仅测试 / 显式 fixture，禁止冒充生产成功
```

---

## 6. 能力调用时序（工具一轮）

```text
AgentEngine 收到 tool_calls
  → PreToolUse Hook
  → PermissionGatedTools.execute_tool
       → grant 命中？否则 PermissionRequested + 等待 interaction/permission.respond
       → CapabilityGateway：Schema → PathScope → PermissionClass → 沙箱执行
  → PostToolUse Hook
  → 结果结构化写回 messages（保留 tool_call_id）
  → EventSequencer：ToolCallCompleted（先落盘）
  → 再请求 Provider
```

子 Agent：

```text
工具 task / execute_task_batch
  → 可选 assignment 交互（一次批）
  → ProductionRuntime.spawn_child_task
       → RunManager.create_run(parent_run_id, depth+1)
       → 独立 provider/key/model（Profile 或显式；禁止继承父 Key）
       → 子 Engine 独立事件流
  → 父 Run waiting_subagent → 汇总 TaskRecord
```

---

## 7. 设计约束（Daemon 内）

1. **单权威**：跨进程不共享 `global_run_manager()`；生产 Host 只做 UDS 客户端。  
2. **单执行接口**：UI / scheduler / 子编排不得绕过 `ExecutionAuthority` / `run.*`。  
3. **无密钥事件**：Credential 只内存租约；错误脱敏。  
4. **persist-first**：事件先写库再广播。  
5. **诚实能力**：`DaemonCapabilities.methods` = `IMPLEMENTED_METHODS`；Codex 不可用不得标 executable。  
6. **扩展不旁路**：MCP/Skill/Scheduler 最终仍汇入 Engine 或受控工具面。

---

## 8. 源码索引

```text
src-agent-daemon/src/
  main.rs rpc.rs client.rs authority.rs
  run_manager.rs production.rs
  session_harness.rs prompt_queue_store.rs
  conversation_store.rs event_log.rs checkpoint.rs
  interaction_store.rs task_store.rs subagent_store.rs
  mcp_runtime.rs scheduler_store.rs skill_store.rs
  extension_store.rs memory_store.rs artifact_store.rs
  cli_runtime_bridge.rs codex_runtime_bridge.rs
  natives_db_broker.rs storage/

crates/
  agent-core/ provider-adapters/ capability-gateway/ assistant-protocol/
```

---

## 9. 相关文档

| 文档 | 角色 |
|------|------|
| 本文 | Daemon **内部**能力划分 |
| `NATIVE_ENGINE_FULL_REMEDIATION.md` | 全链契约与阶段进度 |
| `NATIVE_ENGINE_ENV.md` | 环境变量与运维 |
| [`EXECUTION-ENGINE-CAPABILITY-AUDIT.md`](./EXECUTION-ENGINE-CAPABILITY-AUDIT.md) | 执行引擎能力审计与缺口定级（2026-07-26） |
| ~~`ASSISTANT-ENGINE-LINKAGE-STATUS.md`~~ | 已于 2026-07-23 文档清理时删除，勿再引用 |
| ADR-0011 | 生产闭环缺口决策 |
