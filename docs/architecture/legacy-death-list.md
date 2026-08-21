# Legacy Removal Death List（ADR-0020 §7 死亡证明）

> **状态**: 执行中（2026-08-20）
> **依据**: ADR-0020 §7「Legacy 删除必须有引用清单、迁移/回滚证据与 death proof」
> **原则**: 删除前必须先完成替代路径与数据保护；禁止「先 rm -rf 再修编译」；
> 禁止仅删除 crate 留下前端 dead UI 或旧表写路径。

## 1. 已确认死亡并可安全删除（无任何生产引用）

| 路径 | 引用证据 | 处置 | 状态 |
|---|---|---|---|
| `examples/minimal-agent/` | 仅 `crates/agent-core/src/facade.rs:16` 注释提及；非 workspace 成员；无 CI/脚本引用 | DELETE | 本阶段删除 |
| `extension-host/` | 全仓库 grep 仅文档/审计/`architecture-check.mjs` 提及；无 `src/`、`src-tauri/`、`package.json`、CI 生产引用 | DELETE | 本阶段删除 |

## 2. 待 Host ProxyEngine cutover 后删除（当前仍为生产路径）

> AGENTS.md / ADR-0020：当前 Provider 执行仍走 `Renderer → Host → UDS → Agent Daemon`
> 为 current production fact。ADR-0020 P0 parity（三协议 fixture、stream lifecycle、
> OAuth、Key Pool、Secret migration）通过后一次切到 Host，再同批删除 Daemon caller/fallback。

| 路径 | 现状 | 删除前置条件 |
|---|---|---|
| `src-agent-daemon/**` | 生产执行器（`routing.rs` 等），`production.rs:588` 仍使用 | P0 parity + Host ProxyEngine cutover |
| `crates/agent-core/**` | Daemon 运行依赖；`facade.rs` 注释引 minimal-agent | Daemon 删除后 |
| `crates/harness-core/**` | agent-core 依赖 | 同上 |
| `crates/assistant-protocol/**` | `src-tauri/Cargo.toml`、`provider-adapters/Cargo.toml` 生产依赖（wire types） | 新边界类型接管后 |
| `crates/capability-gateway/**` | Daemon/legacy 依赖链 | 同上 |
| `crates/contract-linter/**` | `src-tauri/Cargo.toml` 生产依赖 | 旧 Module/Workshop 移除后 |
| `src/components/assistant/**`、`jobs/**`、`capabilities/**`、`library/**` 前端 | Shell/MainContent 仍有路由分支 | 新 IA 入口替代后 |
| `src-tauri/src/assistant_service/`、`runtime/{claude_cli,codex_cli}.rs` 等 AgentRuntime wrapper | 旧执行语义 | AI Tool Integration adapter 接管后 |

## 3. 本阶段已完成

- `examples/minimal-agent/` 删除（无生产引用，仅 legacy 注释提及）。
- `extension-host/` 删除（无生产引用；Plugin Runtime 语义由 ADR-0020 冻结）。
- 删除后 `architecture-check.mjs` 的 `LEGACY_FROZEN_DIRS` 保留条目无害（walk 跳过缺失目录）。

## 4. 验收

- `rtk cargo check --workspace` 通过（删除不破坏编译）。
- `rtk npm run typecheck` 通过（前端无 extension-host 引用残留）。
- 剩余第 2 节路径保持原状，等待 P0 parity cutover（跨模块拉通阶段）。
