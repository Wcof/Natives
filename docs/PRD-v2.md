# Natives — Product Requirements Document v2.0

> **版本**: 2.0.0
> **日期**: 2026-06-14
> **状态**: Draft
> **基于**: 43 个架构设计决策 (DESIGN_DISCUSSION.md Q1-Q43) + 架构规范 (ARCHITECTURE.md)
> **参考实现**: CodePilot (AI-native patterns) + FanBox (zero-dependency backend patterns)
> **变更**: 从「纯插件容器」升级为「带文件管理 + AI 工作台能力的插件容器」

---

## Problem Statement

当前桌面应用生态存在三个核心问题：

1. **工具碎片化**：用户需要在多个独立应用之间切换（AI 客户端、终端、文件管理器、IDE），每个应用有自己的配置、凭证、界面风格，缺乏统一入口。

2. **AI 工具集成困难**：AI CLI 工具（Claude Code、Codex、Aider 等）需要 API Key 配置、终端环境、文件预览等配套能力，但这些能力分散在不同工具中，用户需要手动串联。

3. **扩展性受限**：现有应用的功能是开发者硬编码的，用户无法自行扩展或定制界面。即使有插件系统，也通常绑定在特定应用内部，无法跨应用复用。

用户需要一个**统一的桌面基座**，像 Steam 之于游戏一样，提供应用浏览、安装、运行、管理的完整体验，同时内置终端、文件管理、AI 工作台能力，让 AI 工具开箱即用。

---

## Solution

**Natives** 是一个 AI 时代的桌面应用容器（"AI Steam Base"），基于 Electron + Next.js + SQLite 构建。它提供：

1. **统一基座**：三栏布局的桌面容器，左侧订阅侧边栏、中间插件内容区/文件浏览器、右侧预览面板/工坊/设置、底部可折叠终端。

2. **插件生态**：页面级插件通过 iframe + 本地 HTTP 服务加载，每个插件是独立的 HTML/JS/CSS 页面，通过 Bridge API（`window.natives.*`）与基座通信。插件从本地目录 `~/.natives/modules/` 发现和加载。

3. **文件管理**：内置文件浏览器（网格/列表视图）、文件预览（Markdown WYSIWYG、HTML 实时渲染、代码高亮、多媒体播放）、文件搜索（模糊+全文）、文件操作（创建/重命名/删除/移动）、Git 集成（只读）、磁盘用量透视。

4. **AI 工作台**：终端增强（跟随模式、可点击路径、Agent 状态）、Agent 变更监控（实时仪表盘、跟随模式、变更收件箱、会话回放）、项目记忆、Skills X-Ray、Agent 用量追踪、RTK 用量分析。

5. **完整终端**：基于 node-pty + xterm.js 的完整 PTY 终端，支持 TUI 程序、窗口调整、多会话。终端启动时自动注入用户配置的环境变量（API Key 等）。

6. **安全沙箱**：iframe sandbox 隔离（无 allow-same-origin）+ Session Token 鉴权 + 路径前缀隔离 + CSP 策略 + Host/Origin 头验证，确保插件之间互不干扰。

7. **创意工坊**：内置插件浏览、安装、管理界面，支持目录和 ZIP 两种安装方式。开发者用任何工具 + AI 辅助开发插件，基座提供模板和 SDK 类型声明。

---

## User Stories

### 基座体验

1. As a 用户, I want 打开 Natives 后看到仪表盘（已安装模块概览、系统状态、快捷操作），so that 我能快速了解当前环境并进入工作。

2. As a 用户, I want 通过左侧边栏看到所有已安装模块的图标和名称，so that 我能快速切换到任意模块。

3. As a 用户, I want 拖拽侧边栏中的模块图标来调整排列顺序，so that 我能把常用的模块放在前面。

4. As a 用户, I want 折叠/展开左侧边栏、右侧面板、底部终端，so that 我能根据当前任务调整布局空间。

5. As a 用户, I want 使用 Cmd+K 全局搜索功能，so that 我能快速找到模块、文件或功能。

6. As a 用户, I want 在通知中心查看来自各模块的通知消息，so that 我不会错过重要信息。

7. As a 用户, I want 在设置面板中切换主题颜色（深色/浅色）和布局参数，so that 我能自定义界面风格。

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

### 文件管理（新增 — 来自 Fanbox）

39. As a 用户, I want 在中间内容区浏览本地文件和文件夹（网格/列表视图），so that 我能在基座内管理文件而不用切换到 Finder。

40. As a 用户, I want 文件浏览器支持缩略图预览、排序（名称/时间/大小）、面包屑导航，so that 我能高效定位文件。

41. As a 用户, I want 在侧边栏快速访问常用目录（桌面、文档、下载、代码目录），so that 我能快速跳转。

42. As a 用户, I want 收藏常用文件和文件夹，so that 我能快速访问重要资源。

43. As a 用户, I want 在右侧面板预览文件内容（Markdown WYSIWYG 编辑、HTML 实时渲染、代码高亮、图片/视频/音频/PDF 内联播放），so that 我无需打开外部应用即可查看文件。

44. As a 用户, I want 预览 Markdown 文件时可以直接编辑（WYSIWYG 模式），so that 我能快速修改文档。

45. As a 用户, I want HTML 文件在沙箱 iframe 中实时渲染，so that 我能预览网页效果。

46. As a 用户, I want 使用 Cmd+K 全局模糊搜索文件名（评分算法），so that 我能快速找到任何文件。

47. As a 用户, I want 使用 `content:关键词` 前缀进行全文搜索，so that 我能在文件内容中查找。

48. As a 用户, I want 搜索范围可在当前文件夹和全盘之间切换，so that 我能控制搜索范围。

49. As a 用户, I want 创建新文件和文件夹，so that 我能在基座内组织项目。

50. As a 用户, I want 重命名、删除（移到回收站）、移动文件，so that 我能管理文件。

51. As a 用户, I want 从 Finder 拖放文件到基座中导入，so that 我能快速复制文件。

52. As a 用户, I want 文件写入使用原子操作（临时文件 + fsync + rename），so that 并发编辑不会损坏文件。

53. As a 用户, I want 查看 Git 仓库的状态（porcelain）和文件 diff（Monaco DiffEditor），so that 我能了解代码变更。

54. As a 用户, I want 查看文件夹的磁盘用量（du 精确大小），so that 我能管理磁盘空间。

55. As a 用户, I want 复制图片/文件到系统剪贴板，so that 我能与其他应用交换数据。

### AI 工作台（新增 — 来自 Fanbox）

56. As a 用户, I want 终端跟随文件浏览器（切换目录时终端自动 cd），so that 我不需要手动 cd。

57. As a 用户, I want 终端输出中的文件路径可点击跳转，so that 我能快速定位 Agent 修改的文件。

58. As a 用户, I want 终端输出中的 HTTP/HTTPS 链接可点击打开，so that 我能快速访问 URL。

59. As a 用户, I want 终端标签显示 Agent 状态点（运行中/空闲/已退出），so that 我能了解 Agent 状态。

60. As a 用户, I want 终端边缘呼吸动画（Agent 把控制权交还给用户时），so that 我能知道 Agent 完成了任务。

61. As a 用户, I want 拖拽文件到终端中插入路径作为 Agent 上下文，so that 我能快速引用文件。

62. As a 用户, I want 长时间任务完成时收到系统通知（包含 Agent 最后回复摘要），so that 我不需要一直盯着终端。

63. As a 用户, I want 文件卡片在 Agent 写入时闪烁（强度与变更频率成正比），so that 我能实时看到 Agent 的工作。

64. As a 用户, I want 跟随模式绑定到特定终端标签，文件视图自动跳转到 Agent 当前编辑的文件，so that 我能实时跟踪 Agent 的工作。

65. As a 用户, I want 跟随模式下预览底部显示实时叙述行（正在写入 X / 运行 Y / 搜索 Z），so that 我能了解 Agent 在做什么。

66. As a 用户, I want 变更收件箱聚合本次会话中所有被修改的文件，so that 我能快速回顾 Agent 的工作成果。

67. As a 用户, I want 会话回放（时间轴滑块）逐步重放 Agent 触摸过的文件，so that 我能理解 Agent 的工作过程。

68. As a 用户, I want 查看任何项目文件夹的 Agent 会话历史（来自 Claude Code 和 Codex 会话日志），so that 我能回顾过去的 AI 工作。

69. As a 用户, I want 恢复之前的 Agent 会话（`claude --resume`），so that 我能继续未完成的工作。

70. As a 用户, I want 查看本机所有 Agent Skills 的列表、触发统计、健康状态，so that 我能管理我的 Skills。

71. As a 用户, I want 启用/禁用/卸载 Skills，so that 我能控制哪些 Skills 对 Agent 可见。

72. As a 用户, I want 查看 Claude Code 的 5 小时窗口用量、周配额、本地 Token 统计，so that 我能了解用量情况。

73. As a 用户, I want 查看 Codex 的用量和计划类型，so that 我能了解用量情况。

74. As a 用户, I want 查看 RTK 的 Token 节省统计、命令历史、使用趋势，so that 我能了解 RTK 的效果。

### AI 文件整理（新增 — 来自 Fanbox）

75. As a 用户, I want AI 提议文件整理方案（只读元数据，不读内容），so that 我能快速整理项目。

76. As a 用户, I want 审核每个整理建议（勾选/取消），so that 我能控制哪些文件被移动。

77. As a 用户, I want 一键撤销整个整理操作，so that 我能安全地尝试整理。

### 发布向导（新增 — 来自 Fanbox）

78. As a 用户, I want 检查 Node 项目的发布就绪状态（package.json、git 状态、CHANGELOG、gh CLI），so that 我能了解发布前需要做什么。

79. As a 用户, I want 一键递增版本号、更新 CHANGELOG、推送到 GitHub Release，so that 我能快速发布项目。

### 截图快递 + 图片标注（新增 — 来自 Fanbox）

80. As a 用户, I want 系统截图时自动弹出浮动卡片（发送到终端/保存到素材/标注），so that 我能快速处理截图。

81. As a 用户, I want 使用画笔、箭头、文字、模糊/遮挡工具标注图片，so that 我能在基座内编辑图片。

### 安全加固（新增 — 来自 Fanbox）

82. As a 用户, I want HTTP 服务验证 Host 头（防 DNS 重绑定），so that 我的本地服务不会被外部访问。

83. As a 用户, I want POST 请求验证 Origin 头（防 CSRF），so that 恶意网页无法调用我的 API。

### 更新通知（新增 — 来自 Fanbox）

84. As a 用户, I want 应用启动时检查 GitHub Releases 更新，so that 我能及时了解新版本。

85. As a 用户, I want 收到更新胶囊通知（可静音特定版本），so that 我能选择是否更新。

---

## Implementation Decisions

### 模块划分

本次实现需要构建以下核心模块：

#### M1: Electron Shell (`electron/main.ts` + `electron/preload.ts`)

**职责**：应用入口、窗口管理、IPC 隔离桥、FOUC 防护。

**关键接口**：
- `BrowserWindow` 创建（`show: false`，FOUC Guard）
- `contextBridge.exposeInMainWorld` 暴露 `nativesAPI` 对象
- IPC 频道：DB CRUD、终端控制、模块管理、环境注入、文件操作、Git、搜索

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

**职责**：本地 HTTP 服务，提供插件静态文件、Bridge API 路由、文件管理 API。

**关键接口**：
- `startServer(port?)` — 启动 HTTP 服务（自动选择空闲端口）
- `getPort()` — 返回实际监听端口
- SDK 端口发现：基座注入 `<script src="http://localhost:{port}/natives-sdk.js">`，SDK 从 `document.currentScript.src` 自动提取端口，插件开发者无需关心
- 路由表：
  - `GET /natives-sdk.js` — Bridge SDK 脚本
  - `GET /modules/{moduleId}/{path}` — 静态文件
  - `POST /api/bridge/{namespace}/{method}` — Bridge API
  - **文件管理 API**（新增）：
    - `GET /api/fs/list?path=` — 目录列表
    - `GET /api/fs/read?path=` — 文件内容
    - `GET /api/fs/raw?path=` — 原始文件流（Range 支持）
    - `GET /api/fs/thumb?path=&w=` — 缩略图
    - `GET /api/fs/search?q=&root=` — 模糊文件名搜索
    - `GET /api/fs/grep?q=&root=` — 全文搜索
    - `GET /api/fs/content?q=&root=` — Spotlight 搜索
    - `GET /api/fs/recent?root=` — 最近修改文件
    - `GET /api/fs/git?path=` — Git 状态
    - `GET /api/fs/git-file?path=` — Git diff
    - `GET /api/fs/archive?path=` — 压缩包内容
    - `GET /api/fs/du?path=` — 磁盘用量
    - `POST /api/fs/write` — 原子写入
    - `POST /api/fs/create` — 创建文件/目录
    - `POST /api/fs/rename` — 重命名
    - `POST /api/fs/trash` — 删除到回收站
    - `POST /api/fs/move` — 移动文件
    - `POST /api/fs/open` — 用系统应用打开
    - `GET /api/fs/locate?path=&name=` — 终端路径定位
    - `POST /api/fs/term-verify` — 批量路径验证

**安全策略**：
- CSP 头注入
- Session Token 验证（X-Session-Token + X-Module-Id）
- 路径前缀隔离
- MessageEvent.source 窗口引用匹配（替代 origin 检查，详见 ADR-0002）
- **Host 头验证**（新增，防 DNS 重绑定）
- **Origin 头验证**（新增，防 CSRF）

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

**插件间通信（IPC）**：
- sandbox iframe 无法使用 BroadcastChannel / SharedWorker，所有消息经主进程中转
- `ipc.send(targetModuleId, payload)` — 定向发送，基座验证权限后路由到目标 iframe
- `ipc.broadcast(payload)` — 广播到所有活跃 iframe
- 基座可做权限检查和消息审计

**Session Token 机制**（两阶段握手）：
- 基座启动时生成 masterSecret
- iframe 加载 SDK 后，主动向基座发送 `{ type: 'token-request', moduleId }`（消除竞态）
- 基座验证 moduleId 合法性后，生成 sessionToken = HMAC(masterSecret, moduleId + timestamp)，通过 postMessage 下发
- 插件重载时自动重新请求（SDK 初始化时始终先请求 token，不假设内存中有值）
- 基座侧维护 moduleId → token 映射，同一 moduleId 可重新下发（旧 token 自动失效）
- SDK 保存在内存变量中（非 localStorage）
- 所有 postMessage 和 HTTP 请求需携带 token
- 详见 [[ADR-0001-session-token-handshake]]

---

#### M5: Terminal (`src/main/shell.ts`)

**职责**：多会话 PTY 终端，环境注入，终端增强。

**关键接口**：
- `createSession(env?)` — 创建 PTY 会话，返回 sessionId
- `write(sessionId, data)` — 写入数据
- `resize(sessionId, cols, rows)` — 调整窗口大小
- `kill(sessionId)` — 终止会话
- `onData(sessionId, cb)` — 数据流回调
- `onExit(sessionId, cb)` — 退出回调

**实现**：
- node-pty（主方案）+ child_process.spawn（降级方案）
- @xterm/xterm 前端渲染（WebGL + unicode11 CJK 支持）
- Session Token 握手防 XSS
- 环境变量注入仅对新会话生效（已运行会话保持原有环境，用户需新开终端标签）

**终端增强**（新增）：
- **跟随模式**：终端自动 cd 跟随文件浏览器目录变化
- **可点击路径**：终端输出中的文件路径可点击跳转（stat 验证，处理空格、中文、换行路径）
- **可点击链接**：HTTP/HTTPS 链接可点击打开
- **Agent 状态点**：终端标签显示运行中/空闲/已退出状态
- **呼吸动画**：Agent 把控制权交还给用户时终端边缘发光
- **拖拽文件到终端**：插入文件路径作为 Agent 上下文
- **系统通知**：长时间任务完成时发送通知（包含 Agent 最后回复摘要）
- **终端最大化**：填满整个窗口
- **停靠模式切换**：底部/右侧布局
- **静音切换**：关闭通知声音
- **退出确认**：终端会话仍在运行时确认退出

---

#### M6: Module Manager (`src/main/module-manager.ts`)

**职责**：模块生命周期管理（安装/卸载/启用/禁用/更新）。

**关键接口**：
- `scanModules()` — 扫描 `~/.natives/modules/` 目录，检测版本变化
- `installModule(pathOrZip)` — 安装模块（目录或 ZIP）
- `uninstallModule(moduleId)` — 卸载模块
- `enableModule(moduleId)` / `disableModule(moduleId)` — 启用/禁用
- `updateModule(moduleId)` — 更新模块（备份数据 → 替换文件 → 保留数据 → 重载）
- `validateManifest(manifest)` — Zod 校验 manifest.json
- `checkCompatibility(manifest)` — 检查 minNativesVersion 与当前版本兼容性

**Manifest 规范**：
- 必填：id, name, version, entry, type, permissions
- 可选：description, author, icon, minNativesVersion, api.bridge, lifecycle.heartbeatInterval, lifecycle.loadTimeout, i18n.name, i18n.description

**更新机制**：
- 启动扫描时比较 `manifest.json` 的 `version` 字段，检测到变化标记为"可更新"
- 更新流程：备份 module_data → 替换插件文件 → 保留 module_data → 重新加载
- Bridge API 版本不兼容时显示警告，不自动更新

**版本兼容性检查**：
- 基座通过 `window.natives.meta.nativesVersion` 暴露当前 Bridge API 版本
- 插件在 `manifest.json` 中声明 `minNativesVersion`
- 主版本号不同 → 阻止加载并提示；次版本号不同 → 警告但允许运行
- 遵循语义化版本（SemVer）

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
- LRU 策略，默认最多 5 个后台 iframe
- 单个 iframe 内存软上限 150MB
- 系统可用内存 < 1GB 时自动回收最久未使用的

**状态保持分层策略**：
- 热层（当前可见）：iframe 保持活跃，JS 内存状态自然保留
- 温层（最近 5 个）：iframe 保持在 DOM 中但隐藏，状态保留
- 冷层（超出温层）：iframe 被销毁，状态丢失
- 持久层（插件主动保存）：插件通过 `window.natives.db.set()` 将关键数据保存到 module_data，重载后通过 `get()` 恢复

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

**主题精简**（v2.0 变更）：从 3 套精简为 2 套：
- **Terminal Volt（深色）**：默认主题
- **Frosted Jasmine / 磨砂茉莉（浅色）**：备选主题（原 Warm Archive 更名）
- ~~Editorial Index~~：删除
- ~~Warm Archive~~：已更名

> **注意**：`warm-archive` 已重命名为 `frosted-jasmine`，`editorial` 已移除。代码中的 CSS `data-theme` 属性值及所有配置文件均已更新。

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
- `/files` — 文件浏览器（新增）
- `/store` — 应用商店
- `/workshop` — 创意工坊
- `/modules` — 模块管理
- `/settings` — 设置

**组件**：
- `shell/Sidebar.tsx` — 左侧边栏（新增：文件快捷入口、收藏夹、Agent 项目列表）
- `shell/ContentArea.tsx` — 中间主内容区（新增：文件浏览器视图）
- `shell/RightPanel.tsx` — 右侧面板（新增：文件预览面板）
- `shell/Terminal.tsx` — 底部终端（增强：跟随模式、可点击路径、Agent 状态）
- `iframe/IframeContainer.tsx` — iframe 渲染 + 切换
- **新增组件**：
  - `files/FileBrowser.tsx` — 文件浏览器（网格/列表视图）
  - `files/FilePreview.tsx` — 文件预览（Markdown/HTML/代码/多媒体）
  - `files/FileSearch.tsx` — 文件搜索（Cmd+K 集成）
  - `files/GitPanel.tsx` — Git 状态和 Diff
  - `files/DiskUsage.tsx` — 磁盘用量透视
  - `ai/AgentMonitor.tsx` — Agent 变更监控仪表盘
  - `ai/ChangeInbox.tsx` — 变更收件箱
  - `ai/SessionReplay.tsx` — 会话回放
  - `ai/ProjectMemory.tsx` — 项目记忆
  - `ai/SkillsPanel.tsx` — Skills X-Ray
  - `ai/UsagePanel.tsx` — Agent 用量追踪
  - `ai/RtkPanel.tsx` — RTK 用量分析（新增）
  - `ai/FileOrganizer.tsx` — AI 文件整理
  - `tools/ReleaseWizard.tsx` — 发布向导
  - `tools/ScreenshotExpress.tsx` — 截图快递
  - `tools/ImageAnnotator.tsx` — 图片标注编辑器
  - `common/UpdateNotification.tsx` — 更新通知胶囊

---

#### M13: Watchdog (`src/main/watchdog.ts`)

**职责**：子进程守护。

**实现**：PID 轮询（每 2s `process.kill(parentPid, 0)`），父进程死亡 → 子进程自退出。

---

#### M14: File Manager (`src/main/file-manager.ts`) — 新增

**职责**：文件系统操作，文件预览生成，文件搜索。

**关键接口**：
- `listDir(path)` — 目录列表（元数据：名称、路径、isDir、kind、hidden、size、mtime、btime、项目徽章）
- `readFile(path)` — 读取文件内容（文本 ≤2MB，大文件取前 256KB）
- `streamFile(path, range)` — 流式文件内容（Range 支持）
- `generateThumb(path, width)` — 生成缩略图（macOS sips/qlmanage）
- `searchFiles(query, root)` — 模糊文件名搜索（评分算法：子序列匹配、连续奖励、词边界奖励、位置奖励、最近修改奖励）
- `grepContent(query, root)` — 全文搜索
- `spotlightSearch(query, root)` — macOS Spotlight 搜索（mdfind）
- `writeFileAtomic(path, content, expectedMtime)` — 原子写入 + mtime 冲突检测
- `createFile(path)` / `createDir(path)` — 创建文件/目录
- `renamePath(oldPath, newPath)` — 重命名
- `trashPath(path)` — 删除到回收站
- `movePath(from, to)` — 移动（同卷 rename，跨卷 copy+delete）
- `getDiskUsage(path)` — 磁盘用量（du -sk）
- `getGitStatus(path)` — Git 状态（porcelain）
- `getGitDiff(path)` — Git diff（HEAD vs working tree）
- `locateFromTerminal(output)` — 从终端输出定位文件路径
- `verifyPaths(paths)` — 批量路径验证

**实现**：
- 零依赖（纯 Node.js 内置模块：fs, path, os, crypto, child_process）
- 缩略图：macOS `sips`（图片）+ `qlmanage`（视频/PDF）
- 搜索：子序列匹配评分算法
- 全文搜索：macOS `mdfind`（Spotlight）+ grep 降级
- 原子写入：临时文件 + fsync + rename
- 回收站：macOS `osascript` AppleScript

---

#### M15: Agent Integration (`src/main/agent-integration.ts`) — 新增

**职责**：Agent 会话管理、Skills 管理、用量追踪。

**关键接口**：
- `scanAgentProjects()` — 扫描 Claude Code/Codex 会话日志，返回最近 30 天的项目列表
- `getProjectMemory(path)` — 获取项目的历史会话（首条用户消息为标题）
- `resumeSession(sessionId, engine)` — 恢复会话（`claude --resume` / `codex resume`）
- `scanSkills()` — 扫描 5 个 skill 来源（~/.claude/skills, 项目 .claude/skills, Claude plugins, ~/.codex/skills, ~/.agents/skills）
- `getSkillStats(skillName)` — 获取触发统计（45 天历史）
- `toggleSkill(skillName, enabled)` — 启用/禁用 skill
- `trashSkill(skillName)` — 卸载 skill
- `getClaudeUsage()` — 获取 Claude Code 用量（5 小时窗口、周配额、本地 Token 统计）
- `getCodexUsage()` — 获取 Codex 用量
- `getRtkUsage()` — 获取 RTK Token 节省统计、命令历史
- `watchFileChanges(path, cb)` — 监控文件变更（Agent 变更监控）
- `detectAgentStatus(sessionId)` — 检测 Agent 状态（运行中/空闲/已退出）

**实现**：
- 会话日志：扫描 `~/.claude/projects/` 和 `~/.codex/sessions/`
- Skills：扫描多个目录，健康检查（描述截断检测、缺失 frontmatter、缺失 SKILL.md）
- Claude Code 用量：Anthropic API（`/usage`）+ 本地 Token 统计
- RTK 用量：`rtk gain --history` 命令输出解析
- 文件监控：`fs.watch` + 噪声过滤（忽略 atime 更新、隐藏文件、SQLite sidecar）

---

#### M16: Screenshot & Annotation (`src/main/screenshot.ts`) — 新增

**职责**：截图监控、图片标注。

**关键接口**：
- `watchScreenshotDir(cb)` — 监控系统截图目录
- `recognizeScreenshot(filename)` — 识别截图文件名模式
- `saveAnnotatedImage(dataUrl, path)` — 保存标注后的图片

**实现**：
- 截图目录：`~/Desktop`（macOS 默认）
- 文件名模式：截屏、截圖、截图、Screenshot、Screen Shot、CleanShot、SCR-
- 标注工具：Canvas API（画笔、箭头、文字、模糊/遮挡）
- 格式转换：Canvas toDataURL + sharp（可选）

---

#### M17: Release Wizard (`src/main/release-wizard.ts`) — 新增

**职责**：Node 项目发布管理。

**关键接口**：
- `inspectProject(path)` — 检查发布就绪状态
- `prepareRelease(path, version)` — 准备发布（版本号递增、CHANGELOG 更新）
- `getCommandSequence(path)` — 获取发布命令序列

**实现**：
- 检查：package.json、git status、CHANGELOG.md、gh CLI
- 版本递增：patch+1 预填充
- CHANGELOG：Unreleased 段落变为新版本
- 命令序列：build → commit → push → GitHub Release

---

#### M18: Update Checker (`src/main/update-checker.ts`) — 新增

**职责**：应用更新检查。

**关键接口**：
- `checkForUpdates()` — 检查 GitHub Releases
- `getLatestVersion()` — 获取最新版本
- `muteVersion(version)` — 静音特定版本

**实现**：
- GitHub Releases API（降级：解析 releases 页面重定向 URL）
- 启动 6 秒后首次检查，之后每 2 小时
- 窗口获焦时重新检查（30 分钟节流）
- 胶囊通知（右下角）

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
| Q11 | 内置功能 | 终端+商店+设置+模块管理+搜索+通知+文件管理+AI 工作台 |
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
| Q22 | HTTP 服务 | 单一服务：静态文件 + API + 文件管理 API |
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
| Q33 | Token 握手 | 两阶段握手（iframe 主动请求，消除竞态） |
| Q34 | 来源验证 | MessageEvent.source 窗口引用匹配（替代 origin 检查） |
| Q35 | 插件间通信 | 主进程中转（send + broadcast） |
| Q36 | 终端环境注入 | 仅对新会话生效 |
| Q37 | 状态保持 | 分层策略（热/温/冷/持久）+ 内存限制 |
| Q38 | 崩溃检测 | 双层检测（心跳 + 主动报告） |
| Q39 | 不造轮子 | 适用于插件层，基座层必须自建 |
| Q40 | 无障碍 | 键盘导航 + ARIA + 焦点管理 + 高对比度 |
| Q41 | 插件更新 | 扫描版本变化 → 备份数据 → 替换文件 → 保留数据 |
| Q42 | 版本兼容 | SemVer + minNativesVersion 声明 + 主版本阻断 |
| Q43 | 多窗口 | MVP 单窗口，后期扩展 |
| Q44 | 文件管理 | 基座内置文件浏览器 + 预览 + 搜索 + 操作（来自 Fanbox） |
| Q45 | AI 工作台 | 基座内置 Agent 监控 + 项目记忆 + Skills + 用量（来自 Fanbox） |
| Q46 | 主题精简 | 2 套主题（深色 Terminal Volt + 浅色 Frosted Jasmine），删除 Editorial Index，Warm Archive 更名 |
| Q47 | 安全加固 | Host 头验证 + Origin 头验证（来自 Fanbox） |

---

## UI & Interaction Constraints

> 视觉风格参考 FanBox 的主题系统。Natives 默认使用 Terminal Volt 暗色主题，支持用户切换到 Frosted Jasmine（磨砂茉莉）浅色主题。

### 主题系统

Natives 通过 `data-theme` 属性切换主题，所有颜色/间距/圆角通过 CSS 变量注入。

#### 默认主题：Terminal Volt（深色）

| Token | 值 | 用途 |
|-------|-----|------|
| `--bg` | `#0d0f12` | 主背景 |
| `--bg-2` | `#15181d` | 面板背景 |
| `--bg-3` | `#1c2027` | 悬浮/卡片背景 |
| `--panel` | `#0d0f12` | 侧边栏背景 |
| `--border` | `#2a2f38` | 边框 |
| `--text` | `#d4d7de` | 主文字 |
| `--text-dim` | `#8b90a0` | 次要文字 |
| `--text-faint` | `#555a66` | 弱化文字 |
| `--accent` | `#00ff9c` | 强调色（伏特绿） |
| `--accent-soft` | `rgba(0,255,156,0.15)` | 强调色低透明度 |
| `--accent-ink` | `#0d0f12` | 强调色上文字 |
| `--radius` | `4px` | 全局圆角 |
| `--font-display` | `'JetBrains Mono', monospace` | 展示字体 |

#### 备选主题：Frosted Jasmine / 磨砂茉莉（浅色）

| Token | 值 |
|-------|-----|
| `--bg` | `#fdf6f0` |
| `--bg-2` | `#f9eee4` |
| `--bg-3` | `#f5e5d6` |
| `--panel` | `#fcf3ea` |
| `--border` | `#e8d5c0` |
| `--text` | `#2d1f14` |
| `--text-dim` | `#7a6b5a` |
| `--text-faint` | `#b8a594` |
| `--accent` | `#7a9e7e`（鼠尾草绿） |
| `--accent-soft` | `rgba(122,158,126,0.15)` |
| `--accent-ink` | `#fdf6f0` |
| `--radius` | `12px` |
| `--font-display` | `'Noto Serif SC', Georgia, serif` |

特殊效果：磨砂玻璃侧栏/顶栏 + 纸张微纹理（`radial-gradient` 点阵）。

#### 终端 ANSI 配色（跟随主题）

| 主题 | 背景 | 前景 | 光标 |
|------|------|------|------|
| Terminal Volt | `#0d0f12` | `#d4d7de` | `#00ff9c` |
| Frosted Jasmine | `#fdf6f0` | `#2d1f14` | `#7a9e7e` |

### 字体系统

| Token | 值 |
|-------|-----|
| `--font-ui` | `-apple-system, BlinkMacSystemFont, "PingFang SC", "Microsoft YaHei", "Segoe UI", "Inter", sans-serif` |
| `--font-display` | 跟随主题（Terminal: 等宽, Warm: 衬线） |
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
| 主内容区 | 自适应 | — | — | iframe 容器 / 文件浏览器 |
| 右侧面板 | 320px | 0 | 400px | 可折叠，按需切换（预览/工坊/设置） |
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
| `fadeIn` | 0.18s | ease | 面板入场（translateY(4px) + opacity） |
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

**动画减弱**：`@media (prefers-reduced-motion: reduce)` 强制 `animation-duration: 0.01ms` 和 `transition-duration: 0.01ms`。

**键盘导航**：
- Tab 遍历所有可交互元素（侧边栏、按钮、输入框、iframe）
- Enter 激活当前聚焦元素
- Escape 关闭弹窗、右键菜单、命令面板
- Arrow 键在列表/菜单中移动
- Cmd+Shift+K 从 iframe 跳回基主导航（跳过链接）

**焦点管理**：
- 切换插件时焦点自动移入 iframe
- 切换回基座时焦点回到侧边栏最后聚焦的元素
- 弹窗打开时焦点锁定在弹窗内（焦点陷阱）
- 弹窗关闭后焦点回到触发元素

**ARIA 标注**：
- 所有按钮使用 `role="button"` + `aria-label`
- 面板使用 `role="region"` + `aria-label`
- 状态区域使用 `aria-live="polite"`（非紧急）或 `aria-live="assertive"`（紧急）
- 侧边栏使用 `role="navigation"` + `aria-label`
- 终端使用 `role="terminal"` + `aria-label`

**高对比度**：
- `@media (prefers-contrast: high)` 自动增强边框（`border-width: 2px`）和文字对比度
- 确保所有文字与背景的对比度 ≥ 4.5:1（WCAG AA 标准）

### 交互模式

#### 侧边栏

- **折叠/展开**：Cmd+B 快捷键，状态持久化到 localStorage
- **拖拽调整宽度**：拖拽条使用 `requestAnimationFrame` 平滑更新，钳制 190-420px
- **模块拖拽排序**：`draggable="true"`，设置 `text/plain` MIME 类型
- **模块图标悬停**：`transition: 0.12s`，显示模块名称 tooltip
- **新增**：文件快捷入口、收藏夹列表、Agent 项目列表

#### 文件浏览器（新增）

- **视图切换**：网格视图（缩略图卡片）/ 列表视图（详细行）
- **缩略图大小**：小/中/大三档
- **排序**：名称、修改时间、大小
- **隐藏文件**：切换显示/隐藏
- **面包屑导航**：返回/上级按钮
- **项目徽章**：node、web、python、rust、go、git
- **文件类型检测**：文本、图片、视频、音频、PDF、压缩包、其他（40+ 文本扩展名）
- **中文排序**：`localeCompare` + `numeric: true`
- **拖放导入**：从 Finder 拖入文件

#### 文件预览（新增）

- **Markdown**：Milkdown Crepe WYSIWYG 编辑 + 自动保存（0.8s 延迟）
- **HTML**：沙箱 iframe 实时渲染（独立端口 + 独立 origin），双缓冲零闪白
- **代码**：highlight.js 语法高亮（跟随主题）
- **图片**：内联显示，透明棋盘格背景，HEIC/HEIF 支持
- **视频/音频**：内联播放器
- **PDF**：内联查看器
- **压缩包**：内容列表（文件名 + 大小）
- **大文件**：前 256KB + "...(file too large)" 提示
- **全屏预览**：一键填满窗口，Esc 退出
- **可调整大小**：拖拽分割线

#### 终端

- **展开/折叠**：动画 0.22s ease
- **最大化**：`.term-max` 类隐藏文件区域，终端填满全部空间
- **拖放文件**：`.term-drop` 类显示虚线边框覆盖层（`2px dashed var(--accent)`），`dropIn 0.16s`
- **文字飞入**：选中文字创建 `.fling-ghost` 元素，`0.55s cubic-bezier(0.45,0,0.2,1)` 飞向终端中心
- **终端捕获闪光**：`.term-catch` 动画 500ms 内发光
- **新增**：跟随模式、可点击路径/链接、Agent 状态点、呼吸动画

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
| 右侧面板 | 20 | 工坊/设置/预览 |
| 终端 | 30 | 底部终端 |
| 对话框覆盖层 | 60 | 模态对话框 |
| 右键菜单 | 70 | 上下文菜单 |
| 命令面板覆盖层 | 100 | Cmd+K |
| 截图快递卡片 | 150 | 浮动截图卡片 |
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
- **E2E 测试**：关键用户流程（安装模块、打开模块、使用终端、文件浏览）使用 Playwright E2E 测试。

### 需要测试的模块

| 模块 | 测试类型 | 优先级 |
|------|----------|--------|
| M2 Database | 单元测试（CRUD、迁移、并发） | P0 |
| M3 HTTP Server | 单元测试（路由、认证、CSP、Host/Origin 验证） | P0 |
| M4 Bridge System | 单元测试（权限检查、Token 验证） + 集成测试（postMessage + HTTP） | P0 |
| M6 Module Manager | 单元测试（manifest 校验、安装/卸载） | P0 |
| M8 Environment Injector | 单元测试（加密/解密、注入） | P0 |
| M14 File Manager | 单元测试（CRUD、原子写入、搜索评分、Git） | P0 |
| M15 Agent Integration | 单元测试（会话扫描、Skills 管理、用量解析） | P1 |
| M10 Error Classifier | 单元测试（错误分类、用户提示） | P1 |
| M11 Config Manager | 单元测试（原子写入、并发安全） | P1 |
| M7 iframe Manager | 集成测试（生命周期、LRU） | P1 |
| M5 Terminal | 集成测试（PTY 创建、resize、环境注入、跟随模式） | P1 |
| M16 Screenshot | 单元测试（截图识别、标注保存） | P2 |
| M17 Release Wizard | 单元测试（版本递增、CHANGELOG） | P2 |
| M18 Update Checker | 单元测试（版本比较、通知） | P2 |
| M12 Frontend Shell | 快照测试 + E2E 测试 | P2 |
| M9 Theme Engine | 单元测试（Zod 校验、CSS 变量注入） | P2 |

### 参考先例

- **CodePilot**: Playwright E2E + unit tests (tsx --test) + visual regression
- **FanBox**: 无测试基础设施

---

## Out of Scope

以下功能**不在 v2.0 范围内**：

1. **在线模块分发**：云端模块商店、在线更新、版本管理。v2.0 仅支持本地目录扫描。
2. **远程 URL 模块**：加载远程网页作为插件。v2.0 仅支持本地文件。
3. **MCP Server 模块**：MCP 协议集成。v2.0 仅支持 Web 页面模块。
4. **Electron 原生模块**：原生 Node.js 模块作为插件。
5. **自动更新**：应用自身的自动更新机制（electron-updater）。
6. **全局键盘快捷键体系**：仅保留基础快捷键（Cmd+K 搜索、Cmd+B 侧边栏），完整快捷键体系后期设计。
7. **窗口状态持久化**：面板宽度、折叠状态等 UI 状态的完整持久化。
8. **环境变量诊断工具**（env-doctor）：验证 API Key 有效性的诊断工具。
9. **跨平台打包**：Windows NSIS、Linux AppImage 打包。v2.0 仅支持 macOS DMG。
10. **组件级插件**：插件作为 UI 组件嵌入基座页面。v2.0 仅支持页面级插件。
11. **多窗口**：多个 BrowserWindow 并行。v2.0 仅支持单窗口（数据库单写入者模型天然适合）。
12. **Editorial Index 主题**：已删除，Warm Archive 已重命名为 Frosted Jasmine。仅保留 Terminal Volt 和 Frosted Jasmine。

---

## Further Notes

### 文件结构

```
Natives/
├── docs/
│   ├── architecture/
│   │   ├── ARCHITECTURE.md          # 架构规范
│   │   └── DESIGN_DISCUSSION.md     # 设计讨论记录（43+ 个决策）
│   └── PRD.md                       # 本文档
├── electron/
│   ├── main.ts                      # Electron 入口
│   └── preload.ts                   # IPC 隔离桥
├── src/
│   ├── main/                        # Main Process 模块
│   │   ├── database.ts              # SQLite (9 张表, WAL, 增量迁移)
│   │   ├── http-server.ts           # 本地 HTTP 服务（含文件管理 API）
│   │   ├── module-manager.ts        # 模块生命周期
│   │   ├── bridge-host.ts           # Bridge API 宿主
│   │   ├── shell.ts                 # Shell 管理 (node-pty) + 终端增强
│   │   ├── env-injector.ts          # 环境注入
│   │   ├── file-manager.ts          # 文件管理（新增）
│   │   ├── agent-integration.ts     # Agent 集成（新增）
│   │   ├── screenshot.ts            # 截图快递（新增）
│   │   ├── release-wizard.ts        # 发布向导（新增）
│   │   ├── update-checker.ts        # 更新检查（新增）
│   │   ├── subprocess.ts            # 子进程管理
│   │   └── watchdog.ts              # 进程守护
│   ├── app/                         # Next.js 页面
│   │   ├── page.tsx                 # 仪表盘
│   │   ├── layout.tsx               # Root Layout
│   │   ├── globals.css              # 全局样式（2 套主题）
│   │   ├── files/page.tsx           # 文件浏览器（新增）
│   │   ├── store/page.tsx           # 应用商店
│   │   ├── workshop/page.tsx        # 创意工坊
│   │   ├── modules/page.tsx         # 模块管理
│   │   └── settings/page.tsx        # 设置
│   ├── components/
│   │   ├── shell/
│   │   │   ├── Sidebar.tsx          # 左侧边栏（含文件快捷入口）
│   │   │   ├── ContentArea.tsx      # 中间主内容区
│   │   │   ├── RightPanel.tsx       # 右侧面板（含文件预览）
│   │   │   └── Terminal.tsx         # 底部终端（增强版）
│   │   ├── iframe/
│   │   │   └── IframeContainer.tsx
│   │   ├── files/                   # 文件管理组件（新增）
│   │   │   ├── FileBrowser.tsx
│   │   │   ├── FilePreview.tsx
│   │   │   ├── FileSearch.tsx
│   │   │   ├── GitPanel.tsx
│   │   │   └── DiskUsage.tsx
│   │   ├── ai/                      # AI 工作台组件（新增）
│   │   │   ├── AgentMonitor.tsx
│   │   │   ├── ChangeInbox.tsx
│   │   │   ├── SessionReplay.tsx
│   │   │   ├── ProjectMemory.tsx
│   │   │   ├── SkillsPanel.tsx
│   │   │   ├── UsagePanel.tsx
│   │   │   ├── RtkPanel.tsx
│   │   │   └── FileOrganizer.tsx
│   │   ├── tools/                   # 工具组件（新增）
│   │   │   ├── ReleaseWizard.tsx
│   │   │   ├── ScreenshotExpress.tsx
│   │   │   └── ImageAnnotator.tsx
│   │   └── common/
│   │       └── UpdateNotification.tsx
│   ├── lib/
│   │   ├── bridge-sdk.js            # Bridge SDK (注入到 iframe)
│   │   ├── iframe-manager.ts        # iframe 生命周期 (LRU, heartbeat)
│   │   ├── module-store.ts          # 模块状态管理
│   │   ├── theme-engine.ts          # 主题引擎（2 套主题）
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
| @xterm/xterm | 终端渲染 | WebGL + unicode11 |
| @xterm/addon-fit | 终端适配 | |
| zod | 配置校验 | manifest + 主题 + 设置 |
| lucide-react | 图标 | |
| adm-zip | ZIP 解压 | 模块安装 |
| milkdown | Markdown 编辑器 | WYSIWYG 预览 |
| monaco-editor | 代码编辑器 | Diff 预览 |
| marked | Markdown 渲染 | 只读预览 |
| highlight.js | 代码高亮 | 语法高亮 |

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
├── thumbs/                          # 缩略图缓存（新增，400MB 上限）
├── organize-log/                    # AI 文件整理日志（新增）
├── organize-prefs.md                # AI 文件整理偏好（新增）
└── logs/                            # 运行日志
```

---

## 实施阶段

### 第一阶段：文件管理（优先）

| 模块 | 工作量估算 | 依赖 |
|------|-----------|------|
| M14 File Manager | 2 周 | M3 (HTTP Server) |
| 文件浏览器组件 | 1 周 | M14 |
| 文件预览组件 | 2 周 | M14, milkdown, monaco, marked, highlight.js |
| 文件搜索 | 1 周 | M14 |
| Git 集成 | 3 天 | M14 |
| 磁盘用量 | 2 天 | M14 |
| 剪贴板集成 | 2 天 | Electron API |
| 安全加固（Host/Origin 验证） | 2 天 | M3 |

### 第二阶段：AI 工作台

| 模块 | 工作量估算 | 依赖 |
|------|-----------|------|
| 终端增强 | 1 周 | M5 |
| M15 Agent Integration | 2 周 | M5, 文件系统 |
| Agent 变更监控组件 | 1 周 | M15, fs.watch |
| 项目记忆组件 | 3 天 | M15 |
| Skills X-Ray 组件 | 3 天 | M15 |
| Agent 用量追踪组件 | 3 天 | M15 |
| RTK 用量分析组件 | 2 天 | M15 |

### 第三阶段：辅助功能

| 模块 | 工作量估算 | 依赖 |
|------|-----------|------|
| AI 文件整理 | 1 周 | M15, M14 |
| M17 Release Wizard | 3 天 | M14, git |
| M16 Screenshot & Annotation | 1 周 | Electron API, Canvas |
| M18 Update Checker | 3 天 | GitHub API |
| 主题精简（删除 Editorial Index + Warm Archive 更名磨砂茉莉） | 1 天 | M9 |

### 总工作量估算

| 阶段 | 工作量 |
|------|--------|
| 第一阶段 | ~5 周 |
| 第二阶段 | ~4 周 |
| 第三阶段 | ~3 周 |
| **总计** | **~12 周** |
