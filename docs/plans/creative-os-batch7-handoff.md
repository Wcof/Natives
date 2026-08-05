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

- base SHA：`bf3bc8f`（Batch 6 final）
- final 代码 stable：`5b8e850`（fmt 整理后）

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

## 5. ⚠️ CR-702 部分完成（明确 BLOCKED 项）

**CR-702 只完成了 RuntimeDriver 契约定义，未完成五类 driver 的 facade 迁移。**

- 已完成：driver.rs 的 driver kind 映射、DriverCapabilities、resolve_driver_kind 兼容逻辑
- 未完成：5 个现有 driver（Workshop Static/Local Static/Node/Compose/Run）迁移到统一 facade；`adapters/` 中 `spawn_start/await_ready/stop/delete` 仍直接调用 local/docker 具体实现
- **结论**：本批 CR-702 为 **BLOCKED**（非 PASS）。按计划 §14 门禁要求（"现有五类运行方式全部经统一 contract 通过 start/probe/stop/reconcile"），本批**不应宣告完整完成**。

按执行矩阵，Batch 7 的完成定义包含 CR-702 全量迁移。当前只交付了 CR-701 + CR-703 + CR-702 契约层。**后续 Agent 必须完成 CR-702 的五 driver facade 迁移后才能进入 Batch 8/9。**

## 6. 遗留风险

- **CR-702 未完成**：五 driver facade 迁移是 XL 工作量，需要 Senior Runtime Agent 按 driver 串行 commit
- **Docker 不可用**：Compose/Run driver 无法真实验收
- **`commands::provider` 单测挂起**：环境性
- **FFI 类型命名 warning**：`natural_t`/`mach_port_t` 为 macOS FFI 惯例命名，保留

## 7. 下一批（B8/B9）继承

- commit：`5b8e850`（schema 22）
- 禁止假设：不要假设 CR-702 已完整迁移（只有契约层）；不要假设 service_instances 已有生产写入（store 就绪但 lifecycle 未接入）
- **必须先完成 CR-702 的五 driver facade 迁移**，否则 Batch 8/9 无 driver contract 可依赖