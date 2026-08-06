# Natives Agent Runtime 最终集成验证计划

> 仅在B00–B07全部通过并合并到精确稳定SHA后执行。完整命令只跑一次；失败后先定位，修复必须独立commit，再只重跑受影响门禁和最终摘要。

## 1. 启动门禁

必须同时满足：

- 当前分支是总集成分支，HEAD等于`natives-runtime/b07-operability`解析SHA。
- Working tree clean，无未合并任务分支或生产改动。
- 只剩主集成Worktree；可用空间≥25 GiB；共享Target<25 GiB。
- 无遗留Cargo、Rust、Node、Vitest、Next、Tauri测试进程。
- `CARGO_TARGET_DIR`、jobs/test threads/incremental按缓存政策固定。
- 主工作区node_modules与`package-lock.json`一致；不默认执行`npm ci`。

记录：

```bash
pwd
git status --short
git branch --show-current
git rev-parse HEAD
git worktree list
df -h .
du -sh "$CARGO_TARGET_DIR" node_modules ~/.cargo/registry ~/.npm 2>/dev/null || true
pgrep -afil 'cargo|rustc|rustdoc|vitest|jest|tsx|next|vite|tauri' || true
```

任一资源门禁不满足，状态为`BLOCKED_BY_ENVIRONMENT`，不得写“测试通过”。

## 2. 命令执行账本

顺序执行，保存开始/结束时间、exit code、日志路径、峰值磁盘。使用项目要求的`rtk`。

| 顺序 | 命令 | 通过条件 | 失败分类 |
|---:|---|---|---|
| 1 | `rtk cargo fmt --check` | exit 0 | CODE |
| 2 | `rtk cargo check --workspace --jobs 2` | exit 0 | CODE/ENV |
| 3 | `rtk cargo clippy --workspace --all-targets -- -D warnings` | exit 0 | CODE |
| 4 | `rtk cargo test --workspace -- --test-threads=2` | exit 0，无ignore掩盖关键测试 | CODE/FLAKE/ENV |
| 5 | `rtk npm run typecheck` | exit 0 | CODE/ENV |
| 6 | `rtk npm run lint` | exit 0 | CODE |
| 7 | `rtk npm run test` | exit 0 | CODE/FLAKE |
| 8 | `rtk npm run build` | exit 0 | CODE/RESOURCE |
| 9 | `rtk npm run perf:check` | exit 0，预算无豁免 | PERF |
| 10 | `rtk npm run protocol:check` | exit 0 | PROTOCOL |
| 11 | `rtk npm run verify:native-engine` | exit 0 | PRODUCTION_CHAIN |
| 12 | Release smoke（项目脚本或`cargo build --release`定向） | Daemon/Host启动和握手成功 | CODE/RESOURCE |
| 13 | `rtk npm run tauri:build` | exit 0，APP可启动 | PACKAGE/RESOURCE |

若步骤12无现有脚本，TASK-017只增加最小脚本，不创建第二套build系统。Tauri正式构建只执行一次。

## 3. 真实生产链验证

至少一个测试必须实际走：

```text
Renderer command
→ Tauri Host adapter
→ authenticated UDS
→ Daemon RPC
→ RunManager
→ ProductionRuntime
→ AgentEngine
→ Provider adapter fixture server
→ Capability Gateway
→ real temporary-directory tool
→ Event/Conversation/Ledger/Checkpoint SQLite
→ Daemon restart/replay
→ Renderer projection reducer
```

禁止用直接调用`AgentEngine + FakeTool`替代。Provider可用本地wire-compatible fixture server，工具只操作临时目录，不访问用户项目或外部副作用。

生产链断言：

- Native Provider旁路为0；Tool Gateway旁路为0。
- Run terminal fact只有RunManager提交一次。
- Event sequence严格单调唯一；订阅和replay同一payload/sequence。
- Typed conversation reload进入下一Provider request，block order/tool pair无损。
- Renderer不生成权威sequence/terminal，不重复Assistant message或analytics结算。

## 4. Run / Turn 矩阵

| 场景 | 必须断言 |
|---|---|
| 正常无工具 | TurnStarted/Completed配对，Run terminal恰一 |
| 单/多工具 | 同Turn call/result完整，下一Turn自动继续 |
| Provider可重试错误 | attempt增加但不创建伪Turn，不重复计费ID |
| Provider不可重试 | 结构化失败，零工具执行 |
| Empty/length/no-final/unknown stop | 文本兼容；候选tool零执行且同ID错误结果 |
| Cancel streaming | Provider future退出，无late delta/terminal重复 |
| Tool失败/timeout/cancel | paired result恰一，错误码区分 |
| Max steps/doom loop | 正确stop reason，RunManager单终态 |
| Retry/Continue/Fork/Resume | 新Run与lineage准确，snapshot契约符合任务013 |
| 双终态竞争 | CAS只接受一次，Renderer只结算一次 |
| Daemon重启 | active run进入明确interrupted/resumable/blocked语义 |

## 5. Tool / 副作用矩阵

- 无副作用读工具、文件读、合法文件写、Shell、MCP、Sub Agent。
- Malformed JSON、root非object、required/type/enum/additional/nested错误，handler调用为0。
- Unknown tool、Hook deny、Permission deny/timeout、output limit均形成同ID result。
- Path absolute、dotdot、symlink escape、TOCTOU fixture在checkpoint/ledger/handler I/O前拒绝。
- ParallelSafe/Sequential/Exclusive和相同/不同conflict key；Result源序、Completed完成序。
- Tool handler成功但ledger settlement、ToolCompleted、projector或checkpoint失败：不得回灌成功；重启后`Confirm/Blocked`，自动handler调用为0。
- Cancel所有并行future；Shell kill+wait；MCP pending丢弃；registry最终quiet。

## 6. Crash point矩阵

使用子进程、临时SQLite和barrier，不使用随机sleep：

| CP | Kill点 | 重启后期望 |
|---:|---|---|
| 1 | Tool intent commit前 | 无handler执行，可安全重试 |
| 2 | intent后/handler前 | planned/started可识别，不自动外部重放 |
| 3 | handler返回后/settlement前 | uncertain或Blocked，handler总调用1 |
| 4 | settlement后/ToolCompleted前 | 可恢复fact，不重复handler |
| 5 | TurnCompleted后/projector前 | projector补齐完整Typed Turn |
| 6 | assistant row后/tool results中间 | transaction回滚或幂等补齐 |
| 7 | projector commit后/watermark前 | 重跑不重复消息 |
| 8 | checkpoint metadata/cursor中间 | cursor不前进或可验证，绝不冒充safe |
| 9 | Permission decision后/outbox交付前 | 重启交付一次，handler至多一次 |
| 10 | Queue lease后/Context commit或ack前 | 消息不丢，模型Context至多提交一次 |

每个点验证五类证据：DB rows、event replay、typed reload、ResumePlan、handler invocation count。

## 7. Queue 与交互

- Steering在工具执行期间提交，仅在safe point进入下一上下文。
- Follow-up只在准备结束时消费；one/all混合FIFO符合固定契约。
- Lease后/ack前kill、Actor restart、多Run相同conversation隔离。
- Abort后queue保留/恢复符合契约；不得插入partial assistant。
- Permission双击相同决策幂等，Allow/Deny race有唯一赢家，断连/重启outbox恢复。

## 8. Checkpoint / Resume

- Provider前、stream中、Assistant后、ToolStarted后、Tool执行后、Result前、Checkpoint中、Terminal前崩溃。
- Snapshot包含可解析完整active context；坏JSON、dangling pair、missing block显式Blocked。
- Checkpoint ledger cursor对应可查询ledger sequence，不等于偶然event sequence。
- `uncertain`、`started`、`legacy_unverifiable`均禁止自动恢复副作用。
- Workspace artifact只含ProjectRoot内授权路径；敏感/越界路径无快照。

## 9. Sub Agent

- 最大深度、并发、token/tool budget、timeout、父cancel、三层nested。
- 创建每一步故障都无session/run/reservation orphan。
- Daemon重启后reservation/usage连续；settle失败可恢复。
- Child独立run、credential、event、snapshot、ledger；root/profile/allowlist只能收紧。
- FailurePolicy三种策略生产行为可断言；若已删除则协议/广告无残留。

## 10. Event / Replay / Renderer

- Sequence单调、无重复、数据库unique约束、persist-first。
- 实时订阅与`replay_after(cursor)`边界一致。
- 旧cursor、多个Renderer、慢subscriber、subscriber异常和重连。
- Progress超限按策略drop/coalesce；critical event不drop。
- Terminal revision稳定；Renderer不合成权威event，不重复消息/analytics。

## 11. 数据库与迁移

| 场景 | 期望 |
|---|---|
| 全新DB | 所有表/index/foreign key/WAL启用 |
| 当前deploy旧DB | Additive migration成功，历史对话可读 |
| 更老compat fixture | 集中reader命中被记录，不静默丢块 |
| 重复migration | 幂等，不DROP/重建用户表 |
| SQLITE_BUSY | bounded retry/结构化失败，不丢critical fact |
| SQLITE_FULL/I/O error | fail closed，side effect进入Blocked/uncertain |
| Corrupt event/snapshot | 对应run隔离，禁止静默跳过或自动resume |
| WAL recovery | restart后facts/projector一致 |
| 两个Daemon竞争 | 文件锁/authority阻止双writer |

## 12. 性能与资源

相同Apple Silicon设备、Release构建和固定fixture记录前后：

- Daemon idle RSS、10个并发Run RSS。
- 100MB Progress慢消费者的峰值RSS、queue depth、drop/coalesce计数。
- Event append、Tool fact transaction、Checkpoint、Projector p50/p95/p99。
- Storage actor queue saturation和Tokio stall。
- 多Run/多Sub Agent并发上限。
- 共享Target、node_modules、项目总磁盘增长。

必须满足项目`technical/04`：流式更新批处理和限额、缓存/并发有界；性能改变要有同设备前后证据。

## 13. Exit-code记录格式

```markdown
| Commit | Command/Scenario | Start | Duration | Exit | Log | Result | Failure owner |
|---|---|---|---:|---:|---|---|---|
```

状态只允许：`PASS`、`FAIL_CODE`、`FAIL_ENVIRONMENT`、`NOT_RUN_BLOCKED`。不得使用“基本通过”。

## 14. 最终验收

必须全部满足：

1. TASK-001/002/004/005对应P0关闭。
2. 所有纳入P1达到任务卡标准；未完成P2不得被写成完成。
3. Native Provider/Gateway旁路为0；Run terminal提交入口为1。
4. Unknown side effect自动Resume次数为0；指定10个crash points通过。
5. Event/Queue/Progress/Permission/Sub Agent/DB矩阵通过。
6. Rust、前端、protocol、native-engine、production E2E和migration命令均PASS。
7. Release smoke与Tauri build只执行一次并PASS。
8. 无ignore/feature gate/no-op/fake替代生产验证。
9. 最终Worktree数量为1，无遗留测试进程。
10. Target/项目磁盘增长有归因且未越阈值。

任何一项失败：不合并`deploy`，状态为`NOT_READY_FOR_PRODUCTION`，回到最近稳定批次处理。
