# Natives Agent Runtime 执行任务卡

> 分支与起点规则：任务中的稳定标签必须由批次集成 Agent 解析并替换为精确 SHA 后才能派发。任何 Agent 不得从浮动 branch 或另一 Agent 正在使用的分支开始。

## 通用构建约束

- `CARGO_TARGET_DIR=/Users/ldh/Downloads/project/AiNative/Natives/.cargo-target-shared`
- `CARGO_BUILD_JOBS=2`、`RUST_TEST_THREADS=2`、`CARGO_INCREMENTAL=0`
- 禁止 `cargo clean`、任务级 workspace test、任务级 Tauri/Release build、复制 target/node_modules。
- Rust 任务不需要前端依赖，不允许 `npm install`；协议任务只在主工作区复用现有依赖执行 `npm run protocol:check`。
- 每个任务一个独立 commit；不得顺手格式化、重命名或清理任务范围外文件。

## TASK-000：冻结能力、故障与构建基线

### 所属实施批次
B00。
### 任务目标
建立所有 Agent 共用的可复现基线和十点 crash matrix。
### 来源问题
L01、L02、F04。
### 当前行为
源码 check 通过，但完整命令与真实 crash coverage 不完整。
### 目标行为
每项验证绑定精确 SHA、命令、退出码和资源记录。
### 前置任务
无。
### 可并行任务
无。
### 冲突任务
无。
### 起始 Commit
`9584c3c263c1e83b1066e4208e1ab2d678a9deeb`。
### 执行工作区
主工作区。
### 构建缓存要求
复用共享 Target；禁止 clean/workspace test/npm install；无需前端依赖。
### 允许修改范围
权威引擎进度文档、测试脚本、CI。
### 禁止修改范围
全部生产源码和数据库 Schema。
### 高冲突文件
无。
### 实施步骤
记录 Git/工具链/磁盘；建立 capability matrix；命名十个 crash points；执行一次 fmt/check；写稳定 SHA。
### 必须保持的不变量
不把未运行写成通过。
### 数据结构变化
无。
### 协议变化
无。
### 数据库变化
无。
### 兼容要求
记录 native/compat/test 三类入口。
### 必须新增的测试
仅测试清单和 failpoint contract，不实现生产 failpoint。
### 任务级验证命令
`cargo fmt --check`; `cargo check --workspace --jobs 2`。
### 禁止执行的重型命令
Workspace test、Clippy、npm CI、Tauri build。
### 验收标准
生成 `natives-runtime/b00-baseline` 对应 SHA；命令和资源均可追溯。
### 回滚方法
回滚文档/CI commit。
### 输出物
基线记录、crash matrix、稳定 SHA。
### 预计复杂度
S。
### 风险等级
低。

## TASK-001：Gateway 授权路径先于 Checkpoint I/O

### 所属实施批次
B01。
### 任务目标
Checkpoint 只消费 Gateway 已授权 canonical path。
### 来源问题
N01。
### 当前行为
`production_tools.rs` 在 Gateway scope/permission 前调用 `capture_before`。
### 目标行为
拒绝路径在 fs/checkpoint/handler 三处调用次数均为 0。
### 前置任务
TASK-000。
### 可并行任务
无。
### 冲突任务
002、004、007、009、012。
### 起始 Commit
`natives-runtime/b00-baseline` 解析 SHA。
### 执行工作区
主工作区。
### 构建缓存要求
复用共享 Target；无需前端；禁止 install/clean/workspace test。
### 允许修改范围
Gateway path preflight、Checkpoint API、Daemon Tool adapter及测试。
### 禁止修改范围
Agent loop、UI、Host、Provider。
### 高冲突文件
`capability-gateway/src/lib.rs`; `production_tools.rs`; `checkpoint.rs`。
### 实施步骤
先写 absolute/dotdot/symlink 测试；复用 canonical resolver；preflight 返回受信路径；Checkpoint 拒绝 raw path。
### 必须保持的不变量
PathScope 由 Gateway 权威；Core 不复制安全策略。
### 数据结构变化
仅内部 prepared path 类型。
### 协议变化
无。
### 数据库变化
无 migration。
### 兼容要求
合法 project-relative write 行为不变。
### 必须新增的测试
absolute、dotdot、symlink race、missing path、多文件 patch、permission deny。
### 任务级验证命令
`cargo test -p capability-gateway path_scope`; `cargo test -p natives-agent-daemon checkpoint_path -- --test-threads=2`。
### 禁止执行的重型命令
Workspace test、npm、Release/Tauri。
### 验收标准
所有拒绝路径零 I/O；合法 before/after artifact 正确。
### 回滚方法
整体安全 commit 回滚；不加关闭校验 flag。
### 输出物
代码、精确测试、路径不变量说明。
### 预计复杂度
M。
### 风险等级
极高。

## TASK-002：Shell 持续 Drain、Unicode 截断与资源回收

### 所属实施批次
B01。
### 任务目标
消除大输出死锁、Unicode panic 和取消后孤儿。
### 来源问题
J02。
### 当前行为
子进程退出后才读取 pipe，字符串按任意 byte offset 切片。
### 目标行为
spawn 后并发有界 drain；cancel/timeout kill+wait+join。
### 前置任务
TASK-001。
### 可并行任务
TASK-003。
### 冲突任务
001、007。
### 起始 Commit
`TASK-001_MERGED_SHA`。
### 执行工作区
主工作区。
### 构建缓存要求
共享 Target；无需前端；禁止 clean/workspace test。
### 允许修改范围
ProcessSupervisor、terminal handler、最小 progress adapter、测试。
### 禁止修改范围
协议、DB、UI、Provider。
### 高冲突文件
`process_supervisor.rs`; `production_tools.rs`。
### 实施步骤
失败测试；并发 reader；bounded byte ring；safe lossy/char boundary；统一 cleanup。
### 必须保持的不变量
Result 后无 late completed；所有 child 被监督和 reap。
### 数据结构变化
内部 reader handles/buffer。
### 协议变化
无。
### 数据库变化
无。
### 兼容要求
现有 terminal result shape 不变。
### 必须新增的测试
10MB 双流、交错、CJK/emoji、background、cancel/timeout race、cleanup failure。
### 任务级验证命令
`cargo test -p capability-gateway process_supervisor`; `cargo test -p capability-gateway shell -- --test-threads=2`。
### 禁止执行的重型命令
Workspace test、npm、Tauri build。
### 验收标准
无 hang/panic/orphan；registry quiet。
### 回滚方法
独立 supervisor commit 回滚，旧实现不得作为生产 fallback。
### 输出物
实现、资源测试、峰值内存记录。
### 预计复杂度
L。
### 风险等级
极高。

## TASK-003：UDS 请求帧与连接资源有界化

### 所属实施批次
B01。
### 任务目标
限制无换行/慢请求对 Daemon 内存和 session 的占用。
### 来源问题
N04。
### 当前行为
RPC/client 使用无上限 `read_line`。
### 目标行为
固定最大 frame、读取 deadline、结构化错误和精确清理。
### 前置任务
TASK-001。
### 可并行任务
TASK-002。
### 冲突任务
008。
### 起始 Commit
`TASK-001_MERGED_SHA`。
### 执行工作区
唯一受控 Worktree。
### 构建缓存要求
共享 Target；Cargo 与主工作区错峰；无需前端。
### 允许修改范围
Daemon RPC/client 与测试。
### 禁止修改范围
Core、Gateway、Renderer、协议语义。
### 高冲突文件
`rpc.rs`; `client.rs`。
### 实施步骤
定义常量；bounded read；oversize/timeout error；连接 finally cleanup。
### 必须保持的不变量
协议 v2 方法与认证不变。
### 数据结构变化
无公共数据类型。
### 协议变化
仅既有 error envelope，不新增方法。
### 数据库变化
无。
### 兼容要求
正常 frame 完全兼容。
### 必须新增的测试
边界±1、无换行、slowloris、invalid UTF-8/JSON、client drop。
### 任务级验证命令
`cargo test -p natives-agent-daemon rpc_frame -- --test-threads=2`。
### 禁止执行的重型命令
Workspace check/test、npm、Release/Tauri。
### 验收标准
过大/过慢连接有界终止且 session registry quiet。
### 回滚方法
独立 RPC commit 回滚。
### 输出物
代码、测试、frame 常量说明。
### 预计复杂度
M。
### 风险等级
高。

## TASK-004：Tool Effect 原子结算与真实 Ledger Cursor

### 所属实施批次
B02。
### 任务目标
建立副作用 intent-first、terminal/uncertain 和 Resume 的单一 durable contract。
### 来源问题
D03、D04、D05、F02、G01、G02、H01。
### 当前行为
成功事件失败后的 uncertain 仍可能丢失，checkpoint cursor 实际是 event sequence。
### 目标行为
无法证明完成即 durable uncertain/blocked，自动恢复不重复副作用。
### 前置任务
001–003。
### 可并行任务
无。
### 冲突任务
005–013 多数任务。
### 起始 Commit
`natives-runtime/b01-safety` 解析 SHA。
### 执行工作区
主工作区独占。
### 构建缓存要求
共享 Target；无需安装前端；协议 check 复用主工作区 node_modules。
### 允许修改范围
Core Tool finalize、assistant-protocol、Daemon ledger/event/checkpoint/run manager/storage及测试。
### 禁止修改范围
UI、Host、Provider wire、无关工具。
### 高冲突文件
`engine.rs`; `production_tools.rs`; `run_manager.rs`; stores。
### 实施步骤
先 crash tests；additive migration；transaction command；intent before handler；settlement+fact；real cursor；resume gate；Hook ID。
### 必须保持的不变量
Core 不依赖 SQLite；RunManager 唯一 Run terminal；每个 final call 恰一 result。
### 数据结构变化
Ledger sequence、replay contract、idempotency/external reference。
### 协议变化
仅 assistant-protocol additive resume/error/fact 字段。
### 数据库变化
Additive migration、backfill 标 `legacy_unverifiable`。
### 兼容要求
旧 checkpoint 不自动声称 safe。
### 必须新增的测试
handler前/后、settlement前、ToolCompleted前、checkpoint前逐点 kill。
### 任务级验证命令
`cargo test -p agent-core tool_result`; `cargo test -p natives-agent-daemon side_effect -- --test-threads=2`; `npm run protocol:check`。
### 禁止执行的重型命令
Workspace test、npm install、Tauri build。
### 验收标准
started 必有 terminal/uncertain；unknown 自动 Resume handler 调用为0；cursor可查询。
### 回滚方法
保留 migration；关闭 auto-resume；回滚 binary。
### 输出物
事务实现、migration、crash tests、兼容说明。
### 预计复杂度
XL。
### 风险等级
极高。

## TASK-005：Typed Turn 幂等 Conversation Projector

### 所属实施批次
B03。
### 任务目标
让 committed Turn 可从 event 幂等重建为完整 AgentMessage。
### 来源问题
B01、F02、H02、N02、N03、N06。
### 当前行为
Engine 完成后另行投影，崩溃可永久缺 assistant/tool blocks。
### 目标行为
Projector 事务化写 Turn/message/blocks/results/watermark，启动可补齐。
### 前置任务
TASK-004。
### 可并行任务
无。
### 冲突任务
004、006、013。
### 起始 Commit
`natives-runtime/b02-effects`。
### 执行工作区
主工作区。
### 构建缓存要求
共享 Target；无需前端。
### 允许修改范围
Conversation/event stores、projector、production/run manager、migration/tests。
### 禁止修改范围
Provider wire、Gateway、UI/Host。
### 高冲突文件
`conversation_store.rs`; `production.rs`; `run_manager.rs`。
### 实施步骤
Canonical fixture；upsert transaction；watermark；startup recovery；切生产入口；legacy reader。
### 必须保持的不变量
Event 是可重放事实；Typed history 无损；partial Turn 不投影为完整消息。
### 数据结构变化
Projector watermark/content digest（最小需要）。
### 协议变化
无或 additive event metadata。
### 数据库变化
Additive projector watermark/index。
### 兼容要求
Legacy delta-only 走集中 compat reader并记录命中。
### 必须新增的测试
每行写入间 kill、重复 projector、内容冲突、坏 JSON、完整 tool pair。
### 任务级验证命令
`cargo test -p natives-agent-daemon conversation_projector -- --test-threads=2`; snapshot recovery test。
### 禁止执行的重型命令
Workspace test、npm、Tauri。
### 验收标准
Event transcript 与 reload AgentMessage 相等并进入下一 Provider request。
### 回滚方法
保留 event/watermark；停止 projector后可由修复版本重放。
### 输出物
Projector、migration、restart tests。
### 预计复杂度
L。
### 风险等级
极高。

## TASK-006：同步 SQLite 迁移到 Bounded Storage Actor

### 所属实施批次
B04。
### 任务目标
移除 async 路径直接同步锁并给 DB 命令固定容量。
### 来源问题
F01、F04、H02。
### 当前行为
Daemon stores 共享单 `Mutex<Connection>`，同步 SQL 占用 runtime worker。
### 目标行为
单写 bounded actor/专用 blocking 线程执行已稳定事务 command。
### 前置任务
TASK-005。
### 可并行任务
无。
### 冲突任务
005、007–009。
### 起始 Commit
`natives-runtime/b03-projection`。
### 执行工作区
主工作区。
### 构建缓存要求
共享 Target；无需前端。
### 允许修改范围
Daemon storage、stores、callers、tests。
### 禁止修改范围
Core loop、UI/Host、Provider。
### 高冲突文件
`storage/**` 和所有 Store。
### 实施步骤
定义最少 transaction commands；bounded queue；critical/noncritical saturation；shutdown drain；迁移 callers。
### 必须保持的不变量
Daemon 独占 assistant DB；critical fact 不 drop。
### 数据结构变化
内部 command/reply enum。
### 协议变化
无。
### 数据库变化
无额外 schema，沿用前批 migration。
### 兼容要求
Store 外部行为和 error codes 保持。
### 必须新增的测试
10 Run、BUSY/FULL、queue full、actor crash、shutdown drain、延迟统计。
### 任务级验证命令
`cargo test -p natives-agent-daemon storage_actor -- --test-threads=2`; sqlite_fault test。
### 禁止执行的重型命令
Workspace test、npm、Tauri。
### 验收标准
async 主路径无直接 Mutex<Connection>；队列/延迟有界。
### 回滚方法
回滚到单 `spawn_blocking` writer，不回旧 async sync-lock。
### 输出物
Actor、caller迁移、fault/perf证据。
### 预计复杂度
XL。
### 风险等级
高。

## TASK-007：Progress 与 MCP Late Update 全链路有界化

### 所属实施批次
B04。
### 任务目标
固定 progress 容量、合并频率和 terminal 后拒绝语义。
### 来源问题
H03、J03。
### 当前行为
上游存在 unbounded sender，MCP 取消/late result best effort。
### 目标行为
Slow consumer 不扩大内存；Result/cancel 后 update 永远不持久化。
### 前置任务
TASK-006。
### 可并行任务
无。
### 冲突任务
008、009、012。
### 起始 Commit
`TASK-006_MERGED_SHA`。
### 执行工作区
主工作区。
### 构建缓存要求
共享 Target；无需前端。
### 允许修改范围
Core progress trait、Gateway/MCP producers、Daemon sink/store、tests。
### 禁止修改范围
UI 产品语义、Tool scheduler重写。
### 高冲突文件
`engine.rs`; `production_tools.rs`; `mcp_runtime.rs`。
### 实施步骤
容量/策略常量；bounded send；coalesce；settled registry；cancel join；slow sink tests。
### 必须保持的不变量
Tool Result 恰一次；Progress非权威且不能覆盖 terminal。
### 数据结构变化
内部 bounded channel/status。
### 协议变化
已有 progress event 不变。
### 数据库变化
无 migration；持久化批量化。
### 兼容要求
Renderer event shape 不变。
### 必须新增的测试
100MB、slow store、disconnect、cancel、late update、MCP fake server。
### 任务级验证命令
`cargo test -p capability-gateway progress`; `cargo test -p natives-agent-daemon progress_backpressure -- --test-threads=2`。
### 禁止执行的重型命令
Workspace test、npm、Tauri。
### 验收标准
RSS/queue固定上限；terminal后新增progress为0。
### 回滚方法
回到 bounded drop/coalesce 最小实现，不回 unbounded。
### 输出物
实现、resource test、吞吐记录。
### 预计复杂度
L。
### 风险等级
高。

## TASK-008：Permission Decision Outbox 与幂等响应

### 所属实施批次
B05。
### 任务目标
把决策、事件和 waiter 交付收敛为可重放单一事实。
### 来源问题
A02、J04。
### 当前行为
DB resolved、InteractionResponded 和 waiter wake 非原子，重复响应不幂等。
### 目标行为
同决策重复幂等、冲突拒绝、重启后 outbox 交付。
### 前置任务
006、007。
### 可并行任务
无。
### 冲突任务
009、010。
### 起始 Commit
`natives-runtime/b04-backpressure`。
### 执行工作区
主工作区。
### 构建缓存要求
共享 Target；协议 check复用现有前端依赖；禁止install。
### 允许修改范围
assistant-protocol、interaction store/runtime/RPC/tool adapter、tests/bindings。
### 禁止修改范围
Renderer交互、Provider、Gateway策略。
### 高冲突文件
`interaction_store.rs`; `rpc.rs`; `production_tools.rs`。
### 实施步骤
失败测试；decision transaction/outbox；idempotency compare；delivery/recovery；waiter cleanup。
### 必须保持的不变量
Permission绑定 run/turn/tool；不同 Run profile 隔离；deny 不执行 handler。
### 数据结构变化
Decision revision/outbox state。
### 协议变化
必要时 additive revision/error code。
### 数据库变化
Additive outbox/revision字段或表。
### 兼容要求
旧 pending row 可迁移，不能自动 allow。
### 必须新增的测试
双击、allow/deny race、restart、event failure、timeout/cancel。
### 任务级验证命令
Daemon interaction test；`npm run protocol:check`。
### 禁止执行的重型命令
Workspace test、npm install、Tauri。
### 验收标准
事件失败不抢跑；重复决策 deterministic；waiter最终清理。
### 回滚方法
保留新列/outbox；关闭自动交付，不删除决策。
### 输出物
事务/outbox、协议binding、race tests。
### 预计复杂度
L。
### 风险等级
高。

## TASK-009：Sub Agent Durable Reservation、补偿与失败策略

### 所属实施批次
B05。
### 任务目标
使 child 创建、预算、失败和重启成为可恢复状态机。
### 来源问题
E02、E03、E04、N05。
### 当前行为
预算内存化，失败策略未接线，部分失败可留 session/run，project_id 错写。
### 目标行为
创建可补偿、预算耐久、策略真实、父取消级联且无越权。
### 前置任务
TASK-008。
### 可并行任务
无。
### 冲突任务
010、012、013。
### 起始 Commit
`TASK-008_MERGED_SHA`。
### 执行工作区
主工作区。
### 构建缓存要求
共享 Target；无需前端。
### 允许修改范围
Core subagents、Daemon production tools/run manager/stores/tests。
### 禁止修改范围
UI、Provider wire、扩大 root/profile/allowlist。
### 高冲突文件
`production_tools.rs`; `run_manager.rs`; `subagents.rs`。
### 实施步骤
Durable reservation；创建 saga/compensation；rehydrate；settle error；FailurePolicy接线或删除；修 project_id。
### 必须保持的不变量
Child 独立 run/credential/event/snapshot/ledger；权限与工具只能收紧。
### 数据结构变化
Reservation/status/token/tool counters。
### 协议变化
仅必要的 additive child status。
### 数据库变化
Additive reservation/state。
### 兼容要求
旧 child 无 reservation 时保守阻断恢复。
### 必须新增的测试
三层nested、重启、父cancel、hook/quota deny、overrun、三策略、DB failure。
### 任务级验证命令
`cargo test -p agent-core subagent`; Daemon subagent_lifecycle test。
### 禁止执行的重型命令
Workspace test、npm、Tauri。
### 验收标准
无 orphan；预算重启连续；策略行为有生产断言；无 scope/credential 越权。
### 回滚方法
保留 reservation事实；禁用 child创建，不自动清空未知记录。
### 输出物
状态机、migration、restart/security tests。
### 预计复杂度
XL。
### 风险等级
极高。

## TASK-010：Steering/Follow-up Lease 与 DrainMode 契约收紧

### 所属实施批次
B05。
### 任务目标
明确 one/all、FIFO、safe point 和 crash redelivery。
### 来源问题
C02。
### 当前行为
Durable lease/ack 已接线，但混合 DrainMode 语义不够严格。
### 目标行为
队列至少一次交付，模型 Context 至多提交一次。
### 前置任务
TASK-009。
### 可并行任务
无。
### 冲突任务
011、013。
### 起始 Commit
`TASK-009_MERGED_SHA`。
### 执行工作区
主工作区。
### 构建缓存要求
共享 Target；无需前端。
### 允许修改范围
Core safe point、prompt queue store、run manager/tests。
### 禁止修改范围
UI Prompt Queue产品语义、RPC surface。
### 高冲突文件
`engine.rs`; `prompt_queue_store.rs`; `run_manager.rs`。
### 实施步骤
固定 drain contract；lease transaction；context commit/ack顺序；abort/restart策略；tests。
### 必须保持的不变量
UI只能提交命令，不能改Context；Steering不插入partial assistant。
### 数据结构变化
必要的 lease revision/consumer id。
### 协议变化
无RPC surface变化。
### 数据库变化
仅 additive lease metadata（如需要）。
### 兼容要求
旧 queued row FIFO保留。
### 必须新增的测试
one/all混合、lease后kill、ack前kill、abort、multi-run隔离。
### 任务级验证命令
Core steering test；Daemon prompt_queue_crash test。
### 禁止执行的重型命令
Workspace test、npm、Tauri。
### 验收标准
重复模型上下文提交为0；FIFO和safe point可重放证明。
### 回滚方法
关闭批量 drain，保留 durable row。
### 输出物
契约、实现、crash tests。
### 预计复杂度
M。
### 风险等级
高。

## TASK-011：Context Overflow 有界恢复与 Turn Policy

### 所属实施批次
B06。
### 任务目标
把 overflow、compaction、retry、cancel 形成单一有界策略。
### 来源问题
C03、I03。
### 当前行为
Snapshot/compaction存在，但 Provider overflow 无统一恢复。
### 目标行为
每输入最多一次 compaction retry，产物进入持久 snapshot。
### 前置任务
TASK-010。
### 可并行任务
无。
### 冲突任务
012、013。
### 起始 Commit
`natives-runtime/b05-authorities`。
### 执行工作区
主工作区。
### 构建缓存要求
共享 Target；无需前端。
### 允许修改范围
Core context/compaction/engine、Daemon production和tests。
### 禁止修改范围
Provider wire、UI、Tool scheduler。
### 高冲突文件
`engine.rs`; `production.rs`。
### 实施步骤
统一 error分类；budget/window；one-shot policy；snapshot commit；cancel/summary failure tests。
### 必须保持的不变量
Full history保留；active context由Core修改；tool pair合法。
### 数据结构变化
最小内部 TurnPolicy/overflow outcome，不做万能callback。
### 协议变化
无或 additive error code。
### 数据库变化
沿用 snapshot schema，必要字段 additive。
### 兼容要求
现有正常/length行为不变。
### 必须新增的测试
OpenAI/Anthropic/Gemini overflow fixtures、cancel、summary failure、pairing。
### 任务级验证命令
Core context_overflow/compaction tests。
### 禁止执行的重型命令
Workspace test、npm、Tauri。
### 验收标准
每输入恢复调用≤1；无法压缩时结构化失败且不执行工具。
### 回滚方法
关闭自动 overflow retry，保留原显式失败。
### 输出物
策略、fixtures、snapshot tests。
### 预计复杂度
L。
### 风险等级
高。

## TASK-012：跨 Run Conflict Lease 与工具注册确定性

### 所属实施批次
B06。
### 任务目标
让 Gateway capability 的 conflict/exclusive 在并发 Run 间真实生效。
### 来源问题
D01、D02。
### 当前行为
同 batch 调度有效，跨 Run 无 lease，重复注册名未拒绝。
### 目标行为
重复工具启动失败；同 key 跨 Run 不重叠且取消释放。
### 前置任务
TASK-011。
### 可并行任务
无。
### 冲突任务
013。
### 起始 Commit
`TASK-011_MERGED_SHA`。
### 执行工作区
主工作区。
### 构建缓存要求
共享 Target；无需前端。
### 允许修改范围
Core scheduler、Gateway registry/manifest、Daemon runtime/tool adapter/tests。
### 禁止修改范围
硬编码新工具分类、UI、Provider。
### 高冲突文件
`engine.rs`; `gateway/lib.rs`; `production_tools.rs`。
### 实施步骤
拒绝重复名；定义lease scope；preflight permissions；acquire/release cancellation-safe；双序事件。
### 必须保持的不变量
Gateway声明能力，Core调度；结果源序；权限前不抢跑副作用。
### 数据结构变化
内部 lease key/guard。
### 协议变化
无。
### 数据库变化
若需跨进程 lease仅 additive；单Daemon可先内存 authority并注明上限。
### 兼容要求
未声明 conflict 的工具保持 sequential安全默认。
### 必须新增的测试
同/异key、mixed modes、permission ask、failure/cancel、source/completion order。
### 任务级验证命令
Gateway registry、Core scheduler、Daemon conflict lease tests。
### 禁止执行的重型命令
Workspace test、npm、Tauri。
### 验收标准
重复名为0；相同key overlap为0；cancel后registry quiet。
### 回滚方法
退回全局 sequential安全模式，不回未受控并行。
### 输出物
Lease实现、registry validation、concurrency tests。
### 预计复杂度
L。
### 风险等级
高。

## TASK-013：Typed Provider、Lineage 与 Compatibility Runtime 收敛

### 所属实施批次
B06。
### 任务目标
Native主链只用Typed消息和ProviderTurnRequest，并明确Retry/Continue/Fork/Resume契约。
### 来源问题
A03、B01、B02、G03、G04、I01、I02、J01。
### 当前行为
Native生产优先Typed，但公共Legacy API、转换和CLI兼容仍混杂。
### 目标行为
Legacy仅在compat模块；转换无静默损失；lineage绑定精确snapshot。
### 前置任务
TASK-012。
### 可并行任务
无。
### 冲突任务
014。
### 起始 Commit
`TASK-012_MERGED_SHA`。
### 执行工作区
主工作区。
### 构建缓存要求
共享 Target；无需前端安装。
### 允许修改范围
Core message/engine、Provider adapters、Daemon production/routing/conversation/run manager/CLI bridge/tests。
### 禁止修改范围
Renderer、Host、Pi Runtime集成。
### 高冲突文件
`engine.rs`; `production.rs`; `run_manager.rs`; `conversation_store.rs`。
### 实施步骤
Typed fixtures；隔离compat；Provider转换保真；attempt ID；lineage契约；capability matrix；命中指标。
### 必须保持的不变量
Native只有一个 loop；RunManager唯一terminal；CLI不得冒充Native能力。
### 数据结构变化
内部compat wrapper、稳定attempt/request ID。
### 协议变化
只允许additive capability/lineage字段。
### 数据库变化
必要 lineage/snapshot ref additive。
### 兼容要求
集中legacy reader保留一发布周期并统计命中。
### 必须新增的测试
Thinking/Image/Custom round-trip、provider fixtures、lineage矩阵、CLI/native能力。
### 任务级验证命令
Core typed tests、provider-adapters tests、Daemon lineage_compat tests；必要时 protocol check。
### 禁止执行的重型命令
Workspace test、npm install、Tauri。
### 验收标准
非compat production EngineMessage引用为0；Provider旁路0；无损或显式拒绝。
### 回滚方法
保留compat reader，不恢复双写。
### 输出物
Typed入口、compat模块、lineage tests/capability表。
### 预计复杂度
XL。
### 风险等级
高。

## TASK-014：按状态权威拆分巨型控制器

### 所属实施批次
B07。
### 任务目标
在不改行为/协议/DB前提下按Run、Turn、Tool职责移动代码。
### 来源问题
A01。
### 当前行为
三个文件跨多个状态域，测试和故障注入困难。
### 目标行为
状态authority单一、模块深而窄、公共API不扩大。
### 前置任务
TASK-013。
### 可并行任务
无。
### 冲突任务
015、016。
### 起始 Commit
`natives-runtime/b06-core`。
### 执行工作区
主工作区。
### 构建缓存要求
共享 Target；无需前端。
### 允许修改范围
Agent Core与Daemon控制器/模块文件。
### 禁止修改范围
协议、migration、UI/Host、行为逻辑。
### 高冲突文件
`engine.rs`; `run_manager.rs`; `production_tools.rs`。
### 实施步骤
先characterization tests；按authority提取；每次纯移动可编译；删除旧入口。
### 必须保持的不变量
调用链、event顺序、error code、DB行为完全不变。
### 数据结构变化
无。
### 协议变化
无。
### 数据库变化
无。
### 兼容要求
公共入口保持，内部路径可变。
### 必须新增的测试
Production characterization和public API compile tests。
### 任务级验证命令
Core/Daemon check与相关测试。
### 禁止执行的重型命令
Workspace test、npm、Tauri、无关fmt。
### 验收标准
无行为diff；无part1/utils；authority各一个owner。
### 回滚方法
按纯移动commit逐个回滚。
### 输出物
模块拆分、调用图更新、characterization tests。
### 预计复杂度
XL。
### 风险等级
中。

## TASK-015：Bounded Runtime Metrics Sink

### 所属实施批次
B07。
### 任务目标
提供低基数、脱敏、非权威的Runtime指标。
### 来源问题
H04。
### 当前行为
缺统一Turn/Tool/Queue/Storage延迟和背压指标。
### 目标行为
有界sink覆盖关键路径，sink失败不影响事实。
### 前置任务
TASK-014。
### 可并行任务
TASK-016。
### 冲突任务
无实质冲突。
### 起始 Commit
`TASK-014_MERGED_SHA`。
### 执行工作区
主工作区。
### 构建缓存要求
共享 Target；禁止新增依赖，除非已有库无法满足且另行批准。
### 允许修改范围
新metrics模块、Daemon runtime装配、tests。
### 禁止修改范围
Prompt/path/credential labels、UI dashboard、DB schema。
### 高冲突文件
`runtime.rs` 轻度。
### 实施步骤
指标字典/label预算；最小trait；no-op默认；production adapter；test exporter。
### 必须保持的不变量
指标非权威、脱敏、有界。
### 数据结构变化
内部metrics events。
### 协议变化
无。
### 数据库变化
无。
### 兼容要求
无collector时行为不变。
### 必须新增的测试
fixture Run断言指标存在、cardinality上限、sink failure。
### 任务级验证命令
Core/Daemon metrics tests。
### 禁止执行的重型命令
Workspace test、npm、Tauri、新基础设施安装。
### 验收标准
指标覆盖表完整且无敏感/高基数label。
### 回滚方法
移除adapter保留no-op接口或整体回滚。
### 输出物
Metrics sink、字典、tests。
### 预计复杂度
M。
### 风险等级
中。

## TASK-016：最小 Rust AgentRuntime Facade 与发布元数据

### 所属实施批次
B07。
### 任务目标
提供复用既有AgentEngine的薄Facade和外部编译示例。
### 来源问题
K01、K02、L03。
### 当前行为
agent-core是内部crate，调用者需理解较多低层细节。
### 目标行为
单一Facade组装一次Run，不复制Session/Loop/Store authority。
### 前置任务
TASK-014。
### 可并行任务
TASK-015。
### 冲突任务
无实质冲突。
### 起始 Commit
`TASK-014_MERGED_SHA`。
### 执行工作区
唯一受控Worktree。
### 构建缓存要求
共享Target，Cargo错峰；无需前端；禁止publish。
### 允许修改范围
Agent Core public facade/lib/Cargo metadata、example、权威进度文档。
### 禁止修改范围
Daemon state authority、DB、UI、Pi Runtime。
### 高冲突文件
`agent-core/lib.rs` 轻度。
### 实施步骤
定义最小inputs/outcome；复用EngineProvider/ToolRuntime；in-memory限制；example；metadata。
### 必须保持的不变量
Facade不建立第二套loop/store；Daemon仍是生产durable authority。
### 数据结构变化
最小public request/config/outcome re-export。
### 协议变化
无。
### 数据库变化
无。
### 兼容要求
旧内部调用不破坏；public API明确semver。
### 必须新增的测试
外部临时crate、两工具quick start、Daemon adapter conformance。
### 任务级验证命令
Agent Core facade test；example cargo check。
### 禁止执行的重型命令
Workspace test、npm、Tauri、cargo publish。
### 验收标准
外部示例只依赖Facade；in-memory不宣称crash-safe；metadata完整。
### 回滚方法
独立Facade commit整体回滚。
### 输出物
Facade、example、API限制说明。
### 预计复杂度
L。
### 风险等级
中。

## TASK-017：最终故障矩阵、全仓验证与发布门禁

### 所属实施批次
B08。
### 任务目标
一次性证明生产链、安全恢复、资源和发布构建。
### 来源问题
C01、C04、E01、F03（已解决能力的回归保护），L01、L02、F04及全部任务验收项。
### 当前行为
开发前尚无该最终Commit的全量证据。
### 目标行为
所有命令/场景有真实退出结果和产物记录。
### 前置任务
TASK-015、016。
### 可并行任务
无。
### 冲突任务
所有未合并生产变更。
### 起始 Commit
`natives-runtime/b07-operability`。
### 执行工作区
主集成工作区。
### 构建缓存要求
共享Target；只在此阶段复用主工作区node_modules；安装仅lockfile不一致且获批准。
### 允许修改范围
Tests、scripts、CI、唯一权威进度文档；生产缺陷需单独修复commit。
### 禁止修改范围
把测试放宽、skip/ignore、清缓存掩盖失败。
### 高冲突文件
无计划生产改动。
### 实施步骤
资源门禁；Rust；frontend；protocol/native；migration/crash/E2E；perf；release smoke；Tauri build；清理Worktree。
### 必须保持的不变量
未运行/环境失败不写通过；命令串行；不clean。
### 数据结构变化
无计划变化。
### 协议变化
无计划变化。
### 数据库变化
仅验证全新/旧库/故障迁移。
### 兼容要求
旧数据库和旧cursor/compat fixture必须通过计划契约。
### 必须新增的测试
最终计划列出的生产链、crash、queue、subagent、event、DB、resource矩阵。
### 任务级验证命令
见 `docs/testing/natives-agent-final-integration-validation-plan.md`。
### 禁止执行的重型命令
并行workspace命令、cargo clean、重复npm ci、未检查磁盘的Tauri build。
### 验收标准
P0/P1门槛、全量命令、Release smoke、磁盘/Worktree控制全部通过。
### 回滚方法
不合并；回前一稳定标签；缺陷另起短期分支修复。
### 输出物
Exit-code ledger、测试报告、资源报告、最终稳定SHA/tag。
### 预计复杂度
L。
### 风险等级
高。
