# ADR-0015: 任务模块归属与派发接缝

- **状态**: 已接受
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

- **接缝 A — 派发（JobDispatcher）**：`src-tauri/src/jobs/dispatch.rs` 定义 `JobDispatcher` trait（`is_wired()` + `dispatch()`）。**P0 唯一适配器是 `NotWiredDispatcher`**（`is_wired()=false`，派发恒返回 `NotWired`），前端据此三态诚实展示「执行链路未接线」，禁止假绿 / 假进度（R-F2）。P1 增加第二个适配器对接助理模块，把 JobDefinition 映射为 CreateRunRequest / StartRunRequest 走 run 主链——届时两个适配器让接缝成为真实接缝。
- **接缝 B — 能力绑定**：`agent_profile_id` + `capability_refs` 字段**只存不校验**，校验延迟到派发期执行。原因：能力 profile 体系正在并行完善，此刻钉死校验规则只会制造联动返工；派发期校验天然覆盖「创建时合法、执行时已失效」的场景。

### 4. 引擎侧 scheduler_store 定位为过渡实现

`src-agent-daemon/src/scheduler_store.rs` 当前可工作，**保留运行，暂不动它**（该文件有人并行修复中）。P1 派发接缝接通时收敛：

1. 其 JSON 作业（`jobs.json`）迁入任务模块的 `scheduled_tasks` 表，数据权威归一到 Host；
2. 其 15 秒 tick loop 退役，到期判定统一由任务模块 30 秒 tick 承担；
3. 其 cron 解析（实现已支持 `*` / `N` / `*/N` / `A-B` / `A,B,C`，但文件头注释仍写着仅支持简化形式——注释过时）与任务模块的 cron 解析**下沉为共享 crate**，消除双份实现——同时偿还 R-B3（重复实现收敛）的债。

### 5. Host 旧 scheduler.rs 死代码删除

`src-tauri/src/scheduler.rs` 整文件删除，并清理 `lib.rs` 第 42 行模块声明与第 667–672 行的 6 个命令注册。理由：loop 从未启动、`run_now` / `list_runs` 返回假数据违反 R-F2，且其可用部分（表 CRUD）由任务模块的 `job_*` 命令面完整替代。对该文件应用删除测试：删除后复杂性不在任何调用者处重现——它是纯粹的死代码，不是浅模块。

### 6. 对 NATIVE-DAEMON-CAPABILITY-MAP.md 的修订声明

自本 ADR 起，`NATIVE-DAEMON-CAPABILITY-MAP.md` 中「调度」归属的相应条目（第 82 行能力域表、第 159 行模块职责表、第 203 行 RPC 面）**按新归属解释**：`scheduler_store` / `scheduler.*` 是引擎侧的过渡实现，调度能力的目标归属是 Hub 面任务模块；第 266 行单执行接口红线不变、继续有效。**本 ADR 不直接修改该文档**——引擎侧文件他人并行开发中，文本对齐留待 P1 收敛时随代码一并落盘。

---

## 后果

### 正面

- **局部性**：任务定义、调度判定、运行历史集中在任务模块一处；到期计算的 bug、schedule 语义的变更、历史查询的需求都只改一个地方，不再散落在 Host 桩 / 引擎 store / 前端三处。
- **杠杆率**：派发接缝是深接口——一个 `JobDispatcher` trait（两个方法）背住引擎的全部复杂度（会话创建、Run 生命周期、流式事件、权限门禁、供应商路由）；任务模块与前端只依赖这个小接口。
- **可测性**：schedule 解析与 next_run 计算是可注入时钟的纯函数，`tick_once()` 与 store 分离，均可脱离常驻 loop 单测。
- **诚实性**：删除假数据桩 + NotWired 三态展示，消除现存的 R-F2 违规面。

### 负面 / 成本

- **P0 期任务不可执行，只可管理**：用户能创建、编辑、启停任务并看到 next_run，但「立即执行」被禁用并展示未接线原因。这是刻意取舍——先立数据权威与接缝，不为赶执行链路而旁路单执行接口或伪造进度。
- **cron 解析短期存在两份实现**（任务模块 + scheduler_store 的简化版），记为已知债务，P1 下沉共享 crate 时偿还（见第 4 节）。
- 文档归属表述在 P1 收敛前存在「ADR 声明与能力地图原文不一致」的窗口期，以本 ADR 第 6 节为准。

### 中性

- 引擎侧 `scheduler_store` 在过渡期继续服务其现有调用方，行为不变；ADR-0012 的三面 / 双轨模型不受影响，任务模块只是 Hub 面新增一域。

---

## 落地检查清单

- [ ] `src-tauri/src/jobs/`（store / schedule / runner / dispatch）+ `commands/jobs.rs` 落地，`lib.rs` 注册（R-B5）
- [ ] `scheduled_tasks` / `task_runs` 条件 ALTER 补列（迁移不 DROP / rebuild）
- [ ] `NotWiredDispatcher` 为 P0 唯一适配器；前端「立即执行」禁用态 + 未接线文案（中英 i18n 同步）
- [ ] `src-tauri/src/scheduler.rs` 删除，`lib.rs` 声明与命令注册清理
- [ ] P1：第二个 Dispatcher 适配器对接助理模块；`jobs.json` 迁移；scheduler_store loop 退役；cron 解析下沉共享 crate
- [ ] P1：NATIVE-DAEMON-CAPABILITY-MAP.md 调度条目文本对齐本 ADR

---

## 修订

| 日期 | 变更 |
|------|------|
| 2026-07-26 | 初版，裁定任务模块 Hub 归属、派发与能力绑定双接缝、scheduler_store 过渡定位、Host 死代码删除 |
