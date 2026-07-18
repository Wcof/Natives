# Native 执行引擎全量整改 — 契约冻结与进度（Phase 0）

> 冻结日期：2026-07-17  
> 目标方案：用户 Goal「Native 执行引擎全量整改实施方案」  
> 参考：`/Users/ldh/Downloads/project/grok-build`  
> 基座：Natives 当前 `agent-core` / `provider-adapters` / `capability-gateway` / `assistant-protocol` / `src-agent-daemon` / `src-tauri`  
> 状态语义：`not_started` | `in_progress` | `partial` | `done` | `blocked`

## 0. 结论标签（禁止夸大）

| 标签 | 含义 | 当前 |
|------|------|------|
| 核心原型可测 | Fixture/单测 + 主链骨架 | **done** |
| 生产闭环最小集 | P0/P1 阻断项（工具消息、detached start、UDS façade、Gateway 门、Broker、project_path、persist-first） | **partial → 接近 done** |
| **真正全量完成** | 本文件 DoD 全部满足 | **仍未完成：真网 Anthropic 与 GUI headed E2E 待环境验收** |

当前能力声明中 `mcp=false` / `extensions=false` / `scheduler=false` **不得**当作完成；必须实现后再声明。

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
  ├─ Extension / Skill / Scheduler
  └─ Persistent Event Store (persist-first)
```

唯一执行接口：`ExecutionAuthority`（见 `src-agent-daemon` / 协议层）。

禁止：UI / Tauri command / 旧 Runtime 直接调 Provider、Stream、CLI Runtime、旧 Agent Loop。

## 2. 能力矩阵（归属 / 入口 / 事件源）

| 能力域 | 唯一归属模块 | 唯一入口 | 唯一事件源 | 状态 |
|--------|-------------|----------|------------|------|
| Run 生命周期 | `RunManager` + `agent-core` | `ExecutionAuthority::*` / `run.*` RPC | `EventSequencer` | `partial` |
| Provider 流式/工具 | `provider-adapters` | Engine → ProviderAdapter | RunEvent provider/tool | `partial` |
| 工具执行 | `capability-gateway` | Gateway.execute | ToolCall* events | `partial` |
| 权限 | `agent-core` + Gateway | permission.respond + PermissionClass | PermissionRequested | `partial` |
| Hook | `agent-core` hooks | Engine 钩子点 | Hook* events | `partial` |
| Subagent | `agent-core` + RunManager child runs | tool `task` | 子 Run 事件流 | `partial` |
| Credential | Tauri Broker / natives.db sidecar | credential.resolve | 无密钥事件 | `partial` |
| 事件存储 | EventSequencer + 默认持久化 | run.replay / subscribe | sequence | `partial` |
| MCP | `src-agent-daemon/mcp_runtime` | mcp.* RPC | MCP events | `partial`（OAuth browser/redirect 明确 unsupported） |
| Extension | 待建 | extension.* | Extension events | `not_started` |
| Skill | 待建 | skill discovery inject | — | `not_started` |
| Scheduler | 待建 | scheduler.* | Scheduler events | `not_started` |
| Memory/Compaction | 待建 | engine 内 | Compact* events | `not_started` |
| Artifact/Attachment | 部分 conversation | artifact.* | Artifact events | `partial` |
| Sidecar Supervisor | Tauri | 启动编排 | Daemon health | `not_started` |
| Protocol v2 Envelope | assistant-protocol v2 | UDS wire | — | `in_progress` |
| 旧链删除 | G6 | 物理删 / deprecated | — | `done` |

## 3. 方法清单与实现状态

每条 v2 方法三态：`implemented` | `unsupported` | `invalid_request`。

| 方法 | 目标 | 当前 | 备注 |
|------|------|------|------|
| daemon.getCapabilities | implemented | implemented | 诚实 surface |
| daemon.getStatus | implemented | implemented | |
| daemon.ping | implemented | implemented | |
| provider.list | implemented | implemented | |
| provider.discoverModels | implemented | unsupported | 需接适配器 |
| provider.test | implemented | unsupported | |
| conversation.* | implemented | partial | 多在 Tauri DB，未统一 Authority |
| run.create/start/cancel/retry | implemented | implemented | |
| run.subscribe | implemented | partial | poll；push 待做 |
| run.replay / getEvents / list | implemented | implemented | |
| run.listChildren | implemented | unsupported | |
| permission.respond | implemented | implemented | |
| tool.list | implemented | implemented | |
| agent.list / subagent.list | implemented | unsupported | |
| extension.* / mcp.* / scheduler.* | implemented | unsupported | 能力 false |
| artifact.* | implemented | unsupported | |

**规则**：`ALL_METHODS` 可列目标；`IMPLEMENTED_METHODS` / capabilities 只能列 `implemented`。未实现必须返回明确 `unsupported`，不得假成功。

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
| 2 | Sidecar Supervisor + Credential Broker 硬化 | **partial**（Supervisor 骨架；**生产默认 uds**；无静默 Embedded；Lease 绑定 run_id；自动恢复待） |
| 3 | Run 生命周期/恢复 + 默认事件持久化 | **partial**（状态机扩展；默认 JSONL 事件；Run 快照恢复为 Interrupted；start 幂等；禁 cwd project_path） |
| 4 | Provider 全量 + Tool Gateway 安全 | **partial**（Gateway 主路径已有；Anthropic 真网门禁待凭据） |
| 5 | Hook 全量 + Subagent 产品化 | **done（fixture 双 Provider 父子 Engine 闭环；真网凭据验收待）** |
| 6 | MCP / Extension / Skill / Scheduler / Memory / Artifact | **partial**（MCP 主链与 SSE；OAuth browser/redirect 明确 unsupported） |
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
10. mcp/extensions/scheduler 声明与实现不一致  
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
- [ ] MCP / Extension / Skill / Scheduler / Memory / Compaction / Artifact / Attachment 完成  
- [ ] UI 全路径联调通过  
- [ ] 两家 Provider 真实 Engine E2E  
- [ ] 旧链物理删除或严格 deprecated + CI 禁生产 import  
- [ ] workspace / frontend / security / UI E2E 全过  
- [ ] 能力声明 = 实际实现  
- [ ] 无凭据泄漏 / 权限绕过 / 路径逃逸 / 重复执行  

## 14. 与 ADR-0011 / Gap Checklist 关系

- ADR-0011：阶段性生产闭环缺口（G1–G6）——多数 G 已关闭，**不**等于全量 DoD。  
- `NATIVE_ENGINE_GAP_CHECKLIST.md`：继续追踪 G/M 项。  
- **本文档**：全量整改权威进度与契约；Phase 交付后同步更新本表。  
