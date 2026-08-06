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
- **⚙️ 原生执行引擎** — Agent Daemon（`src-agent-daemon/` + `crates/`）独立进程，经 UDS 与 Tauri Host 通信：run 状态机、provider 适配、工具 Capability Gateway、事件序列持久化（协议 v2）
- **🧩 扩展宿主** — `extension-host/`：隔离的 TypeScript 运行时，承载 MCP、Skills、Hooks 与插件
- **📇 能力库** — MCP / CLI / Agent 能力注册、连接器密钥链路与权限门控（ADR-0016）
- **⏰ 任务模块** — cron 调度、运行详情、表单草稿持久化（ADR-0015）
- **🔌 本地模块运行时** — iframe + 本地 HTTP；Unique Origin 沙箱；Bridge API
- **🧬 本地创意工坊** — 生成/安装 web-module，事件驱动热上架；数据按 domain 长青（Workshop）
- **🔐 环境注入 & 凭证** — 多组环境 + AES-256-GCM 加密存储，Credential Broker 密钥链路
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
| 桌面框架 | Tauri v2 (Rust backend · `src-tauri/`) |
| 执行引擎 | Native Agent Daemon（`src-agent-daemon/` + `crates/`，UDS 协议 v2） |
| 扩展宿主 | `extension-host/`（隔离 TypeScript 运行时） |
| 前端框架 | Next.js 15 (App Router) + 静态导出 |
| 语言 | TypeScript + Rust |
| 数据库 | SQLite (rusqlite, WAL 模式; 宿主库与引擎库权威分立) |
| 终端 | portable-pty + @xterm/xterm |
| 验证 | Zod |
| 凭证加密 | AES-256-GCM + Credential Broker |
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

# ESLint 代码风格检查 + i18n / 硬编码颜色校验
npm run lint

# 运行测试
npm run test

# Next.js 生产构建
npm run build

# 性能检查（typecheck + lint + build + 包体积阈值）
npm run perf:check

# 协议一致性检查（Host/Daemon wire 类型与前端绑定同步）
npm run protocol:check

# 原生引擎验证（Agent Daemon 侧车打包 + 协议贯通）
npm run verify:native-engine

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
Natives/
├── src-tauri/              # Tauri Rust Host（窗口、PTY、宿主 SQLite、凭证、Daemon 监督）
│   ├── src/
│   │   ├── main.rs         # Tauri 入口
│   │   ├── lib.rs          # 库入口
│   │   ├── commands/       # IPC 命令 (db, fs, module, terminal, usage, assistant, jobs …)
│   │   ├── db.rs           # 宿主 SQLite 初始化 + CRUD
│   │   ├── env_manager.rs  # AES-256-GCM 凭证加密
│   │   ├── credential_broker.rs # Credential Broker 密钥链路
│   │   ├── http_server.rs  # 本地 HTTP 服务 (tiny_http)
│   │   ├── terminal.rs     # PTY 终端管理
│   │   ├── daemon/         # Agent Daemon 监督 + UDS RPC 客户端
│   │   ├── assistant_service/ # 运行网关 / 能力目录（Host 侧）
│   │   └── runtime/        # 运行编排
│   └── capabilities/       # Tauri 权限声明 (最小权限)
├── crates/                 # Rust workspace（执行引擎共享库）
│   ├── agent-core/         # Agent 执行核心（run 状态机、事件日志）
│   ├── assistant-protocol/ # Host/Daemon 协议 wire 类型唯一源
│   ├── capability-gateway/ # 工具 schema、策略、沙箱、审计
│   ├── contract-linter/    # KI-3 静态审计（Host 与 Daemon 共享）
│   ├── harness-core/       # Harness 领域模型（Hook、provenance、会话协调）
│   └── provider-adapters/  # Provider 统一适配
├── src-agent-daemon/       # Agent 守护进程（独立进程，UDS 协议 v2）
│   └── src/                # run/会话权威、capability 解析、MCP/CLI 运行时、凭证
├── extension-host/         # 隔离 TypeScript 扩展运行时（MCP/Skills/Hooks/Plugins）
├── scripts/                # 构建、协议同步、i18n/颜色检查等脚本
├── src/
│   ├── app/                # Next.js App Router 页面
│   │   ├── page.tsx        # 主界面（三栏布局）
│   │   ├── ai/             # AI 工作台 (Token 统计、技能面板)
│   │   ├── capabilities/   # 能力库 (MCP/CLI/Agent, ADR-0016)
│   │   ├── jobs/           # 任务模块 (cron 调度, ADR-0015)
│   │   ├── library/        # 资料库
│   │   ├── files/          # 文件管理
│   │   ├── modules/        # 模块管理
│   │   ├── store/          # 本地模块目录（路径历史命名，非联网商店）
│   │   └── tools/          # 工具页
│   ├── components/         # 共享组件
│   │   ├── shell/          # 外壳布局 (Header/Sidebar/Terminal/Settings)
│   │   ├── dashboard/      # 仪表盘组件 (TokenHero, SkillsPanel, ModelStats)
│   │   ├── assistant/      # AI 助手会话组件
│   │   ├── creative/       # 创意工坊组件
│   │   ├── capabilities/   # 能力库组件
│   │   ├── files/          # 文件管理组件
│   │   ├── ui/             # 通用 UI (LiquidGlass, Modal, Toast, Skeleton)
│   │   └── settings/       # 设置面板组件
│   ├── hooks/              # React Hooks
│   ├── i18n/               # 国际化（中/英）
│   ├── lib/                # 前端工具库 (tauri-adapter, design-tokens, theme-engine, assistant-*)
│   └── types/              # TypeScript 类型定义（generated/ 为协议绑定，勿手改）
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

### 生产执行主链

```
UI → Tauri Host (Rust) → UDS → Agent Daemon (src-agent-daemon/ + crates/*)
```

Renderer、Host 命令、调度器与租户均不得绕过此主链。Host 数据与 Daemon 数据为**两个独立 SQLite 权威**；Renderer 不直接打开 SQLite。

### 模块分层

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
│   Tauri Host (Rust) · Agent Daemon · SQLite · UDS │
└─────────────────────────────────────────────────┘
```

### 进程模型

```
Tauri Host Main (Rust · src-tauri/)
├── 窗口 / FOUC / 端口
├── 宿主 SQLite（模块、环境、凭证 broker 等）
├── module_manager / 创意应用生命周期
├── 本地 HTTP（Workshop 静态 + Bridge）
├── PTY 终端 / 环境注入
└── Daemon 监督 + ExecutionAuthority façade

Agent Daemon (src-agent-daemon/ + crates/*)
├── Run / 会话权威存储
├── Provider 适配 + 工具 Capability Gateway
├── 事件序列与持久化
└── 协议 v2 RPC（诚实 capability 表面）

extension-host/（隔离 TypeScript 运行时）
└── MCP / Skills / Hooks / Plugins 扩展宿主
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

详见 [Bridge API 文档](docs/architecture/ARCHITECTURE.md)。

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

数据存储在 `~/.natives/` 目录下（dotfile 模式），**宿主库与引擎库权威分立**：

- **宿主 SQLite**（`src-tauri/`）— 模块、环境、凭证 broker、创意应用等
- **引擎 SQLite**（Agent Daemon）— run / 会话、事件序列、用量等

两者均 WAL 模式 + 外键约束 + 增量迁移；仅各自 Rust 权威读写，Renderer 不直接打开 SQLite，前端经 tauri-adapter + IPC / UDS 同步。

---

## 许可证

[MIT](LICENSE)

---

## 致谢

本项目的设计和实现参考了 [Fanbox](https://github.com/Wcof/fanbox) 项目的架构理念与功能拆解，在此表示感谢。
