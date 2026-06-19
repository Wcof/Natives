# CLAUDE.md

This file provides guidance to Claude AI when working with code in this repository.

## Project Overview

**Natives** — AI 时代的桌面应用容器（"AI Steam Base"）。定位为 "Steam + 创意工坊" 生态基座，一个 Tauri v2 + Next.js + SQLite 桌面容器，用户在其中动态插拔 Web 页面插件模块。

- **技术栈**: Tauri v2 (Rust backend) + Next.js 15 (App Router) + TypeScript 5.7 + SQLite (rusqlite)
- **仓库**: https://github.com/Wcof/Natives

## Mandatory Tools

### RTK — 必须使用

**所有 shell 命令必须加 `rtk` 前缀。** RTK 无专用过滤器时原样透传，始终安全。

```bash
# ❌ Wrong
git status && npm run build

# ✅ Correct
rtk git status && rtk npm run build
```

常用命令速查：

| 场景 | 命令 | 节省 |
|------|------|------|
| 类型检查 | `rtk tsc --noEmit` | 83% |
| 搜索 | `rtk grep <pattern>` | 70-75% |
| Git | `rtk git status/diff/log` | 59-80% |
| 测试 | `rtk npm test` | 90-99% |

### CodeGraph — 必须使用

仓库含 `.codegraph/` 目录时，**搜索/定位代码必须优先使用 CodeGraph**，而非 grep/find/read。

## Development Commands

```bash
# 开发（Next.js + Tauri）
rtk npm run dev              # Next.js dev server at 127.0.0.1:3000
rtk npm run tauri:dev        # Next.js + Tauri 并行开发
rtk npm run build            # Next.js standalone build
rtk npm run tauri:build      # Next.js build + Tauri 打包 (.app / .dmg)
rtk npm run lint             # ESLint
rtk npm run typecheck        # tsc --noEmit

# 测试
rtk npm test                 # 测试: tsx --test (排除旧 Electron 测试)
cd src-tauri && rtk cargo check     # Rust 类型检查
cd src-tauri && rtk cargo test      # Rust 测试
```

## Architecture

### 进程模型（Tauri v2）

```
┌─────────────────────────────────────────────────┐
│  Main Process (Tauri v2 Rust / src-tauri/)       │
│  ┌──────────┐ ┌──────────┐ ┌──────────────────┐  │
│  │ Commands │ │   db.rs  │ │   http_server    │  │
│  │ (IPC)    │ │ (SQLite) │ │ (tiny_http)      │  │
│  └──────────┘ └──────────┘ └──────────────────┘  │
│  ┌──────────┐ ┌──────────┐ ┌──────────────────┐  │
│  │ terminal │ │env_mgr   │ │ token_manager    │  │
│  │(portable)│ │(AES-GCM) │ │ (HMAC-SHA256)    │  │
│  └──────────┘ └──────────┘ └──────────────────┘  │
├─────────────────────────────────────────────────┤
│  Renderer Process (Next.js / src/app + components)│
│  ┌──────────┐ ┌──────────┐ ┌──────────────────┐  │
│  │  Shell   │ │  iframe  │ │  tauri-adapter   │  │
│  │  Layout  │ │ manager  │ │ (window.natives) │  │
│  └──────────┘ └──────────┘ └──────────────────┘  │
├─────────────────────────────────────────────────┤
│  iframe (插件沙箱)                                 │
│  sandbox="allow-scripts allow-forms"              │
│  无 allow-same-origin                             │
│  通信: postMessage + HTTP Bridge                  │
└─────────────────────────────────────────────────┘
```

### 四大设计支柱

1. **零代码嵌入**: iframe sandbox + 本地 HTTP 服务 + Session Token 两阶段握手
2. **样式自定义**: SQLite 持久化 + CSS 变量注入 + 两套内置主题（Terminal Volt / Warm Archive）
3. **环境注入 & Shell 沙箱**: 多环境配置 + AES-256-GCM 加密凭证 + 终端自动注入
4. **子应用隔离**: 每个插件独立 iframe sandbox（`allow-scripts allow-forms`，无 `allow-same-origin`）

### 五道生产级防线

1. **子进程看门狗**: Tauri 命令管理子进程生命周期，崩溃时自动清理
2. **iframe 沙箱安全**: 路径前缀隔离 + Session Token 握手 + MessageEvent.source 验证 + CSP 头
3. **PTY 终端**: portable-pty (Rust) 主方案
4. **数据库单向总线**: SQLite 仅 Rust 后端读写，配置变更通过 IPC `db-state-changed` 广播
5. **FOUC 防护**: 窗口初始隐藏 (`visible: false`)，CSS 变量就绪后 `theme-applied-ready` 握手才显示

### 关键目录结构

```
src-tauri/                   # Tauri Rust 后端（唯一可信后端）
  src/
    main.rs                  # Tauri 入口
    lib.rs                   # 库入口（commands 注册）
    commands/                # IPC 命令（db.rs, fs.rs, terminal.rs 等）
    db.rs                    # SQLite 初始化 + 10 张表 + 增量迁移
    env_manager.rs           # AES-256-GCM 凭证加密
    http_server.rs           # 本地 HTTP 服务（tiny_http）
    token_manager.rs         # HMAC-SHA256 Session Token
    terminal.rs              # portable-pty 终端
  capabilities/
    default.json             # 最小权限声明（无 shell:allow-execute）

src/                         # 前端（Next.js Renderer）
  app/                       # Next.js App Router 页面
  components/                # React 组件
  lib/                       # 前端工具库
    tauri-adapter.ts         # ★ 前端到后端的唯一桥接入口
    design-tokens.ts         # 设计令牌
    iframe-manager.ts        # iframe 生命周期管理
    iframe-sandbox.ts        # Session Token + Bridge SDK
    theme-engine.ts          # 主题引擎（CSS 变量注入）
  i18n/                      # 国际化（en.ts, zh.ts）

docs/
  standards/                 # 13 篇规范（权威约束源，MUST/SHOULD/MAY）
  adr/                       # 架构决策记录
  architecture/              # 架构设计文档
```

### IPC 通信模型

```
Renderer (Next.js)
  │ window.nativesAPI.db.get(key)
  ▼
tauri-adapter.ts  ←── 前端唯一后端入口
  │ invoke('db:get', { key })
  ▼
Tauri Rust Command (src-tauri/src/commands/db.rs)
  │ rusqlite query
  ▼
SQLite (WAL mode, ~/.natives/natives.db)
  │ 变更后 emit('db-state-changed')
  ▼
Renderer 监听事件，同步状态
```

插件通信：
```
iframe (sandbox)
  │ window.natives.db.get(key)  → postMessage
  ▼
Renderer Bridge Handler
  │ invoke('db:get', { key })
  ▼
Tauri Command → SQLite
  │ 响应返回
  ▼
iframe 收到结果
```

## Key Constraints

- **禁止假数据**: 每个用户可见字段必须有真实数据源，无占位零值
- **i18n 同步**: UI 文本变更必须同步更新 `en.ts` 和 `zh.ts`
- **DB schema 变更**: 必须包含增量迁移逻辑（WAL 模式 + 外键），共 10 张表
- **凭证加密**: 使用 AES-256-GCM（`src-tauri/src/env_manager.rs`），密文格式 `v2:<nonce>:<ciphertext>:<tag>`
- **安全红线**: iframe sandbox 无 `allow-same-origin`，前端只能通过 `window.nativesAPI`（tauri-adapter）访问后端
- **Bridge 无假数据**: `src-tauri/src/http_server.rs` 的 `route_bridge` 禁止硬编码用户可见状态（theme/locale/version）

## Standards — 权威约束源

**[`docs/standards/`](docs/standards/README.md) 是本项目所有约束的唯一权威来源。** 接到编码任务前，必须先读 `docs/standards/README.md` 并按文档地图加载相关规范。规范覆盖：产品架构（`product/`）、技术架构（`technical/`）、前端架构（`frontend/`）、UI/UE（`ui-ux/`）。违反 MUST 规则时先停止，向用户确认或补 ADR。
