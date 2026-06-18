# 技术架构 02 · 五大防线与安全红线

> **版本**: 1.0.0 · **日期**: 2026-06-15
> **关联 ADR**: [ADR-0001](../../adr/0001-session-token-handshake.md)（Session Token 握手）、[ADR-0002](../../adr/0002-postmessage-origin-verification.md)（来源验证）、[ADR-0003](../../adr/0003-plugin-ipc-main-process-relay.md)（插件 IPC 中转）、[ADR-0006](../../adr/0006-iframe-crash-detection.md)（崩溃检测）
> **关联源文件**: `src/main/watchdog.ts`、`src/lib/iframe-sandbox.ts`、`src/lib/iframe-manager.ts`、`src/lib/token-manager.ts`、`src/main/shell.ts`、`src/main/env-injector.ts`、`electron/main.ts`

---

## 一、本篇要约束什么

Natives 把不可信的第三方代码（插件）以 iframe 形式跑在用户桌面上，还管理着用户的凭证与终端。**这是本项目最敏感的约束集合**——任一红线失守都可能导致凭证泄露、任意代码执行、或插件越权。本篇把五大防线与若干安全红线写成 MUST。

---

## 二、五大防线（不可妥协）

### 防线 1 · 子进程生命周期 & PID 轮询看门狗

#### R-S1 · 子进程必须挂看门狗
- **等级**：MUST
- **分类**：安全、进程
- **规则**：所有由 Main 派生的长驻子进程（HTTP 服务、终端 PTY 等）**必须**经 `node --require dist/watchdog.js` 注入看门狗，每 2s 用 `process.kill(parentPid, 0)` 探测父进程，父进程死亡时**必须**自动退出。HTTP 服务**必须**启动时自动选择空闲端口，**禁止**硬编码端口。
- **为什么**：防止 Main 崩溃后子进程成为孤儿，持续占用端口或泄露凭证环境。
- **检查方法**：新增长驻子进程时核对是否注入 watchdog、是否动态选端口。

### 防线 2 · iframe 沙箱（插件隔离的核心）

#### R-S2 · iframe sandbox 属性红线
- **等级**：MUST
- **分类**：安全
- **规则**：插件 iframe 的 `sandbox` 属性**必须**为 `allow-scripts allow-forms`，**必须不**包含 `allow-same-origin`、`allow-top-navigation`、`allow-popups`（除非有 ADR 豁免）。
- **正例**：`<iframe sandbox="allow-scripts allow-forms" src="...">`
- **反例**：为了「方便调试」加 `allow-same-origin` → 插件可访问父窗口、cookie、storage，沙箱形同虚设。
- **为什么**：见 ADR-0002。`allow-same-origin` 会让插件脱离沙箱，是本项目最危险的单点配置错误。
- **检查方法**：`grep -r "sandbox=" src/components` 逐一核对属性值。

#### R-S3 · 来源验证用 MessageEvent.source，不用 origin
- **等级**：MUST
- **分类**：安全
- **规则**：基座验证 postMessage 来源时**必须**用 `MessageEvent.source` 窗口引用匹配（创建 iframe 时保存 `contentWindow`，收消息时比对），**禁止**依赖 `event.origin`（sandbox iframe 的 origin 是 `"null"`，无法区分）。
- **为什么**：见 ADR-0002。用 origin 验证在 sandbox iframe 下完全失效。
- **检查方法**：所有 `window.addEventListener('message', ...)` handler 必须含 `source` 比对逻辑。

#### R-S4 · Session Token 两阶段握手
- **等级**：MUST
- **分类**：安全
- **规则**：插件与基座的每次 Bridge 通信（postMessage 与 HTTP）**必须**携带有效 Session Token。Token **必须**经两阶段握手下发：iframe 加载后主动请求 → 基座验证 source 引用后下发。插件重载时**必须**重新握手。
- **为什么**：见 ADR-0001。Token 防止 XSS 注入的脚本冒充插件劫持 IPC/HTTP。
- **检查方法**：HTTP 路由的认证中间件、postMessage handler 的 token 校验是否齐备。

#### R-S5 · 插件静态资源路径前缀隔离
- **等级**：MUST
- **分类**：安全、命名
- **规则**：每个插件的静态资源**必须**走独立路径前缀 `/modules/{moduleId}/`，HTTP 服务**必须**校验请求的 moduleId 与 Token 对应的 moduleId 一致，**禁止**跨模块读取文件。
- **为什么**：路径隔离是文件层面的最后一道防线，配合 Token 防止插件 A 读插件 B 的代码。
- **检查方法**：HTTP 静态文件 handler 的 moduleId 一致性校验。

#### R-S6 · HTTP 服务必须注入 CSP
- **等级**：MUST
- **分类**：安全
- **规则**：本地 HTTP 服务响应插件资源时**必须**注入 `Content-Security-Policy` 头，限制 `connect-src` 到本地、禁止 `allow-top-navigation`。CSP 放宽需走 ADR。
- **为什么**：纵深防御，即使 sandbox 被绕过也限制插件的网络外联。
- **检查方法**：HTTP 响应头是否含 CSP。

### 防线 3 · 完整 PTY 终端

#### R-S7 · 终端用 node-pty，降级路径要显式
- **等级**：SHOULD
- **分类**：安全、进程
- **规则**：终端**应该**优先用 `node-pty.spawn` 以支持 TUI/resize；若编译失败**应该**降级到 `child_process.spawn`，但**必须**在日志中显式记录降级，**禁止**静默降级。
- **为什么**：node-pty 提供完整 PTY 语义；静默降级会让 TUI 程序静默失效，难排查。
- **检查方法**：降级分支是否打日志。

#### R-S8 · 终端 Session Token 防劫持
- **等级**：MUST
- **分类**：安全
- **规则**：终端 IPC（write/resize/kill）**必须**校验调用方为可信的 Renderer（经 preload），**禁止**接受来自 iframe 的直接终端指令。终端会话**必须不**与插件共享同一 Token 域。
- **为什么**：防 XSS 注入脚本通过劫持 IPC 操控终端（终端持有注入的凭证）。
- **检查方法**：终端 IPC handler 的来源校验。

### 防线 4 · 数据库单向总线 & 状态广播

#### R-S9 · 配置变更必须广播
- **等级**：MUST
- **分类**：状态、数据
- **规则**：Main 中任何影响 Renderer 展示的 DB 变更（设置、模块状态、通知等）**必须**通过 `db-state-changed` IPC 广播，Renderer 监听后同步。**禁止**依赖 Renderer 主动轮询或手动刷新。
- **为什么**：单向总线保证多窗口/多面板状态一致，无需用户刷新。
- **检查方法**：DB 写操作后是否触发广播；Renderer 是否监听对应 channel。

### 防线 5 · FOUC Guard & Zod 验证

#### R-S10 · 窗口先隐藏，主题就绪后再显示
- **等级**：MUST
- **分类**：交互、性能
- **规则**：Electron 窗口**必须**以 `show: false` 启动，Renderer 加载配置并挂载 CSS 变量后发送 `theme-applied-ready`，Main 收到后才显示窗口。
- **为什么**：防 FOUC（Flash of Unstyled Content），避免用户看到无主题的裸界面闪烁。
- **检查方法**：`BrowserWindow` 构造含 `show: false`；存在 `theme-applied-ready` 握手。

#### R-S11 · 主题与配置参数必须经 Zod 校验
- **等级**：MUST
- **分类**：数据、主题
- **规则**：从 SQLite 读取的主题色、布局尺寸、配置参数**必须**经 Zod schema 校验后才注入 DOM / 使用（如颜色 hex 正则、像素范围）。**禁止**把未校验的 DB 值直接写进 CSS 变量。
- **为什么**：损坏的配置不应让界面崩溃或注入非法 CSS。
- **检查方法**：`theme-engine.ts` 的 `ThemeSchema` 校验是否覆盖所有注入字段。

---

## 三、凭证安全红线

#### R-S12 · 凭证必须用 safeStorage 加密
- **等级**：MUST
- **分类**：安全、数据
- **规则**：所有敏感凭证（API Key、Token）**必须**用 `electron.safeStorage.encryptString()` 加密后再持久化（`~/.natives/env/`），读取时用 `decryptString()`。**禁止**明文落盘、**禁止**打印到日志。
- **为什么**：凭证泄露=用户 AI 账户被盗用的直接路径。
- **检查方法**：`grep -r "safeStorage" src/main` 覆盖所有凭证写入路径；日志中无明文 key。

#### R-S13 · 凭证不进 Renderer 内存明文
- **等级**：SHOULD
- **分类**：安全
- **规则**：Renderer **应该**只在「需要展示」时获取凭证掩码（如 `sk-...xxxx`），**禁止**在 Renderer 长期持有完整凭证明文。终端注入等场景由 Main 直接完成，不把明文交给 Renderer。
- **为什么**：Renderer 是 XSS 的攻击面，内存里的明文凭证可被脚本窃取。
- **检查方法**：preload 暴露的凭证相关 API 是否仅返回掩码。

---

## 四、本篇合规自检清单

- [ ] 我没有给 iframe 加 `allow-same-origin` / `allow-top-navigation`（R-S2）。
- [ ] postMessage 来源验证用的是 `source` 不是 `origin`（R-S3）。
- [ ] 新增的 IPC/HTTP 通信都校验了 Session Token（R-S4, R-S8）。
- [ ] DB 写操作后触发了 `db-state-changed` 广播（R-S9）。
- [ ] 凭证经过 safeStorage 加密，没有明文落盘或进日志（R-S12）。
- [ ] 主题/配置经 Zod 校验后才注入（R-S11）。
- [ ] 若我的改动触及任一防线，已写 ADR 说明影响（变更流程见 `README.md`）。
