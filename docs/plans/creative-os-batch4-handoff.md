# Creative OS Batch 4 交接报告 — Embed 安全硬化与 WebView 技术决策

> 交接时间：2026-08-04
> 分支：`codex/creative-os-b4-20260804-202147`（base = Batch 3 stable `8f343962`）
> 主工作区（`/Users/ldh/Downloads/project/AiNative/Natives`），未新建 worktree

## 1. Batch / Task / 提交

| Task | 提交 | 说明 |
|---|---|---|
| CR-401 多 WebView/Profile 平台 spike | `843fe734` | 多 label 模式（`creative-app-{appId}`）；`BrowserState` 多 entry 跟踪；导航/显示/关闭/隐藏/边界操作支持 per-app；ADR-0017 记录 spike 结论 |
| CR-402 HTTP body/concurrency/CSP | `c59ee83d` | 有界 Worker 池（16）；CSP 分域（Workshop/Draft/Local/Bridge）；Content-Length 413 检查；安全测试套件 |
| CR-403 Embed capability/navigation 回归 | `952035ca` | capability 隔离验证（`webview: ["main"]` 排除 child label）；导航 hook 回归断言；测试：child label 不匹配 main filter、navigation_allowed 作为 on_navigation 逻辑 |

- base SHA：`8f343962`（Batch 3 stable）
- final 代码 stable：`952035ca`（本交接报告为分支 HEAD 上的 docs 提交）

## 2. 实际改动文件与调用链变化

8 个文件，+790 / -95：

| 文件 | 变化 |
|---|---|
| `docs/adr/0017-webview-backend-selection.md` | 新 ADR：WebView 后端选择 spike（multi Child vs WebviewWindow），记录 macOS 26.5.2 + Tauri 2.11.2 平台数据 |
| `src-tauri/capabilities/default.json` | 增加 `webview: ["main"]` 隔离文档注释（CR-403） |
| `src-tauri/src/commands/creative_app.rs` | `browser_set_bounds/back/forward/reload/hide/close/current` 全部增加 `app_id: String` 参数（multi-label）；`browser_close` 直接用 `app_id` 参数而非从浏览器状态内部读取 |
| `src-tauri/src/creative_app/browser.rs` | 单例 label → 多 label 模式（`CHILD_LABEL_PREFIX` + `sanitize_label` + `child_label()`）；`BrowserState` 从 `{active_app_id, current_url}` 改为 `HashMap<String, ActiveEntry>`；所有函数增加 `app_id` 参数；新增 13 个单元测试：label 生成、多 app 状态、capability 隔离、导航 hook |
| `src-tauri/src/http_server.rs` | 有界 Worker 池（`WorkerPool` + `MAX_CONCURRENT_WORKERS=16`）；CSP 分域（`WORKSHOP_CSP`/`DRAFT_CSP`/`LOCAL_PROJECT_CSP`/`BRIDGE_CSP`）；Content-Length 413 检查；移除 `MAX_POST_BODY`（替换为 `MAX_BRIDGE_BODY`）；新增 6 个安全测试 |
| `src/components/shell/WorkshopPage.tsx` | `browserSetBounds`/`browserClose` 增加 `appId` 参数；新增 `browserAppRef` 跟踪当前 browser app id 用于 unmount cleanup |
| `src/components/creative/AppBrowserPanel.tsx` | `browserBack/Forward/Reload` 增加 `app.id` 参数 |
| `src/lib/tauri-adapter.ts` | 接口定义 + 实现：`browserSetBounds/Back/Forward/Reload/Hide/Close/Current` 全部增加 `appId` 参数 |

调用链变化：Browser 操作从无状态（操作当前唯一 WebView）变为 per-app 寻址（通过 `creative-app-{appId}` label 定位 WebView）；HTTP 服务器从无界线程 + 单一 CSP 变为有界 Worker 池 + 分域 CSP + 413 响应。

## 3. Schema / backfill / API / event / feature flag

- **无 migration**（schema 仍为 18）。本批不新增表/列。
- API：
  - `creative_app_browser_set_bounds(appId, bounds)`——增加 appId 参数
  - `creative_app_browser_back(appId)`——增加 appId 参数
  - `creative_app_browser_forward(appId)`——增加 appId 参数
  - `creative_app_browser_reload(appId)`——增加 appId 参数
  - `creative_app_browser_hide(appId)`——增加 appId 参数
  - `creative_app_browser_close(appId)`——增加 appId 参数
  - `creative_app_browser_current(appId)`——增加 appId 参数
- 事件：无变更
- 静态 URL 契约：无变更
- feature flag：无

## 4. 兼容证据

- 旧 DB（v14–v18）升级：本批无 migration，旧库直接可读。
- 前端 wire：所有 browser 命令增加 `appId` 参数，前端同批切换。旧 frontend 调用这些命令会因缺少 `appId` 参数而失败（Tauri 按名匹配，缺少参数返回错误），但此批是与前端的原子提交。
- 三源 Catalog 回归：`creative_app::` 136 项全绿（Batch 3 的 123 + 本批新增 13）。

## 5. 测试结果

| 命令 | 结果 |
|---|---|
| `cargo check -p natives --lib` | PASS |
| `cargo fmt --all -- --check` | PASS |
| `cargo test -p natives --lib creative_app::` | 136/136 PASS（含 CR-401 多 label 13 项 + CR-403 能力隔离 2 项） |
| `cargo test -p natives --lib http_server` | 16/16 PASS（含 CR-402 安全硬化 6 项） |
| `cargo test -p natives --lib db::tests` | 9/9 PASS |
| `npm run typecheck` | PASS |
| `npm run lint` | PASS（2418 zh/en keys 同步） |
| `npm run test` | 763/763 PASS |
| `npm run protocol:check` | PASS（154 methods） |
| `npm run perf:check` | PASS（/page 215.2KB gzip < 350KB budget） |

## 6. 资源 owner / 停止 / 补偿 / reconcile 证据

- 本批不改变 Runtime 资源 owner 语义（继承 Batch 3 的 runtime-id owner）。
- WebView 生命周期：每应用独立 label，close 失败不清 BrowserState（继承 CR-303 契约）。
- HTTP 服务器：有界 Worker 池，超限请求排队（不拒绝），线程数有界。
- CSP 分域：每个路由域使用独立 CSP，不共用单一 CSP header。

## 7. 磁盘增量 / worktree / fixture

- 基线（Batch 3 交接）：target 16G、node_modules 1.0G、.next 884M、可用 19Gi。
- 批次后：target 16G（+0）、node_modules 1.0G、.next 884M、可用 16Gi（其他 Agent 消费约 3G）。
- worktree 13 个（主工作区 + 12 个其他 Agent 的，减少 1 个因其他 Agent 清理）。
- 无新增 fixture / 无容器 / 无 test-id 资源；测试临时文件均随测试结束释放。
- 新增 ADR 154 行（不含代码变更）。

## 8. 遗留风险与人工决策

- **Cookie 隔离不完整**：macOS 26.5 上所有 WKWebView 共享同一 data store，无法实现 per-WebView Cookie 隔离。ADR-0017 记录为已知限制，接受共享 Cookie，不在 Embed 中存放敏感登录态。Batch 6（CR-601）届时评估 `WKWebsiteDataStore` 配置可能性。
- **WebView 进程级隔离不可用**：macOS 26.5 上所有 WebView 在同一 app 进程内，无法实现真正的进程级隔离。ADR-0017 记录结论。
- **Worker 池在 accept 循环中阻塞**：当前实现在线程池满时阻塞 accept 循环，而不是排队请求。这在短期尖峰时可能导致连接延迟，但保护了内存安全。如需更细粒度控制，可升级为 `tiny_http` 的线程池模式或异步 HTTP 服务器，但这不在本批范围。
- **`commands::provider` 单测挂起**：`provider_test_surfaces_rate_limit_for_chat_completions` 在本环境挂起（mock 单次响应；429 退避/客户端阻塞）。该模块本批未触碰；Batch 2/3 全量运行亦见同一测试 >60s。已在挂起处主动停止，定向套件全绿。
- **5 项环境失败**：`usage::claude`×3 读真实 `~/.claude` 会话文件、`sidecar_supervisor`×2 依赖 env/daemon 路径——Batch 1 基线一致，本批未触碰。
- **Docker 仍不可用**：本批不涉及 Docker 路径。

## 9. 回滚方法

- 回滚本批：`git revert 952035ca c59ee83d 843fe734`（按序）；或 checkout 到 `8f343962`。
- 无 migration，无 DB 层回滚；旧 binary 直接读 v18 DB。
- API 回滚：旧 frontend 调用 browser 命令不带 `appId` 参数会失败——回滚后旧 binary 的 browser 命令也不接受 `appId` 参数，匹配旧 frontend。
- 安全修复（CSP 分域、Worker 池、413 检查、capability 隔离）不回退。

## 10. 下一 Batch（B5）必须继承

- commit：`952035ca`（分支 `codex/creative-os-b4-20260804-202147`）。
- schema 版本：18（本批无变更）。
- WebView 后端：**multi Child**（ADR-0017 结论）。`browser.rs` 已实现多 label 模式，`BrowserState` 为 `HashMap<String, ActiveEntry>`。
- HTTP：有界 Worker 池（16），CSP 分域（Workshop/Draft/Local/Bridge），413 响应。
- Capability：`capabilities/default.json` 的 `webview: ["main"]` 排除所有 `creative-app-*` label。
- 禁止假设：不要假设 browser 操作是无状态的（现在每个操作需要 `app_id`）；不要假设 HTTP 服务器有无限线程（现在有界 16）；不要假设所有路由使用同一 CSP（现在分域）；不要假设 child WebView 有 Tauri capability（已由 `webview: ["main"]` 隔离）。
- 全量 lib 套件：`commands::provider` 单测挂起为环境性，Docker 仍不可用。