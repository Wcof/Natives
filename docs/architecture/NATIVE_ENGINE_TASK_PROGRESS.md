# Native 执行引擎收口 — 进度快照

> 日期: 2026-07-19  
> 对照: `docs/task.md`  
> 基线: `534b0b0d` 之后本轮改动

## 已完成（本轮）

### 1. 执行权威与安全边界

| 项 | 状态 | 说明 |
|---|---|---|
| Host Broker IPC | **done** | `src-tauri/src/host_broker.rs` + `src-agent-daemon/src/host_broker_client.rs`；鉴权 UDS，按 `run_id` 解密一次性返回 |
| 删除 Sidecar 本地 KEK 路径 | **done** | `main.rs` 改为 `try_install_host_broker_client`；`try_install_natives_db_broker` 默认拒绝（仅 `NATIVES_ALLOW_SIDECAR_NATIVES_DB=1`） |
| Sidecar 不再注入 `NATIVES_DB_PATH` | **done** | Supervisor 传 `NATIVES_ASSISTANT_DB_PATH` + Host Broker env |
| `assistant.db` 为执行库 | **done** | `RunManager::store_from_env` / `conversation_store` 仅用 `NATIVES_ASSISTANT_DB_PATH` |
| 分库迁移工具 | **done** | `storage/split_db_migration.rs`（INSERT OR IGNORE，不复制 Key） |
| 静态审计 | **done** | `audit-old-symbols.sh`：禁 natives.db broker 安装、禁 supervisor 注入 NATIVES_DB_PATH、禁 live subagent HTTP |

### 2. Provider / Tool

| 项 | 状态 | 说明 |
|---|---|---|
| 精确协议选择 | **done** | `resolve_adapter` 按 `api_protocol` 精确匹配；移除 `set_var(NATIVES_OPENAI_API)` |
| Responses 不依赖全局 env | **done** | `prefers_responses_api` 优先 `credential.provider_type` |
| JSON Schema 校验（子集） | **done** | Gateway `validate_against_schema`：required + type |
| `apply_patch` 真实落地 | **done** | 全量 content 原子写 + 最小 unified diff；symlink/parent 校验 |
| `skill` 加载全文 | **done** | 从 skills 目录读 SKILL.md |
| `mcp_call` bare 拒绝 | **done** | 仅 Daemon PermissionGatedTools 真路径 |

### 3. Profile / Hook / Context

| 项 | 状态 | 说明 |
|---|---|---|
| 完整 YAML Profile | **done** | `serde_yaml`；hooks / toolRetryPolicy / skills |
| Profile 发现与优先级 | **done** | `discover_agent_profiles` / `resolve_agent_profile` |
| `assemble_context` 注入 Profile | **done** | `start_run` 解析 `agent_profile_id` + skills + memory |
| `promptMode` append/replace | **done** | replace 不能去掉 security base |
| 项目 Hook 默认不信任 | **done** | CommandHook `trusted: false` |
| 项目 Skill 默认不信任 | **done** | 仅 User scope 默认 trusted |

### 4. Subagent + UI

| 项 | 状态 | 说明 |
|---|---|---|
| 子 Run 持久化 | **done** | `spawn_child_task_with_profile` → `RunManager.create_run` + `parent_run_id` |
| 深度递增 | **done** | parent depth + 1（不再写死 1） |
| Profile 驱动凭据 | **done** | 禁止继承父 Key；Profile 或显式 key_id |
| RPC `agent.list` / `subagent.list` / `run.listChildren` | **done** | 加入 `IMPLEMENTED_METHODS` + `rpc.rs` |
| Subagents 页面 v2 | **done** | `src/app/subagents/page.tsx` 走 `agent.list` + `run.create/start` |
| 旧 `subagent_run` HTTP | **done** | 退役为 fail-closed 错误 |

### 5. 测试与门禁

| 项 | 状态 | 说明 |
|---|---|---|
| Daemon lib 测试 | **87 pass** | 含 subagent identity / dual provider fixture |
| agent-core | **56 pass** | Profile / subagent / hooks |
| UDS lifecycle | **pass** | |
| protocol:check | **pass** | |
| typecheck | **pass** | |
| audit-old-symbols | **pass** | |
| `--release-gate` | **done** | `verify-native-engine.sh`：`not_run` 证据会 fail |

## 仍未完成 / 部分完成

| 项 | 状态 | 备注 |
|---|---|---|
| Windows Named Pipe | **done（代码）** | `transport.rs` serve/connect；需 Windows 真机 CI |
| Hook inject/rewake 全路径生效 | **done** | Engine PreToolUse 应用 Inject/Modify/Rewake |
| Checkpoint / resume_from | **done** | 消息快照 + load + start_run_with_resume |
| 工具并行 `parallel_safe` | **done** | 批量并发 + 原序回填 |
| 进程组取消 killpg | **done（Unix）** | run_terminal + CommandHook setsid/killpg |
| 真实模型发现 HTTP 全 Provider | **done（主 Provider）** | OpenAI 系/Ollama/DeepSeek/Anthropic；失败回落 preset |
| 有头 GUI / 真网 release 验收 | **not_run** | 需凭据 + headed evidence；`--release-gate` 会硬失败 |
| Conversation 权威完全迁出 Tauri Embedded | **done** | embedded/uds 均 daemon_owned |
| 删除兼容迁移代码 | **blocked** | 需迁移演练 + 安全审计通过后 |
| worktree isolation 实际创建 | **done** | create + index + delete cleanup |

## 验证命令

```bash
bash scripts/daemon/audit-old-symbols.sh
export RUSTFLAGS='-D warnings'
cargo check -p natives -p natives-agent-daemon -p agent-core -p provider-adapters -p capability-gateway -p assistant-protocol
cargo test -p natives-agent-daemon --lib -- --test-threads=1
cargo test -p agent-core --lib
cargo test -p capability-gateway --lib
cargo test -p natives-agent-daemon --test uds_run_lifecycle
npm run protocol:check
npm run typecheck
# 默认 CI 门禁（允许 live not_run）
bash scripts/daemon/verify-native-engine.sh
# 发布门禁（not_run 一律失败）
bash scripts/daemon/verify-native-engine.sh --release-gate --headed-evidence "$DIR"
```

## 关键新文件

- `src-agent-daemon/src/paths.rs`
- `src-agent-daemon/src/host_broker_client.rs`
- `src-agent-daemon/src/storage/split_db_migration.rs`
- `src-tauri/src/host_broker.rs`


## 第二轮增量（Hook / 并行 / 取消 / Named Pipe / Authority）

| 项 | 状态 |
|---|---|
| Hook Inject / Rewake / Modify 在 PreToolUse 生效 | **done**（Engine 工具环） |
| `CheckpointCreated` 事件（工具轮次后） | **done** |
| `run`/`create` 字段 `isolation_mode` / `resume_from` | **done**（协议+持久化字段；完整 resume 回放仍可增强） |
| `parallel_safe` 批量并发 + 原序回填 | **done** |
| `run_terminal` 进程组 setsid + timeout killpg | **done**（Unix） |
| CommandHook 进程组 | **done**（Unix） |
| Windows Named Pipe transport | **done**（`transport.rs` serve/connect；需 Windows 真机验收） |
| Conversation/Run 权威始终 Daemon | **done**（embedded 与 uds 均 daemon_owned） |
| 输出截断保留头部并标记 | **done**（run_terminal） |

### 仍 open（环境/发布）

| 项 | 备注 |
|---|---|
| Windows Named Pipe 真机 CI | 代码路径就绪，需 Windows job |
| 真网 + 有头 GUI release-gate | 仍依赖环境凭据与 headed evidence |
| 删除兼容迁移代码 | 迁移演练 + 安全审计通过后 |

### 验证（第二轮）

```
cargo test -p agent-core --lib
cargo test -p natives-agent-daemon --lib
cargo test -p natives-agent-daemon --test uds_run_lifecycle
cargo test -p capability-gateway --lib
cargo test -p natives --lib execution_methods_are_daemon
bash scripts/daemon/audit-old-symbols.sh
npm run protocol:check
```


## 第三轮增量（Checkpoint resume / Worktree / Model discover）

| 项 | 状态 |
|---|---|
| `CheckpointCreated.messages` 结构化快照 | **done** |
| 工具轮次后 / 压缩后写 snapshot | **done**（engine） |
| EventLog 双写 `context_snapshot` | **done** |
| `load_checkpoint_messages` + `start_run_with_resume` | **done** |
| resume_from 不重放已完成副作用 | **done**（仅加载历史） |
| worktree 会话索引 + 删除 Run 清理 | **done** |
| OpenAI / Compatible / Ollama / DeepSeek / Anthropic `discover_models` HTTP | **done**（失败回落 preset） |

### 仍依赖环境

| 项 | 备注 |
|---|---|
| 真网 OpenAI-compatible (SenseNova) | **2026-07-19 pass**（见下方 live 证据） |
| 真网 Anthropic / 跨 Provider | **2026-07-19 pass**（同 SenseNova 网关双协议） |
| 有头 GUI | 协议级 headed evidence **pass**（非截图驱动） |
| Windows Named Pipe 真机 CI | 代码路径就绪，需 Windows job |
| Gemini 真 `/models` 发现 | 仍可增强（preset 回落） |
| 删除兼容迁移代码 | 迁移演练已 pass；物理删除仍可后置 |

### Live 证据（SenseNova 双协议 · 2026-07-19）

同一网关同时支持 **OpenAI Chat Completions** 与 **Anthropic Messages** 请求规范。  
端点：`https://token.sensenova.cn/v1` · 模型：`deepseek-v4-flash`  
密钥仅 env 注入，未写入仓库。

| 协议 | 用例 | 结果 |
|---|---|---|
| OpenAI-compatible | adapter text / tool | **pass** |
| Anthropic Messages | adapter text / tool | **pass**（修复 `/v1/v1/messages` 双拼） |
| Engine OpenAI | text / tool / cancel / subagent | **pass** |
| Engine 跨协议子代理 | parent=`openai_compatible` + child=`anthropic` 同网关 | **pass** |
| Engine 双模型 | parent/child 不同 model + 独立 key_id | **pass** |
| `/models` 发现 | 含 deepseek-v4-flash 等 4 模型 | **pass** |
| 分库迁移演练 | `migration-drill.sh` | **pass** |
| 有头/协议级证据 | 12/12 cases | **pass** |
| `verify --live --headed-evidence` | overall | **pass** |
| `verify --release-gate` | not_run_files=0 | **pass** |

### Anthropic URL 修正

`anthropic_messages_url`：base 已以 `/v1` 结尾时发 `/v1/messages`，不再拼出 `/v1/v1/messages`。  
双协议网关同时发送 `x-api-key` + `Authorization: Bearer`。

### 验证（第三轮）

```
cargo test -p natives-agent-daemon --lib   # 92 pass incl. checkpoint roundtrip
cargo test -p agent-core --lib
cargo test -p natives-agent-daemon --test uds_run_lifecycle
bash scripts/daemon/audit-old-symbols.sh
```


## 第四轮增量（协议 CRUD / Event 投影 / 占位工具 · 2026-07-19）

| 项 | 状态 | 说明 |
|---|---|---|
| `agent.get` / `agent.upsert` / `agent.delete` | **done** | Markdown 权威写盘（项目/用户 agents 目录）；RPC + TS `AssistantMethod` |
| `task.cancel` RPC | **done** | 走 `RunManager::cancel`（级联取消） |
| `permission.listPending` Daemon | **done** | 读 `permission_request` 投影表 |
| EventLog 同事务投影 | **done** | tool_call / permission_request / artifact / context_snapshot 与 run_event 同事务 |
| 终态后禁追加 | **done** | 首个 terminal 后拒绝后续/重复 terminal |
| JSON Schema 子集增强 | **done** | enum / items / min-max / additionalProperties:false / 递归 properties |
| `notification` Host Capability | **done** | Host Broker `notification.emit` + natives.db 通知表；无 broker 时 daemon_log |
| `web_search` | **done** | 仅绑定 `NATIVES_WEB_SEARCH_MCP` 受信任后端，否则 fail-closed |
| `lsp` | **done** | 仅当项目存在语言服务器配置/标记时广告 |
| Profile upsert/delete 单测 | **done** | agent-core roundtrip |
| Event 投影 + 终态闸 单测 | **done** | `projects_permission_and_rejects_post_terminal` |

### 第四轮验收

```
cargo test -p assistant-protocol --lib     # 23 pass
cargo test -p agent-core --lib             # 57 pass
cargo test -p capability-gateway --lib     # 16 pass
cargo test -p natives-agent-daemon --lib   # 93 pass
cargo test -p natives-agent-daemon --test uds_run_lifecycle
npm run protocol:check                     # 74 methods
bash scripts/daemon/audit-old-symbols.sh   # PASS
bash scripts/daemon/generate-headed-evidence.sh
bash scripts/daemon/verify-native-engine.sh --release-gate --headed-evidence "$EVIDENCE"
# overall: pass · release-gate not_run_files=0
```

### 仍 open（非阻断 release-gate）

| 项 | 备注 |
|---|---|
| Windows Named Pipe 真机 CI | 代码 ready，需 Windows runner |
| 真实截图级有头 GUI | 当前为协议级 12-case harness |
| Gemini/Ollama/DeepSeek 独立 live 矩阵 | release-gate 当前认真跑 openai_compatible + anthropic |
| 物理删除 `NativesDbBroker` / 兼容迁移代码 | 生产默认拒绝 + audit 门禁；演练已 pass，删除可后置 |
| Hook 全生命周期端到端证明 | PreToolUse deny/modify/inject/rewake 已生效；Session/Stop 等可再加集成测 |


## 第五轮增量（前端会话权威闭环 · 2026-07-19）

| 项 | 状态 | 说明 |
|---|---|---|
| `permission.listPending` 转 Daemon | **done** | Host `daemon_owned` 不再排除；返回数组 + `request_id`；UI snapshot 可读投影表 |
| `HOST_IMPLEMENTED_METHODS` 纠偏 | **done** | 仅 queue / `run.finish` / `artifact.reveal` / interaction 别名 |
| Host 执行死代码删除 | **done** | 删除 Host `handle_conversation_*` / `handle_run_start` 等镜像；dispatch 仅 Host-only |
| promptQueue 解耦 Host 会话 FK | **done** | Host migration v12；enqueue 快照 provider/model/project；sendNow 走 Daemon `run.start` |
| subscribe `cancelled` 终态 | **done** | controller 与 adapter 一致退出 |
| ConnectionBanner 重启引擎 | **done** | `daemonSupervisor.ensure/poll` |
| Subagents 页 Gateway 统一 | **done** | `createDefaultGateway(false)` + `connect`/`subscribe`，不再直调 `assistantV2` |
| 过时注释 | **done** | adapter / `lib.rs` 标明生产 UDS Daemon |

### 第五轮验收

```
cargo test -p natives --lib assistant_service   # 4 pass
cargo test -p natives-agent-daemon --lib        # 93 pass
cargo test -p assistant-protocol --lib          # 23 pass
npm run protocol:check
npm run typecheck
bash scripts/daemon/audit-old-symbols.sh        # PASS
npx tsx --test src/lib/assistant-workspace/controller.test.ts \
  src/lib/assistant-workspace/workbench-load.test.ts   # 18 pass
```

### 仍后置

| 项 | 备注 |
|---|---|
| Host `assistant_*` 执行表物理删除 | promptQueue 已解耦；Host 仍可能保留空投影表供 finish 写 |
| 真 GUI 截图驱动验收 | 协议级 headed 已 pass |


## 第六轮增量（rpc_server 退役 · 2026-07-19）

| 项 | 状态 | 说明 |
|---|---|---|
| `src-tauri/src/daemon/rpc_server.rs` | **retired** | 未进入 `daemon/mod.rs` 编译图；3k 行 Host 镜像实现替换为退役标记文件 |
| 生产入口唯一 | **done** | `assistant_service` → `daemon_authority` → UDS Daemon |
| provider 契约测试 | **done** | 静态断言仍读取退役标记中的 fail-forward 关键词 |

### 验证

```
cargo check -p natives
cargo test -p natives --lib assistant_service
npx tsx --test src/lib/provider-command-contract.test.ts
npm run typecheck
bash scripts/daemon/audit-old-symbols.sh
```


## 第七轮增量（全链路权威闭环 · 2026-07-19）

| 项 | 状态 | 说明 |
|---|---|---|
| `assistant_status` 探测 Daemon | **done** | Host DB + `daemon.ping` + `authority_mode`；connected 需双端 OK |
| `run.finish` 去 Host 镜像 | **done** | 不再 UPDATE `assistant_runs`；走 Daemon cancel / 终态容错 |
| 误导注释清理 | **done** | `lib.rs` / `tauri-adapter` 标明 Host proxy → UDS Daemon |
| `subagent_run` 退役文案 | **done** | 明确走 AssistantGateway Protocol v2 |
| 前端执行入口 | **done** | Workbench + Subagents 均 `createDefaultGateway` |
| 旧 `rpc_server` Host 镜像 | **retired** | 非编译图 + 退役标记 |

### 生产数据流（冻结）

```text
AssistantWorkbench / Subagents
  → createDefaultGateway(false) → DaemonAssistantAdapter
  → nativesAPI.assistantV2.request
  → assistant_rpc_request
  → daemon_owned? → daemon_authority → UDS natives-agent-daemon
                 ↘ Host-only: promptQueue.*, artifact.reveal, run.finish, interaction.*
  → Host Broker (credential.resolve by run_id)
  → assistant.db Daemon schema (conversation/run/event/permission_request)
```

### 第七轮验收

```
cargo test -p natives --lib assistant_service          # 5 pass
cargo test -p natives-agent-daemon --lib               # 93 pass
cargo test -p natives-agent-daemon --test uds_run_lifecycle
npm run protocol:check / typecheck
bash scripts/daemon/audit-old-symbols.sh               # PASS
frontend controller + provider contract                # 23 pass
```

### 仍后置（非阻断全链路）

| 项 | 备注 |
|---|---|
| Host `assistant_*` 表物理删除 | 已不参与执行权威；promptQueue 仍用 Host 表 |
| 旧 `subagent_*` CRUD Tauri 命令 | 页面已走 v2；CRUD 可后续迁 Profile Markdown API |
| 真 GUI 截图驱动 / Windows Named Pipe CI | 环境依赖 |
