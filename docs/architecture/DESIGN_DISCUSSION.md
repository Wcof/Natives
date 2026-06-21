# Natives 架构设计讨论记录

> **讨论时间**: 2026-06-14
> **参与方**: 产品方 (用户) + AI 架构师
> **讨论方式**: Grill-with-doc 互动问答式对齐

---

## 讨论背景

用户希望在根目录下的 `Natives` 进行架构设计，包含三层架构：
1. **整体架构** — 产品定位 + 模块分层 + 数据流
2. **底层架构** — 进程/安全/DB/IPC 等基础设施
3. **前端架构** — UI 渲染/插件加载/主题系统（支持类似 web 页面渲染的热拔插）

---

## 第一轮: 核心架构对齐 (Q1-Q12)

### Q1: "Steam 思维" 技术架构的核心定位

**问题**: Natives 作为一个基座容器（类似 Steam 客户端），用户可以「订阅/安装」不同的应用模块（类似 Steam 游戏）。核心模型是哪种？

**选项**:
- A. App Store 模型 — 用户浏览/订阅应用，点击安装后在 Natives 内运行
- B. Steam + 创意工坊模型 — 除了官方应用，用户还可以通过创意工坊自己开发/分享模块
- C. 操作系统模型 — Natives 更像一个轻量 OS，所有应用都是「进程」
- D. Hub 聚合模型 — 统一入口，通过 webview/iframe 嵌入各种已有的 Web 应用

**决策**: ✅ **以 D (Hub 聚合) 为基础，融合 A、B、C 的特点**

**推理**: 用户的愿景不是简单的某一种模型，而是一个全面的生态基座：
- 具备 Hub 的聚合能力（统一入口、统一认证和环境管理）
- 具备 App Store 的应用浏览和订阅能力
- 具备创意工坊的开发者生态（用户可以自己开发/分享模块）
- 具备 OS 级别的进程管理和资源共享能力

---

### Q2: 前端架构 —「热拔插」的具体含义

**问题**: 前端架构要支持类似 web 页面渲染的「热拔插」，插件粒度和渲染机制是什么？

**选项**:
- A. 页面级插件 — 每个插件是一个完整的页面/SPA，通过 webview 或 iframe 加载
- B. 组件级插件 — 插件可以是一个 UI 组件（如面板、widget），嵌入到基座页面中
- C. 混合模式 — 基座提供骨架页面，用户在骨架的「插槽」中动态插入 AI 生成的页面或组件
- D. AI 全自动生成 — 基座只提供最小容器，所有 UI 都由 AI 实时生成

**决策**: ✅ **A. 页面级插件**

**推理**: 页面级插件是最清晰的隔离边界：
- 每个插件是一个完整的 HTML/JS/CSS 页面，通过 iframe 加载（Q17 修正）
- 只支持本地文件（Q25 修正）
- 基座提供导航和通信框架
- 插件之间天然隔离，不会互相干扰
- 开发者的开发体验最接近标准 Web 开发

---

### Q3: 两种技术架构之间的关系

**问题**: 原始需求提到两种架构（Steam 思维技术架构 + 前端架构），它们之间的关系是？

**原始回答**: "我说错了，应该是三层架构：整体架构、底层架构以及前端架构。"

**决策**: ✅ **三层架构体系**

| 层次 | 职责 | 对应开发领域 |
|------|------|-------------|
| 整体架构 | 产品定位 + 模块分层 + 数据流 | 系统设计、产品规划 |
| 底层架构 | 进程/安全/DB/IPC 等基础设施 | Electron Main Process |
| 前端架构 | UI 渲染/插件加载/主题系统 | Renderer Process (Next.js) |

**推理**: 三层关系是自上而下的依赖关系：
- 整体架构定义「做什么」和「怎么分层」
- 底层架构实现「基础设施」
- 前端架构基于底层能力构建「用户体验」

---

### Q4: 现有项目 CodePilot 和 Natives2 的角色

**问题**: 在 Natives 架构中，CodePilot 和 Natives2 扮演什么角色？

**选项**:
- A. 作为内置应用 — 预装应用，开箱即用，但也可以卸载
- B. 作为参考实现 — 设计参考，Natives 会吸收其设计模式
- C. 作为嵌入式服务 — 直接通过 webview/子进程嵌入运行
- D. 作为可订阅模块 — 和其他第三方应用一样，是商店中可订阅的模块

**决策**: ✅ **B. 作为参考实现**

**推理**: 不直接嵌入 CodePilot 和 Natives2，而是吸收它们的设计模式：
- 从 Natives2 吸收：零依赖后端哲学、单文件 SPA 模式、双缓冲预览
- 从 CodePilot 吸收：Provider 抽象、SSE 流式通信、SQLite 数据模型、i18n 模式
- 这样 Natives 可以独立演进，不受现有项目的技术债务影响

---

### Q5: 技术栈偏好

**问题**: Natives 的技术栈倾向于哪种？

**选项**:
- A. Electron + Next.js + TypeScript — 和 CodePilot 一致
- B. Electron + Vite + React + TypeScript — 更轻量
- C. Electron + Vanilla JS — 和 Natives2 一致
- D. Tauri + Web 技术 — 用 Rust 替代 Electron

**决策**: ✅ **A. Electron + Next.js + TypeScript**

**推理**: 最大化代码复用和团队经验：
- 团队已有 CodePilot 项目的 Electron + Next.js 经验
- TypeScript 提供类型安全
- Next.js 的 App Router 可以用于内置功能页面的路由
- `output: 'standalone'` 模式已在 CodePilot 验证过可用于 Electron 打包

---

### Q6: 三层架构的边界

**问题**: 确认对每层的理解。

**决策**: ✅ **选项 A**
- **整体架构** = 产品定位 + 模块分层 + 数据流
- **底层架构** = 进程/安全/DB/IPC 等基础设施
- **前端架构** = UI 渲染/插件加载/主题系统

---

### Q7: 创意工坊中可订阅的模块类型

**问题**: 在「创意工坊」生态中，用户可以开发/订阅的模块类型包括哪些？

**决策**: ✅ **B. Web 页面模块**

**详细说明**:
- 模块形态为一个独立的 HTML/JS/CSS 页面
- 通过 iframe + 本地 HTTP 服务加载（Q17 修正）
- 只支持本地文件（Q25 修正）
- 不包括 MCP Server、Electron 原生模块、AI Agent 等复杂类型（MVP 阶段）

---

### Q8:「支持 AI 进行设计」的具体含义

**问题**:「其他功能可自己通过 AI 进行设计」具体指什么？

**原始回答**: "开发者自己开发页面和通过，之后挂载在应用中。（我们应用提供较好的本地开放 API，让他可以获取数据以及请求）"

**决策**: ✅ **开发者 AI 辅助开发模式**

**推理**: 核心思路是：
1. Natives 基座提供丰富的 Bridge API（`window.natives.*`）
2. 开发者（可以借助 AI 工具如 Cursor、Claude 等）编写插件页面
3. 插件通过 Bridge API 获取数据、调用系统能力
4. 最终交付的是静态代码包（HTML/JS/CSS + manifest.json）
5. 基座不负责 AI 实时生成 UI，而是为开发者提供足够好的 API 让开发变得简单

---

### Q9: 模块分发机制

**问题**: 创意工坊的模块分发机制倾向于哪种？

**选项**:
- A. 在线模式 — 模块数据存在云端
- B. 离线模式 — 模块下载到本地
- C. 混合模式 — 安装到本地，支持在线更新
- D. 先离线，后期考虑在线

**决策**: ✅ **D. 先离线，后期考虑在线**

**推理**: MVP 策略：
- 先实现本地模块加载（目录扫描 `~/.natives/modules/`）
- 云端分发、在线更新等功能后期再做
- 降低 MVP 复杂度，先跑通核心流程

---

### Q10: 基座页面的核心布局

**问题**: 基座提供的「基础页面」布局设计？

**决策**: ✅ **三栏布局**

```
┌────────┬─────────────────────────┬───────────────┐
│  订阅   │     主内容区 (iframe)    │  工坊/设置面板  │
│  侧边栏 │                         │               │
├────────┴─────────────────────────┴───────────────┤
│                 可折叠终端 (xterm.js)              │
└──────────────────────────────────────────────────┘
```

- 左侧：订阅侧边栏（可折叠）
- 中间：主内容区，iframe 容器（本地 HTTP 服务提供文件）
- 右侧：工坊/设置面板（可折叠）
- 底部：可折叠终端

---

### Q11: 基座必须内置的功能

**问题**: 除了导航和 iframe 容器，基座本身必须内置哪些功能？

**决策**: ✅ **全部都需要**

| 功能 | 优先级 | 说明 |
|------|--------|------|
| 内置终端 | P0 | xterm.js，可运行 CLI 工具和 AI Agent |
| 应用商店/创意工坊浏览 | P0 | 浏览和订阅模块的 UI |
| 设置面板 | P0 | 主题、API Key、环境变量等配置 |
| 模块管理 | P0 | 已安装模块的启用/禁用/卸载/更新 |
| 全局搜索 | P1 | 跨模块搜索功能（Cmd+K） |
| 通知中心 | P1 | 模块消息和事件通知 |

---

### Q12: 插件与基座的通信机制

**问题**: 插件（iframe 中的页面）如何与基座通信？

**选项**:
- A. Bridge API — 通过 Electron preload 暴露 JS API（window.natives.xxx）
- B. PostMessage 协议 — iframe 通过 window.postMessage 通信
- C. HTTP 本地服务 — 基座启动本地 HTTP 服务，插件通过 REST API 调用
- D. 混合模式 — 同时提供 Bridge API 和 HTTP API

**决策**: ✅ **D. 混合模式**（Q23 修正：postMessage + HTTP API）

**推理**:
- 通过 `window.natives.*` 暴露统一的 JS API
- 插件页面通过这些 API 读写数据、发通知等
- 轻量操作用 postMessage（实时事件），重操作用 HTTP API
- Bridge SDK（`natives-sdk.js`）封装底层通信，插件无需关心细节
- 参考 Q23 的详细设计

---

## 第二轮: 实现细节对齐 (Q13-Q25)

> 本轮讨论基于第一轮的决策，深入到实现层面的不确定点。部分决策修正了第一轮的方案（特别是 webview → iframe 的转变）。

### Q13: Bridge API 权限分级策略

**问题**: 插件通过 `window.natives.*` 调用系统能力，涉及数据读写、环境变量、通知等。如果不做权限控制，恶意插件可能读取其他插件数据或窃取 API Key。MVP 阶段怎么处理？

**选项**:
- A. MVP 先放开 — 所有插件默认拥有全部权限，后期再加
- B. 从一开始就引入权限 — manifest.json 中声明所需权限，安装时用户确认授权
- C. 声明但不拦截 — manifest 中必须声明权限（培养规范），但实际不做拦截

**决策**: ✅ **B. 从一开始就引入权限**

**推理**: 参考 CodePilot 的安全设计模式：
- manifest.json 中必须声明所需权限
- 安装时用户确认授权
- 未授权的 API 调用会被拒绝并返回 PermissionDenied 错误
- 权限粒度：`db:read`、`db:write`、`env:read`、`notification`、`ipc:send`、`shell:exec`

---

### Q14: 插件包格式

**问题**: 插件的安装/分发格式，影响开发者体验和用户安装流程。

**选项**:
- A. 纯目录结构 — 插件就是一个文件夹，复制到 `~/.natives/modules/` 即可
- B. ZIP 压缩包 — 插件以 `.zip` 文件分发，安装时自动解压
- C. 两者都支持 — 可以拖入目录也可以拖入 .zip

**决策**: ✅ **C. 两者都支持**

**推理**:
- 开发阶段：开发者直接将目录放到 modules 文件夹中，方便调试
- 分发阶段：打包成 .zip 方便传输和分享
- 基座自动识别：拖入 .zip 时自动解压到 modules 目录，拖入目录时直接注册
- 安装后统一为目录结构存储

---

### Q15: 基座首页内容

**问题**: 用户打开 Natives 后，默认看到什么？这决定了第一印象和核心使用路径。

**选项**:
- A. 最近使用列表 — 展示最近打开过的模块
- B. 应用商店浏览 — 默认展示可安装模块的商店页
- C. 仪表盘 — 展示系统状态、已安装模块概览、快捷操作
- D. 空白引导页 — 新用户看到引导教程，老用户看到上次打开的模块

**决策**: ✅ **C. 仪表盘**

**推理**: 仪表盘作为默认首页的优势：
- 展示系统状态（已安装模块数、终端会话数等）
- 已安装模块的网格概览，一键打开
- 快捷操作（打开终端、进入设置、浏览商店）
- 给用户一个「控制中心」的感觉，符合 Steam 客户端的体验

---

### Q16: 多语言方案

**问题**: 基座的国际化方案。

**选项**:
- A. 手写 en.ts + zh.ts — 和 CodePilot 保持一致
- B. i18next — 成熟的 i18n 框架
- C. 先用简单方案，后期迁移

**决策**: ✅ **A. 手写 en.ts + zh.ts**

**推理**: 和 CodePilot 保持一致：
- 简单直接，无额外依赖
- 团队已有 CodePilot 的 i18n 维护经验
- MVP 阶段两种语言足够
- 缺点（缺少复数/日期格式化）在桌面应用场景下影响不大

---

### Q17: Webview 实现技术选型 ⚠️ 架构修正

**问题**: Electron 官方已将 `<webview>` 标记为 experimental，并建议使用 `WebContentsView` 替代。但 `WebContentsView` 的布局管理更复杂。倾向于哪种实现？

**选项**:
- A. Electron `<webview>` 标签 — 内置 webview，天然沙箱隔离，但已标记 experimental
- B. BrowserView / WebContentsView — Electron 推荐方案，更稳定但 API 更底层
- C. iframe — 标准 Web 技术，sandbox 属性提供安全隔离，但无法加载 `file://` 协议
- D. 混合: iframe + 本地 HTTP — 基座启动本地 HTTP 服务为插件提供文件，避开 webview 实验性问题

**决策**: ✅ **D. 混合: iframe + 本地 HTTP**

> [!WARNING]
> **架构修正**: 此决策修改了 Q2 和 Q12 的部分方案。插件不再通过 Electron `<webview>` 加载，而是通过 iframe + 本地 HTTP 服务加载。Bridge API 的实现方式也相应变化（从 `executeJavaScript` 注入改为 postMessage + HTTP API 混合）。

**推理**:
- 避开 Electron `<webview>` 的 experimental 风险
- iframe 是标准 Web 技术，行为稳定可预测
- 本地 HTTP 服务解决 iframe 无法加载 `file://` 的问题
- HTTP 服务本身也可以提供 Bridge API 的 REST 端点
- 参考 Natives2 的 `server.js` 模式（零依赖 HTTP 服务），已验证可行

---

### Q18: 模块发现机制

**问题**: 本地模块的发现和加载时机，影响「热插拔」体验。

**选项**:
- A. 启动时扫描 — 启动时扫描 `~/.natives/modules/`，运行中不监听变化
- B. 实时监听 (fs.watch) — 监听目录变化，新加/删除模块时自动更新侧边栏
- C. 手动触发 — 用户在模块管理页点击「刷新」或「安装本地模块」按钮
- D. 启动扫描 + 手动刷新 — 启动时自动扫描一次，之后用户可以手动触发

**决策**: ✅ **D. 启动扫描 + 手动刷新**

**推理**:
- 启动时自动扫描确保模块列表最新
- 手动刷新避免 fs.watch 的跨平台稳定性问题
- MVP 阶段简单可靠
- 后期如需实时监听可以作为增强功能加入

---

### Q19: 多 Webview/iframe 内存管理

**问题**: 每个 iframe 虽然比 webview 轻量，但打开多个插件仍会占用可观内存。当用户打开多个插件时，如何管理内存？

**选项**:
- A. 无限制 — 用户打开多少就加载多少
- B. 最多保持 5 个后台 — 超过时自动销毁最早打开的
- C. LRU 策略 + 内存监控 — 监控系统内存，压力大时自动回收最久未使用的
- D. 切换时销毁 — 同时只保持当前活动的 1 个

**决策**: ✅ **C. LRU 策略 + 内存监控**

**推理**:
- 结合智能回收策略，兼顾用户体验和系统稳定
- 正常情况下保留用户打开的 iframe（保持状态）
- 当系统内存压力大时（可通过 `process.memoryUsage()` 和 `os.freemem()` 监控），自动回收最久未使用的 iframe
- 被回收的 iframe 在用户重新切换时自动重新加载
- 设置中可配置最大保持数量（默认 10）

---

### Q20: 插件崩溃处理

**问题**: 插件运行在独立 iframe 中，可能因为 JS 错误、内存溢出、无限循环等原因崩溃。如何处理？

**选项**:
- A. 静默恢复 — 显示错误页面和「重新加载」按钮
- B. 自动重载 — 自动重新加载，最多重试 3 次
- C. 崩溃报告 — 静默恢复 + 在通知中心显示崩溃信息

**决策**: ✅ **C. 崩溃报告**

**推理**:
- iframe 内的 JS 错误不会导致基座崩溃（天然隔离）
- 捕获 iframe 的 `error` 和 `unhandledrejection` 事件
- 在主内容区显示友好的错误页面 + 「重新加载」按钮
- 同时在通知中心记录崩溃信息（错误堆栈、模块 ID、时间）
- 用户可以将崩溃信息复制/报告给插件开发者

---

### Q21: 插件开发者体验

**问题**: 如果要让用户/开发者可以「通过 AI 自己设计」插件，开发体验很重要。倾向于哪种开发工具支持？

**选项**:
- A. 内置开发者工具 — 基座内置插件开发工具（创建模板 + DevTools + Bridge API 调试）
- B. 纯 CLI 工具 — 提供 `natives-cli create-plugin` 命令行
- C. 模板 + 文档即可 — 只提供 plugin-template 目录和 SDK 类型声明
- D. 创意工坊页集成 — 创意工坊页面中集成「新建模块」引导流程

**决策**: ✅ **C. 模板 + 文档即可**

**推理**:
- MVP 阶段保持简单：提供 `plugin-template/` 目录和 `natives-sdk.d.ts` 类型声明
- 开发者用自己的工具（VS Code、Cursor 等）+ AI 辅助开发
- 基座的终端可以用于测试和调试
- 后期可以逐步增加 DevTools 快捷键、Bridge API 调试面板等

---

### Q22: 本地 HTTP 服务设计

**问题**: 选择了 iframe + 本地 HTTP 后，HTTP 服务的架构怎么设计？

**选项**:
- A. 单一静态文件服务 — 一个 HTTP 服务统一服务所有插件，路径按 `/modules/{moduleId}/...` 组织，同时提供 Bridge API 的 REST 端点
- B. 每个插件一个服务 — 每个插件启动独立 HTTP 服务（独立端口）

**决策**: ✅ **A. 单一静态文件服务 + API 路由** (架构师最优判断)

**推理**:
- 参考 Natives2 的 `server.js` 模式：单一 HTTP 服务，零外部依赖
- 静态文件路由：`GET /modules/{moduleId}/{path}` → 从 `~/.natives/modules/{moduleId}/` 读取文件
- Bridge API 路由：`POST /api/bridge/{namespace}/{method}` → 转发到 Main Process 处理
- 避免多端口管理的复杂性
- 单一服务便于统一的安全策略（认证 Token、CORS、CSP 头）

**服务路由设计**:
```
GET  /modules/{moduleId}/{path}      → 静态文件服务
POST /api/bridge/db/get              → 数据读取
POST /api/bridge/db/set              → 数据写入
POST /api/bridge/db/delete           → 数据删除
POST /api/bridge/db/list             → 数据列举
GET  /api/bridge/settings/theme      → 获取主题
GET  /api/bridge/settings/locale     → 获取语言
POST /api/bridge/env/get             → 获取环境变量 (需权限)
POST /api/bridge/notification/send   → 发送通知 (需权限)
GET  /api/bridge/meta                → 获取模块元信息
```

---

### Q23: iframe 环境下的 Bridge API 实现方式

**问题**: 原方案是通过 webview `executeJavaScript` 注入 Bridge API，但现在用 iframe，这个方式不再可用。iframe 环境下 Bridge API 如何实现？

**选项**:
- A. postMessage — iframe 通过 `window.parent.postMessage()` 与基座通信
- B. HTTP API — 插件通过 `fetch('http://localhost:端口/api/bridge/...')` 调用
- C. 混合: postMessage + HTTP — 轻量操作用 postMessage，重操作用 HTTP API

**决策**: ✅ **C. 混合: postMessage + HTTP**

**推理**:
- **postMessage 通道** (轻量、实时)：
  - 主题变更通知 (`theme-changed`)
  - 导航事件 (`navigate`)
  - 模块间通信转发 (`ipc:message`)
  - 侧边栏 badge 更新
  - 生命周期事件 (`activate`、`deactivate`、`destroy`)
- **HTTP API 通道** (重操作、请求/响应)：
  - 数据读写 (`db.get/set/delete/list`)
  - 环境变量获取 (`env.get`)
  - 通知发送 (`notification.send`)
  - 元信息查询 (`meta`)

**Bridge SDK 封装**: 提供 `natives-sdk.js` 文件，插件只需 `<script src="/natives-sdk.js">` 引入，即可使用统一的 `window.natives.*` API。SDK 内部自动选择 postMessage 或 HTTP 通道。

---

### Q24: 插件之间的隔离策略

**问题**: 用 iframe + 本地 HTTP 后，插件之间的隔离不像 webview 那样是进程级的。如何确保插件之间不会互相干扰？

**选项**:
- A. 同域访问 — 全部走同一端口，iframe 可自由访问父窗口（隔离性弱）
- B. 不同端口隔离 — 基座和每个插件使用不同端口
- C. sandbox 属性隔离 — 同端口同域，iframe 加 sandbox 限制 + CSP 头
- D. 路径前缀隔离 — 同端口，不同路径前缀，配合 sandbox 和 postMessage 鉴权

**决策**: ✅ **D. 路径前缀隔离 + sandbox + 鉴权** (架构师最优判断)

**推理**:
- 同端口、不同路径前缀（`/modules/pluginA/`、`/modules/pluginB/`）
- iframe 添加 `sandbox="allow-scripts allow-forms"` 属性：
  - 禁止 `allow-same-origin`：插件无法访问父窗口的 DOM 和 cookie
  - 禁止 `allow-top-navigation`：插件无法导航父窗口
  - 允许 `allow-scripts`：插件 JS 正常运行
  - 允许 `allow-forms`：插件表单正常提交
- HTTP API 调用时携带 Session Token（基座启动时生成，通过 postMessage 下发给 iframe）
- Bridge API 服务端验证 Token + moduleId，确保插件只能访问自己的数据

> [!IMPORTANT]
> **关键安全设计**: `sandbox` 不含 `allow-same-origin` 意味着插件无法通过 JS 访问父窗口或其他 iframe 的内容。这是隔离的核心。同时 `sandbox` 无 `allow-same-origin` 时 `document.cookie` 和 `localStorage` 也会被隔离到独立的 origin。

---

### Q25: 远程 URL 支持

**问题**: Q2 中提到插件「可以是本地文件或远程 URL」。但远程 URL 加载涉及跨域、安全，并且无法使用 Bridge API。怎么处理？

**选项**:
- A. 不需要远程 URL，只支持本地
- B. 支持远程 URL，但单独标记
- C. 远程 URL 作为单独的「书签」功能

**决策**: ✅ **A. 不需要远程 URL，只支持本地**

> [!NOTE]
> **修正 Q2 决策**: 原 Q2 中提到「可以是本地文件或远程 URL」，此处明确为 **只支持本地文件**。远程 URL 支持可作为后期功能考虑。

**推理**:
- 最安全：不引入跨域和网络安全问题
- 简化架构：不需要处理远程 URL 的离线回退、加载超时等边界情况
- 所有插件都是本地文件，统一走本地 HTTP 服务
- 如果用户需要嵌入远程网页，可以作为未来的独立功能（「网页书签」）单独设计

---

## 决策汇总

### 第一轮决策 (Q1-Q12)

| # | 决策项 | 决策结果 |
|---|--------|----------|
| 1 | 核心模型 | Hub 聚合 + App Store + 创意工坊 + OS 级管理 |
| 2 | 插件粒度 | 页面级插件（独立 HTML/JS/CSS，iframe 加载，仅本地文件） |
| 3 | 架构分层 | 三层：整体架构 → 底层架构 → 前端架构 |
| 4 | CodePilot/Natives2 角色 | 参考实现（吸收设计模式，不直接嵌入） |
| 5 | 技术栈 | Electron + Next.js + TypeScript |
| 6 | 三层边界 | 产品定位+分层 / 进程+安全+DB / UI+插件+主题 |
| 7 | 模块类型 | Web 页面模块（MVP 阶段） |
| 8 | AI 设计含义 | 开发者 AI 辅助开发，基座提供 Bridge API |
| 9 | 分发机制 | 先离线（本地目录），后期考虑在线 |
| 10 | 布局设计 | 三栏 + 底部终端 |
| 11 | 内置功能 | 终端+商店+设置+模块管理+搜索+通知（全部需要） |
| 12 | 通信机制 | 混合模式：postMessage（实时）+ HTTP API（重操作），统一 window.natives.* 接口 |

### 第二轮决策 (Q13-Q25) — 含架构修正

| # | 决策项 | 决策结果 | 影响 |
|---|--------|----------|------|
| 13 | 权限分级 | 从一开始就引入权限声明 + 安装授权 | manifest 规范 |
| 14 | 插件包格式 | 目录 + ZIP 都支持 | 模块安装流程 |
| 15 | 首页内容 | 仪表盘（系统状态+模块概览+快捷操作） | 首页设计 |
| 16 | 多语言方案 | 手写 en.ts + zh.ts | i18n 实现 |
| 17 | **嵌入技术** | **iframe + 本地 HTTP** (修正: 不用 webview) | ⚠️ 底层架构重大修正 |
| 18 | 模块发现 | 启动扫描 + 手动刷新 | 模块管理 |
| 19 | 内存管理 | LRU 策略 + 内存监控 | iframe 生命周期 |
| 20 | 崩溃处理 | 静默恢复 + 通知中心崩溃报告 | 错误处理 |
| 21 | 开发者体验 | 模板 + 文档即可 (MVP) | plugin-template |
| 22 | HTTP 服务 | 单一服务: 静态文件 + API 路由 | 服务架构 |
| 23 | Bridge API | 混合: postMessage (实时) + HTTP (重操作) | API 实现 |
| 24 | 插件隔离 | 路径前缀 + iframe sandbox + Token 鉴权 | 安全模型 |
| 25 | 远程 URL | 不支持 (只本地文件) | 简化架构 |

### 架构修正记录

| 修正项 | 原方案 | 新方案 | 修正原因 |
|--------|--------|--------|----------|
| 插件容器 | Electron `<webview>` | iframe + 本地 HTTP | webview 被 Electron 标记为 experimental |
| Bridge 注入 | `executeJavaScript()` | postMessage + HTTP API | iframe 无法使用 executeJavaScript |
| 文件加载 | `file://` 协议 | `http://localhost/modules/...` | iframe 不支持 file:// |
| 进程隔离 | 每个 webview 独立进程 | iframe sandbox 隔离 | 从进程隔离降为沙箱隔离 |
| 远程 URL | 支持 | 不支持（只本地） | 简化安全模型 |

---

## 第三轮: 参考实现交叉比对 (Q26+)

> 本轮基于对 CodePilot 和 Natives2 实际实现的交叉比对，发现设计空白并补全。

### Q26: 终端实现方案 ⚠️ 修正防线 3

**问题**: 五大防线第 3 条决定用 `child_process.spawn` 实现终端（"零原生依赖"），但从 Natives2 参考实现来看，此方案不支持 TUI 交互（resize 无效、全屏程序渲染异常）。Natives 终端需要支持完整交互体验。

**决策**: ✅ **B. 完整终端 — 采用 node-pty**（参考 Natives2 方案）

**推理**:
- `better-sqlite3` 已经是 native 模块，"零原生依赖" 本身不成立
- node-pty 的额外编译成本几乎为零（Electron 项目已有 native 模块）
- 完整终端体验（resize、TUI 程序、光标定位）是基本可用性要求
- Natives2 已验证 node-pty + xterm 的方案可行

### Q27: 插件生命周期契约

**问题**: 插件是纯 HTML 页面，基座无法感知插件内部状态。这影响内存回收（Q19）、崩溃恢复（Q20）、模块间通信等功能的实现。

**决策**: ✅ **A. 定义标准生命周期事件**

**生命周期事件**:

| 事件 | 方向 | 触发时机 | 用途 |
|------|------|----------|------|
| `ready` | 插件 → 基座 | 插件 JS 执行完毕，可以接收消息 | 基座知道可以切换到此插件 |
| `unload` | 基座 → 插件 | 基座即将卸载 iframe（内存回收或用户关闭） | 插件保存状态、清理资源 |
| `error` | 插件 → 基座 | 插件内部未捕获错误 | 崩溃报告（Q20） |
| `heartbeat` | 插件 → 基座 | 定期（30s） | 基座检测插件是否还活着 |

**插件侧实现**（通过 `window.natives.*`）:
```javascript
// 插件代码中
window.natives.lifecycle.ready();           // 通知基座：我准备好了
window.natives.lifecycle.onUnload(async () => {   // 注册卸载回调
  await window.natives.db.set('state', JSON.stringify(state)); // sandbox 无 localStorage，用 db.set 持久化
});
window.natives.lifecycle.onHeartbeat(() => { // 响应心跳
  return { status: 'ok' };
});
```

**基座侧行为**:
- 插件未在 10s 内调用 `ready()` → 标记为加载超时，显示错误页
- `unload` 发出后 3s 内插件未响应 → 强制销毁 iframe
- 连续 3 次心跳无响应 → 标记为无响应，显示错误页
- `error` 事件 → 触发 Q20 崩溃报告流程

**修正防线 3**:

| 机制 | 原方案 | 新方案 |
|------|--------|--------|
| Shell 启动 | `child_process.spawn('/bin/zsh')` | `node-pty.spawn('/bin/zsh')` |
| 窗口调整 | ❌ resize 无效 | ✅ `pty.resize(cols, rows)` |
| TUI 程序 | ❌ 渲染异常 | ✅ 完整支持 |
| 前端渲染 | `@xterm/xterm`（不变） | `@xterm/xterm`（不变） |
| 降级策略 | 无 | node-pty 编译失败时降级到 child_process.spawn |

### Q28: JSON 配置文件崩溃安全

**问题**: `~/.natives/` 下的 JSON 配置文件（用户设置、主题配置等）在写入过程中如果应用崩溃，可能损坏为空文件或不完整 JSON。SQLite 有 WAL 模式保护，但 JSON 文件没有。

**决策**: ✅ **A. 采用原子写入**（参考 Natives2 的 `_cfgChain` 模式）

**实现策略**:
1. **原子写入**: 写入临时文件 → `fsync()` → `rename()` 覆盖原文件
2. **序列化队列**: Promise chain 串行化读-改-写操作，防止并发写入竞争
3. **错误恢复**: 队列中单次写入失败不影响后续操作

```
写入流程:
  config.json → 读取当前内容
  ↓
  mutator(cfg) → 修改内存中的配置
  ↓
  写入 config.json.tmp-<pid>-<timestamp>
  ↓
  fsync() → 确保数据落盘
  ↓
  rename() → 原子替换原文件
```

### Q29: 错误分类体系

**问题**: Natives 的错误来源多样（基座自身、插件、Bridge API、CLI 工具、模块安装），需要结构化的错误分类让用户看到可理解、可操作的提示。

**决策**: ✅ **A. 定义结构化错误分类体系**（参考 CodePilot 的 error-classifier 模式）

**错误类别**:

| 类别 | 来源 | 示例 | 用户提示 |
|------|------|------|----------|
| `PLUGIN_CRASH` | 插件 iframe | JS 未捕获错误、内存溢出 | "插件运行异常，点击重新加载" |
| `PLUGIN_TIMEOUT` | 插件 iframe | 未在 10s 内调用 ready() | "插件加载超时，请检查插件代码" |
| `BRIDGE_PERMISSION_DENIED` | Bridge API | 插件调用未授权的 API | "此插件未获得该权限" |
| `BRIDGE_INVALID_REQUEST` | Bridge API | 参数校验失败 | "请求参数无效" |
| `MODULE_INSTALL_FAILED` | 模块管理 | manifest 校验失败、文件损坏 | "模块安装失败：{原因}" |
| `MODULE_NOT_FOUND` | 模块管理 | 模块目录缺失 | "模块文件缺失，请重新安装" |
| `TERMINAL_SPAWN_FAILED` | 终端 | shell 路径无效、权限不足 | "终端启动失败" |
| `TERMINAL_CRASH` | 终端 | shell 进程异常退出 | "终端进程已退出" |
| `DB_ERROR` | SQLite | 写入失败、磁盘满 | "数据保存失败，请检查磁盘空间" |
| `CONFIG_CORRUPTED` | 配置文件 | JSON 解析失败 | "配置文件损坏，已恢复默认设置" |
| `NETWORK_ERROR` | 网络 | 在线模块加载失败 | "网络连接失败" |
| `UNKNOWN` | 其他 | 未匹配的错误 | "发生未知错误" |

**错误对象结构**:
```typescript
interface ClassifiedError {
  category: ErrorCategory;     // 错误类别
  userMessage: string;         // 用户可读的提示
  actionHint?: string;         // 操作建议
  retryable: boolean;          // 是否可重试
  rawMessage: string;          // 原始错误信息（调试用）
  moduleId?: string;           // 关联的模块 ID
}
```

### Q30: 环境注入 — 凭证管理

**问题**: 四大设计支柱之一 "环境注入 & Shell 沙箱" 缺少具体设计。用户在终端里跑 AI CLI 巜具需要 API Key，Natives 如何管理这些凭证？

**决策**: ✅ **C. 中间模式 — 多组环境变量配置**

**设计**:
- 用户在设置中创建多个 **环境配置**（如 "工作"、"个人"、"测试"）
- 每个配置是一组 key-value 环境变量（如 `ANTHROPIC_API_KEY=sk-xxx`）
- 终端启动时，用户选择注入哪组配置
- 凭证加密存储在 `~/.natives/env/credentials.json`（参考 CodePilot 的加密存储模式）

**数据模型**（扩展 settings 表或新增表）:
```sql
CREATE TABLE env_profiles (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,              -- 显示名称（如 "工作"）
  is_default INTEGER DEFAULT 0,    -- 是否默认注入
  created_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE env_variables (
  profile_id TEXT NOT NULL REFERENCES env_profiles(id) ON DELETE CASCADE,
  key TEXT NOT NULL,               -- 环境变量名（如 "ANTHROPIC_API_KEY"）
  value TEXT NOT NULL,             -- 加密存储的值
  PRIMARY KEY (profile_id, key)
);
```

**注入流程**:
```
终端启动
  ↓
读取默认或用户选择的 env_profile
  ↓
解密 env_variables
  ↓
合并到 node-pty 的 env 参数
  ↓
CLI 工具自动获得 API Key
```

**加密方案**: 使用 AES-256-GCM（`src-tauri/src/env_manager.rs`）。密钥存储在 SQLite settings 表中，密文格式为 `v2:<nonce>:<ciphertext>:<tag>`。原 Electron 方案使用 `electron.safeStorage.encryptString()` / `decryptString()`，已随 ADR-0008 迁移至 Rust 后端。

### Q31: SQLite 迁移策略

**问题**: 设计约束要求 "DB schema changes: Must include migration logic"，但没有定义具体策略。

**决策**: ✅ **A. 增量迁移**（参考 CodePilot 的 `PRAGMA table_info` 模式）

**实现策略**:
1. **启动时检查**: 每次启动用 `PRAGMA table_info(table_name)` 检查每张表的现有列
2. **增量补全**: 对缺失的列执行 `ALTER TABLE ADD COLUMN`，用 helper 函数吞掉 "duplicate column name" 错误
3. **并发保护**: 文件锁（`O_CREAT | O_EXCL`）防止多实例同时迁移
4. **启动恢复**: 清理残留状态（如中断的安装、过期的 session）

**优势**:
- 无需维护版本号和迁移文件
- 幂等安全（重复执行不会出错）
- 适合早期快速迭代（表结构频繁变化）
- CodePilot 已验证 12+ 表规模下可行

### 第三轮决策 (Q26-Q31) — 参考实现交叉比对

| # | 决策项 | 决策结果 | 来源参考 |
|---|--------|----------|----------|
| 26 | 终端实现 | node-pty（完整 PTY）+ 降级到 spawn | Natives2 |
| 27 | 插件生命周期 | 标准事件：ready / unload / error / heartbeat | CodePilot ChannelPlugin |
| 28 | 配置文件安全 | 原子写入（临时文件 + fsync + rename） | Natives2 _cfgChain |
| 29 | 错误分类 | 12 类结构化错误，含用户提示和操作建议 | CodePilot error-classifier |
| 30 | 环境注入 | 多组环境配置，加密存储，终端启动时选择注入 | CodePilot Provider 简化 |
| 31 | DB 迁移 | 增量迁移（PRAGMA table_info + ALTER TABLE） | CodePilot db.ts |
| 32 | Token 机制 | HMAC 生成 + postMessage 下发 + 内存存储 | 安全模型 |

### 第四轮决策 (Q33-Q39) — PRD 深度挑战

| # | 决策项 | 决策结果 | 修正/补充 |
|---|--------|----------|-----------|
| 33 | Token 握手 | 两阶段握手（iframe 主动请求） | 修正 Q32 竞态和重载问题 |
| 34 | 来源验证 | MessageEvent.source 窗口引用匹配 | 修正 Q32 origin=null 问题 |
| 35 | 插件间通信 | 主进程中转（send + broadcast） | 补充 IPC 机制 |
| 36 | 终端环境注入 | 仅对新会话生效 | 补充注入时机 |
| 37 | 状态保持 | 分层策略（热/温/冷/持久）+ 内存限制 | 补充 LRU 细节 |
| 38 | 崩溃检测 | 双层检测（心跳 + 主动报告） | 补充检测机制 |
| 39 | 不造轮子 | 适用于插件层，基座层必须自建 | 澄清哲学范围 |
| 40 | 无障碍 | 键盘导航 + ARIA + 焦点管理 + 高对比度 | 补充无障碍 |
| 41 | 插件更新 | 数据保护式更新（备份 → 替换 → 保留） | 补充更新机制 |
| 42 | 版本兼容 | SemVer + minNativesVersion + 主版本阻断 | 补充兼容性 |
| 43 | 多窗口 | MVP 单窗口，后期扩展 | 补充窗口策略 |

### 文档一致性修正

| 文档 | 修正内容 |
|------|----------|
| ARCHITECTURE.md | 全部从 webview 更新为 iframe + 本地 HTTP（防线 2、进程模型、布局、加载机制、通信协议、文件结构） |
| CLAUDE.md | 四大支柱和五大防线同步更新（iframe sandbox、node-pty、环境注入） |
| DESIGN_DISCUSSION.md | 第一轮 Q2/Q7/Q10/Q11/Q12 的旧 webview 引用修正为 iframe |

### Q32: Session Token 机制与 postMessage 验证

**问题**: iframe sandbox 不含 `allow-same-origin` 时，所有 sandbox iframe 的 `event.origin` 都是 `null`，基座无法通过 origin 区分消息来源。需要基于 Token 的消息验证机制。

**决策**: ✅ **基于 Token 的消息验证**（不依赖 event.origin）

**Session Token 设计**:
- 基座启动时生成全局 `masterSecret`
- 每个插件实例化时，基座生成唯一 `sessionToken = HMAC(masterSecret, moduleId + timestamp)`
- 通过 postMessage 下发给 iframe（仅一次，在 iframe load 事件后）
- natives-sdk.js 将 token 保存在**内存变量**中（非 localStorage，因为 sandbox 无 allow-same-origin 时 localStorage 不可用）

**postMessage 消息格式**:
```typescript
// 插件 → 基座
interface BridgeMessage {
  type: 'bridge-request' | 'bridge-event';
  moduleId: string;        // 插件 ID
  token: string;           // Session Token
  id: string;              // 请求 ID（用于匹配响应）
  method?: string;         // API 方法（如 'db.get'）
  args?: any[];            // 参数
  payload?: any;           // 事件数据
}

// 基座 → 插件
interface BridgeResponse {
  type: 'bridge-response';
  id: string;              // 对应请求的 ID
  result?: any;            // 成功结果
  error?: ClassifiedError; // 错误信息
}
```

**验证流程**:
```
基座收到 postMessage
  ↓
检查 message.data.token 是否有效
  ↓
检查 message.data.moduleId 是否与 token 匹配
  ↓
验证通过 → 处理请求
验证失败 → 忽略消息（不响应，防止探测）
```

**natives-sdk.js 初始化流程**:
```javascript
// 1. 监听基座下发的 token
window.addEventListener('message', (event) => {
  if (event.data?.type === 'natives-init') {
    SESSION_TOKEN = event.data.token;
    MODULE_ID = event.data.moduleId;
    // 2. 通知基座：SDK 就绪
    window.parent.postMessage({
      type: 'bridge-event',
      moduleId: MODULE_ID,
      token: SESSION_TOKEN,
      payload: { event: 'sdk-ready' }
    }, '*');
  }
});
```

---

## 第四轮: PRD 深度挑战 (Q33-Q39)

> 本轮基于对 PRD 的系统性挑战，发现安全漏洞、架构矛盾和设计空白。

### Q33: Session Token 两阶段握手 ⚠️ 修正 Q32

**问题**: Q32 的 token 下发机制存在三个致命问题：
1. 竞态条件 — 基座创建 iframe 后立即 postMessage 发送 token，但 iframe 的 SDK 可能还没加载完 addEventListener
2. 页面重载丢失 — 插件内部刷新后 JS 内存清空，token 丢失，插件变成"砖头"
3. 无恢复路径 — token "仅下发一次"，没有重试机制

**决策**: ✅ **两阶段握手**（由 iframe 主动请求 token）

**流程**:
```
[iframe 加载 SDK]
    ↓
iframe → 基座: { type: 'token-request', moduleId }
    ↓
基座: 验证 source 引用 + moduleId 合法性，生成 token
    ↓
基座 → iframe: { type: 'token-response', token }
    ↓
SDK: 存入内存变量，标记 ready
```

**关键变化**:
- 由 iframe 主动请求，而非基座主动推送 — 消除竞态
- 重载后自动重新请求 — SDK 初始化时始终先请求 token
- 基座侧维护 moduleId → token 映射，同一 moduleId 可重新下发（旧 token 自动失效）

**ADR**: [[ADR-0001-session-token-handshake]]

### Q34: postMessage 来源验证 ⚠️ 修正 Q32

**问题**: sandbox iframe 的 origin 固定为 `"null"`，基座无法通过 event.origin 验证消息来源。恶意插件可以在 postMessage 中伪造 moduleId，越权访问其他模块数据。

**决策**: ✅ **MessageEvent.source 窗口引用匹配**

- 基座创建 iframe 时保存 `iframe.contentWindow` 引用到 `Map<moduleId, Window>`
- 收到 postMessage 时，遍历映射表，检查 `event.source === savedContentWindow`
- 匹配 → 处理请求（同时获得可信的 moduleId）；不匹配 → 丢弃

**ADR**: [[ADR-0002-postmessage-origin-verification]]

### Q35: 插件间通信机制

**问题**: PRD 定义了 `window.natives.ipc.send/on/broadcast` 接口，但未说明底层通信机制。sandbox iframe 无法使用 BroadcastChannel 和 SharedWorker。

**决策**: ✅ **主进程中转**

- `ipc.send(targetModuleId, payload)` → postMessage 到基座 → 权限检查 → postMessage → 目标插件
- `ipc.broadcast(payload)` → 基座向所有活跃 iframe 广播
- 基座可做权限检查和消息审计

**ADR**: [[ADR-0003-plugin-ipc-main-process-relay]]

### Q36: 终端环境变量注入时机

**问题**: 用户切换环境配置后，已打开的终端会话是否立即使用新配置？

**决策**: ✅ **仅对新会话生效**

- 已运行的终端保持原有环境，避免正在执行的命令因环境突变而出错
- 与 VS Code、iTerm2 等主流终端行为一致
- 切换配置后终端区域显示提示："新配置已生效，新开终端标签将使用新配置"

**ADR**: [[ADR-0004-terminal-env-injection-new-sessions-only]]

### Q37: 插件状态保持 vs LRU 回收

**问题**: User Story 16 要求保留状态，LRU 最多保留 10 个后台 iframe。但 sandbox 无 allow-same-origin，无法使用 localStorage/IndexedDB。10 个后台 iframe 可能消耗 500MB+ 内存。

**决策**: ✅ **分层状态策略 + 保守内存限制**

| 层级 | 条件 | 行为 | 状态保留 |
|------|------|------|----------|
| 热层 | 当前可见 | 保持活跃 | ✅ JS 内存 |
| 温层 | 最近 5 个后台 | 隐藏但保留 | ✅ JS 内存 |
| 冷层 | 超出温层 | 销毁 | ❌ 丢失 |
| 持久层 | 插件主动保存 | natives.db.set() | ✅ 持久化 |

- 后台 iframe 上限从 10 降到 5
- 单个 iframe 内存软上限 150MB
- 系统可用内存 < 1GB 时自动回收

**ADR**: [[ADR-0005-plugin-state-preservation-strategy]]

### Q38: iframe 插件崩溃检测

**问题**: iframe 崩溃时主进程无法收到 child_process 事件，需要其他方式检测。

**决策**: ✅ **双层检测**

- 心跳层：插件每 5s 发送 heartbeat，连续 3 次缺失（15s）→ 无响应，再 10s → 已崩溃
- 主动报告：插件通过 `lifecycle.error()` 主动通知基座
- iframe 事件：监听 load/error 事件检测页面级加载失败
- 崩溃后显示友好错误页面 + "重新加载"按钮，不自动重载（避免崩溃循环）

**ADR**: [[ADR-0006-iframe-crash-detection]]

### Q39: "不造轮子"哲学适用范围

**问题**: CLAUDE.md 说"不造轮子"，但 PRD 自建了终端、HTTP 服务器、数据库、Bridge 系统。

**决策**: ✅ **"不造领域轮子"适用于插件层**

| 层级 | 哲学 | 实践 |
|------|------|------|
| 插件层 | 不造领域轮子 | 嵌入 Claude Code、VS Code 等现有工具 |
| 基座层 | 必须自建 | 容器本身就是轮子，没有现成替代品 |

**ADR**: [[ADR-0007-domain-wheel-reinvention-clarification]]

### Q40: 无障碍设计

**问题**: PRD 仅有 `prefers-reduced-motion`，缺少键盘导航和屏幕阅读器支持。作为桌面应用，无障碍是基本要求。

**决策**: ✅ **完整的无障碍支持**

- **键盘导航**：Tab 遍历、Enter 激活、Escape 关闭、Arrow 键列表移动、Cmd+Shift+K 跳过链接
- **焦点管理**：插件切换时焦点自动转移、弹窗焦点陷阱、关闭后焦点回到触发元素
- **ARIA 标注**：按钮 `role="button"`、面板 `role="region"`、导航 `role="navigation"`、状态 `aria-live`
- **高对比度**：`prefers-contrast: high` 增强边框，文字对比度 ≥ 4.5:1（WCAG AA）

### Q41: 插件更新机制

**问题**: PRD 提到启动扫描但未描述更新流程。插件更新时如何保护用户数据？

**决策**: ✅ **数据保护式更新**

- 启动扫描时比较 `manifest.json` 的 `version` 字段，检测到变化标记为"可更新"
- 更新流程：备份 module_data → 替换插件文件 → 保留 module_data → 重新加载
- Bridge API 版本不兼容时显示警告，不自动更新
- 用户可在模块管理页面手动触发更新

### Q42: 插件版本兼容性

**问题**: Bridge API 升级后，旧版插件可能不兼容。如何处理版本差异？

**决策**: ✅ **语义化版本 + minNativesVersion 声明**

- 基座通过 `window.natives.meta.nativesVersion` 暴露当前版本
- 插件在 `manifest.json` 中声明 `minNativesVersion`
- 主版本号不同 → 阻止加载并提示；次版本号不同 → 警告但允许运行
- 遵循语义化版本（SemVer）

### Q43: 多窗口支持

**问题**: Electron 支持多 BrowserWindow，Natives 是否需要多窗口？

**决策**: ✅ **MVP 单窗口**

- 数据库单写入者模型（Q28）天然适合单窗口
- 多窗口需同步插件状态、终端会话、主题配置，复杂度高
- MVP 仅支持单窗口，后期根据用户反馈决定是否扩展

---

## 所有待确认事项已解决 ✅

第一轮遗留的 4 个待确认事项已在第二轮全部解决：
1. ~~Bridge API 权限分级~~ → Q13: 从一开始就引入权限
2. ~~插件包格式~~ → Q14: 目录 + ZIP 都支持
3. ~~基座首页内容~~ → Q15: 仪表盘
4. ~~多语言方案~~ → Q16: 手写 en.ts + zh.ts

第三轮通过交叉比对补充了 6 个决策（Q26-Q31），修正了 3 个文档的不一致。

第四轮通过 PRD 深度挑战补充了 7 个决策（Q33-Q39），修正了 Session Token 机制的安全漏洞，补充了 IPC、状态保持、崩溃检测等设计空白。

第五轮补充了 4 个决策（Q40-Q43），覆盖无障碍设计、插件更新机制、版本兼容性和多窗口策略。
