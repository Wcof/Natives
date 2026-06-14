# Natives — Product Requirements Document

> **版本**: 1.0.0
> **日期**: 2026-06-14
> **状态**: Draft
> **基于**: 32 个架构设计决策 (DESIGN_DISCUSSION.md Q1-Q32) + 架构规范 (ARCHITECTURE.md)
> **参考实现**: CodePilot (AI-native patterns) + FanBox (zero-dependency backend patterns)

---

## Problem Statement

当前桌面应用生态存在三个核心问题：

1. **工具碎片化**：用户需要在多个独立应用之间切换（AI 客户端、终端、文件管理器、IDE），每个应用有自己的配置、凭证、界面风格，缺乏统一入口。

2. **AI 工具集成困难**：AI CLI 工具（Claude Code、Codex、Aider 等）需要 API Key 配置、终端环境、文件预览等配套能力，但这些能力分散在不同工具中，用户需要手动串联。

3. **扩展性受限**：现有应用的功能是开发者硬编码的，用户无法自行扩展或定制界面。即使有插件系统，也通常绑定在特定应用内部，无法跨应用复用。

用户需要一个**统一的桌面基座**，像 Steam 之于游戏一样，提供应用浏览、安装、运行、管理的完整体验，同时内置终端和环境管理能力，让 AI 工具开箱即用。

---

## Solution

**Natives** 是一个 AI 时代的桌面应用容器（"AI Steam Base"），基于 Electron + Next.js + SQLite 构建。它提供：

1. **统一基座**：三栏布局的桌面容器，左侧订阅侧边栏、中间插件内容区、右侧工坊/设置面板、底部可折叠终端。

2. **插件生态**：页面级插件通过 iframe + 本地 HTTP 服务加载，每个插件是独立的 HTML/JS/CSS 页面，通过 Bridge API（`window.natives.*`）与基座通信。插件从本地目录 `~/.natives/modules/` 发现和加载。

3. **完整终端**：基于 node-pty + xterm.js 的完整 PTY 终端，支持 TUI 程序、窗口调整、多会话。终端启动时自动注入用户配置的环境变量（API Key 等）。

4. **安全沙箱**：iframe sandbox 隔离（无 allow-same-origin）+ Session Token 鉴权 + 路径前缀隔离 + CSP 策略，确保插件之间互不干扰。

5. **创意工坊**：内置插件浏览、安装、管理界面，支持目录和 ZIP 两种安装方式。开发者用任何工具 + AI 辅助开发插件，基座提供模板和 SDK 类型声明。

---

## User Stories

### 基座体验

1. As a 用户, I want 打开 Natives 后看到仪表盘（已安装模块概览、系统状态、快捷操作），so that 我能快速了解当前环境并进入工作。

2. As a 用户, I want 通过左侧边栏看到所有已安装模块的图标和名称，so that 我能快速切换到任意模块。

3. As a 用户, I want 拖拽侧边栏中的模块图标来调整排列顺序，so that 我能把常用的模块放在前面。

4. As a 用户, I want 折叠/展开左侧边栏、右侧面板、底部终端，so that 我能根据当前任务调整布局空间。

5. As a 用户, I want 使用 Cmd+K 全局搜索功能，so that 我能快速找到模块或功能。

6. As a 用户, I want 在通知中心查看来自各模块的通知消息，so that 我不会错过重要信息。

7. As a 用户, I want 在设置面板中切换主题颜色和布局参数，so that 我能自定义界面风格。

8. As a 用户, I want 在设置面板中切换语言（中文/英文），so that 我能用熟悉的语言使用应用。

### 插件管理

9. As a 用户, I want 在应用商店中浏览可用模块，so that 我能发现新的工具和应用。

10. As a 用户, I want 一键安装模块（目录拖入或 ZIP 拖入），so that 我能快速扩展功能。

11. As a 用户, I want 在模块管理页面启用/禁用/卸载已安装的模块，so that 我能控制哪些模块在运行。

12. As a 用户, I want 安装模块时看到它需要的权限列表并确认授权，so that 我能控制模块的访问范围。

13. As a 用户, I want 模块列表在启动时自动扫描更新，so that 我手动添加的模块能被自动发现。

14. As a 用户, I want 手动刷新模块列表，so that 我能在运行中安装新模块后立即看到。

### 插件运行

15. As a 用户, I want 点击侧边栏中的模块图标后在主内容区加载该模块的页面，so that 我能使用模块的功能。

16. As a 用户, I want 切换模块时保留之前模块的状态（不重新加载），so that 我能无缝切换而不丢失进度。

17. As a 用户, I want 同时打开多个模块并在它们之间快速切换，so that 我能并行使用多个工具。

18. As a 用户, I want 当打开的模块过多时系统自动回收最久未使用的模块，so that 内存不会被耗尽。

19. As a 用户, I want 模块崩溃时看到友好的错误页面和"重新加载"按钮，so that 我能快速恢复工作。

20. As a 用户, I want 模块崩溃信息记录在通知中心，so that 我能了解问题原因并报告给开发者。

### 终端

21. As a 用户, I want 在底部终端中运行任意 CLI 命令，so that 我不需要切换到外部终端。

22. As a 用户, I want 终端支持完整的 TUI 交互（vim、htop、Claude Code 等），so that 我能在基座内完成所有终端工作。

23. As a 用户, I want 终端窗口支持拖拽调整大小，so that 我能根据需要分配终端空间。

24. As a 用户, I want 创建多个终端会话，so that 我能同时运行不同的命令。

25. As a 用户, I want 终端启动时自动注入我配置的 API Key 和环境变量，so that AI CLI 工具无需手动配置即可使用。

26. As a 用户, I want 选择不同的环境配置（如"工作"、"个人"），so that 我能在不同场景下使用不同的凭证。

### 环境管理

27. As a 用户, I want 在设置中创建和管理多组环境配置，so that 我能为不同用途维护不同的 API Key。

28. As a 用户, I want 环境变量加密存储在本地，so that 我的 API Key 不会被明文泄露。

29. As a 用户, I want 设置一组默认的环境配置，so that 新终端自动使用这组配置。

### 插件开发

30. As a developer, I want 获取插件开发模板（plugin-template 目录），so that 我能快速开始开发。

31. As a developer, I want 使用 `window.natives.*` Bridge API 读写数据、发送通知、获取主题，so that 我的插件能与基座交互。

32. As a developer, I want SDK 类型声明文件（natives-sdk.d.ts），so that 我在开发时能获得代码补全和类型检查。

33. As a developer, I want 通过 manifest.json 声明插件的权限需求，so that 用户在安装时能了解插件需要的能力。

34. As a developer, I want 通过 lifecycle 事件（ready、unload、error、heartbeat）管理插件生命周期，so that 基座能正确管理插件的状态。

35. As a developer, I want 在创意工坊页面中创建新模块模板，so that 我能快速启动新项目。

### 数据与配置

36. As a 用户, I want 我的设置和模块数据持久化在 `~/.natives/` 目录下，so that 重启应用后一切如故。

37. As a 用户, I want 配置文件在应用崩溃时不会损坏，so that 我不会丢失设置。

38. As a 用户, I want 数据库 schema 升级时自动迁移，so that 应用更新不会丢失我的数据。

---

## Implementation Decisions

### 模块划分

本次实现需要构建以下核心模块：

#### M1: Electron Shell (`electron/main.ts` + `electron/preload.ts`)

**职责**：应用入口、窗口管理、IPC 隔离桥、FOUC 防护。

**关键接口**：
- `BrowserWindow` 创建（`show: false`，FOUC Guard）
- `contextBridge.exposeInMainWorld` 暴露 `nativesAPI` 对象
- IPC 频道：DB CRUD、终端控制、模块管理、环境注入

**依赖**：M2 (Database), M3 (HTTP Server), M5 (Terminal), M6 (Module Manager)

---

#### M2: Database Layer (`src/main/database.ts`)

**职责**：SQLite 数据库管理，9 张表的 CRUD，增量迁移，状态广播。

**关键接口**：
- `initDb()` — 建表（IF NOT EXISTS）+ 增量迁移（PRAGMA table_info）
- `getDb()` — 返回 better-sqlite3 实例
- `broadcast(channel, data)` — 通过 IPC 广播状态变更

**Schema（9 张表）**：
- `modules` — 模块注册表
- `module_permissions` — 权限声明
- `settings` — 用户设置
- `module_data` — 模块 KV 存储（按 module_id 命名空间隔离）
- `workshop_cache` — 创意工坊缓存
- `env_profiles` — 环境配置组
- `env_variables` — 环境变量（加密存储）
- `notifications` — 通知历史
- `module_order` — 侧边栏排序

**迁移策略**：PRAGMA table_info 增量迁移 + 文件锁防并发。

---

#### M3: HTTP Server (`src/main/http-server.ts`)

**职责**：本地 HTTP 服务，提供插件静态文件和 Bridge API 路由。

**关键接口**：
- `startServer(port?)` — 启动 HTTP 服务（自动选择空闲端口）
- `getPort()` — 返回实际监听端口
- 路由表：
  - `GET /natives-sdk.js` — Bridge SDK 脚本
  - `GET /modules/{moduleId}/{path}` — 静态文件
  - `POST /api/bridge/{namespace}/{method}` — Bridge API

**安全策略**：
- CSP 头注入
- Session Token 验证（X-Session-Token + X-Module-Id）
- 路径前缀隔离

**参考**：FanBox 的 `server.js` 零依赖 HTTP 服务模式。

---

#### M4: Bridge System (`src/main/bridge-host.ts` + `src/lib/bridge-sdk.js`)

**职责**：插件与基座的通信层，混合 postMessage + HTTP API。

**关键接口**：

*Bridge Host（服务端）*：
- `handlePostMessage(message, moduleId, token)` — 处理 postMessage 请求
- `handleHttpRequest(req, res)` — 处理 HTTP API 请求
- `checkPermission(moduleId, permission)` — 权限检查

*Bridge SDK（客户端，注入到 iframe）*：
- `window.natives.db.get/set/delete/list` — 数据读写
- `window.natives.settings.getTheme/getLocale/onThemeChange` — 设置读取
- `window.natives.env.get` — 环境变量（需权限）
- `window.natives.notification.send/badge` — 通知（需权限）
- `window.natives.ipc.send/on/broadcast` — 模块间通信（需权限）
- `window.natives.lifecycle.ready/onUnload/onHeartbeat` — 生命周期
- `window.natives.meta.moduleId/version/nativesVersion` — 元信息

**通信协议**：
- 轻量操作：postMessage（主题变更、导航、生命周期、badge 更新）
- 重操作：HTTP API（数据读写、环境变量、通知、IPC）

**Session Token 机制**：
- 基座启动时生成 masterSecret
- 每个插件实例化时生成唯一 sessionToken = HMAC(masterSecret, moduleId + timestamp)
- 通过 postMessage 下发给 iframe（仅一次）
- SDK 保存在内存变量中（非 localStorage）
- 所有 postMessage 和 HTTP 请求需携带 token

---

#### M5: Terminal (`src/main/shell.ts`)

**职责**：多会话 PTY 终端，环境注入。

**关键接口**：
- `createSession(env?)` — 创建 PTY 会话，返回 sessionId
- `write(sessionId, data)` — 写入数据
- `resize(sessionId, cols, rows)` — 调整窗口大小
- `kill(sessionId)` — 终止会话
- `onData(sessionId, cb)` — 数据流回调
- `onExit(sessionId, cb)` — 退出回调

**实现**：
- node-pty（主方案）+ child_process.spawn（降级方案）
- @xterm/xterm 前端渲染
- Session Token 握手防 XSS

---

#### M6: Module Manager (`src/main/module-manager.ts`)

**职责**：模块生命周期管理（安装/卸载/启用/禁用）。

**关键接口**：
- `scanModules()` — 扫描 `~/.natives/modules/` 目录
- `installModule(pathOrZip)` — 安装模块（目录或 ZIP）
- `uninstallModule(moduleId)` — 卸载模块
- `enableModule(moduleId)` / `disableModule(moduleId)` — 启用/禁用
- `validateManifest(manifest)` — Zod 校验 manifest.json

**Manifest 规范**：
- 必填：id, name, version, entry, type, permissions
- 可选：description, author, icon, minNativesVersion, api.bridge, lifecycle.heartbeatInterval, lifecycle.loadTimeout, i18n.name, i18n.description

---

#### M7: iframe Manager (`src/lib/iframe-manager.ts`)

**职责**：iframe 生命周期管理（LRU、心跳、崩溃恢复）。

**关键接口**：
- `createIframe(moduleId, url)` — 创建 iframe
- `showIframe(moduleId)` / `hideIframe(moduleId)` — 显示/隐藏
- `destroyIframe(moduleId)` — 销毁
- `onHeartbeatTimeout(moduleId, cb)` — 心跳超时回调
- `onCrash(moduleId, cb)` — 崩溃回调
- `getActiveCount()` — 当前活跃 iframe 数

**生命周期**：
- 插件 10s 内未调用 ready() → 标记超时
- 连续 3 次心跳无响应 → 标记无响应
- unload 后 3s 未响应 → 强制销毁

**内存管理**：
- LRU 策略，默认最多 10 个后台 iframe
- 系统内存压力大时自动回收最久未使用的

---

#### M8: Environment Injector (`src/main/env-injector.ts`)

**职责**：多组环境配置管理，凭证加密存储。

**关键接口**：
- `createProfile(name)` — 创建环境配置
- `deleteProfile(id)` — 删除配置
- `setVariable(profileId, key, value)` — 设置变量（加密存储）
- `getVariables(profileId)` — 获取变量（解密）
- `getDefaultProfile()` — 获取默认配置
- `injectEnv(profileId, env)` — 合并到 env 对象

**加密方案**：`electron.safeStorage.encryptString()` / `decryptString()`

---

#### M9: Theme Engine (`src/lib/theme-engine.ts`)

**职责**：主题系统，CSS 变量注入。

**关键接口**：
- `loadTheme()` — 从 SQLite 读取主题配置
- `applyTheme(theme)` — 注入 CSS 变量到 document.documentElement
- `validateTheme(theme)` — Zod 校验颜色 hex、像素范围
- `onThemeChange(cb)` — 主题变更回调

**CSS 变量**：基础色板（bg-primary/secondary/tertiary, accent, text）、布局参数（sidebar-width, panel-width, terminal-height）、毛玻璃效果。

---

#### M10: Error Classifier (`src/lib/error-classifier.ts`)

**职责**：结构化错误分类，用户可读提示。

**错误类别（12 类）**：
- PLUGIN_CRASH, PLUGIN_TIMEOUT
- BRIDGE_PERMISSION_DENIED, BRIDGE_INVALID_REQUEST
- MODULE_INSTALL_FAILED, MODULE_NOT_FOUND
- TERMINAL_SPAWN_FAILED, TERMINAL_CRASH
- DB_ERROR, CONFIG_CORRUPTED, NETWORK_ERROR, UNKNOWN

**输出**：ClassifiedError 对象（category, userMessage, actionHint, retryable, rawMessage, moduleId）

---

#### M11: Config Manager (`src/lib/config-manager.ts`)

**职责**：JSON 配置文件的原子写入。

**关键接口**：
- `readConfig(path)` — 读取配置
- `updateConfig(path, mutator)` — 原子更新（临时文件 + fsync + rename）

**实现**：Promise chain 串行化，参考 FanBox 的 `_cfgChain` 模式。

---

#### M12: Frontend Shell (`src/app/` + `src/components/`)

**职责**：基座 UI，三栏布局，内置功能页。

**页面**：
- `/` — 仪表盘（首页）
- `/store` — 应用商店
- `/workshop` — 创意工坊
- `/modules` — 模块管理
- `/settings` — 设置

**组件**：
- `shell/Sidebar.tsx` — 左侧边栏
- `shell/ContentArea.tsx` — 中间主内容区
- `shell/RightPanel.tsx` — 右侧面板
- `shell/Terminal.tsx` — 底部终端
- `iframe/IframeContainer.tsx` — iframe 渲染 + 切换

---

#### M13: Watchdog (`src/main/watchdog.ts`)

**职责**：子进程守护。

**实现**：PID 轮询（每 2s `process.kill(parentPid, 0)`），父进程死亡 → 子进程自退出。

---

### 架构决策汇总

| # | 决策 | 决策结果 |
|---|------|----------|
| Q1 | 核心模型 | Hub 聚合 + App Store + 创意工坊 + OS 级管理 |
| Q2 | 插件粒度 | 页面级插件（HTML/JS/CSS，iframe 加载，仅本地文件） |
| Q3 | 架构分层 | 三层：整体 → 底层 → 前端 |
| Q4 | 参考项目角色 | 参考实现（吸收模式，不直接嵌入） |
| Q5 | 技术栈 | Electron + Next.js + TypeScript |
| Q7 | 模块类型 | Web 页面模块（MVP） |
| Q8 | AI 设计含义 | 开发者 AI 辅助开发，基座提供 Bridge API |
| Q9 | 分发机制 | 先离线（本地目录），后期考虑在线 |
| Q10 | 布局 | 三栏 + 底部终端 |
| Q11 | 内置功能 | 终端+商店+设置+模块管理+搜索+通知 |
| Q12 | 通信机制 | 混合 postMessage + HTTP API |
| Q13 | 权限分级 | 从一开始就引入权限声明 + 安装授权 |
| Q14 | 插件包格式 | 目录 + ZIP |
| Q15 | 首页 | 仪表盘 |
| Q16 | 多语言 | 手写 en.ts + zh.ts |
| Q17 | 嵌入技术 | iframe + 本地 HTTP（修正：不用 webview） |
| Q18 | 模块发现 | 启动扫描 + 手动刷新 |
| Q19 | 内存管理 | LRU + 内存监控 |
| Q20 | 崩溃处理 | 静默恢复 + 通知中心报告 |
| Q21 | 开发者体验 | 模板 + 文档 |
| Q22 | HTTP 服务 | 单一服务：静态文件 + API |
| Q23 | Bridge API | 混合 postMessage + HTTP |
| Q24 | 插件隔离 | 路径前缀 + sandbox + Token |
| Q25 | 远程 URL | 不支持（仅本地） |
| Q26 | 终端实现 | node-pty + 降级 spawn |
| Q27 | 插件生命周期 | ready / unload / error / heartbeat |
| Q28 | 配置安全 | 原子写入（temp + fsync + rename） |
| Q29 | 错误分类 | 12 类结构化错误 |
| Q30 | 环境注入 | 多组环境配置 + electron.safeStorage |
| Q31 | DB 迁移 | PRAGMA table_info 增量迁移 |
| Q32 | Token 机制 | HMAC 生成 + postMessage 下发 + 内存存储 |

---

## UI & Interaction Constraints

> 视觉风格参考 FanBox 的三套主题系统。Natives 默认使用 Terminal Volt 暗色主题，支持用户切换。

### 主题系统

Natives 通过 `data-theme` 属性切换主题，所有颜色/间距/圆角通过 CSS 变量注入。

#### 默认主题：Terminal Volt（暗色）

| Token | 值 | 用途 |
|-------|-----|------|
| `--bg` | `#0b0c0a` | 主背景 |
| `--bg-2` | `#131410` | 面板背景 |
| `--bg-3` | `#1c1e17` | 悬浮/卡片背景 |
| `--panel` | `#0e0f0c` | 侧边栏背景 |
| `--border` | `#262920` | 边框 |
| `--rule` | `#262920` | 分割线 |
| `--text` | `#f2f2ea` | 主文字 |
| `--text-dim` | `#9b9d8c` | 次要文字 |
| `--text-faint` | `#62655a` | 弱化文字 |
| `--accent` | `#cdf24b` | 强调色（Volt 绿） |
| `--accent-soft` | `#cdf24b1f` | 强调色低透明度 |
| `--accent-ink` | `#0b0c0a` | 强调色上文字 |
| `--radius` | `4px` | 全局圆角 |
| `--shadow` | `0 10px 40px rgba(0,0,0,0.6)` | 阴影 |

#### 备选主题：Warm Archive（暖色亮色）

| Token | 值 |
|-------|-----|
| `--bg` | `#f5f0e8` |
| `--accent` | `#cc785c`（赤陶色） |
| `--radius` | `9px` |
| `--font-display` | `'Fraunces', Georgia, serif` |

特殊效果：纸张微纹理（`radial-gradient` 点阵）。

#### 备选主题：Editorial Index（编辑风格）

| Token | 值 |
|-------|-----|
| `--bg` | `#f4f1ea` |
| `--accent` | `#ff433d`（Bloomberg 红） |
| `--radius` | `0px`（硬边，粗野主义） |

特殊效果：列表行边框、反转表头、CSS 计数器行号、命令面板 2px 实边框 + 10px 偏移阴影。

#### 终端 ANSI 配色（跟随主题）

| 主题 | 背景 | 前景 | 光标 |
|------|------|------|------|
| Terminal | `#0b0c0a` | `#d6dac9` | `#cdf24b` |
| Warm | `#ece2d2` | `#4a3f30` | `#cc785c` |
| Editorial | `#eae5d8` | `#1a1a1a` | `#ff433d` |

### 字体系统

| Token | 值 |
|-------|-----|
| `--font-ui` | `-apple-system, BlinkMacSystemFont, "PingFang SC", "Microsoft YaHei", "Segoe UI", "Inter", sans-serif` |
| `--font-display` | 跟随主题（Terminal: 等宽, Warm: 衬线, Editorial: 无衬线） |
| `--font-mono` | `ui-monospace, "SF Mono", "Menlo", "JetBrains Mono", monospace` |

**字号规范**：

| 元素 | 字号 | 字重 | 字间距 |
|------|------|------|--------|
| 品牌名 | 17px | 700 | 1px |
| 导航标题 | 11px | — | 1px, uppercase |
| 导航项 | 13px | — | — |
| 模块卡片名 | 12px | — | —, 行高 1.4 |
| 元信息 | 11-12px | — | — |
| 正文 | 14px | — | — |
| 代码 | 12.5px | — | 行高 1.6 |
| Toast | 13px | — | — |
| Kbd 标签 | 11px | — | padding 2px 6px |
| 区域标签 | 10-11px | — | 1-1.5px, uppercase |

### 布局约束

**主布局**：CSS Grid，`grid-template-columns: {sidebarW}fr 1fr`，全屏 `100vh`。

| 区域 | 默认尺寸 | 最小 | 最大 | 行为 |
|------|----------|------|------|------|
| 左侧边栏 | 248px | 190px | 420px | 可折叠（Cmd+B），拖拽调整 |
| 主内容区 | 自适应 | — | — | iframe 容器 |
| 右侧面板 | 320px | 0 | 400px | 可折叠，按需切换 |
| 底部终端 | 280px 高 | 200px | 50% 窗口高 | 可折叠，可最大化 |

**侧边栏折叠**：`grid-template-columns: 1fr`，侧边栏 `display: none`，状态持久化到 localStorage。

**终端停靠**：支持底部（`flex-direction: column`）和右侧（`flex-direction: row`）两种模式。

### 间距规范

| 元素 | 间距 |
|------|------|
| 侧边栏内边距 | 16px 12px |
| 导航区间距 | 18px margin-bottom |
| 导航项内边距 | 7px 8px |
| 顶栏内边距 | 10px 16px |
| 内容区内边距 | 12px 16px 40px |
| 网格间距 | 8px |
| 网格项内边距 | 14px 10px 12px |
| 列表行内边距 | 9px 10px |
| 预览体内边距 | 16px |
| 右键菜单内边距 | 5px |
| 菜单项内边距 | 7px 12px |

### 组件规范

#### 按钮

| 类型 | 样式 |
|------|------|
| **Ghost 按钮** | `border: 1px solid var(--border); background: var(--bg-3); padding: 5px 11px; border-radius: 7px; font-size: 12px` |
| Ghost 悬停 | `color: var(--text); border-color: var(--accent)` |
| **主按钮** | `background: var(--accent); color: var(--accent-ink); font-weight: 600` |
| 主按钮悬停 | `filter: brightness(1.06)` |
| **终端按钮** | `color: var(--accent); background: var(--accent-soft); font-weight: 700` |
| 终端按钮悬停 | `background: var(--accent); color: var(--bg)` |

#### 分段控件

`border: 1px solid var(--border); border-radius: 7px; overflow: hidden`
- 子项：`background: var(--bg-3); padding: 5px 10px; font-size: 12px`
- 激活：`background: var(--accent); color: var(--accent-ink)`

#### 输入框

`background: var(--bg); border: 1px solid var(--border); border-radius: var(--radius); padding: 9px 12px; font-size: 14px`
- 焦点：`border-color: var(--accent)`

#### 开关

`width: 30px; height: 17px; border-radius: 99px`
- 旋钮：11px 圆形，`left: 2px` → `15px`

#### 右键菜单

`background: var(--bg-2); border: 1px solid var(--border); border-radius: 10px; min-width: 168px`
- 菜单项：`padding: 7px 12px; font-size: 13px; border-radius: 6px`
- 危险项：`color: #e06a5b`
- 分割线：`height: 1px; background: var(--border)`

#### Toast 通知

`position: fixed; bottom: 24px; left: 50%; transform: translateX(-50%); background: var(--bg-3); border: 1px solid var(--border); padding: 10px 18px; border-radius: 10px; font-size: 13px; z-index: 200`
- 错误：`border-color: #d9534f`
- 自动隐藏：2200ms 后 `opacity: 0`

#### 滚动条

```css
::-webkit-scrollbar { width: 10px; height: 10px; }
::-webkit-scrollbar-thumb {
  background-color: var(--border); border-radius: 5px;
  background-clip: padding-box; border: 3px solid transparent;
}
::-webkit-scrollbar-thumb:hover { background-color: var(--text-faint); }
```

### 毛玻璃效果

| 元素 | 背景 | 模糊 | 饱和 |
|------|------|------|------|
| 命令面板 | `color-mix(in srgb, var(--bg-2) 82%, transparent)` | `blur(24px)` | `saturate(1.5)` |
| 右键菜单 | `color-mix(in srgb, var(--bg-2) 85%, transparent)` | `blur(20px)` | `saturate(1.4)` |
| 输入对话框 | 同右键菜单 | `blur(20px)` | `saturate(1.4)` |
| 状态栏 | `color-mix(in srgb, var(--bg) 86%, transparent)` | `blur(6px)` | — |
| 侧边栏（桌面） | `color-mix(in srgb, var(--panel) 80%, transparent)` | — | — |

**浮层双层阴影**：
```css
box-shadow: 0 0 0 0.5px rgba(0,0,0,0.12), 0 12px 40px rgba(0,0,0,0.22);
```

### 动画与过渡

#### 主题切换

所有主要表面：`background-color 0.28s ease, border-color 0.28s ease, color 0.28s ease`

#### 布局动画

面板展开/折叠：`.lay-anim` 类启用 `flex-basis 0.22s ease, width 0.22s ease, height 0.22s ease, opacity 0.2s ease`，持续 280ms。

#### 关键帧动画

| 名称 | 时长 | 缓动 | 用途 |
|------|------|------|------|
| `tipIn` | 0.12s | ease | 工具提示淡入 |
| `dropIn` | 0.16s | cubic-bezier(0.2,0.7,0.3,1) | 拖放区域出现 |
| `edIn` | 0.18s | ease | 面板入场（translateY(4px) + opacity） |
| `shotIn` | 0.22s | ease | 卡片入场（translateY(14px) + opacity） |
| `livePulse` | 1.1s | ease-in-out ∞ | 实时状态点脉冲 |
| `tabpulse` | 1.1s | ease-in-out ∞ | 终端标签忙碌点 |
| `liveZap` | 0.5s | ease | 网格项弹跳（scale 1→1.055→1） |
| `liveZapRow` | 0.5s | ease | 列表行闪烁（背景色变化） |
| `editRipple` | 0.7s | cubic-bezier(0.2,0.7,0.3,1) | 编辑涟漪（从图标中心扩散） |
| `changedPulse` | 0.5s | ease | 变更徽章弹入 |
| `changedBreath` | 2.2s | ease-in-out ∞ | 变更文件发光呼吸 |
| `clFlash` | 1.3s | ease-out | 新代码行闪烁 |
| `areaRipple` | 1.1s | cubic-bezier(0.2,0.7,0.3,1) | 完成涟漪（全区域扩散） |
| `termCatch` | 0.5s | ease | 终端捕获闪光 |
| `termAwait` | 1.8s | ease-in-out ∞ | 终端等待呼吸光晕 |

#### 交互过渡

| 元素 | 过渡 |
|------|------|
| 导航项悬停 | `0.12s` |
| 按钮悬停 | `0.12s` |
| 菜单项悬停 | `0.1s` |
| 拖拽条出现 | `opacity 0.15s ease 0.08s`（80ms 延迟） |

#### 无障碍

`@media (prefers-reduced-motion: reduce)` 强制 `animation-duration: 0.01ms` 和 `transition-duration: 0.01ms`。

### 交互模式

#### 侧边栏

- **折叠/展开**：Cmd+B 快捷键，状态持久化到 localStorage
- **拖拽调整宽度**：拖拽条使用 `requestAnimationFrame` 平滑更新，钳制 190-420px
- **模块拖拽排序**：`draggable="true"`，设置 `text/plain` MIME 类型
- **模块图标悬停**：`transition: 0.12s`，显示模块名称 tooltip

#### 终端

- **展开/折叠**：动画 0.22s ease
- **最大化**：`.term-max` 类隐藏文件区域，终端填满全部空间
- **拖放文件**：`.term-drop` 类显示虚线边框覆盖层（`2px dashed var(--accent)`），`dropIn 0.16s`
- **文字飞入**：选中文字创建 `.fling-ghost` 元素，`0.55s cubic-bezier(0.45,0,0.2,1)` 飞向终端中心
- **终端捕获闪光**：`.term-catch` 动画 500ms 内发光

#### iframe 切换

- **切换过渡**：新 iframe 在隐藏状态加载，`onload` 后切换显示（参考 FanBox 双缓冲）
- **加载状态**：居中 "loading..." 文字，`padding: 30px; color: var(--text-faint)`
- **空状态**：`padding: 60px 20px; text-align: center; color: var(--text-faint)`，大图标 44px
- **错误状态**：友好错误页面 + "重新加载" 按钮

#### 模块安装

- **拖入 ZIP/目录**：文件区域显示虚线覆盖层 + "release to install" 文字
- **安装进度**：进度条或 spinner，完成后 `areaRipple` 涟漪动画
- **安装完成**：Toast 通知 + 侧边栏自动刷新

### Z-Index 层级

| 层级 | z-index | 用途 |
|------|---------|------|
| 拖拽条 | 5 | 分割线拖拽 |
| iframe 容器 | 10 | 插件内容 |
| 右侧面板 | 20 | 工坊/设置 |
| 终端 | 30 | 底部终端 |
| 对话框覆盖层 | 60 | 模态对话框 |
| 右键菜单 | 70 | 上下文菜单 |
| 命令面板覆盖层 | 100 | Cmd+K |
| Toast | 200 | 通知 |
| 工具提示 | 300 | 悬停提示 |

### 图标系统

使用 `lucide-react` 图标库，通过 `<svg>` 渲染：
- `viewBox="0 0 24 24"`
- `stroke="currentColor"`
- `stroke-width="2"`
- `stroke-linecap="round"`
- `stroke-linejoin="round"`

图标颜色跟随 `currentColor`，自动适配主题。

---

## Testing Decisions

### 测试原则

- **只测外部行为**：测试模块的公开接口和用户可见的行为，不测内部实现细节。
- **隔离测试**：每个模块可以独立测试，不依赖其他模块的运行时状态。
- **快照测试**：UI 组件使用快照测试捕获回归。
- **E2E 测试**：关键用户流程（安装模块、打开模块、使用终端）使用 Playwright E2E 测试。

### 需要测试的模块

| 模块 | 测试类型 | 优先级 |
|------|----------|--------|
| M2 Database | 单元测试（CRUD、迁移、并发） | P0 |
| M3 HTTP Server | 单元测试（路由、认证、CSP） | P0 |
| M4 Bridge System | 单元测试（权限检查、Token 验证） + 集成测试（postMessage + HTTP） | P0 |
| M6 Module Manager | 单元测试（manifest 校验、安装/卸载） | P0 |
| M8 Environment Injector | 单元测试（加密/解密、注入） | P0 |
| M10 Error Classifier | 单元测试（错误分类、用户提示） | P1 |
| M11 Config Manager | 单元测试（原子写入、并发安全） | P1 |
| M7 iframe Manager | 集成测试（生命周期、LRU） | P1 |
| M5 Terminal | 集成测试（PTY 创建、resize、环境注入） | P1 |
| M12 Frontend Shell | 快照测试 + E2E 测试 | P2 |
| M9 Theme Engine | 单元测试（Zod 校验、CSS 变量注入） | P2 |

### 参考先例

- **CodePilot**: Playwright E2E + unit tests (tsx --test) + visual regression
- **FanBox**: 无测试基础设施

---

## Out of Scope

以下功能**不在 MVP 范围内**：

1. **在线模块分发**：云端模块商店、在线更新、版本管理。MVP 仅支持本地目录扫描。
2. **远程 URL 模块**：加载远程网页作为插件。MVP 仅支持本地文件。
3. **MCP Server 模块**：MCP 协议集成。MVP 仅支持 Web 页面模块。
4. **Electron 原生模块**：原生 Node.js 模块作为插件。
5. **自动更新**：应用自身的自动更新机制（electron-updater）。
6. **全局键盘快捷键体系**：仅保留基础快捷键（Cmd+K 搜索），完整快捷键体系后期设计。
7. **窗口状态持久化**：面板宽度、折叠状态等 UI 状态的完整持久化。
8. **环境变量诊断工具**（env-doctor）：验证 API Key 有效性的诊断工具。
9. **双缓冲 iframe 切换**：零闪白的 iframe 切换机制（建议采纳但非 MVP 必需）。
10. **xterm.js vendor patch**：keyCode 20/229 补丁验证。
11. **跨平台打包**：Windows NSIS、Linux AppImage 打包。MVP 仅支持 macOS DMG。
12. **组件级插件**：插件作为 UI 组件嵌入基座页面。MVP 仅支持页面级插件。

---

## Further Notes

### 文件结构

```
Natives/
├── docs/
│   ├── architecture/
│   │   ├── ARCHITECTURE.md          # 架构规范
│   │   └── DESIGN_DISCUSSION.md     # 设计讨论记录（32 个决策）
│   └── PRD.md                       # 本文档
├── electron/
│   ├── main.ts                      # Electron 入口
│   └── preload.ts                   # IPC 隔离桥
├── src/
│   ├── main/                        # Main Process 模块
│   │   ├── database.ts              # SQLite (9 张表, WAL, 增量迁移)
│   │   ├── http-server.ts           # 本地 HTTP 服务
│   │   ├── module-manager.ts        # 模块生命周期
│   │   ├── bridge-host.ts           # Bridge API 宿主
│   │   ├── shell.ts                 # Shell 管理 (node-pty)
│   │   ├── env-injector.ts          # 环境注入
│   │   ├── subprocess.ts            # 子进程管理
│   │   └── watchdog.ts              # 进程守护
│   ├── app/                         # Next.js 页面
│   │   ├── page.tsx                 # 仪表盘
│   │   ├── layout.tsx               # Root Layout
│   │   ├── globals.css              # 全局样式
│   │   ├── store/page.tsx           # 应用商店
│   │   ├── workshop/page.tsx        # 创意工坊
│   │   ├── modules/page.tsx         # 模块管理
│   │   └── settings/page.tsx        # 设置
│   ├── components/
│   │   ├── shell/
│   │   │   ├── Sidebar.tsx
│   │   │   ├── ContentArea.tsx
│   │   │   ├── RightPanel.tsx
│   │   │   └── Terminal.tsx
│   │   └── iframe/
│   │       └── IframeContainer.tsx
│   ├── lib/
│   │   ├── bridge-sdk.js            # Bridge SDK (注入到 iframe)
│   │   ├── iframe-manager.ts        # iframe 生命周期 (LRU, heartbeat)
│   │   ├── module-store.ts          # 模块状态管理
│   │   ├── theme-engine.ts          # 主题引擎
│   │   ├── error-classifier.ts      # 错误分类
│   │   └── config-manager.ts        # 原子写入配置
│   ├── hooks/
│   ├── types/
│   │   └── index.ts
│   └── i18n/
│       ├── en.ts
│       └── zh.ts
├── plugin-template/
│   ├── README.md
│   ├── manifest.json
│   ├── index.html
│   └── natives-sdk.d.ts
├── CLAUDE.md
├── package.json
├── tsconfig.json
├── next.config.ts
└── electron-builder.yml
```

### 关键依赖

| 依赖 | 用途 | 备注 |
|------|------|------|
| Electron | 桌面框架 | contextIsolation: true, 无 nodeIntegration |
| Next.js | 前端框架 | App Router, output: 'standalone' |
| TypeScript | 类型安全 | 全项目使用 |
| better-sqlite3 | 数据库 | serverExternalPackages, WAL 模式 |
| node-pty | PTY 终端 | native module, 需 electron-rebuild |
| @xterm/xterm | 终端渲染 | |
| zod | 配置校验 | manifest + 主题 + 设置 |
| lucide-react | 图标 | |

### 数据目录

```
~/.natives/
├── natives.db                       # SQLite 数据库 (WAL 模式)
├── modules/                         # 已安装模块
│   ├── com.example.myapp/
│   │   ├── manifest.json
│   │   ├── index.html
│   │   └── ...
│   └── ...
├── env/
│   └── credentials.json             # 加密存储的凭证
├── config.json                      # 用户配置（原子写入）
└── logs/                            # 运行日志
```
