# Natives Full Implementation Plan

> 基于 GitHub Issues #1-#24 的完整开发计划
> 日期: 2026-06-14

## 阶段划分

```
Phase 0 — 项目基础设施 (无依赖)
Phase 1 — 核心后端 (可并行, 依赖 Phase 0)
Phase 2 — 模块核心 (依赖 Phase 1)
Phase 3 — 模块扩展 (可并行, 依赖 Phase 2)
Phase 4 — 上层功能 (可并行, 依赖 Phase 1-3)
Phase 5 — 集成与测试
```

---

## Phase 0 — 项目基础设施

### Task 0.1: 创建 package.json + 配置文件

**文件**: `package.json`, `tsconfig.json`, `next.config.ts`, `electron-builder.yml`, `.gitignore`

**关键内容**:
- Electron + Next.js + TypeScript 依赖
- better-sqlite3 在 `serverExternalPackages`
- `output: 'standalone'`
- scripts: `dev`, `build`, `start`, `test`, `electron-rebuild`

**AC**:
- [ ] `npm install` 成功
- [ ] TypeScript 编译通过 (`tsc --noEmit`)
- [ ] Next.js build 成功 (`next build`)

### Task 0.2: 创建目录结构

按 PRD 的 file structure 创建所有目录。

---

## Phase 1 — 核心后端 (5 条并行线)

### Track 1: Electron 骨架 (Issue #1)

**依赖**: Phase 0
**文件**: `electron/main.ts`, `electron/preload.ts`, `src/app/layout.tsx`, `src/app/page.tsx`

**关键实现**:
- BrowserWindow 创建 (`show: false`, FOUC Guard)
- contextBridge.exposeInMainWorld → `nativesAPI`
- IPC 频道注册 (占位)
- FOUC: window.webContents.send('theme-applied-ready') → BrowserWindow.show()
- electron-builder macOS DMG 配置

**AC**:
- [ ] 应用启动显示空白窗口 (无 FOUC)
- [ ] `window.nativesAPI` 存在
- [ ] IPC 双向通信正常

### Track 2: SQLite 数据库层 (Issue #2)

**依赖**: Phase 0
**文件**: `src/main/database.ts`, `src/main/database.test.ts`

**关键实现**:
- better-sqlite3, WAL 模式
- 9 张表: modules, module_permissions, settings, module_data, workshop_cache, env_profiles, env_variables, notifications, module_order
- 增量迁移: PRAGMA table_info + ALTER TABLE
- 文件锁防并发 (O_CREAT | O_EXCL)
- getDb() / broadcast() / CRUD helpers

**AC**:
- [ ] 9 张表正确创建
- [ ] 增量迁移重复执行不出错
- [ ] 单元测试覆盖 CRUD/迁移/并发

### Track 3: 配置管理器 (Issue #3)

**依赖**: Phase 0
**文件**: `src/main/config-manager.ts`, `src/main/config-manager.test.ts`

**关键实现**:
- readConfig(path) / updateConfig(path, mutator)
- 原子写入: temp file → fsync → rename
- Promise chain 串行化
- 错误恢复

### Track 4: HTTP 服务器 (Issue #7)

**依赖**: Phase 0
**文件**: `src/main/http-server.ts`, `src/main/http-server.test.ts`

**关键实现**:
- startServer(port?) / getPort()
- 路由: /natives-sdk.js, /modules/{moduleId}/{path}, /api/bridge/{namespace}/{method}
- CSP 头注入
- 路径前缀隔离 (禁止 ../)
- 自动选择空闲端口

### Track 5: 三栏布局 UI (Issue #17)

**依赖**: Phase 0
**文件**: `src/components/shell/Sidebar.tsx`, `src/components/shell/ContentArea.tsx`, `src/components/shell/RightPanel.tsx`, `src/components/shell/Terminal.tsx`, `src/app/globals.css`, `src/app/layout.tsx`

**关键实现**:
- CSS Grid: grid-template-columns
- 侧边栏 190-420px, 可折叠 (Cmd+B)
- 右侧面板 0-400px, 可折叠
- 底部终端 200px-50%, 可折叠, 可最大化
- z-index 层级
- 拖拽调整宽度

---

## Phase 2 — 模块核心

### Task: 模块发现与 Manifest 校验 (Issue #4)

**依赖**: #2
**文件**: `src/main/module-manager.ts`, `src/main/module-manager.test.ts`
**关键**: scanModules(), validateManifest() (Zod), checkCompatibility()

### Task: iframe 沙箱与 Token (Issue #8)

**依赖**: #7
**文件**: `src/lib/iframe-sandbox.ts`, `src/lib/token-handler.ts`, `src/lib/bridge-sdk.js`
**关键**: sandbox iframe, 两阶段握手, HMAC token, MessageEvent.source 验证

---

## Phase 3 — 模块扩展 (6 条并行线)

### Track: 模块安装/管理 (Issue #5)
**依赖**: #4
**文件**: `src/main/module-manager.ts` (扩展)

### Track: Bridge 数据读写 (Issue #9)
**依赖**: #8, #2
**文件**: `src/main/bridge-host.ts`, `src/lib/bridge-sdk.js` (扩展)

### Track: Bridge 设置/生命周期 (Issue #10)
**依赖**: #8
**文件**: `src/main/bridge-host.ts`, `src/main/bridge-lifecycle.ts`

### Track: iframe 管理器 (Issue #11)
**依赖**: #8
**文件**: `src/lib/iframe-manager.ts`

### Track: IPC 通信 (Issue #13)
**依赖**: #8
**文件**: `src/main/bridge-ipc.ts`

### Track: 通知系统 (Issue #14)
**依赖**: #8
**文件**: `src/main/bridge-notification.ts`

---

## Phase 4 — 上层功能 (9 条并行线)

- Issue #6: 侧边栏模块列表
- Issue #12: 状态持久化
- Issue #16: 环境变量注入
- Issue #18: 仪表盘与设置页
- Issue #19: 主题引擎
- Issue #20: 命令面板 (Cmd+K)
- Issue #21: 错误分类 + 通知中心
- Issue #22: 创意工坊 UI
- Issue #23: 模块更新
- Issue #24: 无障碍

---

## 执行策略

1. **Phase 0**: 单线程设置
2. **Phase 1**: 5 条并行线, 使用 sub-agent dispatch
3. **Phase 2**: 顺序执行 (#4→#5, #7→#8)
4. **Phase 3**: 6 条并行线
5. **Phase 4**: 10 条并行线

每条任务使用 TDD 垂直切片: one test → one impl → repeat.
