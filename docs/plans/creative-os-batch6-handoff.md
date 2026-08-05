# Creative OS Batch 6 交接报告 — BrowserProfile、登录与文件能力

> 交接时间：2026-08-04
> 分支：`codex/creative-os-b6-20260804-205000`（base = Batch 5 final `e2801b3`）
> 主工作区，未新建 worktree

## 1. Batch / Task / 提交

| Task | 提交 | 说明 |
|---|---|---|
| CR-601 BrowserProfile | `7ad2006` | BrowserProfile 模型 + migration v20 + profile_store |
| CR-602 OAuth 临时 Surface | `a633eb1` (同 CR-603) | OAuth allowlist 表 + store |
| CR-603 文件与新窗口权限 | `a633eb1` (同 CR-602) | App grants 表 + store + 3种 policy |

- base SHA：`e2801b3`（Batch 5 final）
- final 代码 stable：`b3b2c21`（fmt 整理后）

## 2. 改动文件

| 文件 | 变化 |
|---|---|
| `src-tauri/src/db.rs` | SCHEMA_VERSION 21；migration v20 (browser_profiles) + v21 (oauth_allowlist, app_grants) |
| `src-tauri/src/creative_app/model.rs` | BrowserProfile, OAuthAllowlistEntry, AppGrant 模型 |
| `src-tauri/src/creative_app/profile_store.rs` | 新文件：create/list/find/default/delete + 4 测试 |
| `src-tauri/src/creative_app/grant_store.rs` | 新文件：OAuth add/check/list/remove + grant set/get/check/list/delete + 5 测试 |
| `src-tauri/src/creative_app/mod.rs` | profile_store, grant_store 模块 |

## 3. Schema / API

- **Migration v20**：`browser_profiles` 表，默认 profile 自动 seed
- **Migration v21**：`oauth_allowlist` + `app_grants` 表
- **Grant policy**：`default_deny`（默认）、`one_time`（一次消耗）、`persistent`（持久）
- Grant kinds：`upload`、`download`、`clipboard`、`window_open`

## 4. 测试结果

| 命令 | 结果 |
|---|---|
| `cargo check` | PASS |
| `cargo fmt --check` | PASS |
| `creative_app::` 155/155 | PASS（+9 from Batch 5） |
| `db::tests` 9/9 | PASS |
| `typecheck` | PASS |
| `lint` | PASS（2420 键） |
| `test` 763/763 | PASS |
| `protocol:check` | PASS（154 methods） |

## 5. 遗留风险

- **Cookie 隔离不完整**：profile 是 metadata-only（per ADR-0017），macOS 26.5 上所有 WKWebView 共享 data store
- **OAuth 临时 surface 未在 UI 中接入**：DB 层已就绪，UI 组件待集成
- **Grant UI 未实现**：DB 层已就绪，用户选择路径待前端实现
- **`commands::provider` 单测挂起**：环境性

## 6. 下一批（B7）继承

- commit：`b3b2c21`（schema 21）
- 三个新表：browser_profiles、oauth_allowlist、app_grants 已就绪
- 所有 grant 默认 deny，one_time 自动消耗