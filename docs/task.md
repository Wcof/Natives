# Natives Native 执行引擎收口整改方案

## 审计结论

基于提交 `534b0b0d`，整改尚未完成。仓库自带验证脚本全部通过，但真实 Provider 与有头 GUI 验证均为 `not_run`。

| 领域 | 结论 | 主要缺口 |
|---|---|---|
| Daemon / Protocol v2 | 部分完成 | UDS、Run、重放已接通；`agent.list`、`subagent.list` 未实现，Windows Named Pipe 缺失 |
| Provider | 部分完成 | 已是真实 HTTP；模型发现仍多为静态列表，凭据未绑定真实 `run_id`，Responses 选择依赖全局环境变量 |
| Tool Runtime | 部分完成 | 统一入口已存在；无完整 JSON Schema 校验、流式进度、真实取消、并行执行，若干工具仍是占位实现 |
| Hook Runtime | 部分完成 | Command/HTTP Handler 已有；多数生命周期只注册未触发，modify/inject/rewake 未完整生效，项目 Hook 被默认信任 |
| Agent Profile / Context | 未接通 | Profile 只支持简化 YAML 且生产执行使用 `assemble_context(None, ...)`；Skills、Plugins、Profile 均未真正注入 |
| Subagent | 未完成 | 子 Engine 可运行，但不是 RunManager 管理的持久化子 Run；深度固定为 1，无 Profile、Checkpoint、恢复、Worktree；旧 Tauri 直连 Provider 子代理仍在生产 UI 中 |
| 数据与凭据 | 不符合方案 | Sidecar 直接打开并解密 `natives.db`，同时把执行表写进该库；`assistant.db` 尚未成为唯一执行存储 |
| UI / 发布验证 | 部分完成 | 主聊天已走 Gateway/Daemon；旧 Subagent 页面仍走独立 API，Profile/Hook 管理缺失，真实 Provider 与有头 GUI 未验收 |

## 公共接口与数据决策

- Protocol v2 继续作为唯一生产协议；补齐 `agent.list/get/upsert/delete`、`subagent.list`、`run.listChildren`、`permission.listPending`、`task.cancel`，不恢复 v1 双轨。
- `run.create/start` 增加并持久化 `agent_profile_id`、`resume_from`、`isolation_mode`；子代理必须由 Profile 提供明确的 `provider_id + key_id + model_id`。
- `RunEvent` 增加 `checkpoint_created`、`artifact_created`；所有事件递归脱敏、先持久化，并禁止终态后继续追加事件。
- `ToolManifest` 成为唯一工具声明，删除重复 `Tool` 元数据结构；`ToolHandler` 返回 Started/Progress/Completed 流，并接收统一取消令牌。
- Agent Profile 使用完整 YAML 解析，补齐 `hooks`、`toolRetryPolicy` 等原计划字段；Markdown 文件为权威来源，`assistant.db` 只保存解析缓存、诊断和绑定。
- 明确分库：
  - `natives.db`：Provider 配置和加密 Key，仅 Tauri 可访问。
  - `assistant.db`：Conversation、Message、Run、Event、Tool、Permission、Artifact、Profile、Subagent、Checkpoint、Extension、MCP、Skill、Hook。
  - Sidecar 只接收 `NATIVES_ASSISTANT_DB_PATH`，不再获得 `natives.db` 路径或 KEK。

## 实施变更

### 1. 先收口执行权威与安全边界

- Tauri 启动独立、鉴权的 Host Broker IPC；Daemon 按真实 `run_id` 请求单个凭据，Tauri 解密后一次性返回内存数据。删除 Sidecar 本地 `NativesDbBroker`。
- 将所有 Conversation、Run、Event、Permission、Prompt Queue、Artifact 请求交给 Daemon；Tauri 只保留生命周期、凭据、确认 UI和系统级能力。
- 统一 Run 终结逻辑：启动前失败、无凭据、Provider 错误、持久化错误、取消和超时都必须落入唯一确定终态；每个 Run 恰好一个终态事件。
- EventLog 在同一事务中维护 Tool Call、Permission、Artifact、Checkpoint 投影；失败或取消时也保存已有文本和结构化消息块。
- 增加幂等迁移：先备份两个数据库，再把当前 `natives.db` 中的 Daemon 执行表和旧 `assistant_sessions` 合并进 `assistant.db`；执行数据冲突时以当前 Daemon 记录为准，Key 永不复制。验证后删除旧执行表和旧 Subagent 表。

### 2. 补齐 Provider 与 Tool Runtime

- 使用精确 `ProviderProtocol` 选择 Adapter，移除字符串包含匹配和全局环境变量；OpenAI Chat/Responses 成为两个明确协议。
- 为所有已广告 Provider 实现真实模型发现、能力缓存、连接健康写回；认证错误不重试，429/5xx/网络错误遵守 `Retry-After` 和有上限退避，Context Length 触发压缩后单次重试。
- 执行前进行完整 JSON Schema 校验；无效参数作为结构化 Tool Result 回填模型，不执行副作用。
- Tool 取消必须终止子进程及其进程组；输出按 Manifest 安全截断并标记，不以超限错误丢弃全部结果。只并行执行 `parallel_safe` 工具，结果按原调用顺序回填。
- 修正安全边界：写入路径校验最近存在父目录和符号链接，文件写入原子化；Web/HTTP Hook 校验 DNS、全部解析地址和重定向；终端只执行程序加 argv。
- 完成占位工具：
  - `apply_patch` 执行真实 unified diff。
  - `skill` 加载已信任 Skill 全文。
  - `mcp_call` 只走 MCP Gateway。
  - `notification` 调用 Tauri Host Capability。
  - `web_search` 绑定受信任、声明搜索能力的 MCP 后端。
  - `lsp` 仅在项目配置语言服务器后广告。
  - 模块生成、Contract Linter、Artifact 工具通过 Host Capability 调用现有 Natives 实现。
- Tool 写文件时产生 `file_changed` 和 Artifact；Tool、Permission、Timeout、Cancel 均具备可重放事件。

### 3. 接通 Profile、Hook、Context 与恢复

- Profile 按“工作目录最近层级 → Git 根 → 项目 Plugin → 用户 → 内置”解析，同名高优先级覆盖；每次 Run 固化解析快照。
- Context 按顺序组装：安全基础 Prompt、Profile、根到当前目录的 AGENTS.md、选定 Skills、Plugin 指令、MCP Manifest、Memory、Conversation 历史、子代理结果。
- `promptMode` 只支持明确的 `append`/`replace`；`replace` 不能移除安全和权限约束。上下文预算取模型上限与 Profile `tokenBudget` 的较小值，80% 阈值触发压缩。
- 压缩保存真实结构化摘要和合法 Tool Call 配对；每轮 Tool 完成、压缩和终结前创建 Checkpoint，支持显式 `resume_from`，不静默重跑已完成副作用。
- Hook 配置统一解析 `.claude/settings.json`、各类 `hooks.json`、Plugin manifest 和用户目录；项目/Plugin Command Hook 默认禁用，用户按内容哈希授权后才能执行。
- 在 Session、Prompt、Tool、Permission、Subagent、Compact、Stop 全路径各触发一次 Hook；Pre/Post 的 deny/modify/inject/rewake 必须实际改变对应输入、结果或循环，异步 Hook 不得倒改已提交终态。

### 4. 让 Subagent 成为真实子 Run，并切换 UI

- `task` 只接受 `agent_profile_id`、任务、上下文模式和可选 `resume_from`；Provider、Key、Model、权限及工具白名单从 Profile 解析，禁止自动继承父 Key 或 Permission。
- 子代理通过 RunManager 创建持久化子 Run，记录 `parent_run_id`、深度、预算、工作目录、上下文快照和 Checkpoint；默认只继承父摘要。
- 实现总并发、最大深度、总 Token 预算、时限、嵌套任务、级联取消、TaskOutput、恢复及结构化完成结果。
- `worktree` 模式使用独立 detached Git worktree；失败或可恢复 Run 保留工作区，删除 Run 时再清理；非 Git 项目明确拒绝该模式。
- Extension Registry 将已信任 Plugin 的 Skill、Hook、MCP、Command/Tool 贡献真正注册进运行时，启停状态持久化；MCP OAuth 继续保持“已知但未广告”，不在本轮虚构实现。
- 将现有 Subagent 页面改为 Protocol v2 Agent Profile/子 Run 管理；手动执行 Profile 也走 `run.create/start`。删除 Tauri 中独立的阻塞式 Subagent HTTP 执行链。
- 主工作台只消费 v2 RunEvent 和持久化 Message Blocks；接入 Profile、Hook、Tool、Permission、子 Run、Checkpoint、Artifact 检视，删除未使用的重复 Inspector 和 Reducer。

## 测试与发布门禁

- 扩展现有验证脚本，静态禁止：Sidecar 访问 `natives.db`、旧 Subagent HTTP 执行、Tauri 本地 Run Authority、生产 Fixture fallback。
- 增加自动化覆盖：
  - 分库迁移、重复执行、冲突和回滚备份。
  - Broker 鉴权、真实子 `run_id` 绑定、并发 Run 不串 Key。
  - 缺凭据启动失败进入 `failed`，恰好一个终态，终态后无事件和副作用。
  - 所有事件字段递归脱敏。
  - Schema、符号链接逃逸、DNS/重定向 SSRF、进程树取消、输出截断和并行顺序。
  - Profile 完整 YAML、优先级、工具白名单和 Prompt Mode。
  - Hook 全生命周期、改写/阻断、授权、超时和失败策略。
  - Compaction、Checkpoint、崩溃恢复及 Tool Call 配对。
  - Subagent 多 Provider/Key/Model、嵌套、深度、并发、预算、Worktree、取消和恢复。
  - UDS/macOS/Linux 与 Windows Named Pipe 生命周期。
- 保持 `cargo check --workspace --all-targets`、`cargo test --workspace`、前端测试、TypeScript 和协议同步检查全绿。
- 新增 `--release-gate`：OpenAI Chat/Responses、Anthropic、Gemini、DeepSeek、Ollama、OpenAI-compatible 的真实或官方兼容端点验证，以及有头 GUI 的问答、工具、权限、取消、重连、Profile、Subagent 场景，任何 `not_run` 均阻止发布。
- 数据迁移演练、安全审计和旧链静态审计全部通过后，才删除兼容迁移代码并发布。

## 假设与默认值

- 继续以原方案的完整 Agent Runtime 范围为验收目标，不缩减权限、安全、凭据隔离、Hook 或 Subagent 能力。
- Profile Markdown 是声明权威，数据库仅作缓存和运行快照。
- 当前 `run.subscribe` 长轮询语义保留，只要满足低延迟、顺序、重放和取消不互相阻塞；不额外建设另一套推送协议。
- Embedded Authority 仅保留测试构建，正式构建强制 Sidecar。
- MCP OAuth 不属于原验收清单，保持明确 unsupported，待实际产品需求出现再增加。
