# Natives Agent Runtime 多 Agent 执行调度方案

## 1. 所有权模型

- `deploy`：只作为基线和最终合并目标；任务 Agent 不直接提交。
- `codex/agent-runtime-upgrade-20260804`：总集成分支，仅总集成 Agent 使用。
- 批次集成分支：`codex/runtime-bNN-<slug>-<base-sha8>`，仅该批次集成 Agent 使用。
- 任务分支：`codex/runtime-task-NNN-<slug>-<agent>-<yyyymmddhhmm>`，名称必须唯一。
- 同一分支不能被两个 Agent/Worktree checkout。任务 Agent 不得切换、rebase 或 force-update 别人的分支。
- 每个批次验收后记录稳定 Commit，并创建计划中的轻量 tag；后续只从解析后的精确 SHA 派发。

## 2. 工作区限制

当前可用磁盘约 34 GiB，已有 Cargo Target 7.8 GiB、主工作区 node_modules 1.0 GiB。因此：

- 同时最多 2 个 Git worktree：主工作区 1 + 任务 Worktree 1。
- 同时最多 2 个任务 Agent；只有表中明确标为并行的波次可启用第二个。
- 只有集成 Agent 操作主集成分支；并行 Agent 使用唯一 Worktree/任务分支。
- Worktree 不生成自己的 target、node_modules、release/DMG；合并并验收后立即 `git worktree remove <exact-path>`，不得删除未合并分支。
- 若可用空间低于 20 GiB或共享 Target超过25 GiB，停止创建 Worktree和重型测试；低于15 GiB停止Cargo。

## 3. 动态波次调度表

`START_SHA` 必须由集成 Agent在派发时写成40位 SHA；下表的 tag/占位符不是允许直接 checkout 的浮动值。

| 波次 | 任务 | 是否并行 | 起始 Commit | 工作区 | 共享构建目录 | 验证要求 |
|---|---|---:|---|---|---|---|
| W00 | 000 | 否 | `9584c3c263...` | 主 | shared debug target | fmt + workspace check一次 |
| W10 | 001 | 否 | `b00-baseline`解析SHA | 主 | shared | Gateway/Daemon path tests |
| W11 | 002 | 是，主 | `TASK-001_MERGED_SHA` | 主 | shared，Cargo错峰 | supervisor/shell tests |
| W11 | 003 | 是，唯一WT | 同上 | Worktree | shared，Cargo错峰 | RPC frame exact test |
| W12 | B01集成 | 否 | 002/003 commits | 主 | shared | Gateway+Daemon组合check/test |
| W20 | 004 | 否 | `b01-safety`解析SHA | 主独占 | shared | ledger/tool/crash/protocol |
| W30 | 005 | 否 | `b02-effects`解析SHA | 主独占 | shared | projector/restart tests |
| W40 | 006 | 否 | `b03-projection`解析SHA | 主 | shared | storage/fault tests |
| W41 | 007 | 否 | `TASK-006_MERGED_SHA` | 主 | shared | progress/MCP/resource tests |
| W50 | 008 | 否 | `b04-backpressure`解析SHA | 主 | shared | permission race/protocol |
| W51 | 009 | 否 | `TASK-008_MERGED_SHA` | 主 | shared | subagent lifecycle tests |
| W52 | 010 | 否 | `TASK-009_MERGED_SHA` | 主 | shared | queue crash tests |
| W60 | 011 | 否 | `b05-authorities`解析SHA | 主 | shared | context/compaction tests |
| W61 | 012 | 否 | `TASK-011_MERGED_SHA` | 主 | shared | scheduler/conflict tests |
| W62 | 013 | 否 | `TASK-012_MERGED_SHA` | 主 | shared | typed/provider/lineage tests |
| W70 | 014 | 否 | `b06-core`解析SHA | 主独占 | shared | characterization + check |
| W71 | 015 | 是，主 | `TASK-014_MERGED_SHA` | 主 | shared，Cargo错峰 | metrics tests |
| W71 | 016 | 是，唯一WT | 同上 | Worktree | shared，Cargo错峰 | facade/example check |
| W72 | B07集成 | 否 | 015/016 commits | 主 | shared | Core+Daemon+example |
| W80 | 017 | 否 | `b07-operability`解析SHA | 主独占 | shared | 完整最终矩阵，串行 |

## 4. 每次派发协议

集成 Agent在派发任务前必须提供：

```text
TASK_ID=
START_SHA=<40位>
TASK_BRANCH=<唯一名称>
WORKSPACE=<绝对路径>
CARGO_TARGET_DIR=/Users/ldh/Downloads/project/AiNative/Natives/.cargo-target-shared
ALLOWED_PATHS=
FORBIDDEN_PATHS=
TARGETED_TESTS=
UPSTREAM_ACCEPTANCE=
```

任务 Agent开始前执行并回报：

```bash
pwd
git status --short
git branch --show-current
git rev-parse HEAD
git worktree list
df -h .
du -sh "$CARGO_TARGET_DIR" 2>/dev/null || true
pgrep -afil 'cargo|rustc|rustdoc|vitest|jest|tsx|next|vite' || true
```

如果 HEAD 不等于 `START_SHA`、分支被占用、工作区不干净或已有 Cargo 进程，任务不得开始。

## 5. 分支与合并流程

1. 集成 Agent从上一稳定 SHA 创建批次集成分支。
2. 串行任务直接创建短期任务分支，验收后由集成 Agent fast-forward/cherry-pick到批次分支。
3. 并行任务必须从同一 SHA创建两个唯一分支；主任务用主工作区，第二任务用唯一 Worktree。
4. 任务 Agent只提交允许范围；不合并、不rebase、不处理跨任务冲突。
5. 集成 Agent检查 `git diff <start>...<task>` 路径边界和测试结果后合并。
6. 有冲突时，优先要求后开始任务基于已合并 SHA重做；禁止大规模手工拼接两个状态机实现。
7. 批次级测试通过后记录 SHA、创建稳定 tag、删除任务 Worktree和已合并短期分支。
8. 批次失败回到上一稳定 tag；additive migration不删除，关闭自动执行路径并修复。

建议提交：

```text
fix(runtime): authorize checkpoint paths before capture
fix(tool-runtime): make side-effect settlement durable
fix(conversation): project committed typed turns idempotently
fix(runtime): bound progress and storage queues
refactor(agent-core): split turn and tool orchestration
test(runtime): cover crash recovery matrix
```

## 6. 并行安全判定

允许 `002 || 003`：RPC文件与ProcessSupervisor文件不重叠；两者仅共享构建缓存，Cargo必须错峰。

允许 `015 || 016`：Metrics主要新增Daemon模块，Facade主要新增Agent Core public层；从拆分后的同一SHA开始，禁止双方修改对方允许范围。

其他任务不并行，原因不是Agent数量不足，而是共同修改以下状态权威：

- 004/005/006：数据库事务、event、projection。
- 007/008/009：`production_tools.rs`和runtime actors。
- 010/011/012/013：`engine.rs`、`run_manager.rs`、`production.rs`。
- 014：移动上述高冲突代码，必须独占。

## 7. 批次交接模板

```markdown
# Batch BNN handoff

- Base SHA:
- End SHA:
- Stable tag:
- Merged tasks:
- Migrations:
- Protocol changes:
- Compatibility state:
- Tests passed (command + exit):
- Tests not run:
- Shared target before/after:
- Free disk before/after:
- Remaining worktrees:
- Known risks:
- Next batch START_SHA:
```

## 8. 禁止事项

- 不得 Push/PR，除非用户另行授权。
- 不得让任务 Agent直接修改`deploy`或总集成分支。
- 不得用`git reset --hard`、自动stash、强制rebase处理用户工作。
- 不得同时运行两个Cargo workspace命令。
- 不得为提高并行度再创建第三个Worktree。
- 不得让Agent自行安装sccache/ccache或改全局Rust配置。
