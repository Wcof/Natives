# 本地创意（local_project）整改实施方案

> **领域**: 个人创意 / 三来源统一管理（Workshop + GitHub 容器 + 本地项目）
> **来源边界**: [ADR-0013](../adr/0013-creative-app-dual-source.md)（双来源冻结）+ 本地项目为第三来源
> **约束映射**: [`standards/`](../standards/README.md) technical/02-security · 03-data
> **基石源码**: `src-tauri/src/creative_app/local/` · `src/components/shell/WorkshopPage.tsx`
> **状态**: 实施中（分支 `feat/creative-app-local-remediation`）
> **日期**: 2026-07-24

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
- [ ] F8 卡片复制地址/系统浏览器
- [ ] F9 移除死复选框
- [ ] 更新 ADR-0013 落地清单勾项 + standards technical/02·03 相关条目
- [ ] i18n 中英文同步（新增表单字段与卡片动作文案）
