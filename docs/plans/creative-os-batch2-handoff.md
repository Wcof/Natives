# Creative OS Batch 2 交接报告 — Operation 事实与应用级并发

> 交接时间：2026-08-04
> 分支：`codex/creative-os-b2-20260804-172208`（base = Batch 1 stable `38199708`）
> 主工作区（`/Users/ldh/Downloads/project/AiNative/Natives`），未新建 worktree；其余
> `codex/runtime-task-*` / `codex/agent-runtime-audit-*` worktree 属于其他 Agent，未触碰。

## 1. Batch / Task / 提交

| Task | 提交 | 说明 |
|---|---|---|
| CR-201 Operation journal 与事件契约 | `89344eca` | operations 表 v18、operation store、mutation 返回 operation id + projection、事件、前端 DTO/wire（Rust 后端 + 接线） |
| CR-202 Application-keyed lock 与全局资源限流 | `89344eca` | 与 CR-201 同一后端提交（见 §3「同批切换」说明） |
| CR-203 Renderer Operation 投影 | `2b01863c` | hook 订阅/snapshot、busyIds derived adapter、Browser 错误可见、i18n、前端测试 |

- base SHA：`3819970857587d16f7ea1f186053b294f6883808`（Batch 1 stable）
- final SHA：`2b01863c`（本批 stable）

> **提交粒度说明**：计划 §3 明确「Operation schema、mutation API、事件和 Renderer busy 必须同批切换；只改锁会丢恢复事实，只加表会成为装饰」；且 CR-201 与 CR-202 的文件清单重叠（都含 `service.rs`、commands），operation journal 的命令层实现消费 app-keyed lock。因此 CR-201+CR-202 作为一个自洽、可回滚的后端提交（`89344eca`），CR-203 单独提交（`2b01863c`）。回滚任一提交均可回到上一批可编译状态。

## 2. 实际改动文件与调用链变化

15 个文件，+1611 / -126：

| 文件 | 变化 |
|---|---|
| `src-tauri/src/db.rs` | migration v18：`operations` 表 + 2 索引；`SCHEMA_VERSION` "17"→"18" |
| `src-tauri/src/creative_app/operation.rs` | 新模块：operation CRUD、guarded phase transition、active snapshot、retention prune、startup settle |
| `src-tauri/src/creative_app/service.rs` | `MutationLockRegistry`（per-app async Mutex + 有界 install semaphore，Weak 键自清理，try_acquire 供 watchdog）；删除了死代码生命周期方法（调用点早已改走 adapters） |
| `src-tauri/src/creative_app/model.rs` | `MutationResult { operation_id, summary }`、`DeleteMutationResult { operation_id, result }` |
| `src-tauri/src/commands/creative_app.rs` | install/start/stop/restart/delete 全部 journal（pending→waiting→running→settle），emit `creative-operation` 事件；新增 `creative_app_operations` / `_operation_get` / `_operation_cancel`；锁改 `acquire_app`/`acquire_install` |
| `src-tauri/src/lib.rs` | watchdog 不再持全局锁（DB CAS 兜底，避免被另一 app 长任务阻塞）；启动 reconcile 线程先 `settle_stale_on_startup` |
| `src-tauri/src/creative_app/mod.rs` | 注册 `operation` 模块 |
| `src/lib/tauri-adapter.ts` | `CreativeAppOperation` 类型 + mutation result 类型；start/stop/restart/install/delete wire 改返回 `{operationId, ...}`；新增 `operations/getOperation/cancelOperation/onOperationChanged` |
| `src/hooks/useCreativeAppCatalog.ts` | 初始 operations snapshot + `onOperationChanged` 订阅（terminal 触发 reload）；`busyIds` 改为 operation 派生 ∪ 本会话 in-flight；新增 `operations`/`activeOperationFor` |
| `src/lib/creative-app.ts` | 纯投影 helpers：`isOperationActive/Terminal`、`upsertOperation`、`deriveBusyIds`（applicationId→source id）、i18n label key |
| `src/components/shell/WorkshopPage.tsx` | start 读 `result.summary`；Browser show 失败回滚 panel + toast（不再 fire-and-forget 吞错） |
| `src/i18n/zh.ts` / `en.ts` | `creative.operation.*` 14 键（kind+phase 标签） |
| `src/lib/creative-app.test.ts` / `src/hooks/useCreativeAppCatalog.test.ts` | 投影 helpers 测试 + hook 源码断言 |

调用链前后：mutation 命令由「直接调用 adapters + 全局锁」改为「create operation → waiting → acquire per-app lock → running → adapters → settle_success/failure/cancelled（每次 phase 变更 emit 事件）」。install 走有界 semaphore。watchdog 无锁轮询。

## 3. Schema / backfill / API / event / feature flag

- schema 版本：`_schema_version` 17 → **18**（additive、幂等、可重跑）。
- v18：新增 `operations(id PK AUTOINCREMENT, application_id REFERENCES applications ON DELETE SET NULL, runtime_instance_id REFERENCES runtime_instances ON DELETE SET NULL, kind, phase, actor, redacted_input_json, error_code, error_message, started_at, finished_at, updated_at)` + `idx_operations_application_active(application_id, phase)` + `idx_operations_updated_at`。`application_id` 可空且 FK SET NULL，因此 delete 操作在其自身 application 行删除后仍存活（审计）。
- 保留上限：`RETAIN_TERMINAL=500` 条终态记录，active 永不清理（prune 在 settle 时执行）。
- API：wire 变化——start/stop/restart/install 返回 `{ operationId, summary }`，delete 返回 `{ operationId, result }`（summary 仍在响应中，兼容一个版本）；新增 `creative_app_operations` / `creative_app_operation_get` / `creative_app_operation_cancel`。
- 事件：`db-state-changed` channel `creative-operation`（带 sequence 信封），payload 为 Operation projection。
- feature flag：无。
- 恢复载体：Host 启动时 `settle_stale_on_startup` 把所有非终态 operation 置 failed(error=host_restarted)（#10）。

## 4. 兼容证据

- 旧 DB（v14–v17）升级：v18 仅 CREATE TABLE IF NOT EXISTS + INSERT OR REPLACE schema version；空库/重复 migration 测试通过（`migration_creates_operations_journal`）。
- 三源 Catalog 回归：`creative_app::` 112 项全绿（含 Batch 1 的 100 项 + 12 新项）。
- 旧 mutation 响应：`summary` 字段保留在 `{operationId, summary}` 内；前端同批切换。
- 无用户重注册；source detail / source id / module id 不变；无 Docker 依赖。

## 5. 测试结果

| 命令 | 结果 |
|---|---|
| `cargo check -p natives` | PASS |
| `cargo test -p natives --lib creative_app::` | 112/112 PASS（含 operation 8 + lock 4 + 既有 100） |
| `cargo test -p natives --lib db::tests` | 9/9 PASS（含 v18 迁移测试） |
| `cargo fmt --all -- --check` | PASS |
| `npm run typecheck` | PASS |
| `npm run lint` | PASS（2418 zh/en keys 同步） |
| `npm run test` | 763/763 PASS |
| `npm run protocol:check` | PASS（154 methods，无变化） |
| `npm run perf:check` | PASS（/page 215.2KB gzip < 350KB budget） |
| `cargo test -p natives --lib`（全量） | 未完成（共享 target 被其他 Agent 并发 cargo test 占用，按「Cargo 单进程」纪律主动停止；见 §8） |

全量 lib 套件：本批改动全部落在 `creative_app::*`、`db::*`、`lib.rs` watchdog、`commands/creative_app.rs`，对应定向套件全绿（112 + 9）。全量运行期间另一 Agent 的 `cargo test -p natives --lib` 正在同一共享 target 上编译，与 Batch 1 门禁约定一致（并发跑全量会被干扰），因此停止全量并以此前 Batch 1 验证的基线为准：预期仅剩 5 项环境失败（`usage::claude` 3 项读真实 `~/.claude` 会话文件按墙钟断言 `in_range=0`；`sidecar_supervisor` 2 项依赖 env/daemon 路径），本批未触碰这些模块。

## 6. 资源 owner / 停止 / 补偿 / reconcile 证据

- 本批不新增进程/容器/端口资源；锁与 journal 是内存+DB 层，无新资源生命周期。
- stop 失败：operation 置 failed，instance 由既有 `mark_cleanup_failed` 置 cleanup_failed（active-like，阻止新 start/restart，直到 retry stop 成功）——与 Batch 1 不变量 #3 一致。
- start 被并发 stop 抢占：operation 置 cancelled（reason "start superseded by a concurrent stop"），instance 归 stop 路径。
- 恢复：Host 重启后非终态 operation 置 failed(host_restarted)；instance 恢复仍由既有 reconcile 收敛（本批未改 reconcile 语义）。

## 7. 磁盘增量 / worktree / fixture

- 基线：target 15G、node_modules 1.0G、.next 882M、可用 30Gi。
- 批次后：target 15G（无显著增长）、node_modules 1.0G、.next 886M（+4M，perf:check 构建产物）、可用 29Gi（其余 Agent 消费）。
- worktree 5 个（主工作区 + 4 个其他 Agent 的，含新出现的 `runtime-task-005-projector`）；本批未创建/删除任何 worktree。
- 无新增 fixture / 无容器 / 无 test-id 资源。

## 8. 遗留风险与人工决策

- **CR-201+CR-202 合为一个后端提交**（`89344eca`）：计划 §3「同批切换」依据；回滚按提交整体处理。
- restart 的 health wait 在 app 锁内（`adapters::restart` 整段持锁）：同 app 的 stop 在 restart health 期间会等待 restart 完成，而非抢占。与 start（health 在锁外）行为略有差异；跨 app 并行不受影响。Batch 3 的 stop/restart 证明链可再评估。
- `activeOperationFor` 暴露给 UI 但当前未消费；`operations` 事件在终态时触发 catalog reload（每次 mutation 至多一次 reload，无新增轮询）。
- Docker 本环境仍不可用：install 的 semaphore/事件链只由单测 + 代码路径验证，真实 Docker 验收留待有 Docker 环境（Batch 7 门禁）。
- **全量 lib 套件**：运行期间另一 Agent 在同一共享 target 上并发 `cargo test`（违反「Cargo 单进程」纪律），按 Batch 1 门禁约定主动停止全量，避免 build-lock 长时间互等；以定向套件为准。已按 Batch 1 基线预期 5 项环境失败，本批未触碰相关模块。

## 9. 回滚方法

- 回滚本批：`git revert 2b01863c 89344eca`（按序）；或 checkout 到 `38199708`。
- migration 为 additive，不 drop 表/列；旧 binary 可读 v18 DB。
- 若需避免 v18 生效：`_schema_version` 置回 17 不会降级；需在旧 binary 上重放。operations 表无用户数据，可安全保留或由后续 batch 清理。
- 安全/正确性修复（journal、DB CAS、事件投影、watchdog 去锁）不回退。

## 10. 下一 Batch（B3）必须继承

- commit：`2b01863c`（分支 `codex/creative-os-b2-20260804-172208`）。
- schema 版本：18；`operations` 表可用（kind/phase/actor/redacted/error/timestamps，保留 500 条终态）。
- 事件：`creative-operation` channel 已接入 Renderer。
- 锁：`MutationLockRegistry`（`acquire_app`/`acquire_install`/`try_acquire_app`）；watchdog 无锁运行。
- 缓存：主 target（15G）、node_modules、.next；未安装新依赖。
- 禁止假设：不要假设 mutation 仍返回裸 summary（现为 `{operationId, summary}`）；不要假设 watchog 持锁（现无锁，靠 DB CAS）；不要假设 `operations` 表可任意增长（有 prune）；不要假设同 app restart 的 health 可被并发 stop 抢占（锁内）；Docker 仍不可用。
