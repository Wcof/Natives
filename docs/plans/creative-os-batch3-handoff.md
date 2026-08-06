# Creative OS Batch 3 交接报告 — Runtime Owner 与可信生命周期

> 交接时间：2026-08-04
> 分支：`codex/creative-os-b3-20260804-192335`（base = Batch 2 stable `8ebeca03`）
> 主工作区（`/Users/ldh/Downloads/project/AiNative/Natives`），未新建 worktree；其余
> `codex/agent-runtime-task-*` / `codex/agent-runtime-audit-*` worktree 属于其他 Agent，未触碰。

## 1. Batch / Task / 提交

| Task | 提交 | 说明 |
|---|---|---|
| CR-301 Runtime-owned registry、cancel、logs | `31c9c8bb` | 活体进程/task/日志 registry 全以 runtime_id 为键；per-runtime 日志目录；logs(runtime_id,cursor)；晚到事件按实例归属 |
| CR-302 Stop/Restart/Reconcile 证明链 | `2113aee3` | stop 未验证释放绝不返回 Ok；失败→cleanup_failed 阻断 restart；PID 复用拒绝 |
| CR-303 Static endpoint revoke 与 Preview 补偿 | `8f343962` | static URL instance-scoped/可撤销（stop 前 200 后 410）；browser show/close/delete 三态一致 |

- base SHA：`8ebeca03`（Batch 2 stable）
- final 代码 stable：`8f343962`；本交接报告为分支 HEAD 上的 docs 提交（`git log --oneline 8ebeca03..HEAD` 可见全部 4 个提交）

> 说明：CR-303 提交同时包含对 CR-301/302 文件的 `cargo fmt` 整理（本批早期提交按计划未逐提交跑 fmt，批末一次整理；全部改动限于本批触碰的行，无无关格式化）。

## 2. 实际改动文件与调用链变化

13 个文件，+1151 / -257：

| 文件 | 变化 |
|---|---|
| `src-tauri/src/creative_app/local/runtime.rs` | `LocalRuntimeManager` 的 `procs`/`task_handles`/`LogRegistry` 全部以 **runtime_id** 为键；`LiveLocalProcess` 增加 runtime_id/app_id；`poll_exits` 返回 `(runtime_id, code)`；`start_node_dev/wait_healthy/stop/is_running/current_port/open_url/identity/recent_logs/persisted_tail` 均改 runtime_id 参数；新增 `live_runtime_ids`、`app_aggregate_tail`、`purge_app_logs`；`CreativeAppLogEvent` 增加 `runtimeId`；`static_open_url`/`resolve_preview_urls` 增加 runtime_id（instance-scoped URL） |
| `src-tauri/src/creative_app/local/logs.rs` | `LocalLogStore::open(app_id, runtime_id)` 写入 `{appId}/runs/{runtimeId}/`；`LogRegistry` 以 runtime_id 为键（`get_or_open(app_id, runtime_id)`）；新增 `app_aggregate_tail`（旧 app 日志只读聚合）、`remove_app`、`safe_segment`；`recent_memory_after(cursor, limit)` |
| `src-tauri/src/creative_app/local/lifecycle.rs` | `start_app/await_start_ready/stop_app/delete_app` 增加 runtime_id 参数；`stop_app` 未验证释放时返回 `Err`（CR-302）；`resolve_orphan` 改为 kill+settle orphaned 实例（restart 交给 adapters）；`mark_process_exited(runtime_id)` 只 settle 指定实例、仅当仍是 active 才改源记录；`poll_and_reconcile_exits` 按 live runtime 心跳 |
| `src-tauri/src/creative_app/runtime_store.rs` | 新增 `instance_application_id`、`source_id_for_application`、`settle_instance_by_id`；删除被我改动物化的 `heartbeat_for_source`/`mark_exited_for_source` |
| `src-tauri/src/creative_app/adapters/mod.rs` | `spawn_start/await_ready/stop/delete` 把 instance_id 传入 local 调用；`stop` 成功后清 preview bind |
| `src-tauri/src/creative_app/adapters/local.rs` | 签名同步（start/await/stop/delete 带 runtime_id）；移除死代码 `restart` |
| `src-tauri/src/creative_app/local/mod.rs` | 导出移除 `restart_app` |
| `src-tauri/src/creative_app/local/deps.rs` | `get_or_open(id, "deps-{id}")` |
| `src-tauri/src/commands/creative_app.rs` | `creative_app_logs(runtimeId, tail, cursor)` runtime-scoped + `resolve_log_scope` helper；`get_local_logs` 解析 active runtime；`diagnose` 用 aggregate tail；`resolve_orphan` restart 走 `adapters::spawn_start/await_ready`；`browser_show` 先校验 running 再 show、DB bind 失败补偿 hide；`browser_close` 先 close 再清 bind；`delete` 关掉正在展示该 app 的 WebView |
| `src-tauri/src/http_server.rs` | `/local-projects/{runtimeId}/{creativeId}/…` 仅当该 runtime 实例 active（running/starting）且属于该项目时服务；HTML 注入 tokenized `<base href>`；legacy `/local-projects/{creativeId}/…` 运行期 302 到 tokenized、stop 后 410 |
| `src/lib/tauri-adapter.ts` | `CreativeAppLogEvent` 增加 `runtimeId`；`logs(runtimeId, tail?, cursor?)` |
| `src/components/shell/WorkshopPage.tsx` | 日志读取与过滤用 `runtimeInstanceId ?? appId`（晚到事件不串线） |

调用链前后：活体资源（进程/日志/读 task）从「按 source id 键控」改为「按 runtime instance id 键控」；start/stop/delete 由 adapters 先创建/解析实例再传入 local 驱动；log 事件带 runtime id；static URL 由 `/local-projects/{appId}/…` 改为 `/local-projects/{runtimeId}/{appId}/…` 并在 HTTP 层校验 active runtime。

## 3. Schema / backfill / API / event / feature flag

- **无 migration**（schema 仍为 18）。本批不新增表/列；`operations.runtime_instance_id`（v18）列本批未写入（operation 创建时 runtime id 尚不可知；见 §8）。
- API：
  - `creative_app_logs(runtimeId, tail?, cursor?)`——runtime 作用域 + 增量 cursor（仅返回 seq > cursor）。
  - `creative_app_get_local_logs(id, limit?)`——解析该 app 的 active runtime；停止时返回空数组（前端回退到 `logs` 聚合串）。
  - `creative_app_browser_show`——非 running 应用拒绝展示（返回错误），DB bind 失败补偿 hide。
  - `creative_app_browser_close`——先 close WebView 成功再清 DB bind。
  - `creative_app_delete`——成功后若该 app 正在 child WebView 展示则关闭之。
- 事件：`creative-app-log` 事件新增 `runtimeId` 字段（`appId` 保留）。
- 静态 URL 契约：`http://127.0.0.1:{port}/local-projects/{runtimeId}/{creativeId}{path}`；legacy `/local-projects/{creativeId}{path}` 仅在 active 时 302 到 tokenized，stop 后 410。
- feature flag：无。

## 4. 兼容证据

- 旧 DB（v14–v18）升级：本批无 migration，旧库直接可读。
- 旧日志：`~/.natives/logs/local-creative/{appId}/current.log` 仍保留并作为 app 级聚合只读（dual-read，禁止长期 dual-write）；新日志写 `{appId}/runs/{runtimeId}/`。
- legacy static URL：运行期 302 到 tokenized（read-only redirect）；stop 后 410。回滚到旧 binary 即恢复非 tokenized 行为。
- 前端 wire：`logs` 参数名改为 `runtimeId`（Tauri 按名匹配），前端同批切换；`getLocalLogs` 签名不变。
- 三源 Catalog 回归：`creative_app::` 123 项全绿（Batch 2 的 112 + 本批新增 11）。

## 5. 测试结果

| 命令 | 结果 |
|---|---|
| `cargo check -p natives --lib` | PASS |
| `cargo test -p natives --lib creative_app::` | 123/123 PASS（含 CR-301 隔离/晚到事件 5 项 + CR-302 证明链 4 项） |
| `cargo test -p natives --lib http_server` | 10/10 PASS（含 CR-303 stop 前 200/后 410、legacy 重定向、base href 3 项） |
| `cargo test -p natives --lib db::tests` | 9/9 PASS |
| `cargo fmt --all -- --check` | PASS |
| `npm run typecheck` | PASS |
| `npm run lint` | PASS（2418 zh/en keys 同步） |
| `npm run test` | 763/763 PASS |
| `npm run protocol:check` | PASS（154 methods，无 wire protocol 变更） |
| `npm run perf:check` | PASS（/page 215.2KB gzip < 350KB budget） |
| `cargo test -p natives --lib`（全量） | **524 PASS；5 项已知环境失败；1 项既有挂起**（见 §8） |

新增测试明细：CR-301——`per_runtime_stores_are_isolated`、`recent_memory_after_filters_by_cursor`、`runtime_log_dir_shape`、`two_runs_of_same_app_are_isolated`、`stale_run_exit_does_not_stop_newer_run`、`instance_to_application_to_source_mapping`、`settle_by_id_only_touches_named_instance`；CR-302——`stop_fails_when_port_not_released`、`repeated_stop_is_idempotent`、`pid_reuse_is_rejected_by_strict_identity`、`local_stop_surfaces_release_failure`（契约）；CR-303——`local_project_static_url_is_revoked_after_stop`、`legacy_local_url_redirects_while_running_then_dies`、`base_href_is_injected_before_head_close`。

## 6. 资源 owner / 停止 / 补偿 / reconcile 证据

- 活体进程/日志/读 task 由 runtime instance 唯一 owner：同 app 连续两次运行有独立 slot；晚到 exit 只 settle 其自身实例，不碰新 active 实例与源记录（`stale_run_exit_does_not_stop_newer_run`）。
- stop 未验证释放（进程组/端口/容器仍活）→ `stop_app` 返回 Err → adapter `mark_cleanup_failed`（active-like）→ restart 被 CAS 阻断，直到重试 stop 成功。
- PID 复用拒绝：`identity_matches_live_strict` 校验 start time/executable/cwd，未知资源不杀（`pid_reuse_is_rejected_by_strict_identity`）。
- resolve_orphan：kill 校验通过的身份 → settle orphaned 实例到 stopped → restart 走 adapters 新建实例（CAS 通过）。
- reconcile：Running + live leftover → orphaned（非假 stopped）；`settle_instance` 镜像到 active 实例（Batch 1 不变量，未改）。
- 静态端点：stop 即 410，legacy URL 同死；`browser_close` 先关 WebView 成功才清 DB bind；`delete` 关掉展示该 app 的 WebView；`browser_show` 拒绝非 running 应用。

## 7. 磁盘增量 / worktree / fixture

- 基线（Batch 2 交接）：target 15G、node_modules 1.0G、.next 886M、可用 29Gi。
- 批次后：target 16G（+1G，本批测试构建/重链接）、node_modules 1.0G、.next 884M、可用 26Gi（其余 Agent 消费约 3G）。
- worktree 10 个（主工作区 + 9 个其他 Agent 的，新增 task-007/008/009/010）；本批未创建/删除任何 worktree。
- 无新增 fixture / 无容器 / 无 test-id 资源；测试临时文件（temp dir、mock server 随机端口）均随测试结束释放。

## 8. 遗留风险与人工决策

- **operations.runtime_instance_id 未写入**：operation 在锁/实例创建前 journal（waiting 对 Renderer 可见），start 的 runtime id 在 spawn 时才产生；stop/restart/delete 可回填但保持最小改动。如需 operation↔instance 关联，B5（Window）或后续可回填。
- **Compose project 名称仍按 app 派生**（确定性、跨重启可 stop）：CR-301 目标「活体 registry 以 runtime id 为主键」达成；Docker label/project 的逐实例 owner 化留给 B7 driver facade（执行矩阵 B3/B7 高冲突文件 `adapters/mod.rs` 严格串行）。
- **Docker 本环境仍不可用**：compose stop 验证（`compose_ps_running`）、compose URL revoke 仅由代码路径 + `stop_app` Err 契约测试覆盖，真实 Docker 验收留待有 Docker 环境。
- **全量 lib 套件**：`commands::provider::tests::provider_test_surfaces_rate_limit_for_chat_completions` 在本环境挂起（mock server 单次响应；429 路径退避/客户端阻塞）。该模块本批未触碰；Batch 2 全量运行亦见同一测试 >60s（同一环境）。已在挂起处主动停止，定向套件全绿。5 项环境失败与 Batch 1 基线一致（`usage::claude`×3 读真实 `~/.claude` 会话文件、`sidecar_supervisor`×2 依赖 env/daemon 路径）。
- `cargo fmt` 整理并入 CR-303 提交（见 §1 说明）。

## 9. 回滚方法

- 回滚本批：`git revert 8f343962 2113aee3 31c9c8bb`（按序）；或 checkout 到 `8ebeca03`。
- 无 migration，无 DB 层回滚；旧 binary 直接读 v18 DB。
- 静态 URL：回滚后旧 binary 用非 tokenized `static_open_url`，legacy 路由恢复直接服务（新 binary 的 410/302 逻辑消失）。
- 安全/正确性修复（runtime-id owner、stop 证明链、endpoint revoke）不回退。

## 10. 下一 Batch（B4）必须继承

- commit：`8f343962`（分支 `codex/creative-os-b3-20260804-192335`）。
- schema 版本：18（本批无变更）；`operations` 表含 `runtime_instance_id` 列（未回填）。
- 活体资源：`LocalRuntimeManager`/`LogRegistry` 均以 runtime_id 为键；`logs(runtime_id, cursor)` API；`CreativeAppLogEvent.runtimeId`。
- HTTP：`/local-projects/{runtimeId}/{creativeId}/…` instance-scoped + active 校验 + `<base href>`；legacy 302/410。
- Compose project 名称仍按 app 派生（B7 driver facade 接手）。
- 禁止假设：不要假设 live registry 按 app id 键控（现为 runtime id）；不要假设 stop 失败返回 Ok（现为 Err → cleanup_failed）；不要假设 static URL 在 stop 后仍可服务（现为 410）；不要假设全量 lib 套件能跑完（`commands::provider` 单测挂起为环境性）；Docker 仍不可用。
