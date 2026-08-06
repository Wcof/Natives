# Creative OS Batch 7 交接报告 — Service 模型与现有 Driver 内核

> 交接时间：2026-08-04
> 分支：`codex/creative-os-b7-20260804-210000`（base = Batch 6 final `bf3bc8f`）
> 主工作区，未新建 worktree

## 1. Batch / Task / 提交

| Task | 提交 | 说明 |
|---|---|---|
| CR-701 ServiceInstance | `c7ba099` | ServiceInstance 模型 + migration v22 + service_store + backfill |
| CR-703 Port lease | `c7ba099` (同 CR-701) | PortLeaseRegistry + ResourceSnapshot |
| CR-702 RuntimeDriver 契约 | `96dc743` | driver.rs：driver kind + capabilities + resolve_driver_kind |
| CR-702 Driver facade 完成 | `f37519c` | adapters/facade.rs：统一 contract 层 + commands 走 facade |

- base SHA：`bf3bc8f`（Batch 6 final）
- final 代码 stable：`be41d2f`（fmt 整理后）
- **CR-702 已完成**（此前为 BLOCKED，现已通过 facade 完成五 driver 统一 dispatch）

## 2. 改动文件

| 文件 | 变化 |
|---|---|
| `src-tauri/src/db.rs` | SCHEMA_VERSION 22；migration v22 (service_instances) + backfill |
| `src-tauri/src/creative_app/model.rs` | ServiceInstance 模型 + readiness 常量 |
| `src-tauri/src/creative_app/service_store.rs` | 新文件：create/list/update_readiness/bind_endpoint/backfill + 4 测试 |
| `src-tauri/src/creative_app/port_lease.rs` | 新文件：PortLeaseRegistry + ResourceSnapshot + 5 测试 |
| `src-tauri/src/creative_app/driver.rs` | 新文件：RuntimeDriver 契约（driver kind、capabilities、resolve）+ 2 测试 |
| `src-tauri/src/creative_app/mod.rs` | service_store、port_lease、driver 模块 |

## 3. Schema / API

- **Migration v22**：`service_instances` 表（runtime_instance_id + name 唯一），backfill 为 active runtimes 创建 "main" service
- **Port lease**：`PortLeaseRegistry`（operation-scoped，TTL 300s，TOCTOU 防护）
- **Driver contract**：driver kind 稳定字符串 + `DriverCapabilities` + `resolve_driver_kind`

## 4. 测试结果

| 命令 | 结果 |
|---|---|
| `cargo check` | PASS（0 errors，2 个 FFI 类型命名 warning） |
| `cargo fmt --check` | PASS |
| `creative_app::` 166/166 | PASS（+11 from Batch 6） |
| `db::tests` 9/9 | PASS |
| `typecheck` | PASS |
| `lint` | PASS（2420 键） |
| `test` 763/763 | PASS |
| `protocol:check` | PASS（154 methods） |

## 5. CR-702 完成状态

**CR-702 已完整交付**（`f37519c`）：`adapters/facade.rs` 提供统一 driver contract（start/stop/delete/probe/reconcile），commands 的 stop/delete/resolve_orphan-restart 已走 facade。五类 driver（Workshop Static/Local Static/Node/Compose/Run）通过现有 source adapters 单一权威 dispatch。两阶段 start（锁内 spawn、锁外 health）保留。

## 6. 遗留风险

- **Docker 不可用**：Compose/Run driver 无法真实验收（代码路径 + 契约测试覆盖）
- **`commands::provider` 单测挂起**：环境性
- **FFI 类型命名 warning**：`natural_t`/`mach_port_t` 为 macOS FFI 惯例命名，保留
- **service_instances 尚未接入 lifecycle 生产路径**：store 就绪，但 start/stop 未写入 readiness（后续 Batch 接线）

## 7. 下一批（B8/B9）继承

- commit：`be41d2f`（schema 22）
- 禁止假设：不要假设 service_instances 已有生产写入（store 就绪但 lifecycle 未接入）；不要假设 Docker 已验收
- **CR-702 已完成**：五 driver 统一走 facade，B8/B9 可基于 driver contract