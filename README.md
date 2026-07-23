# Natives

> **本机个人全能 AI 工作台** · 本地创意工坊 · v0.1.0  
> （历史曾称 AI Steam Base；联网 Steam 式分享为远期能力）

[![Tauri](https://img.shields.io/badge/Tauri-v2-blueviolet.svg)](https://v2.tauri.app/)
[![Next.js](https://img.shields.io/badge/Next.js-15-black.svg)](https://nextjs.org/)
[![TypeScript](https://img.shields.io/badge/TypeScript-5.7-blue.svg)](https://www.typescriptlang.org/)
[![Rust](https://img.shields.io/badge/Rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Release](https://img.shields.io/badge/Release-v0.1.0-brightgreen.svg)](https://github.com/Wcof/Natives/releases)
[![License](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)

Natives 是**本机个人全能 AI 工作台**：统一管理凭证与环境，内置终端与 AI Agent 编排，并在本地运行可热插拔的 Web 模块（创意工坊域）。你可在工作台上自研/AI 生成能力；**满意后再考虑分享**。联网商店式发现与上架为 **P2**，当前聚焦本地闭环。

产品边界权威：[`docs/adr/0012-product-identity-workshop-scope.md`](docs/adr/0012-product-identity-workshop-scope.md) · [`docs/standards/product/01-positioning.md`](docs/standards/product/01-positioning.md)

---

## 核心特性

- **💻 完整 PTY 终端** — portable-pty (Rust) + xterm.js，多 Tab、环境注入、TUI
- **🤖 AI 工作台** — Agent / Subagent 编排与用量等（Hub / capability）
- **🔌 本地模块运行时** — iframe + 本地 HTTP；Unique Origin 沙箱；Bridge API
- **🧬 本地创意工坊** — 生成/安装 web-module，事件驱动热上架；数据按 domain 长青（Workshop）
- **🔐 环境注入 & 凭证** — 多组环境 + AES-256-GCM 加密存储
- **🎨 玻璃态视觉系统** — Liquid Glass，多皮肤与设计令牌
- **📊 CCUsage Token 统计** — 追踪 Claude/Codex 等用量（真实数据源）
- **🌐 国际化** — 中 / 英双语
- **🛠️ 内置工具集** — 文件、截图标注、Git 状态、Release Wizard 等
- **📦 分发（P2）** — 将个人外置能力分享给他人：本地工坊跑通后再做联网商店

## 设计哲学

| 哲学 | 核心思想 |
|------|----------|
| 工作台优先 | 全能工作台是身份；工坊是其中一域 |
| 双轨组合 | web-module（SPA）与 capability（MCP/CLI/Agent）分轨 |
| UI 可生成 | 短寿模块可 AI 生成；长青的是数据与契约 |
| 用户主权 | 审美和布局决策归用户 |
| 不造领域轮子 | 租户层嵌入现成工具；宿主基础设施自建 |

## 技术栈

| 领域 | 技术 |
|------|------|
| 桌面框架 | Tauri v2 (Rust backend) |
| 前端框架 | Next.js 15 (App Router) + 静态导出 |
| 语言 | TypeScript + Rust |
| 数据库 | SQLite (rusqlite, WAL 模式, 10 张表) |
| 终端 | portable-pty + @xterm/xterm |
| 验证 | Zod |
| 凭证加密 | AES-256-GCM |
| 图标 | lucide-react |
| 皮肤引擎 | CSS 自定义属性 + 动态主题注入 |

---

## 快速开始

### 环境要求

- macOS (Apple Silicon / Intel)
- Node.js >= 18
- Rust toolchain (rustup)
- npm 或 pnpm

### 安装

```bash
git clone https://github.com/Wcof/Natives.git
cd Natives
npm install
```

### 开发

```bash
# 启动 Next.js + Tauri 开发服务器
npm run tauri:dev
```

### 构建与打包

```bash
# TypeScript 类型检查
npm run typecheck

# ESLint 代码风格检查
npm run lint

# 运行测试
npm run test

# Next.js 生产构建
npm run build

# Tauri 打包 (生成 .app / .dmg)
npm run tauri:build
```

打包产物位于：

| 产物 | 路径 |
|------|------|
| DMG 安装镜像 | `src-tauri/target/release/bundle/dmg/Natives_0.1.0_aarch64.dmg` |
| APP 应用程序包 | `src-tauri/target/release/bundle/macos/Natives.app` |

---

## 项目结构

```
Natives2/
├── src-tauri/              # Tauri Rust 后端
│   ├── src/
│   │   ├── main.rs         # Tauri 入口
│   │   ├── lib.rs          # 库入口
│   │   ├── commands/       # IPC 命令 (db, fs, module, terminal, usage, plugins)
│   │   ├── db.rs           # SQLite 初始化 + CRUD
│   │   ├── env_manager.rs  # AES-256-GCM 凭证加密
│   │   ├── http_server.rs  # 本地 HTTP 服务 (tiny_http)
│   │   └── terminal.rs     # PTY 终端管理
│   └── capabilities/       # Tauri 权限声明 (最小权限)
├── src/
│   ├── app/                # Next.js App Router 页面
│   │   ├── page.tsx        # 主界面（三栏布局）
│   │   ├── ai/             # AI 仪表盘 (Token 统计、技能面板)
│   │   ├── files/          # 文件管理
│   │   ├── modules/        # 模块管理
│   │   ├── store/          # 本地模块目录（路径历史命名，非联网商店）
│   │   ├── tools/          # 工具页
│   │   └── workshop/       # 插件工作区
│   ├── components/         # 共享组件
│   │   ├── shell/          # 外壳布局 (Header/Sidebar/Terminal/Settings)
│   │   ├── dashboard/      # 仪表盘组件 (TokenHero, SkillsPanel, ModelStats)
│   │   ├── files/          # 文件管理组件
│   │   ├── ai/             # AI 相关组件
│   │   ├── ui/             # 通用 UI (LiquidGlass, Modal, Toast, Skeleton)
│   │   └── settings/       # 设置面板组件
│   ├── hooks/              # React Hooks
│   ├── i18n/               # 国际化（中/英）
│   ├── lib/                # 前端工具库 (tauri-adapter, design-tokens, theme-engine)
│   └── types/              # TypeScript 类型定义
├── docs/                   # 见 docs/README.md
│   ├── README.md           # 文档索引与权威优先级
│   ├── standards/          # 约束唯一权威
│   ├── adr/                # 架构决策记录
│   └── architecture/       # 现状描述与领域设计
├── eslint.config.mjs       # ESLint flat config
├── next.config.ts           # Next.js 配置（静态导出）
└── package.json             # 项目依赖和脚本
```

---

## 架构概览

### 三层架构

```
┌─────────────────────────────────────────────────┐
│               应用层 (Application)               │
│   本地模块 · 创意工坊 · Hub 内置页 · AI 工作台     │
├─────────────────────────────────────────────────┤
│               框架层 (Framework)                 │
│   Shell 布局 · iframe 管理 · Bridge API         │
├─────────────────────────────────────────────────┤
│               服务层 (Service)                   │
│   DB CRUD · 环境注入 · 模块安装/卸载 · 搜索        │
├─────────────────────────────────────────────────┤
│            基础设施层 (Infrastructure)            │
│   Tauri v2 (Rust) · SQLite · tiny_http · IPC     │
└─────────────────────────────────────────────────┘
```

### 四大设计支柱

1. **零代码嵌入** — iframe sandbox + 本地 HTTP 服务 + Session Token 握手
2. **样式自定义** — 主题色、圆角、玻璃透明度用户自定义，SQLite 持久化，动态 CSS 变量注入
3. **环境注入** — 多组环境配置 + AES-256-GCM 加密存储
4. **子应用隔离** — 每个插件独立 iframe sandbox（无 allow-same-origin）

### 五道生产级防线

1. **子进程看门狗** — Tauri 命令管理子进程生命周期，崩溃时自动清理
2. **iframe 沙箱安全** — sandbox (`allow-scripts allow-forms`, 无 `allow-same-origin`) + 路径隔离 + 两阶段 Token 握手 + CSP 头
3. **完整 PTY 终端** — portable-pty (Rust) 主方案
4. **数据库单向总线** — SQLite 仅 Rust 后端读写，配置变更通过 IPC 广播
5. **FOUC 防护** — 窗口初始隐藏 (`visible: false`)，CSS 变量就绪后才显示 + Zod 验证

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
| [文档索引](docs/README.md) | 权威优先级与目录地图 |
| [编码规范](docs/standards/README.md) | **约束唯一权威**（MUST/SHOULD/MAY） |
| [ADR-0012 产品身份](docs/adr/0012-product-identity-workshop-scope.md) | 工作台 / 三面 / 双轨 / P0–P2 |
| [架构现状](docs/architecture/ARCHITECTURE.md) | 架构描述（非红线） |
| [ADR 目录](docs/adr/) | 架构决策记录 |
| [设计讨论（历史）](docs/architecture/DESIGN_DISCUSSION.md) | Q1–Q43 溯源；冲突以 ADR 为准 |

---

## 数据存储

SQLite 数据库存储在 `~/.natives/` 目录下（dotfile 模式）：

- WAL 模式 + 外键约束
- 10 张表 + 增量迁移（`PRAGMA table_info`）
- 仅 Rust 后端 (`src-tauri/`) 读写，前端通过 tauri-adapter + IPC 同步

---

## 许可证

[MIT](LICENSE)

---

## 致谢

本项目的设计和实现参考了 [Fanbox](https://github.com/Wcof/fanbox) 项目的架构理念与功能拆解，在此表示感谢。
