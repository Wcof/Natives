# G-009 结论：旧 localStorage K/V key `settings:home_workspace`

- **Task**: G-009（P0 · 依赖 G-001 · owner Main）
- **日期**: 2026-08-22
- **CONCLUSION: delete** —— 旧 K/V key 从未在任何发布版本中承载过真实用户数据；该 key 连同 `migrate_v27` 的 `import_legacy_home_workspace` 迁移代码，将在 I 组（旧路删除切片）整体删除，无需数据迁移。

## 证据（全部只读取证，可复核）

| # | 证据 | 命令 / 位置 |
|---|---|---|
| 1 | key 字面量 `settings:home_workspace` 全历史最早出现在 commit `e912e6a`（2026-08-21 09:28，ADR-0020 重构） | `git log -S 'settings:home_workspace' --all --reverse` → 首条即 `e912e6a`；`src/lib/home-workspace/persistence.ts:28` `LEGACY_STORAGE_KEY` |
| 2 | 全部 13 个 release tag 均早于 `e912e6a`，且 `e912e6a` 不属于任何 tag | `browser-20260808-175539` 等 5 个 @ 2026-08-08 18:45；`c0/c1/c4/c8-20260808-175539` 同日；`natives-runtime/b00..b07` @ 2026-08-04~05。`git merge-base --is-ancestor e912e6a <每个tag>` 全部 NOT IN |
| 3 | 最新发布版本（08-08 tag）中 `src/lib/home-workspace/` 目录**不存在**，无任何代码写该 key | `git show browser-20260808-175539:src/lib/home-workspace/persistence.ts` → fatal: path not in tag |
| 4 | v27 迁移的导入是"一次性导入后删除 key"，三分支全部删 key，且幂等（已导入则只删 stale key） | `src-tauri/src/db/migration_v27.rs`：L123 `get_setting` 读取；L130 损坏分支 `delete_setting`；L145-149 `SELECT 1 FROM workspaces` 判已导入 → `delete_setting`；L225 成功分支 `delete_setting` |
| 5 | 发布版本中 workspace 数据不存在（tag 内 workspace 相关文件 28 个均为 ADR-0020 前的旧 Home 结构，无 K/V workspace 写入路径；历史 `localStorage` 在 workspace/home 路径的唯一引入即 a2b9fe8 的 Data View 展示态记录，属 view state 非 workspace 权威数据） | `git ls-tree -r --name-only browser-20260808-175539 \| grep -ic workspace` = 28；`git log -S localStorage -- src/{components,lib}/{home,workspace}*` → 首条 `a2b9fe8`（Data View 展示态） |
| 6 | `DEFAULT_DOCUMENT` 为纯默认文档（`hidden: []`、`instances: []`、`layouts: {lg:[],md:[],sm:[]}`），不含任何用户内容 | `src/lib/home-workspace/model.ts:64-70` |

## 判定推理

1. 该 key 的**写入代码**只在 `e912e6a`（2026-08-21）之后存在；
2. 该 commit 之前**没有任何 release tag** 覆盖它（所有 tag 都在 08-08 及之前）；
3. 因此线上发布版本**从未运行过**写该 key 的代码 → key 不可能承载真实用户数据；
4. 当前 `e912e6a`/`a2b9fe8` 属于未发布的分支工作（`feat/v2-workspace-design-system` 前身），开发者本机可能产生过该 key 的开发态数据，v27 迁移已保证一次性导入并删除；
5. 结论：**无需 migrate**（无用户数据需要迁移），**delete**（key 与迁移代码随 I 组旧路删除整体清除）。

## 残留风险（人工确认项）

- R1: 若存在未打 tag 但已分发给用户的构建（如内部测试包），其 `settings` 表中可能残留该 key —— v27 导入逻辑已覆盖（读取→导入→删除），无需额外动作。
- R2: `migrate_v27.import_legacy_home_workspace` 属迁移期代码，AGENTS.md 允许保留至删除切片；**I 组删除旧路时必须同步删除该函数与 L123-225 相关代码**，避免"已删除链路仍有迁移代码引用"。
- R3: Data View 展示态 localStorage（`dataViewState.ts`）是另一类数据（view filter/sort/selection，非 workspace 权威），契约有 `workspace_view_states` 表，属 C-021..025 Data View 切片的收编范围，不在 G-009 结论内。

## 复核命令

```sh
cd /Users/ldh/Downloads/project/AiNative/Natives
git log -S 'settings:home_workspace' --all --oneline --reverse | head -1   # → e912e6a
git tag | xargs -I{} sh -c 'git merge-base --is-ancestor e912e6a {} && echo IN-{} || echo NOT-IN-{}'  # 全部 NOT-IN
git show browser-20260808-175539:src/lib/home-workspace/persistence.ts 2>&1 | head -1  # → fatal: not in tag
```
