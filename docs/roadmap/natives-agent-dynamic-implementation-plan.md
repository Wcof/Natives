# Natives Agent Runtime 动态实施计划

> 当前结论：`PLAN_READY`。计划基线为 `deploy@9584c3c263c1e83b1066e4208e1ab2d678a9deeb`。本轮只生成计划，没有修改生产代码。

## 1. 动态划分结果

- 实施批次：9 个，其中 7 个代码批次、1 个基线批次、1 个最终验收批次。
- 任务：18 个（TASK-000–017）。
- P0 实施任务：4 个（001、002、004、005）；P0 问题 8 项全部覆盖。
- P1 实施任务：9 个（000、003、006–012，其中 000 同时负责验证基线）。
- P2/后置任务：4 个（013–016）；最终验收 017 是 release blocker。
- 安全并行组：`002 || 003`、`015 || 016`。
- 最长关键路径：`000 → 001 → 002/003 → 004 → 005 → 006 → 007 → 008 → 009 → 010 → 011 → 012 → 013 → 014 → 015/016 → 017`。

批次不是按目录或数量平均拆分，而是由 trust boundary、共同事务、恢复语义、核心文件冲突和测试闭环形成。

## 2. 统一执行约束

1. 基线分支 `deploy`；总集成分支建议 `codex/agent-runtime-upgrade-20260804`，只允许总集成 Agent 写入。
2. 每批创建唯一短期集成分支；任务分支必须包含任务号、Agent 标识和时间戳，禁止多个 Agent 抢同一分支。
3. 第一个任务从精确 SHA `9584c3...` 开始；后续任务从上一稳定标签解析出的精确 SHA 开始，禁止从浮动 branch 开始。
4. 同时最多主工作区 1 个、任务 Worktree 1 个；并行结束立即合并并删除 Worktree。
5. 所有 Rust Debug check/test 复用 `/Users/ldh/Downloads/project/AiNative/Natives/.cargo-target-shared`，`CARGO_BUILD_JOBS=2`、`RUST_TEST_THREADS=2`、`CARGO_INCREMENTAL=0`。
6. 任务 Agent 不执行 workspace test、npm install、Tauri/Release build 或 `cargo clean`。
7. 数据库迁移仅 additive；公共协议只在 `assistant-protocol` 定义并生成 binding。

# 实施批次：可信基线与资源门禁（B00）

## 批次形成原因

当前源码 check 已通过，但前端依赖和完整故障矩阵未独立验证。所有后续 Agent 必须共享同一事实和资源阈值。

## 批次目标

固定能力矩阵、crash points、工具链、磁盘和一次性验证规则。

## 对应问题

L01、L02、F04。

## 前置批次

无。

## 阻塞的后续工作

全部开发任务。

## 批次内任务

TASK-000。

## 任务执行顺序

单任务。

## 可并行任务

无。

## 必须串行任务

TASK-000 必须先完成并记录稳定 SHA。

## 高冲突文件

无生产文件；只允许权威进度源、脚本和 CI。

## 允许的临时兼容状态

可记录未运行的真实 Provider smoke。

## 批次完成后必须消除的临时状态

禁止“待验证”没有原因、命令和责任批次。

## 任务级测试

`cargo fmt --check`、一次 `cargo check --workspace --jobs 2`。

## 批次级集成测试

与任务级相同，不运行 workspace test。

## 验收标准

精确 HEAD、十个 crash point、缓存/磁盘基线和稳定标签全部记录。

## 回滚方案

文档/CI commit 可独立回滚。

## 磁盘与构建资源要求

不创建 Worktree，不安装依赖；开始/结束记录 target、node_modules 和可用空间。

# 实施批次：信任边界与进程安全（B01）

## 批次形成原因

N01 与 J02 是可直接触发的安全/可靠性缺陷，必须先于复杂持久化迁移；RPC 上限与其文件不冲突，可利用唯一并行 Worktree。

## 批次目标

任何未授权路径不发生 Checkpoint I/O；Shell/RPC 内存、管道与连接资源有界。

## 对应问题

N01、J02、N04。

## 前置批次

B00。

## 阻塞的后续工作

B02 的 Tool effect contract。

## 批次内任务

TASK-001、002、003。

## 任务执行顺序

Wave 1：001 在主工作区。Wave 2：001 合并后，002 主工作区、003 唯一受控 Worktree 并行。Wave 3：批次集成。

## 可并行任务

002 与 003；必须从同一 `TASK-001_MERGED_SHA` 开始。

## 必须串行任务

001 必须先于 002；002 不得与其他 `production_tools.rs` 任务并行。

## 高冲突文件

`production_tools.rs`、`capability-gateway/lib.rs`、`rpc.rs`。

## 允许的临时兼容状态

旧 RPC client 可继续使用相同 JSON line 协议，只增加上限和结构化错误。

## 批次完成后必须消除的临时状态

`contains("..")` 不能作为路径授权；Shell 不得退出后才 drain；RPC 不得无界 `read_line`。

## 任务级测试

Gateway path、checkpoint path、process supervisor、RPC frame 精确测试。

## 批次级集成测试

`cargo check -p capability-gateway -p natives-agent-daemon`；相关 crate 测试；无 workspace test。

## 验收标准

路径拒绝零 I/O；10MB 双流不 hang；Unicode 不 panic；oversized/slow client 被有界终止。

## 回滚方案

001 安全修复不提供关闭开关；002/003 各自独立 commit 可整体回滚到 B00。

## 磁盘与构建资源要求

最多 1 个任务 Worktree；两个 Agent 不同时启动 Cargo，复用共享 Target。

# 实施批次：副作用事实单一权威（B02）

## 批次形成原因

Ledger intent/settlement、Tool event、uncertain、Checkpoint watermark 和 Resume gate 是一个不可拆事务语义。

## 批次目标

工具副作用无法被证明完成时，系统必须 durable 标记并阻止自动重放。

## 对应问题

D03、D04、D05、F02、G01、G02、H01。

## 前置批次

B01。

## 阻塞的后续工作

Typed projector、Storage actor、Resume、Scheduler。

## 批次内任务

TASK-004。

## 任务执行顺序

先 migration/协议与失败测试，再 store transaction command，再 production wiring，最后 resume/checkpoint。

## 可并行任务

无；单 Agent 独占主工作区。

## 必须串行任务

全部步骤串行，不能把“新增字段”和“生产读写”拆成两批。

## 高冲突文件

`engine.rs`、`production_tools.rs`、`run_manager.rs`、`checkpoint.rs`、事件/ledger stores。

## 允许的临时兼容状态

旧 checkpoint cursor 只能标记 `legacy_unverifiable`；可禁止 auto-resume。

## 批次完成后必须消除的临时状态

event sequence 冒充 ledger cursor、失败的 best-effort uncertain、started 永久悬挂。

## 任务级测试

Tool pairing、ledger transition、checkpoint cursor、resume decision、协议同步。

## 批次级集成测试

Agent Core + Daemon side-effect/restart tests；指定 crash points 1–4。

## 验收标准

Handler 成功但任一事实写失败时不得回灌成功；自动 Resume 调用未知副作用次数为 0。

## 回滚方案

Migration additive；可回滚 binary 并关闭 auto-resume，不删除新事实。

## 磁盘与构建资源要求

单工作区、单 Cargo 进程；不运行前端命令。

# 实施批次：Typed Conversation 恢复闭环（B03）

## 批次形成原因

Projector 必须基于 B02 稳定的 event/store contract；Turn、Message、Block、Tool pair 和 watermark 必须同一事务完成。

## 批次目标

Committed Turn 在 Daemon 重启后必能幂等恢复成 Provider 下一轮使用的完整 AgentMessage。

## 对应问题

B01、F02、H02、N02、N03、N06。

## 前置批次

B02。

## 阻塞的后续工作

Storage actor、Typed compat 删除、Context resume。

## 批次内任务

TASK-005。

## 任务执行顺序

先 event-to-message fixture，再 projector transaction/watermark，再启动恢复，再替换生产二次投影。

## 可并行任务

无。

## 必须串行任务

Schema、projector、reload、Provider context 接线不可拆。

## 高冲突文件

`conversation_store.rs`、`production.rs`、`run_manager.rs`、`event_log.rs`。

## 允许的临时兼容状态

Legacy delta-only run 可通过集中 compat projector 读取并记录命中。

## 批次完成后必须消除的临时状态

TurnCompleted 与 typed row 之间无恢复路径、坏 event 静默跳过、按 ID 前缀猜 FK。

## 任务级测试

Projector/reload/snapshot 精确 SQLite fixture 与进程 kill points。

## 批次级集成测试

Daemon conversation/replay/resume 组合测试；协议不变则不跑前端。

## 验收标准

Canonical event transcript 与 reload AgentMessage 深度相等，完整 tool pair 进入下一 Provider request。

## 回滚方案

保留 run_event 和 watermark；停止有缺陷的 projector 后可由修复版本重放。

## 磁盘与构建资源要求

单工作区；SQLite fixtures 使用系统临时目录并在测试结束清理。

# 实施批次：存储与流式背压（B04）

## 批次形成原因

Storage actor 必须消费 B02/B03 已确定的事务 command；Progress/MCP 随后才能使用有界写入与饱和策略。

## 批次目标

异步 runtime 不直接被单 SQLite Mutex 阻塞；流式状态内存和持久化速率有固定上限。

## 对应问题

F01、F04、H02、H03、J03。

## 前置批次

B03。

## 阻塞的后续工作

Permission、Sub Agent、Metrics。

## 批次内任务

TASK-006、007。

## 任务执行顺序

006 完成并合并后才开始 007。

## 可并行任务

无。

## 必须串行任务

Storage command/queue 先于 Progress producer/consumer。

## 高冲突文件

`storage/**`、各 Store、`production_tools.rs`、`engine.rs`、`mcp_runtime.rs`。

## 允许的临时兼容状态

非关键 Progress 可按固定策略 coalesce/drop；critical event 不允许 drop。

## 批次完成后必须消除的临时状态

`UnboundedSender`、async worker 直接锁同步 Connection、结果后 late progress。

## 任务级测试

Storage actor saturation/shutdown、Progress slow consumer/100MB producer/MCP cancel。

## 批次级集成测试

Daemon storage/progress/MCP tests；一次 `cargo check --workspace`。

## 验收标准

容量常量和超限语义可查询；DB busy/full 不丢 critical fact；RSS/queue depth 有界。

## 回滚方案

Actor 可回滚为单 `spawn_blocking` writer，但不能回到 async 线程同步锁；Progress 可降级 coalesce，不得无界。

## 磁盘与构建资源要求

性能测试输出只保留摘要；禁止把 100MB fixture 写进仓库。

# 实施批次：交互、子 Agent 与队列权威（B05）

## 批次形成原因

三项均跨持久事实和内存 waiter/actor，并共享 `production_tools.rs`、`run_manager.rs`，只能依序收敛。

## 批次目标

Permission、Child reservation 和 Prompt Queue 在崩溃、重复响应和取消下保持单一事实。

## 对应问题

A02、C02、E02、E03、E04、J04、N05。

## 前置批次

B04。

## 阻塞的后续工作

Context policy、Typed convergence。

## 批次内任务

TASK-008、009、010。

## 任务执行顺序

Permission outbox → Sub Agent reservation/failure → Queue lease/drain。

## 可并行任务

无。

## 必须串行任务

008、009、010 按顺序；禁止三个 Agent 同时重写 runtime actors。

## 高冲突文件

`production_tools.rs`、`run_manager.rs`、`rpc.rs`、`engine.rs`。

## 允许的临时兼容状态

旧 interaction/queue rows 允许集中 reader；命中必须记录。

## 批次完成后必须消除的临时状态

resolved DB 与 waiter 分叉、child orphan、FailurePolicy 仅类型存在、mixed DrainMode 未定义。

## 任务级测试

Permission race/restart、nested child/budget/cancel、queue lease/ack crash。

## 批次级集成测试

Daemon production chain：permission → child → parent result；queue restart；protocol check。

## 验收标准

重复/冲突响应确定；child 无 orphan/越权；queue 至少一次交付但模型上下文至多提交一次。

## 回滚方案

逐任务回滚；新表/列保留；遇不确定状态关闭自动执行，不删除事实。

## 磁盘与构建资源要求

无并行 Worktree；所有嵌套 child 测试限制深度/并发。

# 实施批次：Core 语义与兼容面收敛（B06）

## 批次形成原因

Context、Scheduler 和 Typed compatibility 都修改 AgentEngine 主循环，必须在可靠性状态机稳定后串行完成。

## 批次目标

统一 Context overflow、跨 Run tool conflict 和 Native typed Provider 入口。

## 对应问题

A03、B01、B02、C03、D01、D02、G03、G04、I01、I02、I03、J01。

## 前置批次

B05。

## 阻塞的后续工作

模块拆分和公开 Facade。

## 批次内任务

TASK-011、012、013。

## 任务执行顺序

Context policy → Scheduler lease → Typed/Lineage/compat 收敛。

## 可并行任务

无。

## 必须串行任务

全部，因 `engine.rs`、`production.rs` 和 `run_manager.rs` 高冲突。

## 高冲突文件

上述三个文件，以及 `conversation_store.rs`、Gateway registry。

## 允许的临时兼容状态

Legacy reader 集中在 compat 模块，保留一个发布周期和命中指标；CLI 明示非 Native authority。

## 批次完成后必须消除的临时状态

非 compat production 的 EngineMessage、重复工具名、跨 Run exclusive 抢跑、无限 overflow retry。

## 任务级测试

Context fixtures、scheduler conflict、typed round-trip、provider adapters、lineage matrix。

## 批次级集成测试

相关三个 crate + Daemon production/compat tests；`npm run protocol:check` 如协议变化。

## 验收标准

Provider/Tool 单入口；typed block 无静默损失；同 conflict key 不重叠；每输入最多一次 compaction retry。

## 回滚方案

每任务独立回滚；compat reader 不与 typed writer 双写。

## 磁盘与构建资源要求

单工作区、串行 Cargo；不安装前端依赖。

# 实施批次：模块边界、可观测性与 Facade（B07）

## 批次形成原因

只有 B06 后接口才足够稳定。先纯移动控制器，再从同一 SHA 有限并行 Metrics 与 Facade，避免冻结错误 API。

## 批次目标

降低状态域耦合，建立有界指标和最小嵌入 API，不建立第二套 Runtime。

## 对应问题

A01、H04、K01、K02、L03。

## 前置批次

B06。

## 阻塞的后续工作

最终发布验收。

## 批次内任务

TASK-014、015、016。

## 任务执行顺序

Wave 1：014 主工作区。Wave 2：015 主工作区，016 唯一 Worktree。Wave 3：集成。

## 可并行任务

015 与 016，从 `TASK-014_MERGED_SHA` 开始。

## 必须串行任务

014 先于二者；Facade 不得在拆分前冻结旧 public API。

## 高冲突文件

三个巨型控制器、`agent-core/lib.rs`、runtime装配。

## 允许的临时兼容状态

In-memory facade 可用于嵌入/测试，但必须声明非 crash-safe。

## 批次完成后必须消除的临时状态

无语义 `part1/utils` 拆分、无界/高基数指标、Facade 内第二套 loop。

## 任务级测试

Core/Daemon regression、metrics labels、外部示例编译。

## 批次级集成测试

Agent Core + Daemon + example；不执行 publish 或 Tauri build。

## 验收标准

状态 authority 归属清晰；指标不泄密且有界；示例只依赖公开 Facade。

## 回滚方案

纯移动 commit、Metrics commit、Facade commit 分开；任何一个可独立回滚。

## 磁盘与构建资源要求

最多 1 个额外 Worktree；example 使用共享 Target。

# 实施批次：生产链最终验收（B08）

## 批次形成原因

完整测试、前端、迁移、Release/Tauri 构建成本高且只有在全部代码合并后有可信意义。

## 批次目标

证明生产链、恢复、安全、性能和发布构建满足合并门槛。

## 对应问题

L01、L02、F04，以及所有任务的验收标准。

## 前置批次

B07。

## 阻塞的后续工作

合并 `deploy`、发布、新功能开发。

## 批次内任务

TASK-017。

## 任务执行顺序

资源检查 → Rust → 前端 → protocol/native engine → migration/production E2E → Release smoke → Tauri build。

## 可并行任务

无；重型命令串行。

## 必须串行任务

所有完整构建，避免共享 Target 锁争用和磁盘峰值。

## 高冲突文件

原则上只改测试/脚本/权威进度文档；发现产品缺陷必须另建短期修复 commit 后重跑受影响门禁。

## 允许的临时兼容状态

无。外部 Provider 无凭证可标环境未验证，但不能假绿。

## 批次完成后必须消除的临时状态

所有任务 Worktree、悬挂测试进程、未解释缓存增长、无 exit code 的“已通过”。

## 任务级测试

见最终验证计划。

## 批次级集成测试

即完整最终测试矩阵。

## 验收标准

P0 全关；纳入 P1 达标；生产链/crash/migration/Renderer replay/Release smoke 全绿；磁盘增长可解释。

## 回滚方案

验收失败不合并；回到 `natives-runtime/b07-operability` 或前一个稳定标签定位。

## 磁盘与构建资源要求

开始前可用空间至少 25 GiB；低于 20 GiB 暂停重型测试，低于 15 GiB 禁止 Cargo；禁止清缓存，先人工分析。

## 3. 总体验收门槛

- 所有 P0 issue 关闭并有生产链/故障注入证据。
- 纳入 P1 达到各任务卡目标；不能以 Trait、Migration、No-op 代替接线。
- Run terminal 只有 RunManager；Turn 和 Tool facts persist-first；Renderer 不产生权威事实。
- Provider Native 旁路为 0；Tool Gateway Native 旁路为 0；compat capability 显式。
- Unknown side effect 不自动 Resume；Tool 成功但事实提交失败进入 durable uncertain/blocked。
- Event sequence 单调唯一；Queue crash 不重复提交模型上下文；Progress/RPC/storage 有界。
- 全量 Rust、前端、protocol、native-engine、migration、生产链与 Release smoke 有真实 exit code。
- Worktree 同时不超过 2；最终只保留主集成工作区；共享 Target 增长可解释。

## 4. 计划自检

- [x] 9个批次由依赖、事务、恢复语义和文件冲突动态形成，并非预设数量。
- [x] 48个审计问题均映射到实施或回归任务；8个P0位于前置关键路径。
- [x] 同一核心文件的任务默认串行；仅两组无核心冲突任务允许并行。
- [x] 每个任务都有起始Commit解析规则、允许/禁止路径、定向测试、重型命令禁令和回滚。
- [x] 每个批次都有形成原因、波次、兼容状态、集成测试和验收门槛。
- [x] Workspace test与Tauri Release只在B08执行。
- [x] 任务Agent不运行`npm ci`，不创建独立target，不执行`cargo clean`。
- [x] 共享Cargo Target和主工作区前端依赖复用方式明确。
- [x] 最大Worktree总数为2，合并后立即清理，分支名称唯一且不共享。
- [x] 磁盘空间阈值、每批监控、异常增长暂停机制明确。
- [x] 最终生产链、crash、migration、resource和Release测试完整。
- [x] 本计划没有修改生产代码、创建migration、提交或Push。

## 5. 结论

```text
PLAN_READY
```

可以从 TASK-000 开始实施；不得跳过 B00 或直接并行修改高冲突核心文件。
