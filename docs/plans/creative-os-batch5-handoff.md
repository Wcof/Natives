# Creative OS Batch 5 交接报告 — Surface、Endpoint 与多窗口 Shell

> 交接时间：2026-08-04
> 分支：`codex/creative-os-b5-20260804-204021`（base = Batch 4 stable `221680a4`）
> 主工作区，未新建 worktree

## 1. Batch / Task / 提交

| Task | 提交 | 说明 |
|---|---|---|
| CR-501 Surface/Endpoint/Window schema | `f7544ce` | 三张 additive 表 + migration v19 + backfill + store + API |
| CR-502 WebView backend/Window registry | `08c74d1` | window.rs 模块：Tauri WebView ↔ WindowInstance 映射 |
| CR-503 Dock/Task Switcher | `c3f6d8d` | CreativeDock 组件 + i18n |

- base SHA：`221680a4`（Batch 4 final）
- final 代码 stable：`782ab84`（fmt 整理后）

## 2. 改动文件

10 个文件，+1034 / -65：

| 文件 | 变化 |
|---|---|
| `src-tauri/src/db.rs` | SCHEMA_VERSION 19；migration v19 创建三张表 + backfill 调用 |
| `src-tauri/src/creative_app/model.rs` | 新增 `ApplicationSurface`、`RuntimeEndpoint`、`WindowInstance` 模型 |
| `src-tauri/src/creative_app/surface_store.rs` | 新文件：create/list/update/delete + backfill v19 + 7 测试 |
| `src-tauri/src/creative_app/window.rs` | 新文件：open_window/close_window/minimize_window/restore_window + 3 测试 |
| `src-tauri/src/creative_app/mod.rs` | 增加 `surface_store`、`window` 模块 |
| `src-tauri/src/commands/creative_app.rs` | 6 个新 Tauri commands：surface_list/window_list/open/close/minimize/restore |
| `src-tauri/src/lib.rs` | 注册新 commands |
| `src/lib/tauri-adapter.ts` | `CreativeAppSurface`、`CreativeAppWindow` 类型 + adapter 方法 |
| `src/components/creative/CreativeDock.tsx` | 新组件：活动应用 dock 栏 |
| `src/i18n/en.ts` + `zh.ts` | 新增 `actionClose`、`dockLabel` 键 |

## 3. Schema / API

- **Migration v19**：`application_surfaces`、`runtime_endpoints`、`window_instances` 三张表（additive，CASCADE 外键）
- **Backfill**：自动为无 surface 的 application 创建 main surface；从活跃 runtime 的 preview_targets 回填 endpoint
- **API**：`creative_app_surface_list`、`creative_app_window_list`、`creative_app_window_open`、`creative_app_window_close`、`creative_app_window_minimize`、`creative_app_window_restore`

## 4. 测试结果

| 命令 | 结果 |
|---|---|
| `cargo check -p natives --lib` | PASS |
| `cargo fmt --all -- --check` | PASS |
| `cargo test -p natives --lib creative_app::` | 146/146 PASS（+10 from Batch 4） |
| `cargo test -p natives --lib db::tests` | 9/9 PASS |
| `npm run typecheck` | PASS |
| `npm run lint` | PASS（2420 键） |
| `npm run test` | 763/763 PASS |
| `npm run protocol:check` | PASS（154 methods） |
| `npm run perf:check` | PASS（215.3KB） |

## 5. 磁盘增量

target 16G（不变）、node_modules 1.0G、.next 884M、可用 15Gi（较 Batch 4 基线减少 1Gi，其他 Agent 消费）。

## 6. 遗留风险

- **Dock 未接入 WorkshopPage**：CreativeDock 组件已创建但未接入 WorkshopPage 布局（需要集成负责人决定具体位置）
- **close≠stop 已实现但未在 UI 中暴露**：WindowInstance 状态与 RuntimeInstanceStatus 类型级分离，但 UI 中关闭窗口不会自动停止 runtime
- **`commands::provider` 单测挂起**：环境性，本批未触碰
- **Docker 不可用**：本批不涉及 Docker

## 7. 回滚方法

`git revert 782ab84 c3f6d8d 08c74d1 f7544ce`（按序）；或 checkout 到 `221680a4`。migration v19 是 additive，回滚后旧 binary 不读新表。

## 8. 下一批（B6）继承

- commit：`782ab84`（分支 `codex/creative-os-b5-20260804-204021`）
- schema 版本：19（application_surfaces、runtime_endpoints、window_instances 表已就绪）
- WebView 后端：multi Child（ADR-0017），WindowInstance 持久化已就绪
- 禁止假设：不要假设 surface 没有 main surface（已 backfill）；不要假设 close 影响 runtime（close≠stop）