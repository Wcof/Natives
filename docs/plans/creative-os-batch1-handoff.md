# Creative OS Batch 1 交接报告 — Registry 完整性与迁移基座

> 交接时间：2026-08-04
> 分支：`codex/creative-os-b1-20260804-143635`（从 `origin/deploy@9584c3c2` 创建）
> 主工作区（`/Users/ldh/Downloads/project/AiNative/Natives`），未新建 worktree；其余
> `codex/runtime-task-*` / `codex/agent-runtime-audit-*` worktree 属于其他 Agent，未触碰。

## 1. Batch / Task / 提交

| Task | 提交 | 说明 |
|---|---|---|
| 前置 docs-only | `25439c62` | 计划 + 审计基线入库；`.gitignore` 忽略历史 `.cargo-target-shared/` |
| CR-101 只读身份解析与幽灵行迁移 | `32cea9db` | #01 关闭 |
| CR-102 Active Runtime/Plan DB CAS | `e7cc6b3e` | #06/#07 关闭 |
| CR-103 LaunchProfile v1 兼容升级器 | `d8557076` | #34 的 plan/manifest 部分 |

- base SHA：`9584c3c263c1e83b1066e4208e1ab2d678a9deeb`（origin/deploy HEAD）
- final SHA：`d8557076`（本批 stable）

## 2. 实际改动文件与调用链变化

9 个文件，+1740 / -106：

| 文件 | 变化 |
|---|---|
| `src-tauri/src/db.rs` | migration v15（幽灵清理+审计表）、v16（active 去重+partial unique 索引）、v17（startup_plans 版本化列+Compose owner 修复）；`backfill_creative_identity` 修正 local compose owner；`SCHEMA_VERSION` 常量（当前 "17"） |
| `src-tauri/src/creative_app/runtime_store.rs` | 只读 `application_id_for` / `attach_identity`；`create_instance` DB CAS；全部 `mark_*` 过渡检查 affected rows（typed Conflict/NotFound）；`active_instance_id` 纳入 cleanup_failed/orphaned；`upsert_active_plan` 写版本化列+单 active；`get_active_plan`/`parse_launch_plan`/`LaunchProfile` |
| `src-tauri/src/commands/creative_app.rs` | browser show/close 改为 resolve→只读 lookup，读路径不再创建身份 |
| `src-tauri/src/commands/module.rs` | module install 注册统一身份 |
| `src-tauri/src/creative_app/adapters/internal.rs` | enable 注册统一身份 |
| `src-tauri/src/creative_app/adapters/lifecycle_matrix_tests.rs` | reads 不创建身份 / browser 不产幽灵 / 注册后 application_id 稳定 |
| `src-tauri/src/creative_app/store.rs` | `modules_survive_v8` 陈旧断言改用 `SCHEMA_VERSION` |
| `src-tauri/src/error.rs` | 新增 `Error::Conflict(String)` |
| `src-tauri/src/env_manager.rs` | `get_encryption_key` 在 cfg(test) 下校验缓存 key 与当前连接 DB 存储 key，消除跨测试污染（修复既有 `crud_and_unique_root` flake） |

调用链前后：Browser show/close 由「find-or-create(LocalProject) 优先 → 必然造幽灵行」改为「adapters::resolve → application_id_for 只读」。Catalog list/get 由「attach_identity 读时创建」改为「读时只查」。Start/Stop/Install/Register 保留 find-or-create（写入路径注册身份）。

## 3. Schema / backfill / API / event / feature flag

- schema 版本：`_schema_version` 14 → **17**（v15/v16/v17 均为 additive、幂等、可重跑）。
- v15：新增 `creative_identity_reports` 审计表；删除「无 source row 且无 plan/runtime/preview 引用」的确认幽灵行（先备份 row JSON），其余 quarantine/report（含 cross-source collision）。
- v16：`idx_runtime_instances_one_active`（partial unique，`status IN ('starting','running','stopping')`）、`idx_startup_plans_one_active`（`is_active=1`）；迁移先去重（保留最新 active，runtime 重复降级 orphaned，plan 重复置 is_active=0，均留审计）。
- v17：`startup_plans` 增加 `schema_version`(INTEGER) / `driver_kind`(TEXT) / `ownership_mode`(TEXT)，从 plan_json 回填；修复 local compose 实例 owner_kind `local_process`→`docker_compose`。
- API：无 wire 类型变化（Browser 命令签名不变，仍传 source id）。错误新增 `Error::Conflict`；double-start 的旧文案「this app already has an active runtime instance; stop it first」保留（Conflict 与预检查同文案），前端无需改。
- feature flag：无。
- i18n：无 UI 文案变更，无需同步。

## 4. 兼容证据

- 旧 DB（v8–v14 各版本）升级：v12 backfill 幂等 + v15/16/17 修复幂等（有重复 migration 测试）。
- 三源 Catalog / Workshop / Local / GitHub 注册路径：既有 tests 全绿（`creative_app::` 100 项 + `db::tests::` 8 项）。
- 旧 plan_json 读时升级（`get_active_plan` NULL 列推导）；未知字段容忍；future schema fail closed（`parse_launch_plan`）。
- 无需用户重注册；source detail / source id / module id 不变。

## 5. 测试结果

| 命令 | 结果 |
|---|---|
| `cargo check -p natives` | PASS |
| `cargo test -p natives --lib creative_app::` | 100/100 PASS（3 次重复，确定性） |
| `cargo test -p natives --lib db::tests::` | 8/8 PASS |
| `cargo fmt --all -- --check` | PASS |
| `npm run typecheck` | PASS |
| `npm run lint` | PASS（2405 zh/en keys 同步） |
| `npm run protocol:check` | PASS（154 methods，无变化） |
| `cargo test -p natives --lib`（全量） | 见下方环境失败说明 |

全量 lib 套件：本批范围内（`creative_app::*`、`db::*`）全部通过。5 个环境相关失败与 Batch 1 无关（涉及模块未被本批触碰），属环境依赖：

- `usage::claude::tests::claude_*`（3 项）：读取真实 `~/.claude/projects/...jsonl` 会话文件并按当前墙钟断言「in-range nonzero rows」；实测 `in_range=0`（会话时间戳早于当前窗口），是真实用户数据 + 时间相关，非代码回归。
- `sidecar_supervisor::tests::config_from_env_defaults`、`default_daemon_binary_is_resolved_to_an_existing_path`：依赖运行环境的 env/config/daemon 二进制路径。

另：共享 `target` 目录正被其他 Agent（`codex/runtime-task-*`）的测试二进制并发占用，全量跑会被干扰，建议 Batch 门禁以定向测试为准（计划 §10 的 Batch 1 测试命令即定向 + `cargo check` + protocol）。

已知非本批失败（基线已存在，归 agent-core owner）：`agent-core` 的
`completes_simple_text_turn`、`session_end_hook_fires_after_success`
`PERSISTENCE_FAILED`；与本批无关，本批未触碰。

## 6. 磁盘增量

- 基线：target 15G、node_modules 1.0G、.next 882M、`.cargo-target-shared` 7.8G（历史，未写）、磁盘可用 34Gi。
- 批次后：target 15G（无显著增长）、node_modules/.next 不变、磁盘可用 34Gi。
- 无新增 fixture / 无 test-id 资源 / 无容器（Docker 不可用）。

## 7. 遗留风险与人工决策

- `active_instance_id` 现把 `cleanup_failed`/`orphaned` 视为 active-like（阻止新 start）。这是按全局不变量 #3 的预期行为；恢复入口是 `creative_app_resolve_orphan` 或 stop（retry cleanup）。Batch 3 的 stop/reconcile 证明链会进一步接管。
- partial unique index 只覆盖 `starting/running/stopping`；`cleanup_failed/orphaned` 的防并发由代码级 `has_active_instance` 保证（迁移去重将其降级到 orphaned 也正是因为索引范围）。
- `Error::Conflict` 为新增错误类型；前端展示其 Display 字符串，与旧文案兼容一个版本。
- 环境无 `docker` executable：Compose/Run 相关行为只由 fixture/单测验证，未做真实 Docker 验收（Batch 7 门禁明确要求有 Docker 环境）。

## 8. 回滚方法

- 回滚本批：`git revert d8557076 e7cc6b3e 32cea9db`（按序）；或 checkout 到 `25439c62`。
- migration 为 additive，不 drop 表/列；旧 binary 可读 v17 DB（新增列可空）。
- 若要避免 v15/16/17 生效：`_schema_version` 置回 14 并重跑不会降级；需在旧 binary 上重放。ghost 删除前已备份 row JSON 到 `creative_identity_reports`，可手动还原。
- 安全修复（读路径不造身份、CAS、fail-closed）不回退。

## 9. 下一 Batch 必须继承

- commit：`d8557076`（分支 `codex/creative-os-b1-20260804-143635`）。
- schema 版本：17；`creative_identity_reports` 审计表可用。
- 缓存：主 target（15G）、node_modules、.next；未安装新依赖。
- fixtures：无新增。
- 禁止假设：不要假设 cleanup_failed/orphaned 不阻塞 start（现在会阻塞）；不要假设 `mark_*` 0-row 返回 Ok（现在返回 Conflict/NotFound）；Docker 仍不可用。
