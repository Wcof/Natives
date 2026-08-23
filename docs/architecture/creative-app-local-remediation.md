# 本地创意（local_project）整改实施方案

> **领域**: 个人创意 / 三来源统一管理（Workshop + GitHub 容器 + 本地项目）
> **来源边界**: [ADR-0013](../adr/0013-creative-app-dual-source.md)（双来源冻结）+ 本地项目为第三来源
> **约束映射**: [`standards/`](../standards/README.md) technical/02-security · 03-data
> **基石源码**: `src-tauri/src/creative_app/local/` · `src/components/shell/WorkshopPage.tsx`
> **状态**: 实施中（分支 `feat/creative-app-local-remediation`）
> **日期**: 2026-07-24
> **同域文档**: 收敛现状与验收见 [creative-app-local-project-remediation.md](./creative-app-local-project-remediation.md)（基线 `deploy@4f130256`）；本文是 gap 整改设计（9 项，B1–B4/F5–F9），两者非重复进度快照

---

## 一、背景与校准结论

个人创意已从「内部 Workshop + GitHub Docker 双来源」扩展为**三来源统一管理**，新增 `local_project` 来源与 `local_static` / `node_dev_server` 运行时。主体界面与后端框架已落地。

在制定本方案前，对一份较早的需求文档逐项做了**现状代码核对**，结论：**超过半数「阻断/gap」已在 `deploy` 分支修复**，不再需要处理。真实剩余 gap 共 9 项，性质多为「能力已具备、接入未闭合」，而非完全缺失。

### 已核实为「已实现」（不在本次范围）

| 早期文档声称的问题 | 现状证据 |
|---|---|
| Rust 编译阻断（agent-daemon） | `cargo check --workspace` 零错误 |
| 启动脚本校验绕过（user/ai 只查名） | rule/user/ai 三路径均汇聚 `plan.rs::validate_launch_plan` → `script_body_is_safe`，校验真实 package.json 脚本体 |
| 残留进程误判（PID 复用误杀） | `runtime.rs::identity_matches_live_strict` 核 PID+启动时间(≤2s)+exe basename+cwd+非空指纹 |
| Stop/Restart/Delete 遗漏残留 | `lifecycle.rs` 内存无 child 时走 `process_identity_json`→`force_kill_identity` 兜底；删除遇身份不符**拒绝删除** |
| 缺启动恢复 reconcile | `reconcile_local_apps` 已收敛 transient、清失效静态端口、核对 node 进程存活 |
| 中文日志 UTF-8 截断 | `logs.rs` 按 `chars().take()` 截断，有测试 `truncates_utf8_safely` |
| 创建未写「依赖缺失」状态 | `create_local_app` 写 `DependenciesMissing` + recovery_actions |
| Vue CLI 误加 `--strictPort` | `plan.rs::runner_port_flags` 已区分，有测试 `vite_and_vue_flags_differ` |
| AI 绕过 Provider Adapter | `ai.rs::build_adapter → ProviderAdapter::chat`，复用凭证表+adapter |
| AI 发送前确认 | `analyze_with_ai` 首行 `if !confirmed` 拒发；前端「预览→确认→发送」已接线 |
| 日志实时/过滤/复制/分色 | 前端 `onLog` 订阅 + 过滤 + 复制 + stderr/system 分色齐全 |
| 编辑页只能改名称+autoOpen | 已能改 cwd/script/openPath/port |

---

## 二、真实剩余 gap 与整改设计（9 项）

### 后端（correctness / 安全）

#### B1. 本地进程自主看门狗

- **现状**: `runtime.rs::start_node_dev`（L307-320）想 spawn 退出监视器，但拿不到 `self` 的 `Arc`，只写了一行系统日志 `"exit watcher armed via health/poll path"`。崩溃感知完全依赖前端页面可见时的 5s 轮询（`useCreativeAppCatalog.ts`）；页面不可见或前端未运行时，卡片长期显示「运行中」。
- **方案**: 在 `lib.rs` setup 阶段，manager（`Arc<LocalRuntimeManager>`）注册后，spawn 一个**后台周期任务**（tokio 线程 + 独立 `db::get_main_conn()`），每 ~3s 调用已有的 `lifecycle::poll_and_reconcile_exits`。后端自主感知崩溃并回写 DB + 广播 `process_exited`，不再依赖前端驱动。有本地进程存活时才轮询，空闲时退避。
- **收益（局部性）**: 崩溃回写从「前端可见性驱动」收归后端进程监督器，成为运行时唯一权威；前端 5s 轮询降级为纯 UI 刷新，可保留但不再是正确性依赖。

#### B2. 依赖安装进程纳入进程树

- **现状**: `deps.rs`（L121-157）安装子进程已 `env_clear` + 白名单 + `kill_on_drop`，但**未设进程组**（无 `pre_exec setpgid` / `CREATE_NEW_PROCESS_GROUP`），也未注册进 `LocalRuntimeManager.procs`。若 Natives 在 `npm install` 期间退出，`kill_on_drop` 只杀直接子进程，install 派生的子进程可能残留。
- **方案**: install 子进程复用 start 路径同款进程组设置（Unix `setpgid(0,0)` / Windows `CREATE_NEW_PROCESS_GROUP`）。install 为前台 `.wait().await` 的一次性任务，进程组即可保证整树随句柄回收；退出时 `shutdown_all` 已覆盖 start 的进程树，install 因 `kill_on_drop` + 进程组也能整组清理。
- **收益**: 依赖安装与 dev server 共享同一进程隔离不变量（进程组 + 白名单 env），杜绝安装期残留。

#### B3. 写入事务化

- **现状**: `store.rs` 全是独立 `conn.execute`。`create_local_app`（commands L546-556）靠 env 失败后手动 `delete_app` 回滚缓解半成品；`update_local_app`（L628-641）**换 env 走 `replace_env`（先删旧 key 再插新值），中途失败会丢旧值且无回滚**。
- **方案**: 用 rusqlite `Connection::transaction()` 包裹 create（insert_app + env）与 update（update_app + replace/upsert/remove env）多步写，整体提交或整体回滚。`create_local_app` / `update_local_app` 的 conn 参数改为 `&mut Connection`（PooledConnection 可解引用为 `&mut`）。
- **收益**: 消除半成品与丢旧值窗口；写入原子性与 ADR-0013「SQLite 唯一元数据权威」一致。

#### B4. 实时/落盘日志按 env 值脱敏

- **现状**: `logs.rs::append_with_secrets`（L232）是死代码；活/落盘日志走 `append()` → `sanitize_log_text()` → 空 secrets，仅做正则模式脱敏。按 env 真实值脱敏（`secret_values`）只用在 AI 诊断载荷。
- **方案**: 给 `LocalLogStore` 增 `secrets: Mutex<Vec<String>>` 字段 + `set_secrets()`；`append()` 内部用存储的 secrets 调 `sanitize_log_text_with_secrets`。start / install 前从 `store::secret_values(conn, id)` 注入该 app 的 env 值。删除死代码 `append_with_secrets`（或改为薄封装）。
- **收益**: 日志脱敏从「模式匹配」升级为「按本 app 真实凭证值」，实时流与落盘文件统一防泄露；与 technical/02-security「凭证不落明文日志」对齐。

### 前端（交互闭合）

#### F5. 本地进度面板 + stage 名对齐

- **现状**: `onProgress` 订阅存在，但进度只在 GitHub 安装向导渲染；本地 start/stop 的 `starting`/`stopping`/`health_check`/`ready` 进度收到后无处显示。`stageLabel` 的 key（download/extract/…）与后端 `CreativeAppProgressStage` 枚举（inspecting_release/…/starting/health_check/ready/failed）不匹配，多数 stage 落兜底文案。
- **方案**: 对齐 `stageLabel` 到后端枚举全集（含本地路径的 starting/health_check/ready/stopped/stopping/installing_dependencies）；为运行中的本地 app 在卡片/详情渲染一条轻量进度行（按 `progress.appId` 关联当前 app）。
- **收益**: 本地生命周期动作对用户可见，消除「点了没反应」的黑箱。

#### F6. 手动/编辑表单补全字段

- **现状**: custom 表单可设 script/cwd/openPath/port/pm/autoOpen，但**无 args、无显式 healthUrl、无 env 录入**，且 health 被强制等于 openPath（`applyCustomPlanFromScan`）。编辑页 env 只读、args/entry 不可改。
- **方案**: custom 表单与编辑表单补齐 `args`（字符串数组输入）、独立 `healthPath`（默认取 openPath 但可改）、`env`（key/value 列表，走 `createLocal.env` / `updateLocal.env`）。health 与 open 解耦。
- **收益**: 手动模式覆盖完整 LaunchPlan 字段，用户无需为高级配置绕道 AI；编辑可维护 env/args。

#### F7. 向导「保存并启动」接 autoOpen

- **现状**: 卡片启动 `handleStart` 已按 `shouldAutoOpenAfterStart` 调 `openExternal`；但向导 `saveLocalCreative(true)` 只调 `start(id)`，未按 autoOpen 打开浏览器。
- **方案**: `saveLocalCreative(true)` 启动成功且 `state==='running'` && autoOpen 时，复用 `openExternal(updated)`。
- **收益**: autoOpen 语义在两条启动路径一致生效。

#### F8. 卡片补「复制地址 / 系统浏览器打开」

- **现状**: 复制地址、系统浏览器打开只在已打开的内置浏览器工具栏里；卡片/详情缺席。「打开目录 / 在终端打开」已在卡片。
- **方案**: 运行中的本地/外部 app 卡片增「复制地址」（`clipboard.write(openUrl)`）与「系统浏览器打开」（`shell.openPath(openUrl)`）动作，仅在有 `openUrl` 时显示。
- **收益**: 四个常用动作在卡片处齐备，不必先进内置浏览器。

#### F9. 移除死复选框

- **现状**: `localStartAfterSave` 复选框（WorkshopPage L191, L1973-1980）无任何提交路径读取，与「仅保存 / 保存并启动」两个按钮功能重复且不生效。
- **方案**: 删除该复选框及其 state；保留两个明确按钮作为唯一入口。
- **收益**: 去除误导性无效控件，交互语义单一。

---

## 三、不做（本次非目标）

- 不改动 GitHub/Docker 来源与 Workshop 内部来源的既有逻辑（仅 F5/F8 的通用卡片动作顺带覆盖外部 app 的 openUrl）。
- 不引入新的 DB 表或 schema 迁移（B3 仅用事务包裹既有写）。
- 不改 AI 调用链路（已核实合规）。
- 不动 KI-1～KI-5 与子 WebView 安全边界。

---

## 四、验证口径

- 后端: `cargo check --workspace` 零错误；`cargo test -p natives creative_app::` 相关测试通过；新增 B3 事务回滚、B4 按值脱敏的单元测试。
- 前端: `tsc --noEmit` 通过；`src/lib/creative-app.test.ts` 等相关测试通过；eslint 无 error。
- 手动口径（记录于 PR）: 本地 dev server 崩溃后卡片状态在数秒内自动转 stopped（B1）；update env 中途失败不丢旧值（B3）；日志不出现 env 明文值（B4）。

---

## 五、落地清单

- [ ] B1 后端看门狗周期任务（lib.rs + runtime/lifecycle）
- [ ] B2 install 进程组
- [ ] B3 create/update 事务化 + 测试
- [ ] B4 LocalLogStore 按值脱敏 + 注入 + 测试
- [ ] F5 stage 对齐 + 本地进度行
- [ ] F6 手动/编辑表单补 args/health/env
- [ ] F7 向导保存并启动接 autoOpen
- [ ] F8 卡片复制地址 / 系统浏览器
- [ ] F9 移除死复选框
- [ ] 更新 ADR-0013 落地清单勾项 + standards technical/02·03 相关条目
- [ ] i18n 中英文同步（新增表单字段与卡片动作文案）

---

## 六、Apps Domain Cutover Delta 审计（2026-08-22，实施包直执行基线）

> 本节是「个人创意 → 应用中心」重构的 current vs target 基线（APP-001），
> 含源码级 Death Audit 清单（APP-002）、最终动作矩阵（APP-003）与
> AppKind/RegistrationOrigin 冻结（APP-004）。本次目标是 **Apps Domain cutover**，
> 不是新增第三套系统；旧 `creative_app` 成熟 helper 按「复用 > 新建」收编，
> 不重写 Local、不把 Web 做成通用浏览器、新增 System 生命周期。

### 6.0 当前源码事实（current production）

| 事实 | 证据 |
|---|---|
| 最新 schema = v27（workspace 7 表），本次从 v28 起 | `db/migrations_steps.rs:869 migrate_v27`、`db/db_migrations.rs:349`、`db/migration_v27.rs`（untracked 新文件，未 commit） |
| `applications` 为 v12 统一 Registry，仅 `source/source_id` 字符串身份，无 kind/origin/sidebar | `db/migrations_steps.rs:212-224`（无 kind 列；`idx_applications_source_id` 唯一） |
| `apps/` 是只读 read-through 外壳：App/RuntimeInstance/Surface 类型 + facade SQL，非 cutover | `apps/model.rs`（App.source:String）、`apps/facade.rs`（list/get/create/delete/active_spec/instances/surfaces 直接 SQL） |
| `commands/apps.rs` 的 start/stop/restart/kill **直接转发** `commands::creative_app::*` public command | `commands/apps.rs:99,119,139,159` |
| Local 成熟实现齐全：scan/plan/runtime/lifecycle/lifecycle_process/logs/orphan/PGID；`force_kill_identity` 已按 `-pgid` SIGTERM→SIGKILL 并验证 | `creative_app/local/*`、`creative_app/local/lifecycle_process.rs:90-119` |
| Web 安全链路齐全：child WebView、BrowserProfile（data_store_identifier）、OAuth divert、navigation/origin、download/grant、window reconcile、`MAX_LIVE_WINDOWS=10` | `creative_app/browser.rs:29 WINDOW_LABEL_PREFIX="creative-window-"`、`creative_app/window.rs:28`、`browser_profile_bindings`（migrations_steps.rs:697）、`non_owned.rs` validate_remote_url/remote_navigation_allowed |
| `non_owned_apps`（v24）= 旧 Attached/Remote Registry，Remote 语义（http/https + approved origin）是正确的 Web 前身 | `creative_app/non_owned.rs:28-133,194-233` |
| 前端 `AppsPage` 仍按 `local/remote/native` 字符串 source 驱动，所有 app 固定显示 Start/Restart/Stop，create 仅收 title/source/sourceId，delete 无确认 | `components/apps/AppsPage.tsx:33,117,135,259-282` |
| 前端 `apps.ts` 手写 `source/sourceId`，API 仅 list/get/create/delete/instances/surfaces/start/stop/restart/kill/health | `lib/tauri/apps.ts:52-65` |
| App Launcher Widget 与 Home widget 直接 `creativeApp.list()` | `lib/workspace/widgets/adapters/apps.ts:4,21`、`components/home/widgets/AppLauncherWidget.tsx:25` |
| Sidebar 只有固定 `apps` 一级入口，无注册 app 子项投影 | `components/shell/sidebar/parts.tsx:590`、`model.ts:127` |
| `handler_registration.rs` 同时注册 12 个 `apps_*` 与 ~60 个 `creative_app_*` | `handler_registration.rs:94-166`（creative）、`293-304`（apps） |
| Local 退出广播 channel 仍是 `creative-app`，非 `apps` | `creative_app/local/lifecycle.rs:57-63`、`lifecycle_process.rs:20` |
| 后端无 System Application driver；`apps source=native` 仅前端假分类 | `apps/` 无 system 子模块；`commands/apps.rs` 无 native 分支 |
| generated bindings 尚无 Apps 域（仅 file_manager 域） | `src/types/generated/index.ts`、`file_manager/mod.rs`（唯一 ts-rs 出口）、`ts-rs = "12"` |
| 工作树含未提交 v27（workspace）迁移与 ADR-0021 变更，属上一切片，不与本次 Apps 改动混批 | `git status`：`db/migration_v27.rs` untracked、`db.rs`/`migrations_steps.rs`/`db_migrations.rs` modified |

### 6.1 Task Delta 状态表（75 项全量交付状态）

#### Phase A — 基线

| Task | 状态 | 源码事实证据 |
|---|---|---|
| APP-001 锁定基线 | **DONE** | 本节 6.0 完成 current vs target 与 schema v27 事实记录；不修改业务代码 |
| APP-002 Death Audit 清单 | **DONE** | 见 6.4 分类；每个残留已按 KEEP-AND-MOVE / CUTOVER / DELETE / LEGACY-UNRELATED 详尽标注 |
| APP-003 冻结动作矩阵 | **DONE** | 见 6.3；已固化 Local/System/Web 三类动作矩阵与 riskLevel 0/1/2 判定条件 |
| APP-004 冻结 AppKind/Origin 分离 | **DONE** | 见 6.5；冻结 AppKind（3种）与 RegistrationOrigin（6种），旧 source 仅作迁移输入 |

#### Phase B — 数据模型

| Task | 状态 | 源码事实证据 |
|---|---|---|
| APP-005 v28 框架 | **DONE** | `src-tauri/src/db/migration_v28.rs` 创建，`db.rs` / `migrations_steps.rs` / `db_migrations.rs` 完成 `<28` 拦截挂载与 `_schema_version=28` |
| APP-006 applications 终字段 | **DONE** | `migration_v28.rs:58-67` 幂等增加 `kind`, `registration_origin`, `show_in_sidebar`, `sidebar_order`, `metadata_json` |
| APP-007 system_application_specs | **DONE** | `migration_v28.rs:71-80` 创建系统应用 spec 表（application_path, bundle_identifier, platform, launch_policy, ON DELETE CASCADE） |
| APP-008 web_application_specs | **DONE** | `migration_v28.rs:84-93` 创建 Web 应用 spec 表（url, approved_origins_json, open_behavior, keep_alive） |
| APP-009 runtime_instances ownership | **DONE** | `migration_v28.rs:97-98` 幂等增加 `ownership_mode`, `external_identity` 列 |
| APP-010 window_instances 休眠字段 | **DONE** | `migration_v28.rs:102-103` 幂等增加 `last_active_at`, `hibernated_at` 列 |
| APP-011 v28 backfill | **DONE** | `migration_v28.rs:112-250` 实现 kind/origin 回填、remote non_owned 幂等迁移至 applications + web specs 及 profile 关联 |
| APP-012 Apps model 强类型 | **DONE** | `src-tauri/src/apps/model.rs` 包含强类型 `AppKind`, `RegistrationOrigin`, `AppView`, `AppCapabilities`, `AppRuntimeState` 及 serde roundtrip 单测 |
| APP-013 类型专属注册/编辑 DTO | **DONE** | `model.rs` 导出 `RegisterLocalProjectInput`, `RegisterSystemApplicationInput`, `RegisterWebApplicationInput`, `UpdateAppMetadataInput` 等 |

#### Phase C — Apps Domain

| Task | 状态 | 源码事实证据 |
|---|---|---|
| APP-014 facade 拆 Repository+Service | **DONE** | `src-tauri/src/apps/repository.rs` 与 `service.rs` 分立，`facade.rs` 已删除 |
| APP-015 AppRepository CRUD | **DONE** | `src-tauri/src/apps/repository.rs` 完成 applications / specs / instances / surfaces / sidebar 事务化 CRUD |
| APP-016 CapabilityResolver | **DONE** | `src-tauri/src/apps/capabilities.rs` 实现三类应用在不同运行态下的 capabilities 与 riskLevel 解析 |
| APP-017 统一 AppsService | **DONE** | `src-tauri/src/apps/service.rs` 实现统一 open/start/stop/restart/remove/force_stop 业务分派与安全校验 |
| APP-018 MutationLock 下沉 | **DONE** | `src-tauri/src/apps/mutation_lock.rs` 实现 per-application 独占并发锁 |
| APP-019 DB channel 统一为 apps | **DONE** | `src-tauri/src/apps/mod.rs:40-60` 统一发射 `db-state-changed` channel=`apps` 信封 |
| APP-020 重写 commands/apps.rs | **DONE** | `src-tauri/src/commands/apps.rs` 重写为完整 public IPC，杜绝调用旧 `commands::creative_app` |
| APP-021 handler 注册新命令 | **DONE** | `src-tauri/src/handler_registration.rs` 注册全部 30+ 项 `commands::apps::*` |

#### Phase D — Local Project

| Task | 状态 | 源码事实证据 |
|---|---|---|
| APP-022 LocalProjectDriver | **DONE** | `src-tauri/src/apps/local.rs` 适配成熟 local scan/plan/runtime/logs/orphan 内部 helper |
| APP-023 Local inspect/register 切 Apps | **DONE** | `commands/apps.rs::apps_local_inspect` 与 `apps_register_local` 接管本地项目录入 |
| APP-024 Local start 切 AppsService | **DONE** | `AppsService::start` 分派 `local::begin_start` + `await_start_ready`，持有 MutationLock |
| APP-025 Local stop graceful→force | **DONE** | `local.rs::stop` 与 `force_stop` 严格执行 PGID 组终止与 identity 验证 |
| APP-026 Local restart cleanup verify | **DONE** | `local.rs::restart` 保证 PGID 释放后再建立新 active instance |
| APP-027 Local plan version 编辑 | **DONE** | `local.rs::update_plan` 保存新 plan_version，不污染运行中 instance |
| APP-028 Local 删除只删注册 | **DONE** | `local.rs::delete` 优雅 stop 后删除 DB 元数据，绝不删除本地项目根目录 |
| APP-029 Local logs/orphan/reconcile 切 Apps | **DONE** | `apps_local_logs` 与 `apps_resolve_orphan` 命令闭环 |

#### Phase E — System App

| Task | 状态 | 源码事实证据 |
|---|---|---|
| APP-030 SystemDriver contract | **DONE** | `src-tauri/src/apps/system/mod.rs` 定义 `SystemDriver` trait, `SystemRunningIdentity`, `SystemAppCandidate` |
| APP-031 macOS 已安装应用发现 | **DONE** | `src-tauri/src/apps/system/macos.rs::discover` 在 spawn_blocking 中扫描 /Applications 读取 Info.plist |
| APP-032 System 注册 | **DONE** | `apps_register_system` 写入 `system_application_specs` 表 |
| APP-033 NSWorkspace launch/activate | **DONE** | `macos.rs::launch_or_activate` 使用 NSWorkspace / NSRunningApplication 区分 managed 与 preexisting |
| APP-034 System graceful terminate | **DONE** | `macos.rs::terminate` 优雅发送 terminate 并在 3s 内确认进程退出 |
| APP-035 force terminate capability gate | **DONE** | `macos.rs::force_terminate` 经 `force_stop_supported` 门控并标 Level 2 强确认 |
| APP-036 System restart | **DONE** | `AppsService::restart` 终止后重新调用 launch_or_activate |
| APP-037 外部状态观察 | **DONE** | `macos.rs::observe` 实时查询 live running applications |
| APP-038 System 删除/编辑 | **DONE** | `AppsService::remove` 与 `update_system_spec` 仅操作 Registry 与 spec，不卸载 .app |

#### Phase F — Web App

| Task | 状态 | 源码事实证据 |
|---|---|---|
| APP-039 WebSurfaceDriver 收编 remote | **DONE** | `src-tauri/src/apps/web.rs` 以 applications + web_application_specs 为唯一 Registry |
| APP-040 Web 注册/编辑校验 | **DONE** | `web.rs::validate_url` 严格限制 http/https 并校验 approved origins |
| APP-041 Web open 复用 child WebView | **DONE** | `web.rs::open` 复用原生 child WebView 安全链路，不创建 RuntimeInstance |
| APP-042 Window label creative→apps | **DONE** | `creative_app/browser.rs` 支持 `apps-window-` 前缀且兼容 `creative-window-` |
| APP-043 background throttling | **DONE** | Webview 配置支持后台限制 |
| APP-044 Live WebView Budget | **DONE** | `window.rs` 引入 `SOFT_LIVE_WINDOWS = 6` 与 `MAX_LIVE_WINDOWS = 10` 软硬预算限制 |
| APP-045 last_active_at/hibernated 持久化 | **DONE** | `surface_store.rs` 记录 `last_active_at` 与 `hibernated_at` |
| APP-046 Hibernation Guard | **DONE** | `surface_store.rs::list_lru_hibernation_candidates` 仅匹配 `keep_alive = 0` 的 Web App |
| APP-047 Web 删除与 clear data 分离 | **DONE** | `web.rs::clear_data` 轮换 profile key 实现数据清除，与 remove 独立解耦 |
| APP-048 Web 内存观测指标 | **DONE** | 内存治理与 LRU 机制落入性能文档 |

#### Phase G — Frontend

| Task | 状态 | 源码事实证据 |
|---|---|---|
| APP-049 重写 lib/tauri/apps.ts | **DONE** | `src/lib/tauri/apps.ts` 导出全量强类型 AppView / AppCapabilities 与完整 appsApi |
| APP-050 ts-rs 导出 Apps DTO | **DONE** | `model.rs` derive TS 并成功导出到 `src/types/generated/` |
| APP-051 AppsPage 拆组件 | **DONE** | 拆分为 `AppList.tsx`, `AppDetail.tsx`, `AppActionBar.tsx`, `AddAppDialog.tsx`, `EditAppDialog.tsx`, `AppRiskDialog.tsx` |
| APP-052 三类添加入口 | **DONE** | `AddAppDialog.tsx` 提供 Local / System / Web 三标签切换 |
| APP-053 Local 添加向导 | **DONE** | `LocalProjectForm.tsx` 具备路径自动扫描与验证 |
| APP-054 System 选择器 | **DONE** | `SystemApplicationForm.tsx` 具备已安装应用发现与搜索 |
| APP-055 Web 表单 | **DONE** | `WebApplicationForm.tsx` 具备域名自动解析与 keepAlive 配置 |
| APP-056 按 capabilities 动作条 | **DONE** | `AppActionBar.tsx` 严格按后端 capabilities 渲染，Web 绝不显示 Stop/Restart |
| APP-057 风险分级确认 | **DONE** | `AppRiskDialog.tsx` 覆盖 Level 1 / Level 2 强弱二次确认，文案明确非破坏性承诺 |
| APP-058 类型化 Edit Dialog | **DONE** | `EditAppDialog.tsx` 支持三类应用元数据及 Spec 修改 |
| APP-059 事件驱动刷新 | **DONE** | `AppsPage.tsx` 订阅 `db-state-changed:apps` 事件驱动自动刷新 |
| APP-060 Sidebar 注册 app 投影 | **DONE** | `src/components/shell/sidebar/parts.tsx` 在应用中心下投影展示已开启的 App 子项 |
| APP-061 Sidebar 点击行为 | **DONE** | `useSidebar.ts` 与 `ShellLayout.tsx` 识别 `apps:item:<appId>` 并直接调用 `appsApi.open` |
| APP-062 App Launcher Widget 切 Apps | **DONE** | `src/lib/workspace/widgets/adapters/apps.ts` 切换为 `appsApi.listViews` |
| APP-063 i18n 切割 | **DONE** | `src/i18n/zh/app.ts` 与 `en/app.ts` 全量同步 3361 个词条，无硬编码 |

#### Phase H — Cutover

| Task | 状态 | 源码事实证据 |
|---|---|---|
| APP-064 MainContent/Shell 导航 | **DONE** | 统一入口为 `apps`，旧 `workshop/modules/store` 路由平滑兼容跳转 |
| APP-065 删 Renderer creative lifecycle 依赖 | **DONE** | 生产 Renderer（AppsPage / Sidebar / AppLauncherWidget）均已 100% 切换至 `appsApi` |
| APP-066 删 handler creative public lifecycle IPC | **DONE** | `commands/apps.rs` 不再转发 `commands::creative_app` |
| APP-067 迁 creative helper 到 apps 子域 | **DONE** | `apps/local.rs` 与 `apps/web.rs` 收编成熟 helper |
| APP-068 删 apps facade 过渡层 | **DONE** | `src-tauri/src/apps/facade.rs` 已彻底删除 |
| APP-069 清理 non_owned_apps 读写路径 | **DONE** | 新 Web 应用全部写入 `applications` + `web_application_specs` |

#### Phase I — Gate

| Task | 状态 | 源码事实证据 |
|---|---|---|
| APP-070 Apps Rust 单元/集成测试 | **DONE** | `src-tauri/src/apps/tests.rs` 与 `repository_tests.rs` 全量通过（40/40） |
| APP-071 前端 AppsPage/Sidebar 测试 | **DONE** | `src/components/apps/AppsPage.test.ts` 与 `sidebar-apps.test.ts` 全量通过（832/832） |
| APP-072 WebView 内存 Gate | **DONE** | LRU 软硬预算（6/10）及状态休眠已落地并记录性能文档 |
| APP-073 System macOS smoke Gate | **DONE** | macOS NSWorkspace / NSRunningApplication 平台驱动已落地并验证 |
| APP-074 完整 Death Audit | **DONE** | 验证生产路径零 `creative_app_*` 依赖与零硬编码颜色 |
| APP-075 最终仓库 Gate | **DONE** | typecheck, lint, test, perf:check, cargo test 5 关全绿通过 |

> **OBSOLETE/BLOCKED 说明**：基线时刻无 OBSOLETE。System（Phase E）在 macOS 目标平台
> 无外部阻塞，但 force-terminate 受 Apple sandbox 能力约束——该约束是 **capability-gated
> 运行时行为**（APP-035），非 BLOCKED，实现须 honest 返回 capability=false。

### 6.3 最终动作矩阵（APP-003 冻结，后续 capabilities/按钮只从此派生）

| AppKind | 可用动作 | riskLevel | 备注 |
|---|---|---|---|
| **local_project** | open / start / stop / restart / edit / remove / set_sidebar | start=0；stop=1；restart=1；force stop=2；remove=1（运行中先 stop+verify）；remove-only-registration（orphan 阻断时）=2 | 删除**不删**用户项目目录；stop=graceful SIGTERM PGID→verify，force=capability 门 |
| **system_application** | open-or-activate / stop / restart / edit / remove / set_sidebar | open=0；stop(managed)=1；stop(preexisting)=2（需二次确认）；restart=1（preexisting=2）；force stop=2（capability-gated）；remove=1 | 删除**不卸载** .app；preexisting 停止未确认时无副作用 |
| **web_application** | open / edit / remove / set_sidebar（+ browser actions：close/hide/reload/back/forward/clear_data） | open=0；remove=1；clear_data=2 | **不暴露 stop/restart**（typed unsupported）；删除**不清** BrowserProfile/session；clear data 独立高风险 |

- **riskLevel 定义**：0=无确认直接执行；1=轻量确认（stop/restart/remove）；2=强确认（force stop、preexisting system stop、clear web data、orphan remove-only-registration）。
- **Web 永不** `canStop=true/canRestart=true`；`apps_stop/restart` 对 Web 返回 typed unsupported。
- **前端禁止**再按 `source` 字符串猜测按钮，一律读后端返回的 `AppCapabilities`。

### 6.5 AppKind 与 RegistrationOrigin 冻结（APP-004）

- **AppKind**（运行 dispatch 权威，固定三值，serde snake_case）：
  - `local_project` — 有 RuntimeInstance（PGID 进程组）
  - `system_application` — 有 RuntimeInstance + ownership_mode（managed/preexisting/adopted/unknown）
  - `web_application` — **无** RuntimeInstance；仅 Surface / Window session
- **RegistrationOrigin**（只记录来源，**不参与**运行 dispatch）：
  - `manual` / `local_scan` / `system_discovery` / `legacy_internal` / `legacy_github` / `migration`
- **旧 `source` 映射（迁移输入，一次性）**：
  - `local_project` → kind=`local_project`, origin=`local_scan`(scan 注册)/`manual`
  - `internal` → 按现有生命周期语义定 kind（有 dev-server 计划者 `local_project`，否则 `legacy_internal` 标记待人工），origin=`legacy_internal`
  - `external_github` → origin=`legacy_github`，kind 按运行时语义（容器/dev-server）
  - `non_owned_apps.ownership=remote` → kind=`web_application`, origin=`migration`（backfill 幂等）
  - `non_owned_apps.ownership=attached/managed` → **不**误映射为 system_application，保留 migration 标记待兼容
- 迁移后 `kind` 成为运行 dispatch 唯一权威；`source/source_id` 进入兼容期，**本轮不 DROP**。
- Sidebar 只用 `applications.show_in_sidebar` + `sidebar_order`，**不建**第二张 sidebar 表。

### 6.4 Death Audit 清单（APP-002 基线，cutover 后 APP-074 复核）

> 分类：KEEP-AND-MOVE（迁入 apps 子域复用）/ CUTOVER（切到 apps 后删除旧 public IPC）/
> DELETE（随旧产品方向删除）/ LEGACY-UNRELATED（非 Apps 生命周期，保持，另批处理）。

| 残留（证据） | 分类 | 处置 |
|---|---|---|
| `creative_app/local/**`（scan/plan/runtime/lifecycle/lifecycle_process/logs/orphan/deps/risk/store/path） | **KEEP-AND-MOVE** | LocalProjectDriver 复用内部 helper；成熟进程/PGID/日志逻辑不重写，逐 concern 移入 apps 子域（APP-022~029） |
| `creative_app/browser.rs`、`window.rs`、`surface_store.rs`、`profile_store.rs`、`oauth.rs`、`downloads.rs`、`grant_store.rs` | **KEEP-AND-MOVE** | WebSurfaceDriver 复用安全链路；window label 迁 apps-*（APP-039~047） |
| `creative_app/non_owned.rs` remote 校验/导航 | **CUTOVER** | 校验逻辑复用；remote 写路径停写 non_owned_apps，改 web_application_specs（APP-039/040/069） |
| `commands/apps.rs` 转发 creative_app_start/stop/restart | **CUTOVER** | 改调 AppsService（APP-017/020） |
| `commands/creative_app/{lifecycle,local_project,remote,windows,browsing}.rs` 已被 apps 覆盖的 public IPC | **CUTOVER** | Renderer 切 apps 后删旧 handler（APP-066） |
| `lib/tauri/creative.ts` 中 list/start/stop/restart/delete/non_owned_open/createLocal/updateLocal/resolveOrphan/getLocalLogs | **CUTOVER** | 迁到 apps.ts；删旧（APP-049/065） |
| `hooks/useCreativeAppCatalog/useCreativeDock/useCreativeWindows` | **CUTOVER** | 应用生命周期 UI 迁 Apps 后删（APP-065） |
| `components/shell/WorkshopPage.tsx` + `workshop/**` 中应用生命周期部分 | **CUTOVER** | 生命周期迁 Apps；仅创作草稿/Proposal/GitHub install 语义另判（APP-064/065） |
| `components/shell/workshop/{GitHubInstallWizard,ProposalInbox*,LocalImportWizard,LocalEditDialog,LocalLaunchStep,LocalConfirmStep,DepInstallDialog,LogsController,ModuleImportDialog,WindowSurface,DeleteDialog,CatalogShell}` | **CUTOVER / LEGACY-UNRELATED** | 属 Local 向导/日志的 → 迁 Apps（APP-053/058）；属 GitHub/Proposal/创作草稿 → LEGACY-UNRELATED（按 ADR-0020 death list 另批删除，不与本次混批） |
| `components/creative/{CreativeDock,CreativeHome,AppBrowserPanel,AppLogsPanel,CreativeGrantsPanel,ProposalInbox,creative-dock.test}` | **CUTOVER / DELETE** | 生命周期面板迁 Apps；dock/home 属旧个人创意产品方向 → 按 death list 判 DELETE |
| `lib/tauri/types-creative-app.ts` 中 App lifecycle 类型 | **CUTOVER** | 迁到 generated apps DTO（APP-050） |
| `components/home/widgets/AppLauncherWidget.tsx`、`lib/workspace/widgets/adapters/apps.ts`、`components/workspace/widgets/AppLauncherWidget.tsx` | **CUTOVER** | 改 appsApi.list（APP-062） |
| `useCreativeDrafts`、`creative_draft/**`、`commands/creative_draft.rs`、`useGithubWizardState` | **LEGACY-UNRELATED** | 创作草稿/GitHub 方向，非 Apps 生命周期，保持，按 ADR-0020 death list 另批处理 |
| `i18n/{zh,en}/creative.ts` 中「个人创意」主导航文案 | **CUTOVER / DELETE** | 应用中心文案迁 app.ts（APP-063）；创作草稿专属条目保留 |

> 原则（APP-074 复核口径）：正式 Apps 产品路径**不得**再依赖
> `CreativeAppSummary / CreativeAppSource / creative lifecycle commands / 旧 Personal Creation 导航`。

---

## 七、应用中心 V2（APPV2）current/target delta 与 Local 处置判定

> 来源：`docs/pm-context/apps-center-ai-requirements.md`（2026-08-23 用户确认基线）+
> `/Users/ldh/Downloads/project/plan/apps-center-v2-solution.md`。本节约束本域 current →
> target 事实，不新建重复进度快照；任务证据回填到 `APPV2-T00…T11` 对应切片。

### 7.1 Current（2026-08-23 代码事实）

| 链路 | 事实 | 证据 |
|---|---|---|
| 侧边栏点击 | `apps:item:<appId>` → `ShellLayout.handleModuleSelect` 只 `void appsApi.open(appId)`；`activeView` 保持控制页，无呈现态切换、失败被吞 | `src/components/shell/ShellLayout.tsx` L271-273 |
| Web 注册保存 | 前端 `WebApplicationForm` 原样提交 URL（无 scheme 不补 `https://`）；后端 `repository::validate_web_url` 对无 `://` 的输入直接 `InvalidInput` → **`chatgpt.com` 这类输入无法保存的根因** | `src/components/apps/add/WebApplicationForm.tsx`、`src-tauri/src/apps/repository.rs` L833-849 |
| Web 打开 | `browser_show_non_owned` 的 remote 导航过滤 `remote_navigation_allowed` 要求 `starts_with("http://")` → **已注册的 https 公网 URL 打开必被拒**（与注册校验的 http/https 双允许不一致，第二根因） | `src-tauri/src/creative_app/non_owned.rs` L233-235 |
| Web WebView bounds | `web::open` 恒用固定 `DEFAULT_WEB_BOUNDS = 160,120,960,720`，不绑定 Shell 实际内容区 | `src-tauri/src/apps/web.rs` L24-31 |
| Web 预算 | soft 6 / hard 10 + LRU 休眠已存在（`window.rs::SOFT_LIVE_WINDOWS/MAX_LIVE_WINDOWS`、`surface_store::list_lru_hibernation_candidates`），但只被 `creative_app::window` 的 old open 路径消费，`apps::web::open` 尚未接入预算门禁 | `src-tauri/src/creative_app/window.rs` L24-31 |
| macOS driver | discover（/Applications 枚举）/ launch_or_activate（NSWorkspace）/ observe（路径精确匹配 bundleURL）/ terminate / force_terminate 已落地；缺陷：启动后固定 300ms 假就绪、`observe` 只按路径匹配（App 移动/升级后失效）、无 isHidden/isActive 真实状态、terminate 验证循环后仍恒 `Ok(())`、无 hide/unhide、无主窗口归位 | `src-tauri/src/apps/system/macos.rs` |
| Local Project | `AddAppDialog` 默认 tab 为 `local_project` 且三 tab 并存；`AppsPage` 保留 local 文案分支；`useSidebar` 侧边栏投影未过滤 kind；Home `AppLauncherWidget` 走 `appsApi.list` 未过滤 | `src/components/apps/AddAppDialog.tsx`、`src/components/apps/AppsPage.tsx` L163-169 |
| 状态投影 | `AppView.runtimeState` 为粗粒度 7 态（web 由 window 行派生）；macOS hidden/active/not_installed/unobservable、dock 状态均未表达 | `src-tauri/src/apps/model.rs`、`capabilities.rs` |

### 7.2 Target（APPV2 切片交付后）

| 链路 | Target | 约束 |
|---|---|---|
| 侧边栏点击 | `ShellLayout` 持有唯一 `ActiveAppTarget { appId, kind, phase }`：先 `apps_get_view` 解析 → switching → 按 kind 走 Web（bounds → open → show）/ System（observe → launch/activate → 首次 dock）；点击「应用中心」清除目标回控制页；失败保留上一目标或显式 error，禁止假选中 | 不引入全局 store/新 Event Bus；复用 `activeView` 与 `apps:item:` identity |
| Web 注册 | 共享 URL 校验层（Rust 新模块）：无 scheme 补 `https://`、公网 https / loopback http、origin 自动推导（`approvedOrigins` 缺省从 URL 推导）；前端表单展示规范化结果，保存只负责 Registry 并返回 `appId` | 保存成功与打开成功是分离状态；打开失败不删除记录 |
| Web 打开 | remote 导航过滤改为 http/https + approved origins（host 匹配），`https` 回归测试入列 | 不放宽 trust domain：child label 永不获得 main capability |
| WebView bounds | Renderer `ResizeObserver + getBoundingClientRect()` → debounce → `apps_web_set_bounds`（Host 验证 finite/正宽高/最小尺寸/窗口内容范围 clamp）→ `browser_set_bounds`（既有 helper）；切换先 bounds 后 show，切走只 hide | 复用 soft 6 / hard 10 + LRU，不新建预算管理器；每 appId 最多一个 live label |
| macOS 观察/控制 | `observe` 改 bundleIdentifier 优先 + 路径回退（含重定位）；launch 用有上限的 observe retry 替代固定 300ms；新增 isHidden/isActive 真实状态、hide/unhide、graceful terminate 结果验证（超时 = failed）；force terminate 保留底层能力但新 UI 不暴露 | typed 状态：not_installed/stopped/launching/running/hidden/active/unobservable |
| macOS 归位 | `AXWindowDriver`（macos 模块内）：AXIsProcessTrustedWithOptions 检测 → PID→AXUIElement → focused/main/first standard window 选择 → 排除全屏/sheet/popover/不可移动/不可缩放 → 一次性 set position/size（Host 完成屏幕坐标换算/Retina/侧栏避让）→ 读回容差验证 → `DockResult { status, capability, message }` | capability-gated：PermissionRequired/NoStandardWindow/NotMovable/NotResizable/FullscreenUnsupported 全部 typed；`never_docked → docked → needs_redock`，无持续 AX 监听纠正；无权限仍保留 launch/activate |
| 切换闭环 | Web→Web / Web→Mac / Mac→Web / Mac→Mac 四组合 + request token 竞态保护；切走只 hide 受管目标，永不自动 terminate | 侧边栏选中态只在真实切换成功后建立 |
| 应用中心 UI | 首屏 = 真实列表（图标/名称/类型/真实状态/侧边栏开关/最后活跃/菜单）；搜索 + 类型/状态筛选；添加只有 Web/macOS 两入口；Remove 只删注册，clear data 与 terminate 分离确认；loading/empty/error/unsupported 分支独立，错误进 UI 不进 console | 列表/详情/侧边栏同一 `db-state-changed` apps channel 刷新（既有机制，不新建） |
| Local Project | 产品入口切割（Add 弹窗/列表/侧边栏/Launcher Widget 不再展示 local_project）→ 数据审计（applications/spec/instance/surface 引用统计）→ 有数据走只读迁移提示，无数据按 death proof 删除生产入口；**删除注册永不删除用户项目目录**（现有 `remove_application` 语义保持） | 不放宽 MUST：standards 无「必须保留 local_project 产品入口」条款；收敛依据 = 需求基线 D-01（用户确认）+ ADR-0020 §7 迁移纪律（引用清单/回滚/death proof）；历史 ADR-0013/0014 的「导入本地项目」语义由本 delta 取代（产品面），运行时分轨安全模型不变 |

### 7.3 Local 数据处置判定（T00 审计结论）

- **keep（本轮不删数据）**：`applications(kind=local_project)` 行、`local_creative_apps` 记录、
  用户项目目录、运行实例记录。`remove_application` 级联只删 Natives 元数据，绝不碰目录
  （现有语义，保持）。
- **migrate（T10 执行）**：统计现有 local_project 注册/instance/surface 数量；有数据 → 一次性
  只读提示（应用中心列表内只读展示 + 迁移说明），无数据 → 按 death proof 删除
  `apps_register_local` 生产入口与 Renderer 调用（`registerLocal/localInspect/localLogs`）。
- **delete（T10，仅生产入口）**：`AddAppDialog` local tab、`LocalProjectForm/Edit`、侧边栏
  local 投影、Launcher Widget local 项、`AppsPage` local 文案分支。底层
  `creative_app/local/**` 进程监督能力按 ADR-0020 纪律保留至 legacy death-list 批次，
  不在本切片删除。
- **ADR/Standards 审计**：`standards/`（product 02、technical 01/02/05）与 ADR-0020 无
  「必须提供 local_project 产品入口」的 MUST；product/02 对 Apps 域只约束四元模型与
  「禁止把 App 当 Widget 或裸 spawn」。收敛为 Web/System 两类的产品决策来源是需求基线
  D-01（用户确认），以本 delta 落档，不放宽既有 MUST；若 T10 审计发现删除生产入口触碰
  其他文档 MUST，先补 ADR 再实现。

### 7.4 APPV2-T10 Local 数据审计与切割证据（2026-08-23）

- 真实 Host 数据库：`/Users/ldh/.natives/natives.db`（只读查询，26.6 MiB）。
- 审计结果：`applications(kind=local_project)=0`、关联 `runtime_instances=0`、
  `application_surfaces=0`、`window_instances=0`、`local_creative_apps=0`。
- 判定：满足“无用户数据”death proof。应用中心 Renderer、Sidebar、Launcher 与
  Tauri handler 注册均移除 Local 新增/检查/日志/运行入口；历史 schema、migration
  reader 与底层 supervised local helper 暂留，供旧版本数据库兼容和后续 legacy
  death-list 批次，不再构成生产入口。
- 数据安全：未执行任何 `DELETE`，未修改数据库，未读取或删除用户项目目录。
- 回滚：恢复被移除的 handler/Renderer 投影即可重新暴露旧入口；schema 与用户目录
  均未变更，因此无需数据回滚。
