# ADR-0017: WebView Backend Selection for Embed Surface

## 状态

**DRAFT → APPROVED (T08 spike, 2026-08-06)** — 平台 spike 实验记录。本 ADR 记录 macOS 26.5.2 + Tauri 2.11.2 上的实际测试数据，以决定 Embed Surface 的多 WebView 后端方案。T08 在 Tauri 2.11.5 / wry 0.55.1 上追加了 per-profile data store 实测，修订了「Cookie 无法按 profile 隔离」的旧结论（见「T08 更新」）。

## 上下文

Creative OS Embed 需要为每个外部应用（Web 模块、预览、OAuth 回调）显示独立的 WebView 实例，且**绝不**授予这些实例主应用 Tauri capability。

当前实现（Batch 3, CR-303）使用 Tauri v2 的 `add_child`（unstable API）将子 WebView 嵌入主窗口。备选方案是使用 `WebviewWindow` 创建独立窗口。两种方案在 capability 隔离、多实例、Cookie 隔离、资源和重启行为上有根本差异。

## 平台版本

| 维度 | 值 |
|---|---|
| OS | macOS 26.5.2 (Darwin 25.5.0) |
| Tauri CLI | 2.11.2 |
| Tauri crate | 2.x (feature `unstable` enabled) |
| rustc | 1.96.0 (2026-05-25) |
| WebKit | macOS 26.5.2 内置 (WebKit 2.0+，基于 Safari 26.x) |
| 架构 | arm64 (Apple Silicon M-series) |

## 方案 A: 多 Child Webview（当前实现）

### 实现方式

- 使用 `window.add_child()` 在 main 窗口内创建子 WebView
- 单一 label `"creative-app-browser"`
- `on_navigation` 回调仅允许 `127.0.0.1` / `localhost`
- 子 WebView 不继承主窗口的 Tauri capability（默认无 core/shell/fs 权限）

### 实测数据

| 维度 | 结果 |
|---|---|
| 多实例 | 当前单例（唯一 label）；可扩展为多 label 但 `add_child` 布局嵌套 |
| Cookie 隔离 | 子 WebView 与主窗口共享同一 `WKWebView` data store 进程，**Cookie 不隔离** |
| localStorage | 共享 data store → **不隔离** |
| Service Worker | 共享 data store → **不隔离** |
| 重启持久 | data store 持久化在 `~/Library/WebKit/` → 重启后 Cookie/SW 存活 |
| 清理 | 需手动清 `WKWebsiteDataStore`；无 API 级 per-profile 清除 |
| OAuth 弹窗 | popup 在新 child 中打开，`on_navigation` 不拦截 → 可导航到任意 URL，除非 `on_window_open` 有 hook |
| 10 窗资源 | 未实测（当前单例）；每个 child 约 50-150MB RSS，10 窗预估 500MB-1.5GB |
| 关闭 | `wv.close()` 移除 child WebView 视图层 |
| 重新显示 | 需重新 `add_child`（close 后 label 不可复用） |
| Capability 隔离 | ✅ 默认无 main capability；WebView 不继承 `default.json` 中 `windows: ["main"]` 的权限 |

### 已知限制

1. `add_child` 标记为 `unstable`——Tauri 2.x 中可能未有稳定承诺
2. 子 WebView 与主窗口共享 WebKit data store → Cookie/localStorage 不隔离
3. 子 WebView 内无独立 `on_window_open` 拦截（当前版本；回头看 Tauri 2.11 hook）
4. 单 label 设计不能同时显示多个应用
5. 布局嵌套在 `LogicalPosition`/`LogicalSize` 中，窗口 resize 时需手动更新

## 方案 B: WebviewWindow（独立窗口）

### 实现方式

- 使用 `WebviewWindowBuilder` 创建独立窗口
- 每个应用一个窗口，label 为 `"creative-app-{appId}"`
- 独立窗口有自己的 capability 配置（通过 `capabilities/config.json` 中的 `webview` 数组控制）
- 窗口关闭 = 应用关闭；窗口最小化 = 后台

### 预计特征

| 维度 | 预期 |
|---|---|
| 多实例 | 天然多窗口，label 唯一 |
| Cookie 隔离 | macOS 上每个 `WKWebView` 共享同一 data store，除非显式配置 `WKWebsiteDataStore.nonPersistent()` |
| localStorage | 同 data store 问题 |
| 资源 | 每个窗口约 80-200MB，10 窗预估 800MB-2GB |
| Capability 隔离 | 通过 `capabilities/*.json` 中 `windows: ["creative-app-*"]` 精确控制 |
| 关闭/重启 | 窗口关闭 = WebView 销毁；重启需新窗口 |
| 导航拦截 | `on_navigation` 同样可用 |

### 潜在问题

1. macOS 上 `WKWebView` 默认共享 data store → Cookie/localStorage 隔离仍需要额外配置
2. 独立窗口的窗口管理（focus/minimize/restore/tile）需要额外实现
3. 窗口数量多时 Dock 和 Cmd+Tab 切换器可能混乱
4. 需为每个窗口配置 capability 标识符或使用通配符

## 方案 C: 独立 WebView 进程（实验性）

在 macOS 26.5 上，Tauri 的 `WebviewWindow` 实际上是同一进程内的不同 WKWebView 实例。要达到真正的进程级隔离，需要：

1. 每个 WebView 在独立子进程中（macOS `_WKProcessPool` 配置）
2. 或使用 Tauri multiwebview 的实验性进程池 API

**当前 macOS 26.5 上无法实现真正的进程级隔离**——所有 WebView 仍在同一 app 进程内。

## 测试记录

### 测试 1: Cookie 隔离

**方法**: 在子 WebView 中登录服务 A，打开同一 provider 的服务 B → Cookie 是否共享？

**结果**: ✅ Cookie 共享——同一 data store 下，服务 B 自动继承服务 A 的登录态。

**结论**: 默认情况下**无 Cookie 隔离**。如需要隔离（不同用户同时登录不同服务），必须使用 `WKWebsiteDataStore.nonPersistent()` 或 `_WKProcessPool`。

### 测试 2: Multi-instance 资源

**方法**: 创建 10 个子 WebView 加载不同 localhost 页面，观察 RSS 变化。

**结果**: 未实测（当前环境限制）。文献值：每个空 WKWebView 约 40-60MB，加载页面后 80-150MB。

### 测试 3: Capability 逃逸

**方法**: 在子 WebView 中通过 `window.__TAURI__` 或 `window.__TAURI_INTERNALS__` 调用 Tauri IPC。

**结果**: ✅ 子 WebView 无 `__TAURI__` 注入。`__TAURI_INTERNALS__` 存在但不含 `invoke` 所需 capability → 所有 IPC 调用返回 `"not allowed"`。

**结论**: Capability 隔离有效——子 WebView 无法调用主应用的 Tauri 命令。

## 决策

**推荐：方案 A（多 Child Webview）继续，但需解决以下问题：**

1. ✅ **多实例支持**：扩展为多 label 模式（`"creative-app-{appId}"`），每个实例有其 WebView 对象
2. ❌ **Cookie 隔离**：当前平台无法实现每个 WebView 的独立 data store。接受共享 Cookie 作为已知限制，记录在 ADR，不在 Embed 中存放敏感登录态
3. ✅ **Capability 隔离**：已验证有效
4. ⏳ **导航拦截**：`on_window_open` 在 Tauri 2.11 中可用，需要补充实现
5. ⏳ **资源管理**：10 窗上限需在 CR-402 中实现为 Worker 池上限

**拒绝方案 B** 的理由：
- 独立窗口不解决 Cookie 隔离问题（同一 data store）
- 窗口管理（focus/minimize/tile）需要大量额外代码
- 用户期望 Embed 应用在主窗口内，而非独立窗口（与 Workshop 一致）

**拒绝方案 C** 的理由：
- 当前 macOS 版本无法实现真正的进程级 WebView 隔离
- 需要私有 API（`_WKProcessPool`），不稳定且可能被 App Store 拒绝

## 后续工作

1. **CR-401b**（本批）：将 child WebView 从单例扩展为多 label 模式
2. **CR-402**（本批）：HTTP 安全硬化（body 限流、worker 池、CSP 分域）
3. **CR-403**（本批）：Embed capability/navigation 回归门
4. **Batch 5**（CR-502）：根据本 ADR 选定的 backend 交付真实多窗、Dock、focus/minimize/restore/background
5. **Batch 6**（CR-601）：BrowserProfile 与 Cookie 隔离方案（届时评估 `WKWebsiteDataStore` 配置可能性）

## 附录: WebKit 版本详情

macOS 26.5.2 的 WebKit 版本对应 Safari 26.x 的 WebKit 引擎。具体版本号可通过 `sw_vers -productVersion` 和 `system_profiler SPApplicationsDataType | grep -i webkit` 获取。

## T08 更新: per-profile data store 实测（2026-08-06）

T08（Creative BrowserProfile / OAuth / grants）在 Tauri 2.11.5 + wry 0.55.1 上复核了 profile/cookie 隔离能力，结论修订如下。

### 平台能力

- wry 0.55.1 在 macOS ≥ 14 / iOS ≥ 17 走 `WKWebsiteDataStore(dataStoreForIdentifier:)`；Tauri 2.11.5 的 `WebviewBuilder::data_store_identifier([u8;16])` 直通该 API（`tauri-runtime-wry` → `with_data_store_identifier`）。
- 因此 cookie / localStorage / IndexedDB / service worker **可以按 profile 隔离、持久**；`AppHandle::remove_data_store(uuid)` / `fetch_data_store_identifiers()` 可清理与枚举 store。
- 旧结论「所有 WKWebView 共享同一 data store、无法按 profile 隔离」基于 Tauri 2.11.2 时期；现平台已支持，**该限制取消**。

### 对产品声明的影响

- `browser_profiles` 从「metadata-only / forward-looking」升级为**真实能力**：每个 profile 拥有独立的 16 字节 WebKit data store 标识（`platform_store_key` = 32-hex），`browser_show` 把该标识交给 `data_store_identifier`；同一 app 的多个 WebView 共享其 app 绑定 profile，不同 app/profile 互不串站。
- 删除 profile 时 Host 负责清理对应 data store；OAuth 临时 surface 用 `incognito`（nonPersistent store），完成/取消即清理。
- Embed 静态 surface（`creative-app-*`）仍**仅限 loopback** 导航、永不获得 Workshop Bridge / Tauri capability；OAuth 授权走独立的 `creative-oauth-*` 临时 surface（allowlist + loopback callback）。两个 trust domain 互不混用（见 T08 `oauth.rs`）。
- `window.open`（`window_open` grant）的允许路径被**重归入受控 child webview**（`creative-popup-*`，loopback-only + 共享 profile store），**从不使用**系统默认 OS popup（其导航不受 `on_navigation` 约束）。

### 遗留限制

- `data_store_identifier` 是 per-WebView store 绑定，不是 per-process 隔离；多 WebView 仍共享进程。真正进程级隔离（`_WKProcessPool`）仍不可用，维持拒绝方案 C。
- OAuth 回调判定为「临时 surface 内任意 loopback 导航」；多段 loopback 流程（本地登录页先于 callback）会把第一段 loopback 当作回调。T08 按「authorize → 一次性 callback」契约交付，超出契约的流程需后续 ADR 扩展。
- **OAuth 凭证边界（R-S12/R-S13）**：T08 的 Host 边界是受控授权 surface（allowlist 导航 + 临时 incognito window + 完成/取消清理）。临时 surface 不持久化任何 token/refresh；回调仅携带授权 code 返回给发起方做 app 专属 exchange（Redirect URI 需与 app 在 Provider 注册的 loopback 回调一致，Host 无法替 app 构造 authorize URL）。exchange 得到的 refresh token 必须经现有 `capability_secrets`（AES-256-GCM envelope，R-S12）持久化，禁止明文落盘。Host 侧完整 exchange 路径由 `mcp_oauth_start`（ADR-0016 decision 7）提供，面向已知 token endpoint 的场景。

## 参考

- [Tauri v2 WebviewWindow docs](https://v2.tauri.app/reference/webview-window/)
- [ADR-0008: Tauri 迁移](0008-electron-to-tauri-migration.md)
- [ADR-0012: 产品身份与工坊范围](0012-product-identity-workshop-scope.md)
- [Creative OS Batch 3 交接报告](../plans/creative-os-batch3-handoff.md)