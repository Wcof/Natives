# Native 执行引擎整改基线（Phase 0）

> 冻结日期：2026-07-17  
> 分支：`codex/settings-shell-redesign`  
> 参考源：`/Users/ldh/Downloads/project/grok-build`  
> 证据目录：goal scratch `implementer/`

## 1. 工作区状态（不得覆盖）

用户在途修改（必须保留）：

- 助理会话名颜色、流式位置、停止、思考阶段标题等 UI 修复
- `AssistantWorkbench` / `AssistantSidebarSection` / blocks / message-view 等

执行引擎整改在上述修改之上叠加，禁止 reset/checkout 用户文件。

## 2. 已知缺口（实时）

> **2026-07-17 审计校准（ADR-0011）**：不得标记「全量整改完成」。  
> 权威缺口表：`docs/architecture/NATIVE_ENGINE_GAP_CHECKLIST.md`（G1–G6 + M*）。

| ID | 问题 | 当前状态 |
|---|---|---|
| K1 / G4 | Daemon 非执行权威 | **部分完成**：嵌入式 `RunManager`；独立 sidecar 与 Tauri **不共享** `OnceLock` RunManager |
| K2 / G1 | Provider 工具闭环 | **已修**；真网 SenseNova/`deepseek-v4-flash` 文本+工具二轮 E2E 已通过（2026-07-17） |
| K3 / G6 | 双执行链 | **入口硬切断 + deprecated**；dead_code warnings 仍多 |
| K4 / M3 | Hook | **已修**：env + `.claude/hooks.json` command hooks |
| K5 / M4–M6 | Subagent | **已修**：Schema / 真实 provider_id / 生产 Hook |
| K6 / M1 | `run.retry` | **已修**：retry 后 detached start |
| K7 / M2 | 事件 | 内存 + 可选 `NATIVES_EVENT_LOG_DIR` jsonl |
| G2 | sidecar RPC 阻塞 | **已修** |
| G3 | Protocol v2 | **已修**（poll）；v1 Rpc 类型壳兼容保留 |
| G4 | 独立 Daemon | **已修**：`daemon_authority` façade；auto/uds/embedded |
| G5 | Gateway 权限强制 | **已修**：execute + PermissionClass + task 主路径 |

## 3. 真实 Provider 测试凭据约定

**禁止写入仓库。** 本地可选：

```bash
export NATIVES_TEST_OPENAI_KEY=sk-...
export NATIVES_TEST_OPENAI_BASE=https://api.openai.com/v1
export NATIVES_TEST_ANTHROPIC_KEY=sk-ant-...
export NATIVES_TEST_DEEPSEEK_KEY=...
export NATIVES_TEST_GEMINI_KEY=...
export NATIVES_TEST_OLLAMA_BASE=http://127.0.0.1:11434
export NATIVES_TEST_SCRATCH=/path/to/scratch   # dumps run-events.json
```

CI / 默认：fixture SSE 解析 + EchoProvider 闭环。

## 4. 架构（目标 = 进行中）

权威全量方案与 DoD：`docs/architecture/NATIVE_ENGINE_FULL_REMEDIATION.md`  
环境变量：`docs/architecture/NATIVE_ENGINE_ENV.md`

```
UI ── Protocol v2 ──► ExecutionAuthority (Embedded | Uds)
                         │
         Embedded ───────┼── global_run_manager (tests/dev only)
         Uds ────────────┴── DaemonClient → Agent Daemon
                                                   ├─ agent-core AgentEngine
                                                   ├─ provider-adapters (real HTTP)
                                                   ├─ capability-gateway tools
                                                   └─ Credential Broker (lease; no long-lived key)
SidecarSupervisor ── spawn / health / restart ──► natives-agent-daemon
```

**生产禁止**：UDS 失败静默降级 Embedded。

## 5. 契约测试（Phase 0 清单）

- [x] Protocol v2 方法表 / RunEvent 序列化 / 脱敏
- [x] Run 状态机 + Engine 文本/工具轮
- [x] Provider SSE：OpenAI / Anthropic / Gemini
- [x] Hook SSRF / untrusted command deny
- [x] Subagent 身份隔离 + cascade cancel
- [x] RunManager create/retry/start/replay
- [x] UI cutover：无 streamChat 启动路径

## 6. 已落地路径速查

| 模块 | 路径 |
|---|---|
| Protocol v2 | `crates/assistant-protocol/src/v2/` |
| Agent Engine | `crates/agent-core/src/engine.rs` |
| Hooks + Command/HTTP | `crates/agent-core/src/hooks.rs`, `hook_handlers.rs` |
| Subagent identity | `crates/agent-core/src/subagents.rs` |
| Providers | `crates/provider-adapters/src/{stream,providers,http_stream}.rs` |
| Tools | `crates/capability-gateway/src/{tools,manifest}.rs` |
| Bridge | `src-tauri/src/agent_engine_bridge.rs` |
| Credential Broker | `src-tauri/src/credential_broker.rs` |
| Daemon RunManager | `src-agent-daemon/src/run_manager.rs` |
