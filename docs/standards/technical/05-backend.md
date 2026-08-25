# 技术架构 05 · 后端（Rust）编码规范

> **版本**: 2.0.0 · **日期**: 2026-08-19
> **关联 ADR**: [ADR-0020](../../adr/0020-ai-native-personal-workspace-rearchitecture.md)、[ADR-0008](../../adr/0008-electron-to-tauri-migration.md)
> **关联源文件**: `src-tauri/`（目标 Native Backend）、`crates/`（共享 codec/contract）、`src-agent-daemon/`（迁移期 legacy）、`src-tauri/src/error.rs`、`src-tauri/src/log_sanitizer.rs`、`src-tauri/src/sidecar_supervisor.rs`
> **关联指南**: [`docs/architecture/CODE_MODULE_GUIDELINES.md`](../../architecture/CODE_MODULE_GUIDELINES.md)

---

## 一、本篇要约束什么

目标 Rust 后端以 **Tauri Host** 为默认 authority；`crates/*` 只承载真正共享或协议中立的深 module；Agent Daemon 是迁移期 legacy。进程边界见 [`technical/01`](01-layering.md)。本篇约束错误处理、日志脱敏、共享代码、异步运行时、子进程监督与模块规模。

---

## 二、错误处理与日志

#### R-B1 · 命令/RPC 路径统一走 `error.rs` 的 Result，禁止 panic
- **等级**：MUST
- **分类**：错误处理、稳定性
- **规则**：Host 侧所有 Tauri 命令与可被外部触发的执行路径**必须**返回 `src-tauri/src/error.rs` 定义的 `Result<T>`（`thiserror` 枚举 + `Serialize` 给前端）；Daemon RPC 路径同理返回结构化错误。命令/RPC 路径上**禁止** `unwrap()` / `expect()` / `panic!` 处理可预期失败（锁中毒、测试代码、真正不可达的不变量除外，且须注释说明为何不可达）。
- **正例**：`fn get_project(id: &str) -> Result<Project> { ... .ok_or_else(|| Error::NotFound(id.into())) }`
- **反例**：`let conn = db.lock().unwrap(); let row = stmt.query_row(...).expect("must exist");`（用户一次坏输入即拉崩整个 Host 进程）
- **为什么**：Host panic 等于整个工作台崩溃；结构化错误才能被前端错误分类展示（见 `product/02`），也才符合协议「三态诚实」而非假绿。
- **检查方法**：`grep -n "unwrap()\|expect(" src-tauri/src/commands/`，逐条确认是否在命令路径上；新命令签名必须是 `-> Result<...>`。

#### R-B2 · 敏感内容出日志前必须过 `log_sanitizer::sanitize`
- **等级**：MUST
- **分类**：安全、日志
- **规则**：任何可能含 API key、token、Bearer 头、凭证或用户主目录路径的字符串，在写入 stdout / stderr / 日志文件前**必须**经 `src-tauri/src/log_sanitizer.rs` 的 `sanitize()`（Daemon 侧同类日志走等价脱敏）。发现新的敏感 pattern 时**必须**补进 sanitizer，而不是在调用点手工截断。
- **正例**：`info!("Provider response: {}", sanitize(&raw_body));`
- **反例**：`eprintln!("[provider] request failed: {req:?}")`（Debug 打印整个请求，Authorization 头原样落盘）
- **为什么**：日志是凭证泄漏最常见的出口；与 `technical/02` 的凭证红线（R-S12、R-S13）同源——加密存储的凭证不能从日志侧漏出去。
- **检查方法**：`grep` Provider/凭证相关模块中的 `println!\|eprintln!\|info!\|error!\|{:?}`，确认原始报文/请求体均经 `sanitize`。

---

## 三、代码共享与协议单一定义

#### R-B3 · 可复用深逻辑归明确 module，禁止复制
- **等级**：MUST
- **分类**：分层、可维护性
- **规则**：两个真实 caller/adapter 共享的类型、codec、算法或流程**必须**放入职责明确的 `crates/` module；Host 专属数据库、Keychain、进程和 HTTP policy **必须**留在 Host。**禁止**为 legacy 兼容复制实现，也禁止把浅 pass-through wrapper 当共享抽象。
- **反例**：`src-tauri/` 与 `src-agent-daemon/` 各有一份内容雷同的重试/解析函数，修 bug 只改了一边。
- **为什么**：共享应提高 interface leverage/locality，而不是延续旧 runtime ownership。
- **检查方法**：Review 时发现两侧出现同名/同形函数即要求下沉；新共享需求先看 `crates/` 是否已有归属。

#### R-B4 · 活跃跨进程协议只有一个 Source of Truth
- **等级**：MUST
- **分类**：协议、分层
- **规则**：仍在使用的 Host↔sidecar 消息、方法与类型**必须**唯一定义在一个 Rust contract crate；前端 binding 必须生成或通过同步检查，**禁止**手写影子定义。现有 Daemon 协议改动仍**必须**运行 `npm run protocol:check`，直到该路径被删除。
- **反例**：在 `src-agent-daemon/` 里私加一个未登记的 RPC 变体字段，前端 TS 类型没同步，运行时静默丢字段。
- **为什么**：迁移期协议漂移会制造无法删除的兼容分支。
- **检查方法**：改动触及 `crates/assistant-protocol` 或 `src/lib/assistant-protocol/` 时，CI/本地必须有 `protocol:check` 通过记录。

---

## 四、Tauri 命令组织

#### R-B5 · 新增 Tauri 命令归属域文件并在 `lib.rs` 注册
- **等级**：SHOULD
- **分类**：命名、结构
- **规则**：新增 Tauri 命令**应该**放进 `src-tauri/src/commands/` 下对应域文件（如 `git.rs`、`terminal.rs`、`module.rs`），无合适域时新建域文件，并在 `src-tauri/src/lib.rs` 的 handler 列表注册；channel 命名遵守 R-T5 的 `domain:action`（此处引用，不重复）。**不应该**把命令散落在 `lib.rs` 或无关模块里。
- **为什么**：按域聚合才能做权限审计与 R-T1 边界检查；`lib.rs` 只做装配，不做业务。
- **检查方法**：新命令 PR 中，`commands/<domain>.rs` 与 `lib.rs` 注册两处同时出现；`lib.rs` 中不出现命令函数体。

---

## 五、异步运行时与子进程

#### R-B6 · 阻塞 IO / 长计算不得直接占用异步运行时线程
- **等级**：SHOULD
- **分类**：性能、稳定性
- **规则**：在 async 上下文中，同步文件 IO、SQLite 长事务、压缩/哈希等 CPU 密集计算**应该**通过 `tokio::task::spawn_blocking`（或 Tauri 的 blocking 变体、专用工作线程）执行；**不应该**在 async fn 里直接跑会阻塞数百毫秒以上的同步调用。长期占用的循环（轮询、watch）用专用线程或独立 task，不与请求处理抢 worker。
- **反例**：async 命令里直接 `std::fs::read` 一个数百 MB 文件，期间同一 runtime 上的其它命令与事件全部卡住。
- **为什么**：Tauri/Tokio 的 worker 线程是全局共享的，一处阻塞放大为整个后端「假死」，直接违反 `technical/04` 的响应预算。
- **检查方法**：Review async 路径中的同步重操作；性能改动按 `technical/04` 跑 `npm run perf:check`。

#### R-B7 · 子进程必须纳入 `sidecar_supervisor` 监督
- **等级**：MUST
- **分类**：进程、安全
- **规则**：Host 侧启动的常驻/业务子进程（AI 工具、应用 runtime、迁移期 Daemon 等）**必须**经 `src-tauri/src/sidecar_supervisor.rs` 或同一受监督基础纳入生命周期管理（看门狗、退出回收、重启策略）；**禁止**裸 `Command::spawn` 后不跟踪句柄、不回收退出状态。一次性短命令也**必须**等待或显式回收。
- **为什么**：这是五大防线之防线 1（R-S1）的后端落点——无监督的子进程意味着孤儿进程、端口/文件句柄泄漏和不可观测的静默失败。
- **检查方法**：`grep -n "Command::new\|spawn(" src-tauri/src/` 中新出现的 spawn 点，确认走 supervisor 或有明确 `wait`/回收路径。

---

## 六、数据访问与模块规模

#### R-B8 · 数据库访问遵守 technical/03 与 R-T2 的分权威
- **等级**：MUST
- **分类**：数据
- **规则**：后端 SQLite 访问**必须**遵守 [`technical/03`](03-data.md)（迁移、WAL、外键、原子写入）与 R-T2 的单一 authority：目标领域归 Host；Legacy Daemon 库只允许迁移前维护，禁止新增目标 schema 或双写。本篇不另立数据规则。
- **为什么**：数据权威已有唯一出处，重复定义只会产生冲突版本。
- **检查方法**：见 `technical/03` 各条与 R-T2 的检查方法。

#### R-B9 · 文件规模与模块拆分遵守 CODE_MODULE_GUIDELINES
- **等级**：SHOULD
- **分类**：结构、可维护性
- **规则**：Rust 文件规模、职责拆分、状态唯一归属**应该**遵守 [`CODE_MODULE_GUIDELINES.md`](../../architecture/CODE_MODULE_GUIDELINES.md)：按职责拆分而非按行数机械拆分；**不应该**出现 `production_part1.rs` 式无语义切割或 `utils.rs`/`helpers.rs` 式复杂度掩盖。
- **为什么**：超大文件与超级管理器是后端可维护性的主要退化路径；该指南已给出可操作阈值，规范侧只需引用。
- **检查方法**：新增/膨胀明显的 `.rs` 文件对照指南第 1 节的拆分依据自查。

#### R-B10 · Workspace 数据权威在 Host SQLite，链路不得引入第二权威
- **等级**：MUST
- **分类**：数据
- **规则**：Workspace（含布局）权威数据**必须**在 Host SQLite（见 ADR-0021 与 `docs/contracts/workspace-v2-contract.md`），数据流保持 `SQLite / Local Files -> Domain Service -> Typed IPC -> Renderer Snapshot/UI`；Renderer **禁止**直接访问 SQLite，**禁止**以 IndexedDB / localStorage 作为 Workspace 权威源。
- **反例**：Renderer 把布局写进 localStorage 并以其为恢复来源；Workspace 命令绕过 Domain Service 直连数据库连接池。
- **检查方法**：Workspace 相关命令与 IPC handler 中不出现 Renderer 侧 DB/IndexedDB/localStorage 权威路径。

#### R-B11 · 布局持久化在停止/flush 时发生，指针移动零写库
- **等级**：MUST
- **分类**：性能、数据
- **规则**：Home Grid/Free Canvas 的 Pointer Move（drag/resize/pan/zoom 手势期间）**必须**只更新内存；**必须**在 Pointer Stop、Resize Stop 或 debounce/flush 时才持久化布局，move 路径对布局数据的写库次数必须为 0。
- **检查方法**：布局 move 路径 `grep` 不得出现 DB 写入；持久化只出现在 stop/debounce/flush 处理器。

#### R-B12 · 序列化契约与 Managed 运行时状态管理
- **等级**：MUST
- **分类**：协议、架构
- **规则**：
  - 公共 DTO 结构体（如 `AppView`）必须显式标注 `#[serde(rename_all = "camelCase")]` 与 `#[ts(export)]`，确保前后端字段命名完全一致，严禁出现前端 `undefined` 字段回退；
  - 具备跨命令生命周期的运行时资源（如 Apps `BrowserState`）**必须**由 Tauri `manage()` 单例容器托管（`State<'_, BrowserStateHandle>`），**严禁**在每个 command 内部无状态新建局部实例导致状态分裂。
- **为什么**：见 ADR-0022。保证协议单一 Source of Truth 与运行时生命周期的唯一权威。

---

## 七、本篇合规自检清单

- [ ] 新增/修改的命令与 RPC 路径均返回结构化 `Result`，无命令路径 `unwrap`/`expect`（R-B1）。
- [ ] 可能含凭证/token 的日志均过 `sanitize`，新 pattern 已补进 sanitizer（R-B2）。
- [ ] 无 Host/legacy 复制实现；共享 crate 是深 module，Host policy 未错误下沉（R-B3）。
- [ ] 活跃协议只有一个 Source of Truth；现有 Daemon 协议改动通过 `protocol:check`（R-B4）。
- [ ] 新命令落在 `commands/` 域文件并在 `lib.rs` 注册，命名符合 R-T5（R-B5）。
- [ ] async 路径无未卸载的阻塞 IO / 长计算（R-B6）。
- [ ] 子进程全部纳入 `sidecar_supervisor` 或有显式回收，无裸 spawn（R-B7）。
- [ ] DB 访问符合 `technical/03` 与 R-T2 分权威（R-B8）。
- [ ] 文件规模与拆分符合 CODE_MODULE_GUIDELINES（R-B9）。
- [ ] Workspace 数据权威在 Host SQLite；Renderer 不直接访问 SQLite、不以 IndexedDB/localStorage 为权威（R-B10）。
- [ ] 布局 Pointer Move 零写库，Pointer Stop/Resize Stop 或 debounce/flush 才持久化（R-B11）。

---

## 八、关联指南

- 进程边界、依赖方向、IPC/协议命名：[`technical/01-layering.md`](01-layering.md)（R-T1～R-T5）
- 五大防线与凭证红线：[`technical/02-security.md`](02-security.md)（R-S1、R-S12、R-S13）
- 数据与持久化：[`technical/03-data.md`](03-data.md)
- 性能预算与主线程纪律：[`technical/04-performance.md`](04-performance.md)
- 文件/函数规模、状态唯一归属：[`CODE_MODULE_GUIDELINES.md`](../../architecture/CODE_MODULE_GUIDELINES.md)
- Legacy Daemon 删除前现状：[`NATIVE-DAEMON-CAPABILITY-MAP.md`](../../architecture/NATIVE-DAEMON-CAPABILITY-MAP.md)

冲突时以本 standards 篇的 MUST 为准。
