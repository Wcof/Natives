# 技术架构 01 · 三层架构与分层依赖

> **版本**: 1.0.0 · **日期**: 2026-06-15
> **关联 ADR**: 无直接关联，承接 `docs/architecture/ARCHITECTURE.md` 第 3 章
> **关联源文件**: `src/main/`、`src-tauri/`、`src/app/`、`src/components/`

---

## 一、本篇要约束什么

Natives 是 Tauri v2（formerly Electron）+ Next.js + SQLite 的多进程、多层系统。最大的技术风险是**跨层越权调用**（如前端直连 SQLite、插件直接 require Node 模块）。本篇把进程边界、模块分层、依赖方向钉死。

---

## 二、进程模型（三类进程，职责不可互换）

#### R-T1 · 三类进程的职责边界
- **等级**：MUST
- **分类**：进程、分层
- **规则**：系统**必须**只有以下三类进程，职责**必须不**交叉：

| 进程 | 角色 | 允许做什么 | 禁止做什么 |
|------|------|-----------|-----------|
| **Main Process**（Tauri v2 主进程 / `src/main/` + `src-tauri/`） | 唯一可信源 | SQLite 读写、子进程管理、文件系统重操作、凭证加解密、IPC handler 注册 | 渲染 DOM、直接操作 UI |
| **Renderer Process**（Next.js / `src/app/` + `src/components/`） | UI 与编排 | 渲染界面、编排用户操作、通过 `window.nativesAPI`（preload 暴露）调用 Main、管理 iframe | 直接访问 SQLite、直接 `require` Node 模块、直接 spawn 子进程 |
| **iframe**（插件） | 不信任的第三方代码 | 经 Bridge API（postMessage + HTTP）与基座通信 | 任何直接访问 Node / preload / 主进程能力 |

- **为什么**：进程边界是 Natives 安全模型的基础（见 `technical/02-security.md`）。职责一旦交叉，整条防线崩塌。
- **检查方法**：
  - Renderer 中**禁止**出现 `better-sqlite3` / `portable-pty` / `fs` 重操作 / `child_process` 的直接 import。
  - Main 中**禁止**出现 React/DOM 操作。
  - 用 `npm run typecheck` + 代码 review 双重把关。

#### R-T2 · SQLite 读写独占 Main
- **等级**：MUST
- **分类**：数据、安全
- **规则**：SQLite 的所有读写**必须**只在 Main Process 发生。Renderer 需要数据时**必须**经 preload 暴露的受限 API → IPC → Main。**禁止**在 Renderer / Next.js server route 中直接打开数据库连接。
- **正例**：Renderer 调 `window.nativesAPI.db.get(key)` → IPC `db:get` → Main 的 `database.ts` 执行 SELECT。
- **反例**：Next.js API route 里 `import Database from 'better-sqlite3'` 直接查 → 违反（违反 DB 单向总线，见 `technical/02` 防线 4）。
- **为什么**：多连接并发写会破坏 WAL 一致性；且 Renderer 直连意味着插件可能经越权路径触达数据库。
- **检查方法**：`grep -r "better-sqlite3" src/app src/components` 应为空。

---

## 三、模块分层与依赖方向

基座代码内部按四层组织，依赖**只能向下**。

```
应用层 (Application)   ← 内置功能页、AI 工作台、工具
        ↓ 依赖
框架层 (Framework)     ← Shell 布局、iframe 管理、Bridge、主题引擎
        ↓ 依赖
服务层 (Service)       ← DB 访问、环境注入、模块管理、搜索
        ↓ 依赖
基础设施层 (Infrastructure) ← Tauri v2 Main、SQLite、watchdog、IPC
```

#### R-T3 · 依赖只能向下，禁止反向与跨层
- **等级**：MUST
- **分类**：分层
- **规则**：上层**可以**依赖下层；下层**必须不**依赖上层；同层之间**应该**避免环形依赖。具体：
  - 基础设施层（`src/main/database.ts` 等）**禁止** import 任何 `src/app/` / `src/components/`。
  - 框架层（`src/lib/theme-engine.ts` 等）**禁止** import 具体业务组件。
  - 应用层组件**可以**调用框架层与服务层。
- **正例**：`FileBrowser.tsx`（应用层）→ `search-engine.ts`（服务层）→ 经 IPC 到 `database.ts`（基础设施层）。
- **反例**：`database.ts` 里 `import { showToast } from '@/lib/notification-ui'` → 违反（基础设施层反向依赖 UI 反馈）。
- **为什么**：分层是可测试、可演进的前提；反向依赖会让「底层」被「UI 变化」绑架。
- **检查方法**：review 时核对 import 路径方向；底层文件出现 `@/components` / `@/app` 即不合规。

#### R-T4 · 跨进程通信只走 IPC + Bridge，禁止 executeJavaScript
- **等级**：MUST
- **分类**：进程、安全
- **规则**：Main → Renderer 的下行**必须**用 `webContents.send` / IPC 事件广播（如 `db-state-changed`）；Renderer → Main 的上行**必须**用 `ipcRenderer.invoke` 经 Tauri adapter 暴露的 API。**禁止**用 `webContents.executeJavaScript` 把代码注入 Renderer（无法被审计、绕过 adapter 隔离）。
- **为什么**：防线 4（DB 单向总线 + 状态广播）依赖显式 IPC；`executeJavaScript` 会破坏 `contextIsolation`。
- **检查方法**：`grep -r "executeJavaScript" src src-tauri/` 除受控注释外应为空。

---

## 四、IPC 命名约定

#### R-T5 · IPC channel 用 `domain:action` 命名
- **等级**：MUST
- **分类**：命名
- **规则**：IPC channel **必须**采用 `domain:action` 小驼峰形式，`domain` 是业务域，`action` 是动词。preload 暴露的方法**应该**与 channel 名语义对齐。
- **正例**（已在 `src/lib/tauri-adapter.ts` 落地）：`db:get`、`db:set`、`terminal:create`、`terminal:resize`、`module:install`、`env:setVariable`、`app:version`。
- **反例**：`get-data`、`doThing`、`handleClick`、`db_query`（下划线 / 无域前缀）。
- **为什么**：统一命名让权限审计、日志检索、跨人协作都能按域定位。
- **检查方法**：新增 `ipcMain.handle` / `ipcRenderer.invoke` 时核对命名。

---

## 五、本篇合规自检清单

- [ ] 我的代码位于正确的进程（Main / Renderer / iframe），没有越权访问。
- [ ] 我没有在 Renderer 里直接访问 SQLite / fs 重操作 / 子进程（R-T1, R-T2）。
- [ ] 我的 import 方向是「上层 → 下层」，没有反向或跨层（R-T3）。
- [ ] 跨进程通信只用了 IPC / Bridge，没有 `executeJavaScript`（R-T4）。
- [ ] 我新增的 IPC channel 遵循 `domain:action` 命名（R-T5）。
