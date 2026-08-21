# ADR-0015: 任务模块归属与派发接缝

- **状态**: 已被 [ADR-0020](./0020-ai-native-personal-workspace-rearchitecture.md) 取代（Jobs 只允许迁移与删除工作）
- **日期**: 2026-07-26
- **决策者**: 技术方（工程归属拍板；产品需求由用户提出）
- **关联**: [ADR-0012](./0012-product-identity-workshop-scope.md)、`docs/architecture/NATIVE-DAEMON-CAPABILITY-MAP.md`、`docs/superpowers/specs/2026-07-26-native-harness-control-plane-design.md`
- **归类**: Hub 面（任务定义 / 调度 / 运行历史）；执行链路经助理模块 → 执行引擎（`run.*` 单执行接口）；不新增 web-module / capability 轨道实体

---

## 上下文

用户需要「一次性 / 周期性自动化任务」：内容制定后交由助理模块转执行引擎执行，并可配置能力模块（agent profile / capability 引用）。但调度能力目前在仓库里**三处割裂**，没有任何一处能承载这个需求：

1. **Host 侧死代码桩** — `src-tauri/src/scheduler.rs`。文件顶部即 `#![allow(dead_code)]`；`start_scheduler()`（第 50 行）的 loop 体是空的（注释「完整实现在后续迭代补齐」），且**全仓库无任何调用点，从未启动**。更严重的是 `scheduler_run_task_now`（第 122 行）硬编码返回 `{"status":"started"}`、`scheduler_list_runs`（第 127 行）恒返回空数组——这两个命令在 `lib.rs`（第 667–672 行）注册并暴露给前端，属于**假数据，直接违反 R-F2**（无假数据红线）。

2. **引擎侧过渡实现** — `src-agent-daemon/src/scheduler_store.rs`。这是唯一**真正工作**的调度器：作业以 JSON 持久化在 `NATIVES_RUNTIME_DIR/scheduler/jobs.json`，`spawn_loop(15)`（第 462 行）以 15 秒 tick 扫描到期作业，`fire_job` 经 `RunManager::start_detached_global(StartRunRequest)` 走 run 主链拉起执行。但作业**不进 Host SQLite**（任务定义游离在数据权威之外），拉起的 run 也不登记 `task_store`，在 `task.list` 中**不可见**——用户无法在统一任务视图里看到调度产生的运行。

3. **两份文档归属声明冲突** — `docs/architecture/NATIVE-DAEMON-CAPABILITY-MAP.md` 第 82 行把「调度」列为引擎（daemon）能力域（`scheduler_store` / `scheduler.*`），第 159 行进一步描述其「作业 JSON + due tick」实现；而 `docs/superpowers/specs/2026-07-26-native-harness-control-plane-design.md` 第 218–220 行明确「Jobs are an independent module and are not owned by the Native execution engine」。归属没有裁决，割裂只会继续加深。

三处唯一的共识是能力地图第 266 行的红线：**UI / scheduler / 子编排不得绕过 `ExecutionAuthority` / `run.*` 单执行接口**。本 ADR 保留并强化这条红线。

---

## 决策

### 1. 任务模块是独立的 Hub 域模块，不归执行引擎

- 采纳 harness 控制面设计（第 218–220 行）的归属裁定：**任务模块（Job Module）是独立模块，不归 Native 执行引擎所有**。
- 按 ADR-0012 三面划分，任务模块属 **Hub 面**（工作台自动化基础设施），与终端、Agent 接入、凭证同级；不是 Workshop 域，也不进引擎能力域。
- **数据权威在 Host**：复用并扩展 Host SQLite 既有 `scheduled_tasks` + `task_runs` 两张表（条件 ALTER 补列，表名不改）。
- 命名：中文名「**任务**」，代码名 **job**（`src-tauri/src/jobs/`、`job_*` 命令、i18n 命名空间 `jobs.*`），避让 daemon 运行时已占用的 `task.*` RPC 命名。

### 2. 调度判定归任务模块，执行不归它

模块边界按「何时」与「怎样」切开：

| 职责 | 归属 |
|------|------|
| 何时到期（schedule 解析、next_run 计算、到期扫描） | 任务模块，Host 侧 30 秒 tick 常驻 loop |
| 怎么执行（会话、Run、流式事件、工具、权限） | **不归任务模块**——统一经派发接缝委托助理模块，最终走 `ExecutionAuthority` / `run.*` 单执行接口 |

**禁止旁路**：任务模块任何路径不得绕过单执行接口自行拉起执行，与能力地图第 266 行红线一致。schedule 计算（`schedule.rs`）实现为可注入时钟的纯函数，独立可测。

### 3. 两条预留接缝

- **接缝 A — 派发（JobDispatcher）**：`src-tauri/src/jobs/dispatch.rs` 定义 `JobDispatcher` trait（`is_wired()` + `dispatch(job, trigger, scheduled_at)`）。生产默认适配器现为 `NativeJobDispatcher`，经 Host `daemon_authority::create_run/start_run` 进入 UDS Daemon `run.*` 主链；计划触发使用原计划时间生成幂等键，手动触发使用点击时间。`NotWiredDispatcher` 仅保留为 fail-closed 契约与测试适配器。
- **接缝 B — 能力绑定**：新命令和 UI 使用协议现有 `CapabilitySelection`，仍复用 `capability_refs` JSON TEXT 列，不新增权威表。旧空数组读取为显式空选择；旧非空无类型数组只读兼容，派发时返回 `JOB_INVALID_CAPABILITY_SELECTION`，禁止猜测 Skill/MCP 类型；新旧字段同时提交直接拒绝。

### 4. 引擎侧 scheduler_store 收敛落地

P1 已完成以下收敛：

1. Host 启动时把 `NATIVES_RUNTIME_DIR/scheduler/jobs.json` 单事务导入 `scheduled_tasks`；按 ID 幂等，同 ID 内容冲突整批回滚并 fail-closed；
2. 成功后保留源 JSON、只读备份与版本化完成 marker，不直接删除用户数据；
3. 15 秒 Daemon tick 与 `scheduler.*` 广告/分发退役，到期判定只由 Host Job 30 秒 tick 承担；
4. `src-agent-daemon/src/scheduler_store.rs` 已删除，Cron 只剩 Host `jobs/schedule.rs` 一份实现，无需再为消除重复而下沉共享 crate；
5. Host Runner 用 `daemon_authority::get_run` 对账非终态 Job Run；Daemon 不可达时保持本地状态，不伪造失败。

### 5. Host 旧 scheduler.rs 死代码删除

`src-tauri/src/scheduler.rs` 整文件删除，并清理 `lib.rs` 第 42 行模块声明与第 667–672 行的 6 个命令注册。理由：loop 从未启动、`run_now` / `list_runs` 返回假数据违反 R-F2，且其可用部分（表 CRUD）由任务模块的 `job_*` 命令面完整替代。对该文件应用删除测试：删除后复杂性不在任何调用者处重现——它是纯粹的死代码，不是浅模块。

### 6. 对 NATIVE-DAEMON-CAPABILITY-MAP.md 的修订声明

`NATIVE-DAEMON-CAPABILITY-MAP.md` 已同步改为 Host Job Module 归属；Daemon 不再广告 `scheduler.*`，第 7 节单执行接口红线不变、继续有效。

---

## 后果

### 正面

- **局部性**：任务定义、调度判定、运行历史集中在任务模块一处；到期计算的 bug、schedule 语义的变更、历史查询的需求都只改一个地方，不再散落在 Host 桩 / 引擎 store / 前端三处。
- **杠杆率**：派发接缝是深接口——一个 `JobDispatcher` trait（两个方法）背住引擎的全部复杂度（会话创建、Run 生命周期、流式事件、权限门禁、供应商路由）；任务模块与前端只依赖这个小接口。
- **可测性**：schedule 解析与 next_run 计算是可注入时钟的纯函数，`tick_once()` 与 store 分离，均可脱离常驻 loop 单测。
- **诚实性**：删除假数据桩 + NotWired 三态展示，消除现存的 R-F2 违规面。

### 负面 / 成本

- 无类型的旧非空 `capability_refs` 无法安全自动迁移；这类 Job 会保留定义，但派发时明确失败，需用户重新选择能力。
- 自动化验证不能替代真实 Provider 桌面验收；手动/定时各一次真实 Run 仍是发布闭环条件。

---

## 落地检查清单

- [x] `src-tauri/src/jobs/`（store / schedule / runner / dispatch / migration）+ `commands/jobs.rs` 落地，`lib.rs` 注册（R-B5）
- [x] `scheduled_tasks` / `task_runs` 条件 ALTER 补列（迁移不 DROP / rebuild）
- [x] `NativeJobDispatcher` 经 Host `daemon_authority` 接入 UDS `run.*`；显式 `scheduled_at` 保证重试幂等
- [x] `CapabilitySelection` 新字段写入 + 旧 `capability_refs` 一版兼容读取
- [x] `src-tauri/src/scheduler.rs` 删除，`lib.rs` 声明与命令注册清理
- [x] `jobs.json` 事务迁移、只读备份、marker、冲突回滚与 Run 状态对账
- [x] Daemon `scheduler_store` / loop / RPC 广告退役，能力地图同步

---

## 修订

| 日期 | 变更 |
|------|------|
| 2026-07-26 | 初版，裁定任务模块 Hub 归属、派发与能力绑定双接缝、scheduler_store 过渡定位、Host 死代码删除 |
| 2026-07-29 | P1 收敛落地：Native Dispatcher、CapabilitySelection 兼容、Run 对账、旧 JSON 事务迁移与 Daemon scheduler_store 删除 |
