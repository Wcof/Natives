# Natives 架构设计文档

> **版本**: v0.1.0 (初始设计)
> **日期**: 2026-06-14
> **状态**: 已评审
> **设计讨论**: [DESIGN_DISCUSSION.md](./DESIGN_DISCUSSION.md)

---

## 1. 概述

### 1.1 产品定位

**Natives** 是 "AI Steam Base" — 一个类似 Steam + 创意工坊的生态基座。它不是一个单体应用，而是一个**基座容器**，用户可以在其中浏览、订阅、安装各种 Web 页面模块，并通过内置终端运行 CLI 工具和 AI Agent。

核心理念：
- **Hub 聚合**: 统一入口，统一认证和环境管理
- **应用商店**: 浏览/订阅模块
- **创意工坊**: 开发者可以自己开发和分享模块
- **OS 级管理**: 进程管理、资源共享、安全沙箱

### 1.2 设计哲学（继承自 CLAUDE.md）

| 哲学 | 核心思想 |
|------|----------|
| 应用消亡论 | 未来不是单体应用的天下，而是可组合服务单元 |
| UI 自生成 | 界面不应由开发者硬编码，应基于用户偏好动态生成 |
| 用户主权 | 最终的审美和布局决策完全属于用户 |
| 完全不写 | 不造轮子，直接调用或嵌入官方工具 |

### 1.3 技术栈

| 领域 | 技术 |
|------|------|
| 桌面框架 | Electron (contextIsolation: true, 无 nodeIntegration) |
| 前端框架 | Next.js (App Router, output: 'standalone') |
| 语言 | TypeScript |
| 数据库 | SQLite (better-sqlite3, WAL 模式) |
| 终端 | @xterm/xterm + child_process.spawn (零原生依赖) |
| 验证 | Zod |
| 图标 | lucide-react |

---

## 2. 三层架构总览

```
┌─────────────────────────────────────────────────────────────┐
│                    第一层: 整体架构                           │
│            产品定位 · 模块分层 · 数据流                        │
├─────────────────────────────────────────────────────────────┤
│                    第二层: 底层架构                           │
│       进程模型 · 五大防线 · SQLite · IPC · 环境注入            │
├─────────────────────────────────────────────────────────────┤
│                    第三层: 前端架构                           │
│       Shell 布局 · 插件加载 · Bridge API · 主题系统           │
└─────────────────────────────────────────────────────────────┘
```

---

## 3. 第一层：整体架构

### 3.1 模块分层

```
┌─────────────────────────────────────────────────┐
│               应用层 (Application)               │
│   订阅应用 · 创意工坊模块 · 内置功能页              │
├─────────────────────────────────────────────────┤
│               框架层 (Framework)                 │
│   Shell 布局 · webview 管理 · Bridge API         │
├─────────────────────────────────────────────────┤
│               服务层 (Service)                   │
│   DB CRUD · 环境注入 · 模块安装/卸载 · 搜索        │
├─────────────────────────────────────────────────┤
│            基础设施层 (Infrastructure)            │
│   Electron Main · SQLite · watchdog · IPC        │
└─────────────────────────────────────────────────┘
```

### 3.2 核心数据流

```
用户操作
  ↓
Renderer (Next.js)
  ↓ IPC
Preload (Bridge)
  ↓ IPC
Main Process
  ↓
SQLite / 子进程 / 文件系统

  ‖ 反向广播
  ↓
db-state-changed → Renderer 自动同步
```

### 3.3 插件数据流

```
插件页面 (iframe, sandbox 隔离)
  ↓ window.natives.db.get(key)
Bridge SDK (natives-sdk.js)
  ↓ postMessage / HTTP API
Renderer (iframe host)
  ↓ IPC
Preload → Main Process
  ↓
SQLite (按 module_id 命名空间隔离)
```

---

## 4. 第二层：底层架构

### 4.1 进程模型

```
Electron Main Process
├── main.ts           — 入口: 窗口管理、FOUC 防护、端口分配
├── database.ts       — SQLite: 5 张核心表、WAL、外键、迁移
├── module-manager.ts — 模块生命周期: 安装/卸载/启用/禁用
├── bridge-host.ts    — Bridge API 宿主端: 请求路由 + 权限检查
├── shell.ts          — Shell 管理: 多会话终端 + Token 握手
├── env-injector.ts   — 环境注入: API Key 加密存储 + 环境配置管理
├── subprocess.ts     — 子进程管理: 动态端口 + watchdog
├── watchdog.ts       — 进程守护: PID 轮询 + 自毁机制
└── http-server.ts    — 本地 HTTP 服务: 静态文件 + Bridge API 路由

Renderer Process (Next.js)
├── Shell UI (三栏布局)
├── 内置功能页 (首页/商店/工坊/设置)
└── iframe 容器管理

iframe (N 个, 同进程 sandbox 隔离)
├── 插件 A (sandbox="allow-scripts allow-forms", 无 allow-same-origin)
├── 插件 B (sandbox 隔离)
└── ...
```

### 4.2 五大防线

#### 防线 1: 子进程生命周期 & PID 轮询看门狗

| 机制 | 实现 |
|------|------|
| 看门狗注入 | `node --require dist/watchdog.js` |
| 健康检测 | 每 2 秒 `process.kill(parentPid, 0)` |
| 自毁 | 父进程死亡 → 子进程自动退出 |
| 端口分配 | 内存端口扫描，动态绑定 |

#### 防线 2: iframe 沙箱安全

| 机制 | 实现 |
|------|------|
| sandbox 属性 | `sandbox="allow-scripts allow-forms"`，禁止 `allow-same-origin` |
| 路径隔离 | 每个插件独立路径前缀 `/modules/{moduleId}/` |
| Session Token | 基座启动时生成，通过 postMessage 下发，HTTP 请求需携带 |
| CSP 策略 | HTTP 服务注入 Content-Security-Policy，限制插件网络范围 |
| 导航拦截 | iframe `sandbox` 禁止 `allow-top-navigation`，插件无法导航父窗口 |
| Bridge 通信 | postMessage（实时事件）+ HTTP API（重操作），无需 executeJavaScript |

#### 防线 3: 零原生依赖 PTY 终端

| 机制 | 实现 |
|------|------|
| Shell 启动 | `child_process.spawn('/bin/zsh' \| 'powershell.exe')` |
| 前端渲染 | `@xterm/xterm` 字符流渲染 |
| 安全握手 | Session Token 防 XSS 劫持 IPC |
| 环境注入 | 运行 CLI 时自动注入 API Key 和环境变量 |

#### 防线 4: 数据库单向总线 & 状态广播

| 机制 | 实现 |
|------|------|
| 读写独占 | SQLite 只在 Main Process 读写 |
| 状态同步 | 配置变更 → `db-state-changed` IPC 广播 |
| 前端响应 | Renderer React 自动同步 |
| 数据隔离 | 每个插件一个 namespace（module_id） |

#### 防线 5: FOUC Guard & Zod 验证

| 机制 | 实现 |
|------|------|
| 窗口隐藏 | `BrowserWindow({ show: false })` |
| 主题加载 | Next.js 读取配置 → 挂载 CSS 变量 |
| 显示握手 | `theme-applied-ready` IPC → 显示窗口 |
| 参数验证 | Zod 校验颜色 hex、边栏像素范围、布局参数 |

### 4.3 SQLite 数据模型

共 5 张核心表：

```sql
-- 1. 模块注册表
CREATE TABLE modules (
  id TEXT PRIMARY KEY,               -- 唯一标识 (如 "com.example.myapp")
  name TEXT NOT NULL,                -- 显示名称
  version TEXT NOT NULL,             -- 语义化版本
  description TEXT,                  -- 描述
  author TEXT,                       -- 作者
  entry_point TEXT NOT NULL,         -- 入口文件路径 (相对于模块目录)
  icon_path TEXT,                    -- 图标路径
  install_path TEXT NOT NULL,        -- 安装目录绝对路径
  module_type TEXT DEFAULT 'web',    -- 'web' (MVP 阶段只支持 web)
  status TEXT DEFAULT 'installed',   -- 'installed' | 'enabled' | 'disabled'
  installed_at TEXT DEFAULT (datetime('now')),
  updated_at TEXT DEFAULT (datetime('now'))
);

-- 2. 模块权限声明
CREATE TABLE module_permissions (
  module_id TEXT NOT NULL REFERENCES modules(id) ON DELETE CASCADE,
  permission TEXT NOT NULL,          -- 'db:read' | 'db:write' | 'notification' | ...
  granted INTEGER DEFAULT 0,
  PRIMARY KEY (module_id, permission)
);

-- 3. 用户设置
CREATE TABLE settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL,               -- JSON 序列化值
  updated_at TEXT DEFAULT (datetime('now'))
);

-- 4. 模块数据存储 (KV Store，按 namespace 隔离)
CREATE TABLE module_data (
  module_id TEXT NOT NULL REFERENCES modules(id) ON DELETE CASCADE,
  key TEXT NOT NULL,
  value TEXT NOT NULL,               -- JSON 序列化值
  updated_at TEXT DEFAULT (datetime('now')),
  PRIMARY KEY (module_id, key)
);

-- 5. 创意工坊元数据缓存
CREATE TABLE workshop_cache (
  module_id TEXT PRIMARY KEY,
  manifest TEXT NOT NULL,            -- JSON: 完整 manifest
  cached_at TEXT DEFAULT (datetime('now'))
);

-- 6. 环境配置组 (Q30)
CREATE TABLE env_profiles (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,                -- 显示名称（如 "工作"、"个人"）
  is_default INTEGER DEFAULT 0,      -- 是否默认注入
  created_at TEXT DEFAULT (datetime('now'))
);

-- 7. 环境变量 (Q30)
CREATE TABLE env_variables (
  profile_id TEXT NOT NULL REFERENCES env_profiles(id) ON DELETE CASCADE,
  key TEXT NOT NULL,                 -- 环境变量名（如 "ANTHROPIC_API_KEY"）
  value TEXT NOT NULL,               -- 加密存储的值
  PRIMARY KEY (profile_id, key)
);

-- 8. 通知历史 (Q11, Q20)
CREATE TABLE notifications (
  id TEXT PRIMARY KEY,
  module_id TEXT REFERENCES modules(id) ON DELETE SET NULL,
  title TEXT NOT NULL,
  body TEXT,
  level TEXT DEFAULT 'info',        -- 'info' | 'warning' | 'error'
  read INTEGER DEFAULT 0,
  created_at TEXT DEFAULT (datetime('now'))
);

-- 9. 侧边栏模块排序 (Q10)
CREATE TABLE module_order (
  module_id TEXT PRIMARY KEY REFERENCES modules(id) ON DELETE CASCADE,
  sort_order INTEGER NOT NULL DEFAULT 0
);
```

**迁移策略** (Q31): 增量迁移 — 启动时用 `PRAGMA table_info()` 检查现有列，`ALTER TABLE ADD COLUMN` 补全缺失列。文件锁防止并发迁移。

### 4.4 用户数据目录

```
~/.natives/
├── natives.db                       # SQLite 数据库 (WAL 模式)
├── modules/                         # 已安装模块
│   ├── com.example.myapp/
│   │   ├── manifest.json            # 模块清单
│   │   ├── index.html               # 入口页面
│   │   ├── app.js                   # 应用逻辑
│   │   └── style.css                # 样式
│   └── com.other.tool/
│       └── ...
├── env/
│   └── credentials.json             # 加密存储的 API Key
└── logs/                            # 运行日志
```

### 4.5 模块 Manifest 规范

```json
{
  "id": "com.example.myapp",
  "name": "My Awesome App",
  "version": "1.0.0",
  "description": "A cool module for Natives",
  "author": "Developer Name",
  "entry": "index.html",
  "icon": "icon.png",
  "type": "web",
  "permissions": [
    "db:read",
    "db:write",
    "notification"
  ],
  "minNativesVersion": "1.0.0",
  "api": {
    "bridge": "1.0"
  },
  "lifecycle": {
    "heartbeatInterval": 30000,
    "loadTimeout": 10000
  },
  "i18n": {
    "name": { "en": "My App", "zh": "我的应用" },
    "description": { "en": "...", "zh": "..." }
  }
}
```

**字段说明**:

| 字段 | 必填 | 说明 |
|------|------|------|
| `id` | ✅ | 全局唯一标识，推荐反向域名格式 |
| `name` | ✅ | 用户可见的显示名称 |
| `version` | ✅ | 语义化版本号 (SemVer) |
| `description` | ❌ | 模块描述 |
| `author` | ❌ | 作者信息 |
| `entry` | ✅ | 入口 HTML 文件路径（相对于模块目录） |
| `icon` | ❌ | 图标文件路径（相对于模块目录） |
| `type` | ✅ | 模块类型，MVP 阶段固定为 `"web"` |
| `permissions` | ✅ | 所需权限声明列表 |
| `minNativesVersion` | ❌ | 最低兼容的 Natives 版本 |
| `api.bridge` | ❌ | 所需 Bridge API 版本 |
| `lifecycle.heartbeatInterval` | ❌ | 心跳间隔（毫秒），默认 30000 |
| `lifecycle.loadTimeout` | ❌ | 加载超时（毫秒），默认 10000 |
| `i18n.name` | ❌ | 多语言显示名称 |
| `i18n.description` | ❌ | 多语言描述 |

---

## 5. 第三层：前端架构

### 5.1 基座 Shell 布局

```
┌──────────────────────────────────────────────────────────┐
│  ◉ ◉ ◉                    Natives                       │
├────────┬─────────────────────────────┬───────────────────┤
│        │                             │                   │
│  📱    │                             │   🔧 工坊面板     │
│  订阅  │      主内容区                │   ├─ 浏览模块     │
│  侧边栏│      (iframe Container)     │   ├─ 模块详情     │
│        │                             │   └─ 安装管理     │
│  ├ App1│      当前激活的插件页面       │                   │
│  ├ App2│      通过 iframe 渲染        │   ⚙️ 设置面板     │
│  ├ App3│      本地 HTTP 服务提供文件   │   ├─ 主题设置     │
│  └ ... │                             │   ├─ API Key      │
│        │                             │   ├─ 环境变量     │
│  ──── │                             │   └─ 通用设置     │
│  ⚙️    │                             │                   │
│  设置  │                             │                   │
├────────┴─────────────────────────────┴───────────────────┤
│  > Terminal                                         ▲ ▼  │
│  $ claude --model claude-sonnet-4                        │
│  > Hello! How can I help you today?                      │
└──────────────────────────────────────────────────────────┘
```

| 区域 | 宽度/高度 | 特性 |
|------|-----------|------|
| 左侧边栏 | 48-280px，可折叠 | 模块图标 + 名称，支持拖拽排序 |
| 主内容区 | 自适应剩余空间 | webview 容器，支持标签页多开 |
| 右侧面板 | 0-400px，可折叠 | 工坊/设置/模块管理，按需切换 |
| 底部终端 | 0-50% 窗口高度，可折叠 | xterm.js，多会话，环境变量注入 |

### 5.2 插件加载机制

```
用户点击侧边栏模块
    ↓
模块已加载? ──是──→ 切换到已有 iframe (保留状态)
    │
    否
    ↓
读取 manifest.json
    ↓
Zod 验证 manifest + 权限声明
    ↓
创建 <iframe sandbox="allow-scripts allow-forms"> 元素
    ↓
通过 postMessage 下发 Session Token
    ↓
加载 entry point (http://localhost:{port}/modules/{moduleId}/index.html)
    ↓
插件调用 natives.lifecycle.ready() → 基座切换显示
    ↓
模块运行中 ←──────→ 用户关闭 → 发送 unload 事件 → 销毁 iframe
                       │
                     保持后台 (LRU 策略, 可配置最大保持数)
```

**关键设计决策**:
- 每个插件运行在独立 `<iframe>` 中，sandbox 属性提供安全隔离
- 通过本地 HTTP 服务加载文件（`http://localhost/modules/{moduleId}/...`）
- Bridge API 通过 postMessage（实时事件）+ HTTP API（重操作）通信
- 未激活的 iframe 隐藏但保持在 DOM 中（保留状态），LRU 策略管理内存
- 插件通过 `natives.lifecycle.ready()` 通知基座加载完成

### 5.3 Bridge API

插件通过 `window.natives` 对象与基座通信。

#### 5.3.1 API 接口

```typescript
interface NativesBridge {
  // === 数据存储 (按 module_id 命名空间隔离) ===
  db: {
    get(key: string): Promise<any>;
    set(key: string, value: any): Promise<void>;
    delete(key: string): Promise<void>;
    list(prefix?: string): Promise<Record<string, any>>;
  };

  // === 全局设置 (只读) ===
  settings: {
    getTheme(): Promise<ThemeConfig>;
    getLocale(): Promise<string>;
    onThemeChange(cb: (theme: ThemeConfig) => void): () => void;
  };

  // === 环境变量 (需权限) ===
  env: {
    get(key: string): Promise<string | null>;
    getProviderCredential(provider: string): Promise<string | null>;
  };

  // === 通知 ===
  notification: {
    send(title: string, body: string, opts?: NotifyOptions): Promise<void>;
    badge(count: number): Promise<void>;
  };

  // === 模块间通信 ===
  ipc: {
    send(targetId: string, channel: string, data: any): Promise<void>;
    on(channel: string, cb: (data: any, sender: string) => void): () => void;
    broadcast(channel: string, data: any): Promise<void>;
  };

  // === 元信息 ===
  meta: {
    moduleId: string;
    version: string;
    nativesVersion: string;
  };
}
```

#### 5.3.2 通信协议

**轻量操作 — postMessage 通道**:
```
插件 (iframe)                    基座 (Renderer)                Main Process
     │                                │                              │
     │  postMessage({                 │                              │
     │    type: 'bridge-request',     │                              │
     │    id: 'req-001',              │                              │
     │    token: 'session-xxx',       │                              │
     │    method: 'settings.getTheme',│                              │
     │    args: []                    │                              │
     │  })                            │                              │
     │ ─────────────────────────────→ │                              │
     │                                │  ipcRenderer.invoke(         │
     │                                │    'bridge:settings.getTheme' │
     │                                │  )                           │
     │                                │ ────────────────────────────→│
     │                                │                              │ SQLite SELECT
     │                                │     ←─────── result ─────── │
     │                                │                              │
     │  ←── postMessage({             │                              │
     │    type: 'bridge-response',    │                              │
     │    id: 'req-001',              │                              │
     │    result: { theme: 'dark' }   │                              │
     │  })                            │                              │
```

**重操作 — HTTP API 通道**:
```
插件 (iframe)                    本地 HTTP 服务                   Main Process
     │                                │                              │
     │  fetch('/api/bridge/db/get',   │                              │
     │    { headers: {                │                              │
     │      'X-Session-Token': 'xxx', │                              │
     │      'X-Module-Id': 'mod-a'    │                              │
     │    }})                         │                              │
     │ ─────────────────────────────→ │                              │
     │                                │  验证 Token + moduleId       │
     │                                │  权限检查 (db:read)           │
     │                                │ ────────────────────────────→│
     │                                │                              │ SQLite SELECT
     │                                │     ←─────── result ─────── │
     │                                │                              │
     │  ←── JSON response             │                              │
     │    { value: { ... } }          │                              │
```

#### 5.3.3 HTTP 服务路由表

```
GET  /natives-sdk.js                  → Bridge SDK 脚本
GET  /modules/{moduleId}/{path}       → 静态文件服务

POST /api/bridge/db/get               → 数据读取 (需权限: db:read)
POST /api/bridge/db/set               → 数据写入 (需权限: db:write)
POST /api/bridge/db/delete            → 数据删除 (需权限: db:write)
POST /api/bridge/db/list              → 数据列举 (需权限: db:read)
GET  /api/bridge/settings/theme       → 获取主题 (无需权限)
GET  /api/bridge/settings/locale      → 获取语言 (无需权限)
POST /api/bridge/env/get              → 获取环境变量 (需权限: env:read)
POST /api/bridge/notification/send    → 发送通知 (需权限: notification)
POST /api/bridge/ipc/send             → 模块间通信 (需权限: ipc:send)
GET  /api/bridge/meta                 → 获取模块元信息 (无需权限)

POST /api/bridge/lifecycle/ready      → 生命周期: ready (Q27)
POST /api/bridge/lifecycle/heartbeat  → 生命周期: heartbeat (Q27)
POST /api/bridge/lifecycle/error      → 生命周期: error (Q27)
```

**认证**: 所有 `/api/bridge/*` 请求需在 Header 中携带 `X-Session-Token` 和 `X-Module-Id`。服务端验证 Token 有效性 + moduleId 匹配。

**CSP 头**:
```
Content-Security-Policy: default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; connect-src http://localhost:*
```

### 5.4 主题系统

#### 5.4.1 CSS 变量

```css
:root {
  /* 基础色板 */
  --color-bg-primary: #1a1a2e;
  --color-bg-secondary: #16213e;
  --color-bg-tertiary: #0f3460;
  --color-accent: #e94560;
  --color-text-primary: #eaeaea;
  --color-text-secondary: #a0a0b0;

  /* 布局参数 */
  --sidebar-width: 220px;
  --panel-width: 320px;
  --terminal-height: 200px;
  --border-radius: 8px;

  /* 毛玻璃效果 */
  --glass-bg: rgba(26, 26, 46, 0.85);
  --glass-blur: 12px;
  --glass-border: 1px solid rgba(255, 255, 255, 0.08);
}
```

#### 5.4.2 主题注入流程

```
Main Process 启动
    ↓
从 SQLite 读取主题配置
    ↓
Zod 验证颜色/尺寸参数
    ↓
IPC 发送给 Renderer
    ↓
Next.js 挂载 CSS 变量到 document.documentElement
    ↓
发送 theme-applied-ready
    ↓
Main Process 显示窗口 (FOUC Guard 完成)
    ↓
插件通过 window.natives.settings.getTheme() 获取主题
```

### 5.5 内置功能页

| 页面 | 路由 | 功能 | 优先级 |
|------|------|------|--------|
| 首页 | `/` | 最近使用的模块、快捷入口 | P0 |
| 应用商店 | `/store` | 浏览可安装的模块 (MVP: 本地目录扫描) | P0 |
| 创意工坊 | `/workshop` | 开发者工具：创建模块模板、测试 | P0 |
| 模块管理 | `/modules` | 已安装模块列表，启用/禁用/卸载 | P0 |
| 设置 | `/settings` | 主题、API Key、环境变量、语言 | P0 |
| 全局搜索 | Modal (Cmd+K) | 跨模块搜索 | P1 |
| 通知中心 | Dropdown | 模块消息和事件通知 | P1 |

---

## 6. 项目文件结构

```
Natives/
├── docs/
│   └── architecture/
│       ├── ARCHITECTURE.md          # 本文档
│       └── DESIGN_DISCUSSION.md     # 设计讨论记录
├── electron/
│   ├── main.ts                      # Electron 入口
│   └── preload.ts                   # IPC 隔离桥
├── src/
│   ├── main/                        # Main Process 模块
│   │   ├── database.ts              # SQLite (9 张表, WAL, 外键, 增量迁移)
│   │   ├── http-server.ts           # 本地 HTTP 服务 (静态文件 + Bridge API)
│   │   ├── module-manager.ts        # 模块生命周期 (安装/卸载/启用/禁用)
│   │   ├── bridge-host.ts           # Bridge API 宿主 (请求路由 + 权限检查)
│   │   ├── shell.ts                 # Shell 管理 (node-pty, 多会话)
│   │   ├── env-injector.ts          # 环境注入 (多组配置 + 加密存储)
│   │   ├── subprocess.ts            # 子进程管理 (动态端口 + watchdog)
│   │   └── watchdog.ts              # 进程守护 (PID 轮询 + 自毁)
│   ├── app/                         # Next.js 页面
│   │   ├── page.tsx                 # 基座首页
│   │   ├── layout.tsx               # Root Layout
│   │   ├── globals.css              # 全局样式
│   │   ├── store/page.tsx           # 应用商店
│   │   ├── workshop/page.tsx        # 创意工坊
│   │   ├── modules/page.tsx         # 模块管理
│   │   └── settings/page.tsx        # 设置
│   ├── components/
│   │   ├── shell/                   # Shell 布局组件
│   │   │   ├── Sidebar.tsx          # 左侧边栏
│   │   │   ├── ContentArea.tsx      # 中间主内容区
│   │   │   ├── RightPanel.tsx       # 右侧面板
│   │   │   └── Terminal.tsx         # 底部终端
│   │   └── iframe/
│   │       └── IframeContainer.tsx  # iframe 渲染 + 切换
│   ├── lib/
│   │   ├── bridge-sdk.js            # Bridge API SDK (注入到 iframe)
│   │   ├── iframe-manager.ts        # iframe 生命周期管理 (LRU, heartbeat, crash recovery)
│   │   ├── module-store.ts          # 模块状态管理
│   │   ├── theme-engine.ts          # 主题引擎
│   │   ├── error-classifier.ts      # 错误分类 (Q29)
│   │   └── config-manager.ts        # 原子写入配置管理 (Q28)
│   ├── hooks/                       # React Hooks
│   ├── types/
│   │   └── index.ts                 # 业务类型定义
│   └── i18n/                        # 国际化
│       ├── en.ts
│       └── zh.ts
├── plugin-template/                 # 插件开发模板
│   ├── README.md                    # 开发指南
│   ├── manifest.json                # manifest 模板
│   ├── index.html                   # 入口页面模板
│   └── natives-sdk.d.ts             # Bridge API 类型声明
├── CLAUDE.md                        # AI 协作指南
├── package.json
├── tsconfig.json
├── next.config.ts
└── electron-builder.yml
```

---

## 7. 架构审查清单

在实现任何新功能前，验证：

- [ ] 是否能通过嵌入现有官方工具实现（"完全不写" 原则）？
- [ ] 是否尊重四大设计支柱（零代码嵌入、样式自定义、环境注入、子应用隔离）？
- [ ] 是否维护五大防线？
- [ ] 插件是否在沙箱中运行，不能直接访问 Node.js？
- [ ] 数据是否按 module_id 命名空间隔离？
- [ ] 是否有反假数据来源追溯？
- [ ] 如果违反了任何原则，是否记录了 ADR（Architecture Decision Record）？
