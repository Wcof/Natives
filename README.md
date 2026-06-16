# Natives

> **AI Steam Base** — AI 时代的桌面应用容器

[![Electron](https://img.shields.io/badge/Electron-34-blue.svg)](https://www.electronjs.org/)
[![Next.js](https://img.shields.io/badge/Next.js-15-black.svg)](https://nextjs.org/)
[![TypeScript](https://img.shields.io/badge/TypeScript-5.7-blue.svg)](https://www.typescriptlang.org/)
[![License](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)

Natives 是一个类似 **Steam + 创意工坊** 的桌面应用容器。它不是单体应用，而是一个**生态基座** — 用户在其中浏览、订阅、安装 Web 页面插件，通过内置终端运行 CLI 工具和 AI Agent，所有凭证和环境配置统一管理。

---

## 核心特性

- **🔌 零代码嵌入** — 插件通过 iframe + 本地 HTTP 服务加载，无需修改源码即可接入
- **🎨 样式自定义** — 主题色、布局参数用户自定义，SQLite 持久化 + 动态 CSS 变量注入
- **🔐 环境注入 & Shell 沙箱** — 多组环境配置 + 凭证加密存储，终端启动时自动注入
- **🛡️ 子应用隔离** — 每个插件在独立 iframe sandbox 中运行，互不干扰
- **💻 完整 PTY 终端** — 基于 node-pty + xterm.js，支持 TUI 程序和窗口调整
- **🏪 创意工坊** — 内置插件浏览、安装、管理界面

## 设计哲学

| 哲学 | 核心思想 |
|------|----------|
| 应用消亡论 | 未来不是单体应用的天下，而是可组合的 MCP/API/微服务单元 |
| UI 自生成 | 界面不应由开发者硬编码，应基于用户偏好动态生成 |
| 用户主权 | 最终的审美和布局决策完全属于用户 |
| 不造领域轮子 | 插件层直接嵌入现有工具；基座层必须自建 |

## 技术栈

| 领域 | 技术 |
|------|------|
| 桌面框架 | Electron (contextIsolation: true, 无 nodeIntegration) |
| 前端框架 | Next.js (App Router, output: 'standalone') |
| 语言 | TypeScript |
| 数据库 | SQLite (better-sqlite3, WAL 模式, 9 张表) |
| 终端 | node-pty + @xterm/xterm（降级: child_process.spawn） |
| 验证 | Zod |
| 凭证加密 | electron.safeStorage |
| 图标 | lucide-react |

---

## 快速开始

### 环境要求

- Node.js >= 18
- npm 或 pnpm

### 安装

```bash
git clone https://github.com/Wcof/Natives.git
cd Natives
npm install
```

### 重新编译原生模块

`better-sqlite3` 和 `node-pty` 是原生模块，安装后需要重新编译：

```bash
npx electron-rebuild -f -w node-pty
```

### 开发

```bash
# 启动 Next.js 开发服务器
npm run dev

# 启动 Electron（另开终端）
npm run electron:dev
```

### 构建

```bash
# Next.js 构建
npm run build

# Electron 打包
npm run electron:build
```

### 其他命令

```bash
npm run lint        # ESLint 检查
npm run typecheck   # TypeScript 类型检查
npm run test        # 运行测试
```

---

## 项目结构

```
Natives/
├── electron/               # Electron 主进程
│   ├── main.ts             # 入口：窗口管理、IPC、数据库、HTTP 服务
│   └── preload.ts          # 预加载脚本：IPC 隔离桥接
├── src/
│   ├── app/                # Next.js App Router 页面
│   │   ├── page.tsx        # 主界面（三栏布局）
│   │   ├── ai/             # AI 相关页面
│   │   ├── files/          # 文件管理
│   │   ├── modules/        # 模块管理
│   │   ├── settings/       # 设置面板
│   │   ├── store/          # 应用商店
│   │   ├── tools/          # 工具页
│   │   └── workshop/       # 创意工坊
│   ├── components/         # 共享组件
│   ├── hooks/              # React Hooks
│   ├── i18n/               # 国际化（中/英）
│   ├── lib/                # 工具库
│   ├── main/               # Electron 主进程逻辑
│   └── types/              # TypeScript 类型定义
├── docs/
│   ├── architecture/       # 架构设计文档
│   │   ├── ARCHITECTURE.md # 三层架构规格
│   │   └── DESIGN_DISCUSSION.md  # 43 个设计决策 (Q1-Q43)
│   ├── adr/                # 架构决策记录
│   ├── standards/          # 编码规范（MUST/SHOULD/MAY）
│   ├── PRD.md              # 产品需求文档
│   └── PRD-v2.md           # PRD v2.0
└── plugin-template/        # 插件开发模板
```

---

## 架构概览

### 三层架构

```
┌─────────────────────────────────────────────────┐
│               应用层 (Application)               │
│   订阅应用 · 创意工坊模块 · 内置功能页              │
├─────────────────────────────────────────────────┤
│               框架层 (Framework)                 │
│   Shell 布局 · iframe 管理 · Bridge API         │
├─────────────────────────────────────────────────┤
│               服务层 (Service)                   │
│   DB CRUD · 环境注入 · 模块安装/卸载 · 搜索        │
├─────────────────────────────────────────────────┤
│            基础设施层 (Infrastructure)            │
│   Electron Main · SQLite · watchdog · IPC        │
└─────────────────────────────────────────────────┘
```

### 四大设计支柱

1. **零代码嵌入** — iframe sandbox + 本地 HTTP 服务 + Session Token 握手
2. **样式自定义** — 用户自定义主题，SQLite 持久化，动态 CSS 变量注入
3. **环境注入** — 多组环境配置 + electron.safeStorage 加密存储
4. **子应用隔离** — 每个插件独立 iframe sandbox（无 allow-same-origin）

### 五道生产级防线

1. **子进程看门狗** — `watchdog.ts` 通过 `node --require` 注入，每 2 秒探测父进程
2. **iframe 沙箱安全** — sandbox + 路径隔离 + 两阶段 Token 握手 + CSP 头
3. **完整 PTY 终端** — node-pty 主方案 + child_process.spawn 降级
4. **数据库单向总线** — SQLite 仅主进程读写，配置变更通过 IPC 广播
5. **FOUC 防护** — 窗口初始隐藏，CSS 变量就绪后才显示 + Zod 验证

---

## 插件开发

插件是独立的 HTML/JS/CSS 页面，放在 `~/.natives/modules/` 目录下即可被发现和加载。

### 插件结构

```
my-plugin/
├── manifest.json     # 插件元数据和权限声明
├── index.html        # 入口页面
├── style.css         # 样式
└── main.js           # 逻辑
```

### Bridge API

插件通过 `window.natives.*` 与基座通信：

```javascript
// 读取配置
const value = await window.natives.db.get('key');

// 获取当前主题
const theme = await window.natives.theme.get();

// 发送通知
await window.natives.notification.send('Hello from plugin!');
```

详见 [`plugin-template/`](plugin-template/) 和 [Bridge API 文档](docs/architecture/ARCHITECTURE.md)。

---

## 文档

| 文档 | 说明 |
|------|------|
| [架构设计](docs/architecture/ARCHITECTURE.md) | 三层架构规格（整体 / 底层 / 前端） |
| [设计讨论](docs/architecture/DESIGN_DISCUSSION.md) | 43 个架构决策的完整 Q&A 记录 |
| [PRD](docs/PRD.md) | 产品需求文档（13 个模块，38+ 用户故事） |
| [PRD v2](docs/PRD-v2.md) | PRD v2.0（整合 Fanbox 功能） |
| [编码规范](docs/standards/README.md) | MUST/SHOULD/MAY 约束体系 |
| [ADR](docs/adr/) | 架构决策记录 |

---

## 数据存储

SQLite 数据库存储在 `~/.natives/` 目录下（dotfile 模式）：

- WAL 模式 + 外键约束
- 9 张表 + 增量迁移（`PRAGMA table_info`）
- 仅 Electron 主进程读写，前端通过 IPC 广播同步

---

## 许可证

[MIT](LICENSE)
