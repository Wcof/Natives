# Natives 项目整改计划 (V4 — 全量审计版)

> 审计日期：2026-06-15
> 基于 PRD 38 个用户故事 + 安全审计的全量代码审计。
> 审计范围：功能完整性、UI/UX、安全、国际化。

---

## 审计总览

| 等级 | 数量 | 说明 |
|------|------|------|
| 🔴 P0 严重 | 4 | 功能完全不可用或存在安全漏洞 |
| 🟠 P1 高 | 6 | 功能部分可用或存在明显缺陷 |
| 🟡 P2 中 | 6 | 体验问题或架构债务 |
| ⚪ P3 低 | 4 | 可后续迭代的增强功能 |

**总体评估**：38 个用户故事中，**20 个完全实现**，**10 个部分实现**，**4 个存在严重 bug**，**4 个未实现**。

---

## 🔴 P0 — 严重（必须立即修复）

### P0-1：终端 PTY 输出从未转发到渲染进程（US13/US15）

**现象**：终端完全不可用。用户可以输入但看不到任何输出，TUI 程序（vim/htop）无法运行。

**根因**：`electron/main.ts` 第 246-252 行，`terminal:create` handler 返回 `sessionId` 后**从未调用** `shell.onData()` 将 PTY 输出转发到渲染进程。

**现状代码**：
```typescript
// electron/main.ts line 246
ipcMain.handle('terminal:create', async (_event, profileId?: string) => {
  const mod = lazyLoad('shell');
  if (!mod) return { sessionId: null, error: 'Shell not available' };
  const env: Record<string, string> = {};
  const sessionId = await mod.createSession(env, profileId);
  return { sessionId };  // ← 到这里就结束了，没有注册 onData/onExit 回调
});
```

**需要修改**：在 `terminal:create` handler 中，获取 `sessionId` 后注册输出转发：
```typescript
const sessionId = await mod.createSession(env, profileId);
mod.onData(sessionId, (data: string) => {
  mainWindow?.webContents.send('db-state-changed', 'terminal:data', { sessionId, data });
});
mod.onExit(sessionId, (exitCode: number) => {
  mainWindow?.webContents.send('db-state-changed', 'terminal:exit', { sessionId, exitCode });
});
return { sessionId };
```

**涉及文件**：
- `electron/main.ts` — 添加 onData/onExit 转发
- `src/components/shell/Terminal.tsx` 第 208 行 — 确认接收端逻辑正确（已正确）

**验收标准**：打开终端能执行 `ls`、`echo hello` 并看到输出；能运行 `top` 等 TUI 程序。

---

### P0-2：bridge-host.ts SQL 查询引用不存在的列（US10）

**现象**：插件调用 `window.natives.env.get()` 时运行时 SQL 错误。

**根因**：`src/main/bridge-host.ts` 第 95 行查询 `profile_name` 和 `value` 列，但 `env_variables` 表的实际列是 `profile_id`（INTEGER FK）和 `value_encrypted`（TEXT）。

**现状代码**：
```typescript
// bridge-host.ts line 95
const row = db.prepare('SELECT value FROM env_variables WHERE profile_name = ? AND key = ?').get('default', key);
```

**正确写法**：
```typescript
const row = db.prepare(`
  SELECT ev.value_encrypted FROM env_variables ev
  JOIN env_profiles ep ON ev.profile_id = ep.id
  WHERE ep.is_default = 1 AND ev.key = ?
`).get(key);
```

**额外问题**：同一文件第 106 行使用硬编码加密密钥 `'natives-encryption-key-v1'`，与 `env-injector.ts` 的 XOR fallback 不兼容。应统一使用 `env-injector.ts` 的解密函数。

**涉及文件**：
- `src/main/bridge-host.ts` 第 88-113 行

**验收标准**：插件通过 Bridge API 能正确读取环境变量。

---

### P0-3：iframe-sandbox.ts 在浏览器上下文中导入 Node.js `crypto`（US6）

**现象**：`IframeSandboxManager` 在浏览器中运行时 import 崩溃，导致整个 iframe 管理和 Token 系统失效。

**根因**：`src/lib/iframe-sandbox.ts` 第 1 行 `import * as crypto from 'crypto'`，这是 Node.js 模块。该文件同时被主进程（Token 生成）和渲染进程（iframe 管理）使用。

**修复方案**：将文件拆分为两部分：
1. `iframe-sandbox-token.ts` — Token 管理（仅主进程，可安全用 Node.js crypto）
2. `iframe-sandbox-manager.ts` — iframe 管理（渲染进程，使用 `window.crypto.subtle`）
3. `iframe-sandbox.ts` — 仅导出 SDK 构建脚本

**涉及文件**：
- `src/lib/iframe-sandbox.ts` — 拆分
- `src/lib/iframe-manager.ts` — 更新 import
- `src/main/http-server.ts` — 更新 Token import

**验收标准**：iframe 加载插件不报 crypto import 错误；Token 握手正常完成。

---

### P0-4：Dashboard 安装按钮绕过权限确认（US1/US7/US12）

**现象**：从 Dashboard 快捷安装模块时，不读取 manifest、不弹权限确认框，直接安装。恶意 ZIP 可静默获取所有权限。

**根因**：`src/app/page.tsx` 第 160-171 行，`handleInstallModule` 直接调用 `api.module.install()`。

**现状代码**：
```typescript
// page.tsx line 160
const handleInstallModule = useCallback(() => {
  const input = document.createElement('input');
  input.type = 'file';
  input.accept = '.zip';
  input.onchange = () => {
    const file = input.files?.[0];
    if (file) {
      window.nativesAPI?.module?.install?.(file.path || file.name);  // ← 直接安装，无权限确认
    }
  };
  input.click();
}, []);
```

**修复方案**：复用 WorkshopPage 的安装流程 — 先 `readManifest`，再弹权限确认对话框，确认后再 `install` + `grantPermission`。

**涉及文件**：
- `src/app/page.tsx` 第 160-171 行
- 可能需要将权限确认对话框提取为共享组件

**验收标准**：从任何入口安装模块都会先显示权限确认对话框。

---

## 🟠 P1 — 高（功能缺陷）

### P1-1：Bridge SDK 仅暴露 3/6 命名空间（US5）

**现象**：插件只能使用 `window.natives.db.*`、`window.natives.meta.*`、`window.natives.lifecycle.*`。`settings`、`notification`、`ipc`、`env` 四个命名空间在 `bridge-host.ts` 有服务端处理，但 SDK 脚本未暴露客户端接口。

**涉及文件**：
- `src/lib/iframe-sandbox.ts` — `buildBridgeSdkScript()` 函数需要补充 4 个命名空间

**验收标准**：插件可调用 `window.natives.settings.getTheme()`、`window.natives.notification.send()` 等 API。

---

### P1-2：env:getVariables IPC 缺少加密密钥参数（US25）

**现象**：`electron/main.ts` 调用 `mod.getVariables(profileId)` 但未传入加密密钥，解密会失败。

**涉及文件**：
- `electron/main.ts` — `env:getVariables` handler
- `electron/preload.ts` — 确认 API 签名

**验收标准**：设置页面能正确显示加密的环境变量值。

---

### P1-3：权限对话框只有 "全部允许"，无选择性授权（US12）

**现象**：用户无法单独拒绝某个权限，只有 "全部允许" 和 "取消安装" 两个选项。

**涉及文件**：
- `src/components/shell/WorkshopPage.tsx` 第 509-571 行

**修复方案**：为每个权限行添加 checkbox，支持 "允许选中" / "全部允许" / "取消"。

**验收标准**：用户可以选择性地授予部分权限。

---

### P1-4：已安装模块的权限无查看 UI（US11）

**现象**：安装后无法查看模块拥有哪些权限。`ModuleDetails` 组件不显示权限列表。

**涉及文件**：
- `src/components/shell/ShellLayout.tsx` — ModuleDetails 组件
- `electron/main.ts` — 需添加 `module:getPermissions` IPC handler

**验收标准**：点击已安装模块可查看其权限列表和授权状态。

---

### P1-5：用户名引导流程缺失（US35）

**现象**：应用首次启动直接进入 Dashboard，没有用户名设置引导。WorkshopPage 读取 `settings:username` 但值为空。

**涉及文件**：
- 需新建 `src/components/onboarding/UsernameOnboarding.tsx`
- `src/app/page.tsx` 或 `src/components/shell/ShellLayout.tsx` — 添加首次启动检测

**验收标准**：首次启动时弹出用户名设置对话框，设置后写入 `settings:username`。

---

### P1-6：错误分类器未接入通知系统（US19）

**现象**：`error-classifier.ts` 已实现 12 种错误分类，但崩溃通知不包含 `actionHint` 和 `retryable` 标志，用户不知道如何处理。

**涉及文件**：
- `src/components/shell/ShellLayout.tsx` 第 191-213 行 — 崩溃处理应调用 `classifyError()`

**验收标准**：崩溃通知包含用户友好的操作提示。

---

## 🟡 P2 — 中（体验/架构）

### P2-1：环境变量删除用空字符串替代真实删除（US27）

**现状**：`SettingsPage.tsx` 第 192 行将 value 设为 `''` 而非删除行。
**修复**：添加 `env:deleteVariable` IPC handler。

### P2-2：XOR 加密 fallback 密码学强度不足（US28）

**现状**：`env-injector.ts` 第 43-47 行 XOR cipher 可被轻易逆向。
**建议**：在 safeStorage 不可用时至少使用 AES-256-GCM，或将 XOR 替换为更强的方案。

### P2-3：模块更新仅刷新 DB 元数据，不替换文件（US2）

**现状**：`installer.ts` 的 `updateModule()` 只更新数据库，不处理文件替换。
**建议**：添加 "检测到版本变更" 提示和文件替换流程。

### P2-4：崩溃覆盖层文本硬编码英文（US18）

**现状**：`iframe-manager.ts` 第 245 行硬编码 "Plugin crashed. Click to reload."
**修复**：接入 i18n `t()` 函数。

### P2-5：通知面板仅 30 秒轮询，无实时推送（US20）

**现状**：`NotificationPanel.tsx` 每 30 秒轮询一次。
**建议**：监听 `db-state-changed` 事件实现即时通知。

### P2-6：状态持久化存在两套重复实现（US16）

**现状**：`state-persistence.ts`（主进程）和 `iframe-manager.ts`（渲染进程）功能重复。
**建议**：统一为一套实现。

---

## ⚪ P3 — 低（后续迭代）

### P3-1：商店页面显示 "Coming Soon"（US9）
远程模块目录，按 PRD Q9 决策 "先离线后在线"，可后续实现。

### P3-2：自定义主题 CSS 变量注入未实现（US30-33）
当前仅 3 套预设主题，不支持用户自定义 CSS 变量。

### P3-3：DB MIGRATIONS 数组为空（US38）
迁移框架就绪但无实际迁移条目。首次需要 schema 变更时添加即可。

### P3-4：env:encrypt/env:decrypt IPC 暴露面过大
preload.ts 暴露了加密/解密 IPC，渲染进程可加密任意字符串。建议限制或移除。

---

## 执行顺序（含依赖）

```
P0-1 (终端输出) ──┐
P0-2 (SQL 修复)   ├── 可并行，无依赖
P0-3 (crypto 拆分) ┤
P0-4 (Dashboard 权限)┘
        │
        ▼
P1-1 (SDK 补全) ──┐
P1-2 (env 加密)   ├── 可并行
P1-3 (权限选择)   ┤
P1-4 (权限查看)   ┤
P1-5 (用户名引导) ┤
P1-6 (错误分类) ──┘
        │
        ▼
P2-* (体验优化) ─── 按优先级逐项处理
```

---

## 工时估算

| 等级 | 项数 | 估算工时 |
|------|------|----------|
| P0 | 4 | 6h |
| P1 | 6 | 8h |
| P2 | 6 | 5h |
| P3 | 4 | 不限（后续迭代） |
| **合计** | **20** | **~19h** |

---

## 附录：安全审计发现

| 编号 | 严重度 | 问题 | 位置 |
|------|--------|------|------|
| SEC-1 | 🔴 高 | Dashboard 安装绕过权限确认 | page.tsx:160 |
| SEC-2 | 🟠 中 | env:encrypt/decrypt IPC 暴露面过大 | preload.ts:398 |
| SEC-3 | 🟠 中 | release:execute 接受任意命令执行 | main.ts:637 |
| SEC-4 | 🟡 低 | XOR 加密 fallback 强度不足 | env-injector.ts:43 |
| SEC-5 | 🟡 低 | CSP 使用 unsafe-inline（必要） | http-server.ts:141 |

**已通过的安全检查**：
- ✅ iframe sandbox 无 allow-same-origin
- ✅ contextIsolation: true, nodeIntegration: false
- ✅ HMAC-SHA256 Token 生成 + 24h TTL
- ✅ Host/Origin 校验仅允许 localhost
- ✅ 路径遍历防护 (sanitizePath)
- ✅ 参数化 SQL 查询
- ✅ 无 eval() / new Function()
