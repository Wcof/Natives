# Native Agent Daemon — 环境变量与运维契约

> 与全量整改方案 §四 / Phase 2 对齐。  
> 权威进度：`NATIVE_ENGINE_FULL_REMEDIATION.md`

## 必读变量

| 变量 | 必填（生产） | 默认 | 说明 |
|------|-------------|------|------|
| `NATIVES_DAEMON_MODE` | 是 | **非测试默认 `uds`**；`cfg(test)`→`embedded` | `uds`/`sidecar`/`remote` 强制 UDS（**无静默 Embedded**）；`embedded` 仅测试/显式开发；`auto` 仅显式开发 |
| `NATIVES_DAEMON_SOCKET` | 是（UDS） | `$XDG_RUNTIME_DIR/natives/natives-agent.sock` 或 `~/.natives/runtime/…` | Unix Domain Socket 路径；**macOS sun_path ≈104 字节**，过长时脚本/daemon 自动缩短为 `/tmp/nuds-*.sock` |
| `NATIVES_DAEMON_BOOTSTRAP` | 是（UDS） | Supervisor 生成 | 握手 bootstrap；**禁止**写入普通日志 |
| `NATIVES_DB_PATH` | 是 | `~/.natives/natives.db` | Provider 密钥库；**绝对路径**；Daemon 不直读密钥表 |
| `NATIVES_RUNTIME_DIR` | 推荐 | `~/.natives/runtime` | socket / pid / lock / bootstrap 文件根 |
| `NATIVES_EVENT_LOG_DIR` | 过渡可选 | 无 | 可选 JSONL；终态默认持久化，不依赖此变量 |
| `NATIVES_REQUIRE_UDS` | 推荐生产 | 未设置 | `1`/`true` 时缺省模式视为 `uds`，禁止静默 Embedded |
| `NATIVES_DAEMON_BIN` | Supervisor | `natives-agent-daemon` | Sidecar 可执行文件 |

## 生产规则

```text
NATIVES_DAEMON_MODE=uds
NATIVES_REQUIRE_UDS=1
```

**禁止**：UDS 失败 → 静默切回 Embedded。  
必须：UI 显示 `SupervisorState::Faulted`，用户明确知道生产引擎不可用。

## Credential 路径

```text
Agent Daemon
  └─ credential.resolve(provider_id, key_id, run_id)
        ↓ 本机 IPC / 进程内 inject（embedded）或 natives.db sidecar broker
Tauri Credential Broker
  └─ 读 NATIVES_DB_PATH → 解密 → 短生命周期 Credential（内存）
```

- Key 不得进入事件、日志、错误、前端响应  
- 子 Agent 不得自动继承父 Key  

## 启动脚本

见：

- `scripts/daemon/start-daemon-macos.sh`
- `scripts/daemon/start-daemon-linux.sh`
- `scripts/daemon/start-daemon-windows.ps1`

脚本行为：

1. 创建 `NATIVES_RUNTIME_DIR`（默认 `~/.natives/runtime`，Linux 优先 `$XDG_RUNTIME_DIR/natives`）
2. 生成或读取 `NATIVES_DAEMON_BOOTSTRAP`，写入 `bootstrap.token`（模式 `0600`）
3. 通过 **环境变量** 启动 daemon（`NATIVES_DAEMON_SOCKET` / `NATIVES_DAEMON_BOOTSTRAP` / `NATIVES_DB_PATH`）
4. **禁止** 将 bootstrap 打印到 stdout/日志；daemon 自身也只打印 `Bootstrap: from NATIVES_DAEMON_BOOTSTRAP` 状态字样
5. 轮询 socket 就绪；pid 写入 `natives-agent.pid`

真网测试环境变量（可选，仅本地/CI 秘密注入）：

| 变量 | 说明 |
|------|------|
| `NATIVES_TEST_OPENAI_KEY` / `NATIVES_TEST_OPENAI_BASE` | OpenAI / OpenAI-compatible / SenseNova |
| `NATIVES_TEST_ANTHROPIC_KEY` 或 `ANTHROPIC_API_KEY` / `ANTHROPIC_AUTH_TOKEN` | Anthropic 适配器（可选 `ANTHROPIC_BASE_URL`） |
| `NATIVES_LIVE_E2E=1` | 启用 `live_engine_e2e` ignored 测试 |
| `NATIVES_LIVE_PROVIDER_ID` | 如 `openai_compatible` / `anthropic` |

## 健康检查

```bash
# socket 存在且可握手
test -S "$NATIVES_DAEMON_SOCKET"
# 或 RPC daemon.ping（需 session）
bash scripts/daemon/start-daemon-macos.sh   # 或 linux
```

## 统一验收与证据

```bash
npm run verify:native-engine -- --scratch /tmp/natives-native-engine-verification
# 有凭据时才启用真网；缺少凭据会明确写入 not_run/credential_absent
npm run verify:native-engine -- --live --scratch /tmp/natives-native-engine-verification-live
```

Runner 会执行旧链审计、Protocol/前端/Rust 测试、UDS 生命周期、双 Provider
fixture 子 Agent 以及 OAuth unsupported 契约，并在 scratch 目录写入脱敏 JSON/日志。
不会写入 API key、bearer token 或完整 Provider 响应。

## 数据库

- 路径：`NATIVES_DB_PATH`（绝对）  
- 启动校验：存在性、文件权限、后续 schema/WAL（Phase 2 硬化）  
- 迁移 / 备份：`scripts/daemon/natives-db-migrate.sh`
