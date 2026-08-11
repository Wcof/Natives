# Natives Agent Runtime 构建缓存与磁盘控制方案

## 1. 当前基线

记录时间：2026-08-09；源码基线 `d25aa644d2f6a71d1d50752627fc9f8032ce7438`。

| 项目 | 当前值 |
|---|---|
| 文件系统可用空间 | 约 17 GiB（已进入低磁盘模式） |
| 唯一共享 Cargo Target | `/Users/ldh/Downloads/project/AiNative/Natives/target`，约 11 GiB |
| 主工作区 node_modules | 约 891 MiB |
| Cargo registry | 约 307 MiB |
| npm cache | 约 193 MiB |
| Rust | `rustc/cargo 1.96.0` |
| Workspace crates | natives、agent-core、assistant-protocol、harness-core、capability-gateway、contract-linter、provider-adapters、natives-agent-daemon |
| 包管理器 | npm；`package-lock.json`；未声明 `packageManager` 字段 |
| sccache / ccache | 未发现；本计划不安装 |

## 2. 单一缓存策略

所有 Debug check/test 统一：

```bash
export NATIVES_REPO_ROOT=/Users/ldh/Downloads/project/AiNative/Natives
export CARGO_TARGET_DIR="$NATIVES_REPO_ROOT/target"
export CARGO_BUILD_JOBS=2
export RUST_TEST_THREADS=2
export CARGO_INCREMENTAL=0
```

规则：

- 主工作区和所有获准任务 Worktree 只复用该目录，不建立任务专属 target。
- 同一时刻只运行一个Cargo命令；共享目录用于复用，不用于并发吞吐。
- Debug/Check/Test共享现有target。Release/Tauri仅最终阶段在同一target串行执行一次，避免另建一套依赖缓存。
- 不同toolchain/target triple若未来确有需要，必须使用共享根下按toolchain/target命名的单个子目录并先做磁盘评估；任务Agent无权自行创建。
- 不复制target，不执行`cargo clean`，不手删fingerprint/build/native artifacts。
- Build script或native dependency异常时先记录命令和目录增长，由集成Agent决定；不得以清缓存作为第一反应。

## 3. Cargo并发与命令分级

### 任务级

- `cargo check -p <受影响crate>`。
- `cargo test -p <crate> <exact_filter> -- --test-threads=2`。
- 如格式命令不能局部化，可执行一次`cargo fmt --check`。
- 禁止workspace test、Clippy all-targets、release和Tauri。

### 批次级

- 集成Agent在任务合并后执行相关crate组合测试。
- 仅B04等跨crate批次允许一次`cargo check --workspace --jobs 2`。
- 同一失败命令最多重跑一次，且必须先解释失败原因；不得盲目循环。

### 最终级

按最终测试计划串行执行workspace check/clippy/test、前端、native verifier、release smoke/Tauri。

## 4. 前端依赖复用

- 保持npm和`package-lock.json`，禁止切换pnpm/yarn。
- Rust任务不需要node_modules，不运行npm命令。
- 协议任务在主工作区复用现有1.0 GiB `node_modules`执行`npm run protocol:check`；受控Worktree不复制、不symlink node_modules。
- 只有修改`package.json`或`package-lock.json`且经批次集成Agent批准，才允许一次受控安装。
- 最终阶段先验证`package-lock.json`和现有依赖；只有依赖缺失/不一致才执行一次`npm ci`，并记录前后大小。不得每任务安装。
- 复用现有`~/.npm`缓存；不删除、不另建任务级npm cache。

## 5. Worktree空间政策

- 常规最大总数：2（主工作区 + 1 个任务 Worktree）。
- 本文第 10 节的“双 Goal 低磁盘模式”可临时放宽为 3（主工作区 + 2 个**仅源码**任务 Worktree）；两个任务 Worktree 禁止产生各自的 `target`、`node_modules`、`.next`、coverage、release、app 或 DMG。
- Worktree必须位于项目父目录，名称包含任务号和唯一时间戳。
- 只有计划明确的并行组或第 10 节登记的两个 Goal 可创建 Worktree。
- 创建前必须确认共享文件冲突已指定合并顺序，任务预计超过 30 分钟，并估算创建后可用空间仍 ≥15 GiB。
- Worktree不得包含自己的target、node_modules、`.next`、release、app或DMG。
- 合并并验收后立即移除精确Worktree路径；删除前确认clean/commit存在。禁止递归删除模糊路径。

## 6. 磁盘阈值

| 条件 | 动作 |
|---|---|
| 可用空间 ≥25 GiB 且 Target <20 GiB | 可执行计划内任务/批次测试 |
| 可用空间 20–25 GiB | 禁止 Release；只做定向测试；新 Worktree 必须是计划登记的仅源码 Worktree |
| 可用空间 15–20 GiB 或 Target >25 GiB | 进入低磁盘模式：允许已登记的仅源码 Worktree，暂停 npm/Cargo 重型测试与构建 |
| 可用空间 <15 GiB | 停止所有Cargo/npm build |
| Target >35 GiB | 停止Cargo test |
| Target >45 GiB | 停止全部Cargo并请求人工处理 |

这些阈值是暂停门槛，不是自动删除授权。禁止删除共享缓存、源码、数据库、未提交文件或用户node_modules。

## 7. 监控命令

每批开始和结束由集成Agent执行：

```bash
df -h .
du -sh /Users/ldh/Downloads/project/AiNative/Natives/target 2>/dev/null || true
du -sh /Users/ldh/Downloads/project/AiNative/Natives/node_modules 2>/dev/null || true
du -sh /Users/ldh/.cargo/registry 2>/dev/null || true
du -sh /Users/ldh/.npm 2>/dev/null || true
git worktree list
pgrep -afil 'cargo|rustc|rustdoc|vitest|jest|tsx|next|vite' || true
```

记录表：

| Batch | Free before | Free after | Target before | Target after | node_modules | Worktrees | 异常增长说明 |
|---|---:|---:|---:|---:|---:|---:|---|
| B00 | 34 GiB | 待填 | 7.8 GiB | 待填 | 1.0 GiB | 2（当前含审计WT） | 基线 |
| B01 | 待填 | 待填 | 待填 | 待填 | 不变 | ≤2 | |
| B02 | 待填 | 待填 | 待填 | 待填 | 不变 | 1 | |
| B03 | 待填 | 待填 | 待填 | 待填 | 不变 | 1 | |
| B04 | 待填 | 待填 | 待填 | 待填 | 不变 | 1 | |
| B05 | 待填 | 待填 | 待填 | 待填 | 不变 | 1 | |
| B06 | 待填 | 待填 | 待填 | 待填 | 不变 | 1 | |
| B07 | 待填 | 待填 | 待填 | 待填 | 不变 | ≤2 | |
| B08 | 待填 | 待填 | 待填 | 待填 | 记录 | 1 | Release只一次 |

## 8. 异常增长处理

1. 停止启动新构建，不kill未知进程。
2. 用`pgrep`确认所有者；联系对应Agent结束/等待。
3. 对比Target的debug/release/build/deps目录大小，只读定位增长来源。
4. 判断是否来自新toolchain/target/profile/feature或Tauri bundle。
5. 集成Agent决定继续、推迟Release或请求用户授权清理；任务Agent不得自行清理。

## 9. 明确禁止

- `cargo clean`及其变体。
- 每任务`cargo build/test --workspace`。
- 每Worktree独立Target/node_modules/npm cache。
- 无必要`npm ci`、切换包管理器或修改全局Rust环境。
- 并行Cargo workspace命令。
- 每Worktree生成`.next`、APP、DMG或Release。
- 为引入缓存而安装sccache/ccache；只有未来有独立基础设施任务和测量收益时再评估。

## 10. 双 Goal 低磁盘模式（2026-08-09）

适用任务：

1. `codex/assistant-engine-production`：助理 / Native 执行引擎 / Harness / Subagent 生产化。
2. `codex/macos-menubar-overview`：macOS 菜单栏常驻与个人概览浮窗。

### 10.1 分支与 Worktree

- 两个 Goal 使用不同 `codex/` 分支和两个仅源码 Worktree 并行开发。
- 主工作区保留为最终集成与唯一重型验证位置，不在两个任务 Worktree 中复制依赖。
- 两个方案都可能修改 `src-tauri/src/lib.rs`：引擎分支只提交 `run.watch` State/handler 装配；菜单栏分支提交 Tray/窗口生命周期，并在引擎分支合并后 rebase。
- 创建第二个任务 Worktree 后必须立即复测可用空间；若低于 15 GiB，停止构建并移除尚未开始、clean 且无提交的精确 Worktree，不得删除缓存腾挪。

### 10.2 开发期门禁

任务 Worktree 默认只运行：

```bash
rtk git diff --check
rtk cargo fmt --check
```

按文件或纯逻辑测试确有必要时，可由集成负责人批准在唯一共享 `target` 中串行执行一次精确测试。任务 Agent 禁止自行运行：

- `cargo test --workspace`
- `cargo check --workspace`
- `cargo clippy --workspace --all-targets`
- `npm run typecheck`
- `npm run test`
- `npm run perf:check`
- `npm run build`
- `tauri build`

前端任务 Worktree 不安装依赖、不复制或软链接 `node_modules`。前端完整检查在提交进入主集成分支后，复用主工作区现有依赖统一执行。

### 10.3 构建租约

- 任意时刻只有集成负责人可以发起 Cargo/npm 重型命令。
- 开始前记录进程、磁盘和 Target 大小；发现已有 `cargo`、`rustc`、`next`、`tsx` 等进程时，先确认所有者，不得并发启动第二套门禁。
- 同一失败命令最多重跑一次；先定位根因。超时或锁等待不是循环重跑的理由。
- 两个 Goal 都完成定向测试后，合并到同一集成分支，再串行运行一次完整门禁和一次 Tauri/Release smoke；不得每个分支各构建一份 App/DMG。

### 10.4 安全自动清理

任务 Agent 可在每个批次结束自动清理，但范围仅限它自己创建且可重新生成的产物：

- 任务 Worktree 内意外产生的 `.next`、`coverage`、`dist`、`out` 或任务专属 `target`。
- 任务创建、带任务唯一前缀并记录在交付报告中的临时目录。
- 已合并、已提交、`git status --short` 为空的精确任务 Worktree，由集成负责人使用 `git worktree remove <absolute-path>` 回收。

执行清理前必须同时满足：

1. 目标是经过解析的绝对路径，且位于该任务 Worktree 或该任务登记的临时目录内。
2. `git check-ignore` 或任务记录证明它是生成物；不得删除 tracked 或未提交文件。
3. `pgrep` 证明没有进程正在使用该目录。
4. 先输出目标和大小，再按精确路径删除；禁止通配符、未解析变量、`git clean -fdx` 或递归清理父目录。

自动清理永远不得触碰：

- 主工作区 `/Users/ldh/Downloads/project/AiNative/Natives/target`
- 主工作区 `node_modules`
- `~/.cargo`、`~/.npm`
- 任意 SQLite、用户项目、凭证、日志证据或未提交文件

### 10.5 最终唯一门禁

两个分支合并到同一个集成 HEAD 后，由集成负责人在主工作区串行执行：

```bash
rtk npm run typecheck
rtk npm run lint
rtk npm run test
rtk npm run protocol:check
rtk npm run verify:native-engine
rtk cargo fmt --check
rtk cargo test --workspace
rtk npm run perf:check
```

涉及的专项测试先于全量门禁运行。`tauri build` / Release smoke 只在上述门禁通过且可用空间满足预算后执行一次。空间不足时标记最终打包为 `blocked_by_disk`，不得让两个 Goal 各自重复构建。

## 11. 全仓模块化整改与最终 deploy 集成（2026-08-09）

适用任务：`codex/modular-architecture-remediation`，详细范围见 `../architecture/MODULAR_ARCHITECTURE_REMEDIATION.md`。

### 11.1 单 Worktree、共享 Subagent

- 本任务只允许一个源码 Worktree；最多三个 Subagent 在同一共享文件系统中按互斥 ownership 并行，禁止每个 Subagent 再创建 Worktree。
- 当前优先复用 `/Users/ldh/Downloads/project/AiNative/natives-modular-architecture-20260809-234252`；
  使用前验证其 branch、HEAD、status 和进程。只有路径不存在或不可安全复用时，才由主 Agent 创建一个替代源码 Worktree。
- 主工作区中的后续研究文档是用户现有改动，不得 stash、reset、提交、删除或覆盖；最终 deploy 仍不 clean
  时标记 `BLOCKED_BY_DIRTY_DEPLOY`，不得为了 fast-forward 擅自处理用户文件。
- 所有 Subagent 复用第 2 节的唯一 Cargo target；不得建立任务 target、node_modules、`.next`、coverage、dist、out、release、APP 或 DMG。

### 11.2 分支吸收与唯一构建

1. 从最新 `deploy` 创建 `codex/modular-architecture-remediation`。
2. 记录所有本地分支及 `--no-merged deploy` 结果；已是祖先的分支记为 no-op。
3. 正在开发分支完成后，依次合入整改集成分支；对合并结果做全仓规模、依赖、数据 authority、UI/UX、性能与安全复审并继续整改。
4. 所有分支与整改提交进入同一 integration HEAD 后，才在主工作区复用现有 node_modules 和共享 Cargo target 串行运行一次完整门禁。
5. 门禁通过后更新本地 `deploy`；不因本任务自动 push。

不得在每个分支各执行 typecheck、workspace Cargo test、Next build 或 Tauri build。冲突必须逐块解决，禁止 `ours/theirs` 整文件覆盖、`reset --hard`、`checkout --` 或 `git clean -fdx`。

### 11.3 自动清理与卡死控制

- 每批开始和结束执行第 7 节监控命令；低于 15 GiB 停止所有 build/test，15–20 GiB 只运行静态扫描和经批准的 exact test。
- 超过 60 秒仍无可解释进展的命令先读取输出、确认是否等待锁/磁盘/进程，再由集成负责人终止；同一失败命令最多重跑一次。
- 可自动清理的范围仅为本 Goal 自己产生且 `git check-ignore` 可证明的精确 `.next`、coverage、dist、out、任务 target、带任务唯一前缀的临时目录，以及已合并/已提交/status clean 的精确 Worktree。
- 清理前必须解析并打印绝对路径与大小，确认无进程占用。禁止清理主共享 target、node_modules、Cargo/npm cache、数据库、日志证据、源码、用户文件或未提交内容。

### 11.4 唯一 candidate、构建复用与完成语义

- 三个 Subagent 不运行 typecheck、全量 npm test、Cargo workspace、Native Engine、perf、Next 或 Tauri build；
  主 Agent独占唯一构建租约，并为 candidate HEAD 记录开始/结束时间、退出码和脱敏日志。
- `<15 GiB` 只允许源码与静态检查；`15–20 GiB` 只允许经批准的 exact test；`>=20 GiB` 才允许主
  Agent 串行执行重型门禁。Tauri/Release 前还必须保证预计构建后保留至少 5 GiB 安全余量。
- `perf:check` 成功生成同一 candidate 的 `out/` 后，可使用独立 Tauri override 将
  `beforeBuildCommand=null` 来复用该输出；默认 Tauri 配置不得改成无条件复用。两条命令之间任何源码、
  config 或 lockfile 变化都会使复用失效并要求重跑 perf。
- 任一 mandatory gate 的 `FAIL`、`BLOCKED_BY_DISK`、`BLOCKED_BY_DIRTY_DEPLOY`、`NOT_RUN` 或 `SKIPPED`
  都不等于通过。空间不足只能保持 Goal active；同一阻断连续三次审计仍无法推进时才标记 blocked，
  不得标记 complete，不得更新 deploy。
- 最终证据写入 ignored 的 `.runtime-evidence/gate/modular-remediation-<candidate>/` 并生成 checksum；
  文档提交不得改变已测试源码，否则重跑受影响门禁。
