# Natives Agent Runtime 构建缓存与磁盘控制方案

## 1. 当前基线

记录时间：2026-08-04；源码基线 `9584c3c263c1e83b1066e4208e1ab2d678a9deeb`。

| 项目 | 当前值 |
|---|---|
| 文件系统可用空间 | 约 34 GiB |
| 现有共享 Cargo Target | `/Users/ldh/Downloads/project/AiNative/Natives/.cargo-target-shared`，7.8 GiB |
| 主工作区 node_modules | 1.0 GiB |
| Cargo registry | 1.3 GiB |
| npm cache | 4.7 GiB |
| Rust | `rustc/cargo 1.96.0` |
| Workspace crates | natives、agent-core、assistant-protocol、harness-core、capability-gateway、contract-linter、provider-adapters、natives-agent-daemon |
| 包管理器 | npm；`package-lock.json`；未声明 `packageManager` 字段 |
| sccache / ccache | 未发现；本计划不安装 |

## 2. 单一缓存策略

所有 Debug check/test 统一：

```bash
export NATIVES_REPO_ROOT=/Users/ldh/Downloads/project/AiNative/Natives
export CARGO_TARGET_DIR="$NATIVES_REPO_ROOT/.cargo-target-shared"
export CARGO_BUILD_JOBS=2
export RUST_TEST_THREADS=2
export CARGO_INCREMENTAL=0
```

规则：

- 主工作区和唯一任务 Worktree复用该目录，不建立任务专属target。
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

- 最大总数：2（主工作区 + 1任务Worktree）。
- Worktree必须位于项目父目录，名称包含任务号和唯一时间戳。
- 只有计划明确的并行组002/003、015/016可创建Worktree。
- 创建前必须确认两个任务不修改同一核心文件、预计超过30分钟且可用空间≥25 GiB。
- Worktree不得包含自己的target、node_modules、`.next`、release、app或DMG。
- 合并并验收后立即移除精确Worktree路径；删除前确认clean/commit存在。禁止递归删除模糊路径。

## 6. 磁盘阈值

| 条件 | 动作 |
|---|---|
| 可用空间 ≥25 GiB 且 Target <20 GiB | 可执行计划内任务/批次测试 |
| 可用空间 20–25 GiB | 禁止新Worktree和Release；只做定向测试 |
| 可用空间 <20 GiB 或 Target >25 GiB | 暂停重型测试，分析增量；不得清理用户数据 |
| 可用空间 <15 GiB | 停止所有Cargo/npm build |
| Target >35 GiB | 停止Cargo test |
| Target >45 GiB | 停止全部Cargo并请求人工处理 |

这些阈值是暂停门槛，不是自动删除授权。禁止删除共享缓存、源码、数据库、未提交文件或用户node_modules。

## 7. 监控命令

每批开始和结束由集成Agent执行：

```bash
df -h .
du -sh /Users/ldh/Downloads/project/AiNative/Natives/.cargo-target-shared 2>/dev/null || true
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
