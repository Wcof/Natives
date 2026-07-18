# Native 执行引擎 — 生产闭环缺口清单

> 依据：ADR-0011（2026-07-17 审计）  
> 状态语义：`open` | `in_progress` | `done` | `wontfix` | `partial`  
> 完成定义：代码落地 + 针对该缺口的测试/验收 + 本表更新

## 总览

| 优先级 | ID | 标题 | 状态 | 阻塞生产 |
|--------|-----|------|------|----------|
| 1 | G1 | EngineMessage → ProviderMessage 结构化工具消息 | `done` | 是 |
| 2 | G2 | sidecar `run.start` 立即返回 + 后台执行 | `done` | 是 |
| 3 | G3 | 真 v2 RPC Envelope + 事件订阅 | `done`（poll；push 流可选增强） | 是 |
| 4 | G4 | Tauri ↔ sidecar UDS / 去掉共享假设 | `done`（+ sidecar natives.db Broker） | 是 |
| 5 | G5 | Gateway 强制 PathScope / PermissionClass / limits | `done` | 是 |
| 6 | G6 | 清理旧执行链与未使用代码 | `done`（旧执行文件物理删除；审计脚本硬门禁） | 否 |

### 次要缺口

| ID | 标题 | 状态 |
|----|------|------|
| M1 | `run.retry` 自动 start | `done` |
| M2 | EventSequencer 可选磁盘持久化 | `done`（`NATIVES_EVENT_LOG_DIR`） |
| M3 | 项目级 Hook 加载（`.claude/hooks.json`） | `done` |
| M4 | 子 Agent 复用完整生产 Hook | `done` |
| M5 | Subagent 工具 Schema 暴露 provider/key/model | `done` |
| M6 | 子 Agent 嵌套硬编码 `provider_id: "child"` | `done` |
| M7 | SearchFiles pattern 过滤 | `done`（G1 波次） |
| M8 | 能力声明与真实 RPC 方法表对齐 | `done`（G3） |

---

## G1 — 结构化工具消息转换 — `done`

见历史落地：`history_message_to_provider`、OpenAI/Anthropic/Gemini wire、`EngineMessage.tool_name`。

## G2 — sidecar run.start 非阻塞 — `done`

`start_detached` / `start_detached_global`；RPC 立即返回 Preparing。

## G3 — Protocol v2 — `done`（poll）

- 响应 `protocol_version = 2.0.0`
- `IMPLEMENTED_METHODS` 诚实能力声明
- 未实现方法 fail-closed
- `run.subscribe` = `subscribe_poll`（非阻塞批）

## G4 — 独立 Daemon 权威 — `done`

**落地**

- `src-agent-daemon/src/client.rs`：`DaemonClient`、`RunAuthorityMode`、`resolve_run_authority_mode`
- `src-tauri/src/daemon_authority.rs`：统一 façade（create/start/cancel/retry/replay/permission）
- `assistant_service` + `daemon/rpc_server` 全部走 façade
- Bootstrap 默认可多会话；`NATIVES_BOOTSTRAP_SINGLE_USE=1` 可强制单次
- 模式：
  - `NATIVES_DAEMON_MODE=embedded` — 进程内
  - `uds` / `sidecar` — 强制 UDS
  - `auto`（非 test 默认）— socket 存在且有 bootstrap 则 UDS，否则 embedded

```bash
export NATIVES_DAEMON_MODE=uds
export NATIVES_DAEMON_SOCKET=$XDG_RUNTIME_DIR/natives-agent.sock
export NATIVES_DAEMON_BOOTSTRAP=<token from daemon stdout>
```

## G5 — Gateway 权限强制 — `done`

- `CapabilityGateway::execute`：traversal / path scope / timeout / output_limit
- PermissionClass + SideEffect 双门
- `task` / `kill_task` / `task_output` 走权限主路径后特化执行
- `run_terminal` 要求 cwd；由工具层注入 project_root
- project_root 从 cwd / project_path 注入

## G6 — 旧链收敛 — `done`

- 已物理删除 `assistant_stream_proxy`、`agent_engine_bridge`、旧 `NativeRuntime`
  协调器及 AgentLoop/StreamProvider/ContextAssembler。
- Native Assistant 不再注册旧 runtime，也不再注册 `cancel_stream` / `runtime_list_catalog` IPC。
- `run.cancel`、会话删除统一走 Protocol v2 `ExecutionAuthority`。
- `scripts/daemon/audit-old-symbols.sh` 对退休文件、生产引用、旧执行调用点均硬失败。

## MCP OAuth browser/redirect — `wontfix`（当前版本明确不支持）

- `mcp.auth.oauthStart` 与 `mcp.auth.oauthCallback` 保留在目标方法目录，便于未来兼容；
  当前不加入 `IMPLEMENTED_METHODS`，不会被 Daemon 能力声明，也不会被 UI 当作可用能力。
- 当前可用认证契约仅是 `mcp.auth.set/status/clear` 的内存 bearer lease；浏览器、PKCE、loopback
  redirect、token exchange 必须在后续单独实现并补 fake OAuth server 契约后，才能把两个方法标为 implemented。
- 这不是“已实现 OAuth”的假绿，Verification 会断言两个 OAuth 方法处于 `Unsupported`。

---

## 验收命令

```bash
cargo test -p provider-adapters --lib tool_message
cargo test -p natives-agent-daemon --lib
cargo test -p capability-gateway
cargo test -p agent-core --lib
cargo check -p natives --lib
cargo test -p natives-agent-daemon --test live_engine_e2e dual_provider_engine_fixture_subagent
bash scripts/daemon/audit-old-symbols.sh
```

### 真网 E2E（可选，需 Key）

```bash
export NATIVES_LIVE_E2E=1
export NATIVES_TEST_OPENAI_KEY=...          # 勿提交仓库
export NATIVES_TEST_OPENAI_BASE=https://...
export NATIVES_TEST_MODEL=deepseek-v4-flash
export NATIVES_LIVE_PROVIDER_ID=openai_compatible
export NATIVES_TEST_SCRATCH=/tmp/natives-live-e2e

cargo test -p provider-adapters --test live_openai_compatible_e2e -- --nocapture --ignored
cargo test -p natives-agent-daemon --test live_engine_e2e -- --nocapture --ignored
```

**2026-07-17 真网结果（SenseNova 兼容端）**

| 用例 | 结果 |
|------|------|
| 文本流 `pong` | pass |
| 工具一轮 + 结构化 tool_call_id 回填 + 二轮总结 | pass（`echo_box` / `call_*`） |
| AgentEngine 文本 turn `ready` | pass |
| **AgentEngine 完整工具闭环** | pass：`list_dir` ToolCallCompleted + 二轮文本 |
| **Subagent `task` 真网** | pass：独立 `key_id=live-child-key`，子 run 完成输出 `subok` |

密钥仅 env 注入，未写入仓库。复现：

```bash
cargo test -p natives-agent-daemon --test live_engine_e2e live_subagent_task_completes -- --nocapture --ignored
```

## 变更日志

| 日期 | 变更 |
|------|------|
| 2026-07-17 | 审计落盘；G1 启动 |
| 2026-07-17 | G1–G3 完成；G4–G6 partial |
| 2026-07-17 | G4 façade + auto/uds；G5 权限主路径；M1–M8 关闭；G6 partial 保留 |
| 2026-07-17 | 真网 SenseNova/deepseek-v4-flash 文本+工具二轮 + Engine E2E 通过 |
| 2026-07-17 | 残留修复：`spawn_child_task` 生产 Hook；task/kill 不再绕过权限主路径；rpc 不可达模式/unused 清理 |
| 2026-07-17 | 审计 P0/P1：UDS natives.db Broker；UDS retry 单次 start；Run.project_path；persist-first；canonical path；Engine 工具真网 E2E |
| 2026-07-17 | **全量整改启动**：Phase 0 契约冻结（`NATIVE_ENGINE_FULL_REMEDIATION.md`）；v2 Envelope；`ExecutionAuthority`；Sidecar Supervisor 骨架；生产 UDS 无静默降级规则；env/启动脚本 |
| 2026-07-17 | 生产默认 `uds`（非 auto）；`mode_requires_uds` 测试；DaemonClient reconnect；Credential Lease+run_id 绑定；Hook 全事件+多路径；`mcp.list`/`scheduler.*` 真实 RPC 且能力声明 true；默认事件持久化 |
| 2026-07-17 | **残留推进**：MCP stdio 会话保活 + `tools/call` + `mcp.call` RPC；SSE `data:` 工具发现；trusted 本地 HTTP 放行；daemon 不打印 bootstrap；跨平台 start 脚本；controller 自动化（project_path/permission/cancel/enqueue 5/5）；`audit-old-symbols` PASS；UDS lifecycle PASS；Anthropic 第二 Provider **代码路径就绪**但真网失败（`ANTHROPIC_*` 指向 SenseNova 兼容网关，非 Anthropic Messages API）；tauri warnings 77→66；**DoD 仍未全量完成** |
| 2026-07-17 | **MCP 主链接入**：PermissionGatedTools.mcp_call 权限门+事件；mcp.liveness/reconnect；双 Provider fixture 子 Agent 完成（parent openai / child anthropic）；daemon lib 59 绿；warnings 46；DoD 仍未全量 |
| 2026-07-17 | Sidecar 真启动 PASS（sun_path 自动 `/tmp/nuds-*`）；gateway security 13；agent-core 41；warnings 41；live 复跑 NOT_FOUND（环境模型路径）；DoD 未全量 |
| 2026-07-17 | live 3/3 恢复；daemon lib **61**；warnings **2**；controller 8/8；fixture race 修复；DoD 未全量 |
| 2026-07-17 | MCP OAuth lease + SSE listener + mcp.auth.*; daemon-adapter 5 测; dual provider registry; daemon **63**; live 3/3; DoD 未全量 |
| 2026-07-17 | streamChat 客户端 fail-closed；Phase6 capability-admin；UI 20 测；env-lock 竞态修复 daemon 63；DoD 未全量 |
| 2026-07-17 | Settings **engine** UI 挂载 capability-admin；task fixture:true 稳定双 Provider；daemon 63×2/live 3/3；DoD 未全量 |
| 2026-07-17 | 收尾：旧链文件物理删除；OAuth 方法从 implemented 移除并固定 Unsupported；权限 waiter 先注册再发事件；持久化事件测试改唯一 run id；统一 runner/CI 通过；无 Provider 凭据故 live 记录 `not_run/credential_absent` |
