# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**Natives** — AI 时代的桌面应用容器（"AI Steam Base"）。定位为 "Steam + 创意工坊" 生态基座，一个静态编译的容器（Electron + Next.js + SQLite），用户在其中动态插拔 Web 页面插件模块。

- **GitHub**: https://github.com/Wcof/Natives
- **技术栈**: Electron 34 + Next.js 15 (App Router) + TypeScript 5.7 + SQLite (better-sqlite3)

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
| 类型检查 | `rtk tsc` | 83% |
| Lint | `rtk lint` | 84% |
| 测试 | `rtk vitest` / `rtk cargo test` | 90-99% |
| Git | `rtk git status/diff/log/add/commit/push` | 59-80% |
| 文件搜索 | `rtk grep <pattern>` / `rtk find <pattern>` | 70-75% |
| 依赖 | `rtk pnpm list` / `rtk pnpm outdated` | 70-80% |
| Token 统计 | `rtk gain` | — |

### CodeGraph — 必须使用

仓库含 `.codegraph/` 目录时，**搜索/定位代码必须优先使用 CodeGraph**，而非 grep/find/read：

- **MCP tools**: `codegraph_explore` — 一次调用回答代码问题（符号源码 + 调用路径）。`codegraph_node` — 单符号源码 + 调用者，或带行号读整文件。
- **Shell 兜底**: `codegraph explore "<symbol/question>"` 和 `codegraph node <symbol-or-file>` 输出相同。

无 `.codegraph/` 目录时跳过 CodeGraph。

## Development Commands

```bash
# 开发（Next.js）
rtk npm run dev              # Next.js dev server at 127.0.0.1:3000
rtk npm run build            # Next.js standalone build
rtk npm run lint             # ESLint
rtk npm run typecheck        # tsc --noEmit

# 测试（Node.js built-in test runner via tsx）
rtk npm test                 # 所有测试: tsx --test src/**/*.test.ts
rtk npx tsx --test src/main/database.test.ts  # 单个测试文件

# Electron 开发
rtk npm run electron:start   # Next.js dev + Electron 并行启动
rtk npm run electron:dev     # esbuild 打包 electron/ 后启动
rtk npm run electron:build   # next build + electron-builder (DMG)
rtk npm run electron:rebuild # electron-rebuild -f -w better-sqlite3
```

## Architecture

### 三层架构

```
应用层   — 订阅应用、创意工坊模块、内置页面
框架层   — Shell 布局、iframe 管理、Bridge API
服务层   — DB CRUD、环境注入、模块安装/卸载、搜索
基础设施 — Electron Main、SQLite、watchdog、IPC
```

### 四大设计支柱

1. **零代码嵌入**: iframe sandbox + 本地 HTTP 服务 + Session Token 两阶段握手
2. **样式自定义**: SQLite 持久化 + CSS 变量注入 + 三套内置主题（Terminal Volt / Warm Archive / Editorial Index）
3. **环境注入 & Shell 沙箱**: 多环境配置 + `electron.safeStorage` 加密凭证 + 终端自动注入
4. **子应用隔离**: 每个插件独立 iframe sandbox（`allow-scripts allow-forms`，无 `allow-same-origin`）

### 五道生产级防线

1. **子进程看门狗** (`watchdog.ts`): `node --require` 注入，每 2s PID 探测，父进程死亡自动退出
2. **iframe 沙箱安全**: 路径前缀隔离 + Session Token 握手 + MessageEvent.source 验证 + CSP 头
3. **PTY 终端**: node-pty 主方案 + child_process.spawn 降级，支持 TUI 和窗口调整
4. **数据库单向总线**: SQLite 仅主进程读写，配置变更通过 IPC `db-state-changed` 广播
5. **FOUC 防护**: 窗口初始隐藏，CSS 变量就绪后 `theme-applied-ready` 握手才显示

### 关键目录结构

```
src/
  app/           — Next.js App Router 页面 + API Routes（api/fs/ 文件系统操作）
  components/    — React 组件（ai/ files/ shell/ iframe/ ui/ tools/ 等）
  main/          — Electron 主进程模块（database.ts, http-server.ts, shell.ts, bridge-*.ts 等 30+ 模块）
  lib/           — 前端工具（theme-engine.ts, iframe-*.ts, design-tokens.ts 等 30+ 模块）
  i18n/          — 国际化（en.ts, zh.ts 双语）
  types/         — TypeScript 类型定义

electron/
  main.ts        — Electron 入口（窗口管理、IPC 广播、FOUC 防护）
  preload.ts     — IPC 隔离桥（暴露 window.natives API）

docs/
  architecture/  — 架构设计文档 + 决策记录
  adr/           — 7 个 Architecture Decision Records
  standards/     — 13 篇规范（权威约束源，RFC 2119 MUST/SHOULD/MAY）
```

### IPC 通信模型

```
Renderer ←→ Preload (contextIsolation) ←→ Main Process
  ↓                                          ↓
window.natives.*                        SQLite / PTY / fs
  ↓
iframe sandbox (postMessage + Session Token)
```

`preload.ts` 暴露 `window.natives` API：`db`（CRUD）、`terminal`（PTY 控制）、`module`（生命周期）、`env`（环境配置加密）、`getTheme/setTheme`。

## Key Constraints

- **禁止假数据**: 每个用户可见字段必须有真实数据源，无占位零值
- **i18n 同步**: UI 文本变更必须同步更新 `en.ts` 和 `zh.ts`
- **DB schema 变更**: 必须包含迁移逻辑（WAL 模式 + 外键），共 9 张表
- **原生模块**: `better-sqlite3` 和 `node-pty` 在 `serverExternalPackages` 中，不能被 Next.js 打包。安装后需运行 `npx electron-rebuild -f -w node-pty`
- **凭证加密**: 使用 `electron.safeStorage.encryptString()` / `decryptString()`
- **安全红线**: `contextIsolation: true`，无 `nodeIntegration`，iframe sandbox 无 `allow-same-origin`

## Standards — 权威约束源

**[`docs/standards/`](docs/standards/README.md) 是本项目所有约束的唯一权威来源。** 接到编码任务前，必须先读 `docs/standards/README.md` 并按文档地图加载相关规范。规范覆盖：产品架构（`product/`）、技术架构（`technical/`）、前端架构（`frontend/`）、UI/UE（`ui-ux/`）。违反 MUST 规则时先停止，向用户确认或补 ADR。

## Architecture Review Checklist

实现新功能前检查：
1. 能否嵌入现有工具（"完全不写"）？
2. 是否符合四支柱？
3. 是否维护五防线？
4. 违反原则则记录 ADR（`docs/adr/`）
