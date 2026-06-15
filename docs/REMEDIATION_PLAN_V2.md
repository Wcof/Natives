# Natives 项目功能缺口与国际化问题整改计划 (V3 — PRD 全量对照版)

> 审计日期：2026-06-15
> 基于 PRD 38 个用户故事的全量代码审计，制定剩余功能补全计划。
> 28/38 已完全实现，7 项部分完成需修复，3 项为低优先级可后续迭代。

---

## 审计结论总览

| 分类 | 数量 | 说明 |
|------|------|------|
| ✅ PRD 完全实现 | 28/38 | 侧边栏、拖拽排序、折叠面板、Cmd+K、通知中心、主题、语言、拖拽安装、启用/禁用/卸载、启动扫描、手动刷新、加载模块、状态保留、多模块并行、LRU、崩溃页面、终端、TUI、拖拽调整、多会话、环境注入、环境管理、加密存储、插件模板、Bridge API、SDK 类型、权限声明、生命周期、工坊创建、数据持久化、原子写入 |
| ⚠️ 部分完成需修复 | 7/38 | 见下方 P0/P1 详细计划 |
| 🟢 低优先级可延后 | 3/38 | US9 远程商店目录、US38 DB 迁移条目、US16 State persistence 代码统一 |
| 🔵 i18n | ✅ 全部完成 | 5 个组件 ~10 处硬编码已全部替换为 t() 调用 |
| ✅ 前期整改完成 | 18/20 | 原整改清单 20 项中 18 项已完成（含用量追踪、崩溃通知、Follow Mode、Session Replay、FileRow 闪烁等） |

---

## 一、P0 — 高严重度（功能失效或安全缺口，必须修复）

### P0-1. US29: 默认环境配置数据断裂

**问题**：UI 写入和后端读取在不同的表，用户设置默认配置后终端仍然使用旧默认。

**涉及文件**：
- `src/components/settings/SettingsPage.tsx` — 第 365 行写入 `settings:default_env_profile`
- `src/lib/env-injector.ts` — `getDefaultProfile()` 读取 `env_profiles.is_default`

**整改方案**：
```
方案 A（推荐）：统一到 env_profiles 表
1. SettingsPage.tsx 星标按钮改为更新 env_profiles.is_default 字段：
   - 先将所有 profile 的 is_default 设为 0
   - 再将选中 profile 的 is_default 设为 1
   - 使用 db.set('env_profiles:default', profileId) 或直接 IPC 调用
2. 移除 settings:default_env_profile 的写入逻辑
3. getDefaultProfile() 已经读 env_profiles.is_default，无需改动

方案 B（备选）：统一到 settings 表
1. getDefaultProfile() 改为读 settings 表的 default_env_profile 键
2. 与 SettingsPage 现有写入逻辑一致
```

---

### P0-2. US12: 安装模块时权限确认缺失

**问题**：权限写入 DB 但 `granted=0`，UI 不弹确认对话框，运行时 `checkPermission()` 永远拒绝。

**涉及文件**：
- `src/components/shell/WorkshopPage.tsx` — onInstall 回调
- `src/main/installer.ts` — 第 63-66 行写入 permissions（granted=0）
- `src/main/bridge-host.ts` — checkPermission() 检查 granted 字段

**整改方案**：
```
1. WorkshopPage.tsx 安装流程改造：
   - 调用 module.install() 获取 manifest 中的 permissions 列表
   - 弹出确认对话框，列出每个权限（如 db:read, db:write, notification 等）
   - 用户点击"允许"后，调用 IPC 将对应权限的 granted 设为 1
   - 用户点击"拒绝"则取消安装或安装但不授予权限

2. 新增 IPC handler `module:grantPermission`：
   ipcMain.handle('module:grantPermission', async (_e, moduleId, permission) => {
     db.run('UPDATE module_permissions SET granted = 1 WHERE module_id = ? AND permission = ?',
       [moduleId, permission])
   })

3. 确认对话框 UI 设计：
   - 标题："模块请求以下权限"
   - 列表：每项显示权限名 + 简要说明
   - 按钮："全部允许" / "允许选择的" / "取消安装"
```

---

### P0-3. US20: 崩溃通知未持久化到通知表

**问题**：bridge-host 发送 IPC 事件但未调用 `sendNotification()` 写入 SQLite，崩溃信息不在通知中心显示。

**涉及文件**：
- `src/main/bridge-host.ts` — 第 58-74 行 lifecycle.error handler

**整改方案**：
```
1. 在 bridge-host.ts 的 lifecycle.error handler 中，增加 sendNotification() 调用：
   import { sendNotification } from './bridge-notification'

   // 在现有 markError() 之后：
   sendNotification(db, {
     module_id: moduleId,
     title: `Plugin ${moduleId} crashed`,
     body: message || 'Heartbeat timeout',
     level: 'error'
   })

2. 确保 sendNotification() 写入 notifications 表（bridge-notification.ts 已实现）
3. NotificationPanel.tsx 自动从 notifications 表读取，无需前端改动
```

---

## 二、P1 — 中严重度（功能缺失，影响用户体验）

### P1-1. US26: 终端环境配置选择 UI

**问题**：后端 profileId 链路完整，但终端 UI 无选择器，始终使用默认配置。

**涉及文件**：
- `src/components/shell/Terminal.tsx` — 第 137 行 createSession 不传 profileId

**整改方案**：
```
1. Terminal.tsx 添加 Profile 选择器：
   - 在终端 tab 栏右侧添加下拉框，列出所有 env profiles
   - 调用 api.env.listProfiles() 获取列表
   - 选中的 profileId 传递给 api.terminal.create(profileId)

2. UI 位置：终端 tab 栏右侧，"+" 按钮旁边
3. 默认选中 getDefaultProfile() 返回的配置
4. 每个终端会话可独立选择不同 profile（tab 关联 profileId）
```

---

### P1-2. US1: Dashboard 缺模块概览和系统状态

**问题**：Dashboard 只有静态快捷卡片，不显示已安装模块列表和系统状态。

**涉及文件**：
- `src/components/shell/Dashboard.tsx` — 当前只有 Quick Actions

**整改方案**：
```
1. 添加"已安装模块"区域：
   - 调用 api.module.scan() 获取模块列表
   - 显示模块图标、名称、版本、状态（启用/禁用）
   - 点击跳转到对应模块

2. 添加"系统状态"区域：
   - 活跃模块数（iframe-manager.getActiveCount()）
   - 终端会话数
   - 数据库大小（~/.natives/natives.db 文件大小）
   - 最近通知摘要

3. 添加"最近活动"区域：
   - 最近打开的模块（从 module_order 或 accessOrder 推断）
   - 最近通知（从 notifications 表读取最近 5 条）
```

---

### P1-3. US26+29: 环境配置体验打通（汇总）

**问题**：US26（终端选择器）和 US29（默认配置断裂）共同导致环境配置功能不完整。

**依赖关系**：P0-2（US29 默认配置修复）→ P1-1（US26 终端选择器）

---

## 三、P2 — 低优先级（可后续迭代）

### P2-1. US9: 应用商店远程目录

**现状**：store/page.tsx 显示 "Coming Soon"，仅列出本地模块。
**建议**：MVP 阶段可接受，后续接入远程模块目录 API。

### P2-2. US38: DB 迁移条目

**现状**：MIGRATIONS 数组为空，迁移框架就绪但无实际条目。
**建议**：下次 schema 变更时自然补充，无需提前编写。

### P2-3. US16: State Persistence 代码统一

**现状**：iframe-manager.ts 自行实现了一套持久化，未复用 state-persistence.ts。
**建议**：架构优化，功能不受影响，可在重构时统一。

---

## 📅 执行顺序

### Phase 1 — P0 关键修复（阻塞功能或安全缺口）

| 序号 | 任务 | 预估工时 | 依赖 |
|------|------|----------|------|
| 1 | US29 默认环境配置数据断裂修复 | 0.5h | 无 |
| 2 | US12 安装权限确认对话框 | 2h | 无 |
| 3 | US20 崩溃通知持久化 | 0.5h | 无 |

### Phase 2 — P1 功能补全

| 序号 | 任务 | 预估工时 | 依赖 |
|------|------|----------|------|
| 4 | US26 终端环境配置选择 UI | 1.5h | Phase 1 #1 |
| 5 | US1 Dashboard 模块概览 + 系统状态 | 2h | 无 |

### Phase 3 — P2 低优先级（可选）

| 序号 | 任务 | 预估工时 | 说明 |
|------|------|----------|------|
| 6 | US16 State persistence 代码统一 | 0.5h | 架构优化 |
| 7 | US9 远程商店目录 | — | 待定 |
| 8 | US38 DB 迁移条目 | — | 下次 schema 变更时 |

---

## ✅ 验收标准

每个整改项完成后，必须满足以下条件：

1. **功能验证**: 相关用户故事（US XX）的验收条件全部通过。
2. **i18n 同步**: 所有新增/修改的 UI 文本在 `zh.ts` 和 `en.ts` 中均有对应条目。
3. **类型安全**: `tsc --noEmit` 零错误。
4. **构建通过**: `npm run build` 成功。
5. **无硬编码**: 整改区域不包含未国际化的英文字符串。
6. **IPC 完整链路**: 涉及 IPC 的整改项，必须验证 preload → main 完整链路可调用。
7. **数据一致性**: 涉及 DB 读写的整改项，确认写入和读取使用同一张表/字段。

---

## 📎 参考文件索引

| 文件 | 用途 |
|------|------|
| `docs/PRD.md` | 产品需求文档（13 模块, 38 用户故事） |
| `docs/architecture/ARCHITECTURE.md` | 三层架构规格 |
| `docs/architecture/DESIGN_DISCUSSION.md` | Q1-Q32 设计决策记录 |
| `src/i18n/zh.ts` / `en.ts` | 国际化翻译文件 |
| `electron/main.ts` | IPC handler 注册 |
| `electron/preload.ts` | API 桥接暴露 |
| `src/types/index.ts` | Window.nativesAPI 类型定义 |
| `src/main/bridge-notification.ts` | 通知写入 DB |
| `src/main/env-injector.ts` | 环境配置管理 |
| `src/main/installer.ts` | 模块安装/权限 |
