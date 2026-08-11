# Grok Build Agent 工程复核

> 复核日期：2026-08-10
> 仓库：`/Volumes/UNTITLED/本人材料/project/grok-build`
> 固定版本：`c68e39f60462f28d9be5e683d9cbe2c57b1a5027`
> 研究范围：Agent Harness、Tool Runtime、权限与沙箱、Prompt/Context、压缩、Hook、Session、Subagent、MCP/ACP、Event/Replay、进程边界与模块设计。
> 结论性质：固定提交的源码、测试合同、配置与用户指南静态复核；未启动 Grok Build、未执行其全仓测试。仓库只有一个可见提交，工作树改动不纳入证据，不评价历史演进成熟度。

## 1. 结论

Grok Build 是本轮最完整的 Rust Agent 工程之一。它的优势不是单个 Tool 数量，而是形成了多层可组合合同：`Tool::Args/Output` 保持类型，`ToolDyn` 只在 JSON 边界做擦除；流式执行要求 `Progress* + exactly-one Terminal`；`TypedExtensions` 按类型注入宿主能力；SessionActor 再统一承接 sampling、permission、Hook、并发 dispatch、interjection、compaction、subagent 和 replay。它还同时实现了可游标重放的 `updates.jsonl`、诊断用 `events.jsonl`、跨压缩 rewind checkpoint，以及 child Session lineage。

但这些能力不能整体等同于 Natives 所需的 Native 执行事实引擎。Grok 默认 sandbox profile 是 `off`，内建 sandbox 应用失败后会无沙箱继续；command/client Hook 明确 fail-open，HTTP Hook 的重定向没有重复 SSRF 校验；并发工具按完成顺序写入模型历史，相同文件锁没有 canonical resource identity；compaction checkpoint 与 chat replacement 不是事务提交；Tool 401 和 managed MCP 重试没有副作用 unknown-outcome journal。Subagent 有可恢复 metadata，但崩溃前的 running child 只会被归为 cancelled，不会续跑。

综合评价：

| 维度 | 评价 | 事实依据 |
| --- | --- | --- |
| Kernel loop | 强 | 中央 sampling/tool loop、safe-point interjection、结构化终止、多个局部 fuse；默认 `max_turns` 可为空，缺统一 wall-clock/tool/token 总预算 |
| Tool runtime | 强 | typed Args/Output、object-safe dispatch、Progress/Terminal、missing-terminal 检测、typed dependency injection |
| Tool 并发 | 中偏强 | batch 并发、快工具先回 UI、同原始路径串行；按完成顺序入史，锁 key 未 canonicalize 且只覆盖当前 batch |
| 权限 | 中偏强 | managed deny 优先，plan edit gate 不被 YOLO 绕过，channel 断开 Reject；路径身份与跨 Run grant/resource binding 不完整 |
| OS sandbox | 中 | Linux Landlock/bwrap/seccomp、macOS Seatbelt、进程组/Job Object；默认关闭，内建 profile 失败 fail-open，macOS 网络限制为空操作，Windows 无对应 OS sandbox |
| Prompt/context | 中 | system prompt 和 PromptContext 可落盘、恢复保留历史 System；`prompt_context.json` 不是恢复权威且会被覆盖，没有 effective prompt hash |
| Compaction | 强（算法）/中（提交） | 两阶段摘要、stale prefix 检查、tool-pair 校验、失败回退；checkpoint/marker/history replacement 非事务、错误只 warning |
| Hook | 弱到中 | 事件面完整、PreToolUse 可改写/阻断；安全失败默认放行，HTTP redirect 可绕过初始 SSRF 校验，输出读取无真实上限 |
| Session/replay | 强（展示恢复）/中（执行恢复） | `eventId`、游标续传、rewind 过滤、delta replay、TurnCompleted durable twin；不是 operation/event transaction，部分事件主动不持久化 |
| Subagent | 中偏强 | child Session、lineage、meta、resume provenance、orphan reconciliation；无 running child 续跑、权限继承、worktree 失败退共享目录、缺独立总预算 |
| 模块深度 | 中 | 测试极广；config/workspace/session/subagent 协调文件达到 2.4k–11k 行，认知和一致性风险高 |

## 2. 证据口径

本报告只把固定提交中能从生产调用链与测试合同相互印证的机制称为“当前能力”。用户指南用于确认配置意图，不能替代实现。仓库只有一个可见提交，因此不从提交数量推断“稳定”“长期验证”或“持续演进”。

关键证据：

- Tool 类型与 object-safe 边界：`crates/common/xai-tool-runtime/src/tool.rs`、`dispatch.rs`、`context.rs`。
- Agent 构建：`crates/codegen/xai-grok-agent/src/agent.rs:14-251`、`builder.rs`、`prompt/context.rs:79-198`。
- 中央 loop：`crates/codegen/xai-grok-shell/src/session/acp_session_impl/turn.rs:1350-1495,1740-2320`。
- Tool dispatch：`acp_session_impl/tool_calls.rs:323-734,1980-2295`。
- Session replay：`session/storage/mod.rs:907-1205`、`agent/mvp_agent/mod.rs:1400-1595`。
- Event log：`crates/codegen/xai-file-utils/src/events/log.rs`、`tracker.rs`、`types.rs`。
- Compaction：`session/compaction.rs:1470-1705`、`chat_persistence.rs`、`persistence.rs:1510-1602`。
- Subagent：`agent/subagent/mod.rs`、`coordinator_lifecycle.rs`、`coordinator_query.rs`、`handle_request.rs` 与 `xai-grok-tools/.../task/`。
- Sandbox：`crates/codegen/xai-grok-sandbox/src/lib.rs`、`profiles.rs`、`child_net.rs` 和 shell config `1244-1337`。
- Hook：`crates/codegen/xai-grok-hooks/src/dispatcher.rs` 及 Session Hook bridge。

## 3. 默认执行链与权威划分

默认路径可以还原为：

```text
MvpAgent / ACP request
  -> SessionActor admission + prompt queue
  -> handle_prompt
  -> process_conversation_turn_with_recovery
  -> build model-visible request from ChatState
  -> Sampler actor
  -> response/tool calls
  -> prepare_tool_call
       -> parse -> PreToolUse Hook -> permission/plan gate
  -> FuturesUnordered dispatch
       -> WorkspaceOps / ToolBridge / MCP / task runtime
  -> completion-order post-flight + ChatState settlement
  -> next sampling loop / turn terminal
  -> ReplayBuffer -> persistence actor + gateway
```

`xai-tool-runtime` 负责通用 Tool 合同，`xai-grok-tools` 负责具体实现与输出类型，`xai-grok-workspace` 负责权限和工作区执行，`xai-grok-sampler` 负责 Provider 请求/重试/流解析，`xai-grok-shell::SessionActor` 是总协调器。这个分层在 crate 级别清楚，但 SessionActor 仍同时拥有权限、Hook、MCP、Prompt、Goal、Subagent、事件和持久化编排，实际 composition root 很重。

ChatState 是模型会话内存权威；`chat_history.jsonl` 保存模型历史，`updates.jsonl` 保存客户端可回放更新，`events.jsonl` 保存诊断事件，Subagent 另有 `meta.json`。这些文件各自有用途，但没有共同的事务提交身份，不能合并解释成一个 Event Store。

## 4. Agent Loop、重试与终止合同

### 4.1 中央 loop

`process_conversation_turn` 每轮在 safe point 依次 drain interjection、skill reminder、monitor event，按需触发 compaction，再构建请求和调用 Sampler。模型返回工具后先记录 assistant/tool-call item，再执行工具；没有工具时经过 TodoGate、晚到 interjection 与 bookkeeping，最终返回 `TurnOutcome::Completed`。

终止对象是结构化的 `Completed`、`Cancelled`、`MaxTurnsReached` 或 ACP error。turn completion 还有持久化的 `TurnCompleted` xAI 通知，作为 fire-and-forget `prompt_complete` 的 durable twin；stale completion 和未启动的 queued item 不会重复发 terminal。这种单点 terminal 与防重思路值得吸收。

### 4.2 局部 fuse 很多，但不是统一预算

- `max_turns` 是可选配置，默认 agent 配置为 `None`；存在时按工具轮次限制，工具 batch 完成后检查 `next_turn > limit`。
- completion requirement 可以有自己的指数退避与 `max_retries`；达到上限后返回最后结果。
- 结构化输出最多纠正 3 次。
- TodoGate 有 `max_fires_per_prompt`。
- 401 成功刷新后最多重提 3 次，延迟固定为 1/2/4 秒；成功响应后重置 incident schedule。
- Sampler transport/empty response 有独立 retry budget。
- Doom-loop recovery 默认关闭；启用后只处理 thinking channel 的高置信 tail repetition，默认最多重新采样 2 次，预算用尽接受最后输出。

这些 fuse 各自合理，但固定树未证明顶层 Run 总是具有 wall-clock deadline、总 Tool call、总 token 和总 Provider attempt 的联合预算。Goal 外层还能在一次 `Completed` 后注入 continuation 并开始下一 round；不能用局部 `max_turns` 宣称全局 Run 有界。

### 4.3 重试与副作用

Tool 层 `call_with_auth_retry` 遇到 auth-shaped error，在共享 `OnceCell` 去重恢复后可再调用一次；managed MCP reactive reauth 成功也会重新 dispatch。实现没有 operation identity、started/unknown/completed journal，也没有证据证明错误一定发生在副作用前。因此对 read-only 请求可接受，对外部写、支付、发布、消息发送等不能透明复用。

## 5. Tool Runtime、并发与输出

### 5.1 typed streaming contract

`Tool` 通过关联类型声明 `Args` 和 `Output`；宿主 JSON 边界由 `ToolDyn` 擦除，具体工具内部仍保持类型。`ToolStream` 将事件区分为 0..N 个 progress 与恰好一个 terminal，adapter 和 `call_terminal` 都能把 stream ended without terminal 转为明确错误。`TypedToolOutput` 同时保留结构化 JSON、模型内容块和可选 completion output，避免“给 UI 的对象”和“给模型的字符串”被迫共用一种表示。

`TypedExtensions` 以 TypeId 注入 CWD、Cancellation、SessionContext 等能力，工具只声明需要的依赖。Computer Hub 的 hard cancel 会取消 token，并直接丢弃执行 Future；terminal/shell 还使用 Unix process group 或 Windows Job Object 清理进程树。这些是可迁移的深模块设计。

### 5.2 并发的真实顺序

Tool batch 的 parse、Hook 与 permission 逐个执行；approved 调用进入 `FuturesUnordered`。每个 Future 完成后立即：

1. 生成 ToolCallUpdate；
2. 将成功或失败 `ToolResult` push 到 ChatState；
3. 运行 post Hook、signal 与 telemetry；
4. 继续收下一个完成项。

因此 UI 和模型历史都是**完成顺序**，不是模型原始 call order。快 read 可以先被模型看到，但同一输入在不同调度时序下可能形成不同历史顺序，降低可重复性。`parallel_dispatch_tests.rs` 中仍有一段测试注释描述旧 `join_all` 和“按输入顺序返回”，而当前生产代码已经改为增量完成；源码合同与测试叙述存在漂移。

同批写工具会尝试从 `file_path`、`path`、`target_file` 提取锁 key，只要某路径出现在 write set，读写共享同一个 `tokio::Mutex`。但 key 只是模型提供的原始字符串，没有 normalize/canonicalize；`a/../b`、相对/绝对路径、symlink 或大小写差异可能逃离同一 bucket。锁只存在于当前 batch，跨 Session、跨 turn 和 background task 没有统一 resource lease。

### 5.3 notification 与事实

工具 progress/notification 使用无界、best-effort channel；掉线或 receiver 关闭不会改变工具事实。这个选择适合 UI 增量，不适合执行账本。Natives 应保留 typed progress，但 durable started/terminal 与副作用状态必须由 Daemon 事务写入，notification 只能是 projection。

## 6. Permission、Sandbox 与 Hook

### 6.1 Permission 优先级

已确认的优先级符合防权限升级原则：managed deny 高于 YOLO、Auto、sandbox auto-allow 和 remembered grant；permission channel 断开时 Reject；plan mode edit gate 不会被 YOLO 绕过。Auto mode 还可以调用 LLM side-query classifier，并在 15 秒 timeout/error 时回退启发式判断。

风险在资源身份。shell 命令有更完整的路径/风险分析，但直接 Read/Edit 的 policy key 与并发锁路径未证明在裁决前统一 canonicalize。remembered grant、资源锁和真正执行对象若不是同一个 canonical identity，就无法形成可审计授权。

### 6.2 Sandbox

- Unix 默认 feature 使用 nono：Linux Landlock、macOS Seatbelt；sandbox 套在整个进程上，子进程继承文件边界。
- Linux custom read-deny 使用 bwrap；bwrap 缺失或 glob 超限时 fail-closed。
- Linux child network seccomp 阻止 connect/bind/send/listen/accept。
- macOS deny glob 使用运行时 Seatbelt regex；但 child network 限制为空操作，内建 HTTP Tool 仍能联网。
- Windows 没有对应 OS sandbox，不过进程清理可用 Job Object。
- 默认 profile 是 `off`。
- built-in profile 应用失败只 warning，然后无沙箱执行；custom profile 应用失败才拒绝。
- Linux deny glob 只展开启动时已存在文件，之后新建的匹配文件不受保护。
- `strict` 仍允许写 workspace、Grok home 和临时目录，并允许较宽系统读取集合。

所以 Grok 的 sandbox 工程覆盖面比纯命令检查强，但默认值和失败策略不足以满足 Natives 的 fail-closed 防线。特别是“strict”是项目内相对级别，不等于无写、无网或最小能力。

### 6.3 Hook

command Hook 与 client Hook 都明确采用 fail-open：启动失败、transport error、timeout、malformed output 等不会自动阻止 Tool。command child 继承完整环境；stdout/stderr 在完整收集后才截断，限制不是读取过程中的 backpressure；进程终止也未证明像普通 terminal 一样完整覆盖 grandchildren。

HTTP Hook 只校验初始 HTTPS URL 和初始 DNS/IP，随后使用 reqwest 默认 redirect policy，最多跟随 10 次重定向；每个重定向目标没有重新执行 private/loopback/metadata SSRF 校验。public HTTPS endpoint 可以重定向到内部地址。blocking body 使用 `response.text()` 全量读取后才截 200 字，malformed 2xx JSON 明确 Allow。

这类 Hook 可以作为体验/观察扩展，不能进入 Natives security decision path。安全 Hook 必须最小环境、每跳重新解析和校验、流式 body 上限、进程组清理，并在 timeout/parse/transport 失败时 deny。

## 7. Prompt、Context 与 Compaction

### 7.1 Prompt authority

`PromptContext` version 1、`system_prompt.txt` 和 chat System item 都会持久化。顶层 Session 恢复时保留历史中的原 System；Subagent 恢复/创建路径会换成新 prompt。attach 时客户端 `systemPromptOverride` 可以原子替换 System 头，model/harness switch 也是实际动态入口。

问题是 `load_prompt_context()` 在固定树被标记为 dead code，生产恢复路径没有把 `prompt_context.json` 当权威；恢复时又会重新构建并覆盖它。因此该文件可能与真正保留在历史里的 System item 不一致。也没有覆盖 prompt sources、tool surface 和 final rendered text 的 effective prompt hash。`Agent::finalize_prompt()` 虽可重渲染动态时间，但固定树没有生产调用者，不能把它写成当前常规漂移路径。

### 7.2 两阶段 compaction

Grok 的 compaction 算法值得借鉴：两阶段预压缩缓存记录 prefix fingerprint、模型和 prefix 长度；提交前重查 stale prefix。选择和 sanitize 后会验证孤立 ToolResult，失败则回退最小安全历史。tool pair 分割与跨 compaction rewind 也有专项测试。

但 `DefaultHasher` fingerprint 只适合进程内 stale detection，不是稳定内容 hash/CAS。checkpoint file、CompactionCheckpoint marker、chat replacement 虽进入同一 persistence FIFO，却是三个独立写入：任一失败只 warning，没有事务、成功回执或同一 commit id。内存历史仍会立即生效，磁盘与内存可能分叉。fork 的 inherited-prefix 保留/释放决策还发生在 checkpoint 入队之后，checkpoint 可能不是最终实际 history。

因此应吸收“两阶段 prepare + stale recheck + tool-pair sanitation”，但提交必须改成 Natives 已有 ContextSnapshot/RunEvent 的事务 CAS，不复制 Grok 的多文件 best-effort protocol。

## 8. Session、Event 与 Replay

### 8.1 两条事件轨

`updates.jsonl` 是面向客户端展示与恢复的更新轨。大多数 ACP/xAI update 在 `_meta` 中带 `eventId` 和时间戳；Session load 会：

- 过滤 rewind 死分支；
- 用客户端 cursor 定位增量 tail；
- tail 出现无 `eventId` 的可转发行时退回 full replay；
- 重播时合并 ToolCall + terminal ToolCallUpdate；
- 重建 event counter，避免恢复后的 live id 倒退；
- 识别未匹配的 SubagentSpawned，并在恢复时对账。

`events.jsonl` 是 schema version 1.0 的诊断轨，覆盖 turn、phase、tool、permission、interjection、goal/laziness 和 MCP 生命周期。它由同步 append writer 写入，失败最多记录一次 warning，没有 ack/fsync，也没有被 Session replay 当成执行权威。

### 8.2 replay 能力边界

ReplayBuffer 会合并高频 ACP text/thought chunk 后再持久化和广播，因此保存的是合并后的展示事实，不保留原始网络 chunk 边界。`ToolCallDeltaChunk` 明确不持久化，依赖最终 canonical ToolCall；AvailableCommands 历史不转发；turn-end plan cleanup 是 transient；某些 progress 只用于 live UI。

正常 turn end 的 `flush_to_disk` 会先 flush ReplayBuffer，再向 persistence actor 发 `FlushAndAck`。但该 ack 只确认 merge buffer 已写，常规路径没有调用 `sync_session_files`；后者只在特定 copy/同步路径使用。JSONL reader 会跳过 malformed/torn line，保证 Session 可打开，但这等于丢失该行，而不是恢复其执行事实。

这是一套很好的客户端 reconnect 和 scrollback replay 设计，不是严格 event sourcing：通知可能丢、持久化错误可能只 warning、两条轨没有事务关联、外部副作用没有 unknown 状态。Natives 可吸收 cursor/dedup/rewind/projection 技术，但事实仍必须落到现有 Daemon RunEvent + side-effect ledger。

## 9. Subagent

child 是独立 Session 和线程，共享父文件系统、terminal、hunk tracker、scheduler 与 PermissionHandle。`meta.json` 记录 running/terminal status、父子身份、cwd、worktree、model 和 resume provenance。completed child 可在进程重启后按磁盘 meta 作为 `resume_from` 来源；Session load 会把遗留 running child 归为 cancelled，并补 `SubagentFinished`。

边界需要准确表述：

- 跨进程 resume 是“基于 completed child 历史再创建 child”，不是恢复崩溃前的 running Future。
- 最大递归深度为 1；到达深度后移除 Task tool。
- capability filter 会无条件保留没有 `ToolKind` 的 MCP/custom tools，未知类型不是默认 deny。
- child 继承父 PermissionHandle；YOLO/bypass 仍可能向下传递。
- child `maxTurns` 优先，否则继承父；两者都可为空。
- 600 秒是前台等待预算，超时后 child 转后台继续，不是 wall-clock execution deadline。
- 固定树未见 child 独立 token/tool-call 总预算。
- worktree 创建失败会退回共享 workspace，不会终止 child，因此隔离语义静默降级。
- completed 内存缓存 TTL 为 30 分钟，但 resume provenance 可从磁盘 meta 读取。
- `meta.json` 直接 `std::fs::write`，没有 temp + rename 或 `fsync`。

优点是 child identity、lineage、terminal reconciliation 与 resume provenance 很完整；缺点是共享执行面、权限继承、隔离降级和预算不足。Natives 应把这些 metadata 合入既有 child Run，而不是引入第二套 Session coordinator。

## 10. 模块深度与测试

固定树约有 2,237 个 Rust 文件，约 1,466 个文件含 `#[test]`/`#[tokio::test]` 形态标注，匹配约 25,439 处；按路径命名为 `tests/`、`*_test.rs`、`*_tests.rs` 的 Rust 文件约 425 个。测试覆盖 permission precedence、plan gate、parallel dispatch、replay cursor、TurnCompleted、compaction、subagent usage/orphan、sandbox 和 Hook 等大量边界。

但测试数量不能抵消模块深度问题。全仓最大热点包括：

- `xai-grok-pager/src/views/settings_modal.rs`：约 12,619 行。
- `xai-grok-shell/src/agent/config.rs`：约 11,284 行。
- `xai-grok-pager/src/app/app_view.rs`：约 10,366 行。
- `xai-grok-workspace/src/handle.rs`：约 9,486 行。
- `xai-grok-sampling-types/src/conversation.rs`：约 9,481 行。
- `xai-grok-workspace/src/permission/manager.rs`：约 5,633 行。

Harness 核心也偏大：`session/compaction.rs` 约 3,321 行、`tool_calls.rs` 约 3,015 行、`agent/subagent/mod.rs` 约 2,827 行、`turn.rs` 约 2,463 行。它们虽继续拆出 concern 文件，但共享 SessionActor 状态和配置表使跨模块不变量仍需人工拼接。并发测试中旧 `join_all` 注释与当前 `FuturesUnordered` 实现漂移，就是这种复杂度的具体信号。

仓库只有一个可见提交，不能像多提交项目那样评价哪些边界经过长期修复。报告只记录固定树的测试密度和当前实现。

## 11. Natives 可吸收设计

### P0：加强现有 Daemon 执行合同

1. **typed Tool envelope**：把 `Args` decode、structured output、model content、0..N progress、exactly-one terminal、missing-terminal error 合并进现有 Capability Gateway；Tool 实现不直接拥有 durable terminal。
2. **object-safe dispatch adapter**：借鉴 `ToolDyn`/dispatch，将 Tool 类型安全留在实现侧，只在注册/IPC 边界做 schema/JSON 擦除；避免所有 Tool 内部长期使用 raw `Value`。
3. **typed invocation context**：采用 TypedExtensions 的按类型依赖注入思路，但允许类型必须来自不可变 Run/Capability snapshot，禁止任意插件注入宿主 authority。
4. **canonical resource identity + lease**：把 Grok 的 per-path mutex 提升为 `canonical physical identity + effect`，覆盖同 batch、跨 turn、background 和 child Run；approval、permission key、operation journal 与 lock 必须引用同一 identity。
5. **副作用重试隔离**：Provider/auth 恢复可以重提纯请求；Tool/MCP 外部副作用必须先写 operation identity，`started/unknown` 不透明重试，`completed` 才可按结果重放。

### P1：增强 Context、Replay 与 Steering

6. **safe-point interjection**：吸收 FIFO drain 和每个 interjection 独立 synthetic item 的边界，接入现有 durable prompt queue/RunEvent；入队、claim、消费和 terminal 都有 disposition。
7. **two-pass compaction sanitation**：吸收 prefix stale check、tool-pair split、orphan validation 和最小历史回退；fingerprint 使用稳定 digest，checkpoint + snapshot + event 在现有事务中 CAS commit。
8. **replay cursor 与 projection dedup**：为 Renderer projection 吸收 `eventId` cursor、full replay fallback、rewind filtering 和 ToolCall folding；明确哪些 progress 是 transient，不能反向当作 Run fact。
9. **effective Prompt identity**：保留 source snapshot、rendered prompt hash、tool surface digest、model/harness revision；恢复必须消费同一 snapshot，不写一个未被恢复读取的旁路 `PromptContext` 文件。

### P2：Subagent 与宿主执行

10. **child lineage 与 orphan reconciliation**：吸收 parent/child/session/model/worktree/resume provenance、terminal 对账和 completed-child resume；映射到 Natives 既有 child Run/store，不复用 Grok `meta.json` 权威。
11. **隔离降级显式失败**：worktree/sandbox/capability profile 创建失败必须产生结构化 terminal 或等待用户裁决，禁止静默回共享 workspace 或无沙箱运行。
12. **跨平台进程树清理**：吸收 Unix process group 与 Windows Job Object；统一应用于 terminal、Hook、MCP stdio 和 child process，取消结果进入 durable Tool terminal。
13. **Sandbox profile snapshot**：固定 profile、平台实际能力和应用结果；Natives 继续保持 macOS Seatbelt 失败拒绝、Windows 无受控沙箱时禁 autonomous shell，不采用 Grok 默认 off/fail-open。

## 12. 明确拒绝

- 不采用 command/client/security Hook fail-open。
- 不只校验 HTTP Hook 初始 URL；每次 redirect 都必须重新 DNS/IP/协议校验。
- 不在完整读取 Hook body 或 stdout/stderr 后才截断。
- 不把无界 notification channel、`updates.jsonl` 或 `events.jsonl` 单独当成执行事实账本。
- 不使用未 canonicalize 的 path 作为 permission grant、resource lock 或 side-effect identity。
- 不按不可预测的完成顺序直接结算模型 ToolResult；执行可以乱序，事实结算应有明确、稳定的 call-order 或依赖拓扑合同。
- 不把 checkpoint、marker、chat replacement 的 FIFO 入队当作事务提交。
- 不在 worktree 或 built-in sandbox 失败时静默退回共享/无沙箱执行。
- 不把 600 秒前台等待、可选 `max_turns` 或 Doom-loop 重采样预算冒充 Run deadline。
- 不把 `prompt_context.json` 这类诊断旁路文件当 effective prompt authority。
- 不对可能已有外部副作用的 Tool/MCP 401 做无 operation journal 的透明重试。
- 不让未知 `ToolKind` 的 MCP/custom tool 因缺 metadata 自动穿过 child capability filter。

## 13. 复核后的单句评价

Grok Build 最适合借鉴 typed streaming Tool、TypedExtensions、safe-point interjection、两阶段 compaction、cursor replay、child lineage 与跨平台进程清理；它的 Hook/sandbox fail-open、非 canonical 资源锁、完成顺序入史、多文件 best-effort 提交和无副作用日志重试不适合作为 Natives Native 执行引擎的可靠性底座。
