# Natives 整改执行方案 V4 — 详细代码方案

> 生成日期：2026-06-15
> 基于 REMEDIATION_PLAN_V4.md 全量审计，逐项给出可直接执行的代码方案。

---

## 🔴 P0 — 严重（必须立即修复）

---

### P0-1：终端 PTY 输出从未转发到渲染进程

**根因**：`electron/main.ts:246` 的 `terminal:create` handler 创建 session 后未注册 `onData`/`onExit` 回调，PTY 输出从未通过 IPC 发送到渲染进程。

**现状**（main.ts:246-252）：
```typescript
ipcMain.handle('terminal:create', async (_event, profileId?: string) => {
  const mod = lazyLoad('shell');
  if (!mod) return { sessionId: null, error: 'Shell not available' };
  const env: Record<string, string> = {};
  const sessionId = await mod.createSession(env, profileId);
  return { sessionId };  // ← 结束，无 onData 转发
});
```

**渲染进程已就绪**（Terminal.tsx:208-213）：
```typescript
api.onDbStateChanged((_event, channel, data) => {
  if (channel === 'terminal:data' && data?.sessionId === sessionId) {
    term.write(data.data);
  }
});
```

#### 修复方案

**文件**：`electron/main.ts` 第 246-252 行

```typescript
// terminal:create — 创建 PTY 并注册输出转发
ipcMain.handle('terminal:create', async (_event, profileId?: string) => {
  const mod = lazyLoad('shell');
  if (!mod) return { sessionId: null, error: 'Shell not available' };

  const env: Record<string, string> = {};
  const sessionId = await mod.createSession(env, profileId);

  // ★ 新增：PTY 数据 → 渲染进程
  mod.onData(sessionId, (data: string) => {
    mainWindow?.webContents.send('db-state-changed', 'terminal:data', { sessionId, data });
  });

  // ★ 新增：PTY 退出 → 渲染进程
  mod.onExit(sessionId, (exitCode: number) => {
    mainWindow?.webContents.send('db-state-changed', 'terminal:exit', { sessionId, exitCode });
  });

  return { sessionId };
});
```

**注意事项**：
- `mainWindow` 是 `electron/main.ts` 顶部的 `BrowserWindow | null` 变量，已在文件中声明
- `onData` 返回取消订阅函数，但 session 销毁时 shell.ts 会自动清理 listeners
- 渲染进程 Terminal.tsx:208 已正确监听 `terminal:data` channel，无需修改

**验收**：
1. 打开终端，输入 `echo hello`，能看到输出
2. 运行 `top` 等 TUI 程序，界面正常刷新
3. 终端窗口调整大小后内容正确重排

---

### P0-2：bridge-host SQL 引用不存在的列

**根因**：`bridge-host.ts:95` 查询 `profile_name` 和 `value` 列，但 `env_variables` 表的实际 schema 是 `profile_id`（INTEGER FK）和 `value_encrypted`（TEXT）。

**现状**（bridge-host.ts:89-113）：
```typescript
case 'env.get': {
  if (!checkPermission(moduleId, 'env:read')) return { error: 'Permission denied' };
  const { key } = data as { key: string };
  try {
    const db = getDb();
    const row = db.prepare(
      'SELECT value FROM env_variables WHERE profile_name = ? AND key = ?'
    ).get('default', key) as { value: string } | undefined;
    if (!row) return { value: null };
    // 解密逻辑使用硬编码 key，与 env-injector.ts 不兼容
  }
}
```

**实际 schema**（database.ts:92-98）：
```sql
CREATE TABLE IF NOT EXISTS env_variables (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id INTEGER NOT NULL REFERENCES env_profiles(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    value_encrypted TEXT NOT NULL,
    UNIQUE(profile_id, key)
)
```

#### 修复方案

**文件**：`src/main/bridge-host.ts` 第 88-113 行

```typescript
// env.get — 通过 Bridge API 读取环境变量
case 'env.get': {
  if (!checkPermission(moduleId, 'env:read')) return { error: 'Permission denied' };
  const { key } = data as { key: string };
  try {
    const db = getDb();
    if (!db) return { error: 'Database not available' };

    // ★ 修复：JOIN env_profiles 查找默认 profile，使用正确的列名
    const row = db.prepare(`
      SELECT ev.value_encrypted
      FROM env_variables ev
      JOIN env_profiles ep ON ev.profile_id = ep.id
      WHERE ep.is_default = 1 AND ev.key = ?
    `).get(key) as { value_encrypted: string } | undefined;

    if (!row) return { value: null };

    // ★ 修复：使用 env-injector 的解密函数，保持加密方案一致
    const { getEncryptionKey } = require('../../lib/env-injector');
    const encryptionKey = getEncryptionKey();

    // 优先 safeStorage
    try {
      const { safeStorage } = require('electron');
      if (safeStorage.isEncryptionAvailable()) {
        const buf = Buffer.from(row.value_encrypted, 'base64');
        return { value: safeStorage.decryptString(buf) };
      }
    } catch { /* safeStorage 不可用，使用 fallback */ }

    // Fallback: XOR cipher（与 env-injector.ts 一致）
    const buf = Buffer.from(row.value_encrypted, 'base64');
    const keyBuf = Buffer.from(encryptionKey, 'utf-8');
    for (let i = 0; i < buf.length; i++) {
      buf[i] = buf[i]! ^ keyBuf[i % keyBuf.length]!;
    }
    return { value: buf.toString('utf-8') };
  } catch (err) {
    return { error: `Failed to read env variable: ${(err as Error).message}` };
  }
}
```

**同文件 `env.getVariables` handler**（如果存在类似问题）也需要同步修复。

**验收**：
1. 插件调用 `window.natives.env.get('MY_KEY')` 不报 SQL 错误
2. 能正确解密并返回加密的环境变量值

---

### P0-3：iframe-sandbox.ts 在浏览器上下文导入 Node.js crypto

**根因**：`src/lib/iframe-sandbox.ts` 第 1 行 `import * as crypto from 'crypto'`，该文件同时被：
- **主进程**（Token 生成）— 可用 Node.js crypto
- **渲染进程**（iframe 管理 + SDK 构建）— 浏览器环境，不可用 Node.js crypto

#### 修复方案：拆分为三个文件

**策略**：将 Token 管理（需要 Node.js crypto）与 iframe 管理 + SDK 构建（纯浏览器）分离。

##### 1. 新建 `src/lib/token-manager.ts`（仅主进程使用）

```typescript
import * as crypto from 'crypto';

// ── Token Management (TASK-002) ──
// 从 iframe-sandbox.ts 拆出，仅在主进程使用

const TOKEN_TTL_MS = 24 * 60 * 60 * 1000; // 24 hours
const ROTATION_INTERVAL_MS = Math.floor(TOKEN_TTL_MS * 0.7);

let masterSecret: string;

function getOrCreateSecret(): string {
  if (masterSecret) return masterSecret;
  try {
    const { getDb } = require('../main/database');
    const db = getDb();
    const row = db.prepare("SELECT value FROM settings WHERE key = '_token_master_secret'").get() as { value: string } | undefined;
    if (row) {
      masterSecret = row.value;
      return masterSecret;
    }
    const newSecret = crypto.randomBytes(32).toString('hex');
    db.prepare("INSERT INTO settings (key, value, updated_at) VALUES ('_token_master_secret', ?, datetime('now'))").run(newSecret);
    masterSecret = newSecret;
    return masterSecret;
  } catch {
    masterSecret = crypto.randomBytes(32).toString('hex');
    return masterSecret;
  }
}

const tokenMap = new Map<string, { token: string; moduleId: string; createdAt: number }>();

export function generateSessionToken(moduleId: string): string {
  const nonce = crypto.randomBytes(8).toString('hex');
  const timestamp = Date.now().toString();
  const data = `${moduleId}:${timestamp}:${nonce}`;
  const secret = getOrCreateSecret();
  const token = crypto.createHmac('sha256', secret).update(data).digest('hex');
  tokenMap.set(token, { token, moduleId, createdAt: Date.now() });
  return `${token}:${timestamp}`;
}

export function validateSessionToken(token: string, moduleId: string): boolean {
  const [hash] = token.split(':');
  if (!hash) return false;
  const entry = tokenMap.get(hash);
  if (!entry) return false;
  if (Date.now() - entry.createdAt > TOKEN_TTL_MS) {
    tokenMap.delete(hash);
    return false;
  }
  return entry.moduleId === moduleId;
}

export function invalidateToken(token: string): void {
  const [hash] = token.split(':');
  if (hash) tokenMap.delete(hash);
}

export function invalidateModuleTokens(moduleId: string): void {
  for (const [key, value] of tokenMap) {
    if (value.moduleId === moduleId) tokenMap.delete(key);
  }
}

export function invalidateAllTokens(): void {
  tokenMap.clear();
}

export function rotateStaleTokens(): number {
  const now = Date.now();
  let rotated = 0;
  for (const [key, value] of tokenMap) {
    if (now - value.createdAt > ROTATION_INTERVAL_MS) {
      tokenMap.delete(key);
      rotated++;
    }
  }
  return rotated;
}

export function getTokenMetrics(): { active: number; oldestMs: number; ttlMs: number } {
  let oldest = Infinity;
  for (const value of tokenMap.values()) {
    if (value.createdAt < oldest) oldest = value.createdAt;
  }
  return {
    active: tokenMap.size,
    oldestMs: oldest === Infinity ? 0 : Date.now() - oldest,
    ttlMs: TOKEN_TTL_MS,
  };
}
```

##### 2. 新建 `src/lib/iframe-sandbox-manager.ts`（渲染进程使用，无 Node.js 依赖）

```typescript
import { generateSessionToken, invalidateModuleTokens, invalidateToken } from './token-manager';

export interface IframeRecord {
  moduleId: string;
  contentWindow: Window | null;
  token: string;
  sessionStart: number;
  lastHeartbeat: number;
}

export class IframeSandboxManager {
  private iframes = new Map<string, IframeRecord>();
  private sourceMap = new Map<string, Window | null>();

  register(moduleId: string, contentWindow: Window | null): { token: string } {
    invalidateModuleTokens(moduleId);
    const token = generateSessionToken(moduleId);
    const record: IframeRecord = {
      moduleId, contentWindow, token,
      sessionStart: Date.now(),
      lastHeartbeat: Date.now(),
    };
    this.iframes.set(moduleId, record);
    this.sourceMap.set(moduleId, contentWindow);
    return { token };
  }

  unregister(moduleId: string): void {
    const record = this.iframes.get(moduleId);
    if (record) {
      invalidateToken(record.token);
      this.iframes.delete(moduleId);
      this.sourceMap.delete(moduleId);
    }
  }

  verifyMessageSource(moduleId: string, source: Window | null): boolean {
    return this.sourceMap.get(moduleId) === source;
  }

  getToken(moduleId: string): string | undefined {
    return this.iframes.get(moduleId)?.token;
  }

  updateHeartbeat(moduleId: string): void {
    const record = this.iframes.get(moduleId);
    if (record) record.lastHeartbeat = Date.now();
  }

  getTimeoutCount(moduleId: string, timeoutMs: number): number {
    const record = this.iframes.get(moduleId);
    if (!record) return 0;
    return Math.floor((Date.now() - record.lastHeartbeat) / timeoutMs);
  }
}
```

##### 3. 重写 `src/lib/iframe-sandbox.ts`（仅导出 SDK 构建脚本）

```typescript
// Re-export from split modules for backward compatibility
export { generateSessionToken, validateSessionToken, invalidateToken,
         invalidateModuleTokens, invalidateAllTokens, rotateStaleTokens,
         getTokenMetrics } from './token-manager';
export { IframeSandboxManager, type IframeRecord } from './iframe-sandbox-manager';

// ── Bridge SDK Builder ──
export function buildBridgeSdkScript(port: number, targetOrigin: string): string {
  // 保持原有实现不变（纯字符串生成，无 Node.js 依赖）
  return `
(function() {
  'use strict';
  var port = ${port};
  var origin = ${JSON.stringify(targetOrigin)};
  var token = null;
  var moduleId = null;
  var pending = {};
  var msgId = 0;

  function requestToken() {
    window.parent.postMessage({ type: 'token-request' }, origin);
  }

  window.addEventListener('message', function(event) {
    if (event.origin !== origin) return;
    var data = event.data;
    if (!data || typeof data !== 'object') return;
    if (data.type === 'token-granted') {
      token = data.token;
      moduleId = data.moduleId;
    }
  });

  requestToken();

  window.natives = window.natives || {};

  window.natives.db = {
    get: function(key) { return bridgeRequest('db', 'get', { key: key }); },
    set: function(key, value) { return bridgeRequest('db', 'set', { key: key, value: value }); },
    delete: function(key) { return bridgeRequest('db', 'delete', { key: key }); },
    list: function(prefix) { return bridgeRequest('db', 'list', { prefix: prefix }); }
  };

  window.natives.meta = {
    moduleId: '',
    version: '',
    nativesVersion: '0.1.0'
  };

  window.natives.lifecycle = {
    ready: function() {
      window.parent.postMessage({ type: 'lifecycle:ready' }, origin);
    },
    onUnload: function(cb) {
      window._nativesOnUnload = cb;
      window.addEventListener('beforeunload', cb);
    },
    onHeartbeat: function(cb) {
      setInterval(function() {
        cb();
        window.parent.postMessage({ type: 'lifecycle:heartbeat' }, origin);
      }, 5000);
    },
    error: function(info) {
      window.parent.postMessage({ type: 'lifecycle:error', info: info }, origin);
    }
  };

  function bridgeRequest(namespace, method, body) {
    return fetch('http://localhost:' + port + '/api/bridge/' + namespace + '/' + method, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'X-Session-Token': token || '',
        'X-Module-Id': moduleId || ''
      },
      body: JSON.stringify(body)
    }).then(function(res) {
      if (!res.ok) throw new Error('Bridge request failed: ' + res.status);
      return res.json();
    });
  }
})();
`;
}
```

##### 4. 更新 import 路径

需要更新以下文件的 import：

| 文件 | 旧 import | 新 import |
|------|-----------|-----------|
| `src/main/http-server.ts` | `from '../lib/iframe-sandbox'` | `from '../lib/token-manager'`（仅需 token 函数） |
| `src/components/shell/ShellLayout.tsx` | `from '@/lib/iframe-sandbox'` | `from '@/lib/iframe-sandbox-manager'` |

**验收**：
1. 渲染进程加载不报 `crypto` import 错误
2. iframe 中插件能正常加载
3. Token 握手正常完成（插件能调用 Bridge API）

---

### P0-4：Dashboard 安装按钮绕过权限确认

**根因**：`page.tsx:160` 的 `handleInstallModule` 直接调用 `install()`，不读取 manifest、不弹权限确认框。

**现状**（page.tsx:160-171）：
```typescript
const handleInstallModule = useCallback(() => {
  const input = document.createElement('input');
  input.type = 'file';
  input.accept = '.zip';
  input.onchange = () => {
    const file = input.files?.[0];
    if (file) {
      window.nativesAPI?.module?.install?.(file.path || file.name);  // ← 直接安装
    }
  };
  input.click();
}, []);
```

**参考实现**：WorkshopPage.tsx 的安装流程（第 110-165 行）已正确实现 `readManifest → 权限对话框 → install + grantPermission`。

#### 修复方案

**文件**：`src/app/page.tsx` 第 160-171 行

```typescript
// ★ 新增 state
const [permDialog, setPermDialog] = useState<{
  source: string;
  moduleName: string;
  permissions: string[];
} | null>(null);
const [installing, setInstalling] = useState(false);

// ★ 重写 handleInstallModule — 先读 manifest，再弹权限确认
const handleInstallModule = useCallback(() => {
  const input = document.createElement('input');
  input.type = 'file';
  input.accept = '.zip';
  input.onchange = async () => {
    const file = input.files?.[0];
    if (!file) return;
    const source = (file as unknown as { path?: string }).path || file.name;

    try {
      const result = await window.nativesAPI?.module?.readManifest?.(source);
      if (result?.manifest) {
        setPermDialog({
          source,
          moduleName: result.manifest.name,
          permissions: result.manifest.permissions || [],
        });
      } else {
        const reason = (result as { error?: string } | undefined)?.error;
        alert(reason || 'Invalid module package');
      }
    } catch (err) {
      alert(`Failed to read module: ${(err as Error).message}`);
    }
  };
  input.click();
}, []);

// ★ 新增：权限确认后的安装逻辑
const handlePermAllowAll = useCallback(async () => {
  if (!permDialog) return;
  setInstalling(true);
  try {
    const installResult = await window.nativesAPI?.module?.install?.(permDialog.source);
    if (installResult?.success) {
      for (const perm of permDialog.permissions) {
        await window.nativesAPI?.module?.grantPermission?.(installResult.moduleId!, perm);
      }
      // 刷新模块列表
      window.dispatchEvent(new CustomEvent('module-installed'));
    } else {
      alert('Installation failed');
    }
  } catch (err) {
    alert(`Installation error: ${(err as Error).message}`);
  } finally {
    setInstalling(false);
    setPermDialog(null);
  }
}, [permDialog]);
```

**新增 JSX**（在 page.tsx 的 return 中添加权限确认对话框）：

```tsx
{/* Permission confirmation dialog */}
{permDialog && (
  <div
    role="dialog"
    aria-modal="true"
    style={{
      position: 'fixed', inset: 0,
      background: 'rgba(0,0,0,0.5)',
      display: 'flex', alignItems: 'center', justifyContent: 'center',
      zIndex: 60,
    }}
    onClick={() => setPermDialog(null)}
  >
    <div style={{
      background: 'var(--bg-2,#131410)',
      border: '1px solid var(--border,#262920)',
      borderRadius: 12, padding: 24, width: 420, maxWidth: '90vw',
    }} onClick={(e) => e.stopPropagation()}>
      <h2 style={{ fontSize: 15, fontWeight: 600, color: 'var(--text)', margin: '0 0 16px' }}>
        Module Permissions
      </h2>
      <p style={{ fontSize: 12, color: 'var(--text-dim)', marginBottom: 16 }}>
        {permDialog.moduleName} requests the following permissions:
      </p>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 8, marginBottom: 20 }}>
        {permDialog.permissions.length === 0 ? (
          <span style={{ fontSize: 12, color: 'var(--text-dim)' }}>No special permissions required</span>
        ) : (
          permDialog.permissions.map((perm) => (
            <div key={perm} style={{
              display: 'flex', alignItems: 'center', gap: 8,
              padding: '8px 10px', background: 'var(--bg-3,#1c1e17)',
              borderRadius: 6, fontSize: 12,
            }}>
              <span style={{ color: 'var(--accent,#cdf24b)' }}>🔑</span>
              <span style={{ color: 'var(--text)', fontFamily: 'var(--font-mono)', fontSize: 11 }}>
                {perm}
              </span>
            </div>
          ))
        )}
      </div>
      <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
        <button className="btn" onClick={() => setPermDialog(null)} disabled={installing}>
          Cancel
        </button>
        <button className="btn btn-primary" onClick={handlePermAllowAll} disabled={installing}>
          {installing ? 'Installing...' : 'Allow All'}
        </button>
      </div>
    </div>
  </div>
)}
```

**验收**：
1. 从 Dashboard 安装 ZIP 时弹出权限确认对话框
2. 显示模块名称和所需权限列表
3. 点击 "Allow All" 后才执行安装
4. 点击 "Cancel" 取消安装

---

## 🟠 P1 — 高（功能缺陷）

---

### P1-1：Bridge SDK 仅暴露 3/6 命名空间

**根因**：`buildBridgeSdkScript()` 只生成 `db`、`meta`、`lifecycle` 三个命名空间的客户端代码，但 `bridge-host.ts` 已实现 `settings`、`env`、`notification`、`ipc` 的服务端处理。

#### 修复方案

**文件**：`src/lib/iframe-sandbox.ts` 的 `buildBridgeSdkScript()` 函数

在现有 `window.natives.db` 定义之后，追加：

```javascript
// ★ 新增：settings 命名空间
window.natives.settings = {
  getTheme: function() { return bridgeRequest('settings', 'getTheme', {}); },
  getLocale: function() { return bridgeRequest('settings', 'getLocale', {}); },
  onThemeChange: function(cb) {
    window.addEventListener('message', function(event) {
      if (event.origin !== origin) return;
      if (event.data && event.data.type === 'theme-changed') {
        cb(event.data.theme);
      }
    });
  }
};

// ★ 新增：env 命名空间（需要 env:read 权限）
window.natives.env = {
  get: function(key) { return bridgeRequest('env', 'get', { key: key }); }
};

// ★ 新增：notification 命名空间（需要 notification 权限）
window.natives.notification = {
  send: function(title, body, level) {
    return bridgeRequest('notification', 'send', { title: title, body: body, level: level || 'info' });
  }
};

// ★ 新增：ipc 命名空间（需要 ipc:send / ipc:receive 权限）
window.natives.ipc = {
  send: function(targetModuleId, channel, payload) {
    return bridgeRequest('ipc', 'send', { targetModuleId: targetModuleId, channel: channel, payload: payload });
  },
  broadcast: function(channel, payload) {
    return bridgeRequest('ipc', 'broadcast', { channel: channel, payload: payload });
  },
  onMessage: function(cb) {
    window.addEventListener('message', function(event) {
      if (event.origin !== origin) return;
      if (event.data && event.data.type === 'ipc:message') {
        cb(event.data.message);
      }
    });
  }
};
```

**同步更新**：`plugin-template/natives-sdk.d.ts` 类型定义已包含这些命名空间，无需修改。

**验收**：
1. 插件调用 `window.natives.settings.getTheme()` 返回当前主题
2. 插件调用 `window.natives.env.get('KEY')` 返回环境变量值
3. 插件调用 `window.natives.notification.send('title', 'body')` 创建通知

---

### P1-2：env:getVariables 缺少加密密钥参数

**根因**：`main.ts:349` 调用 `mod.getVariables(profileId)` 但 `env-injector.ts` 的 `getVariables()` 需要两个参数 `(profileName, encryptionKey)`。

**现状**（main.ts:349-352）：
```typescript
ipcMain.handle('env:getVariables', async (_event, profileId: string) => {
  const mod = lazyLoad('envInjector');
  return mod ? await mod.getVariables(profileId) : {};  // ← 缺少 encryptionKey
});
```

#### 修复方案

**文件**：`electron/main.ts` 第 349-352 行

```typescript
// ★ 修复：传入 encryptionKey，使用 getVariablesById 按 ID 查询
ipcMain.handle('env:getVariables', async (_event, profileId: string) => {
  const mod = lazyLoad('envInjector');
  if (!mod) return {};
  const encryptionKey = mod.getEncryptionKey();
  return mod.getVariablesById(profileId, encryptionKey);
});
```

**说明**：
- `getVariablesById` 按 profile ID 查询（更可靠），`getVariables` 按 profile name 查询
- `getEncryptionKey()` 返回或创建加密密钥（从 DB 读取或生成新的）
- preload.ts 的 `env.getVariables(profileId)` 签名不变

**验收**：
1. 设置页面能正确显示加密的环境变量值
2. 修改环境变量后值正确更新

---

### P1-3：权限对话框只有 "全部允许"，无选择性授权

**根因**：WorkshopPage.tsx 的权限对话框只提供 "Allow All" 和 "Cancel"，无法单独拒绝某个权限。

#### 修复方案

**文件**：`src/components/shell/WorkshopPage.tsx` 权限对话框部分（第 509-571 行）

```tsx
// ★ 新增 state
const [selectedPerms, setSelectedPerms] = useState<Set<string>>(new Set());

// 当 permDialog 打开时，初始化选中状态（默认全选）
useEffect(() => {
  if (permDialog) {
    setSelectedPerms(new Set(permDialog.permissions));
  }
}, [permDialog]);

// ★ 新增：切换单个权限
const togglePerm = useCallback((perm: string) => {
  setSelectedPerms((prev) => {
    const next = new Set(prev);
    if (next.has(perm)) next.delete(perm);
    else next.add(perm);
    return next;
  });
}, []);

// ★ 修改：允许选中权限的安装逻辑
const handlePermAllowSelected = useCallback(async () => {
  if (!permDialog) return;
  setInstalling(true);
  try {
    const installResult = await api?.module?.install?.(permDialog.source);
    if (installResult?.success) {
      // 只授予选中的权限
      for (const perm of selectedPerms) {
        await api?.module?.grantPermission?.(installResult.moduleId!, perm);
      }
      showToast(t(locale, 'workshop.installSuccess'));
      setShowRipple(true);
      setTimeout(() => setShowRipple(false), 1200);
      await loadModules();
    } else {
      showToast(t(locale, 'errors.installFailed'));
    }
  } catch (err) {
    showToast(t(locale, 'errors.installFailed'));
  } finally {
    setInstalling(false);
    setPermDialog(null);
  }
}, [permDialog, selectedPerms, api, locale, loadModules]);
```

**修改 JSX 权限行**（第 543-559 行）：

```tsx
{permDialog.permissions.map((perm) => (
  <div
    key={perm}
    style={{
      display: 'flex', alignItems: 'center', gap: 8,
      padding: '8px 10px', background: 'var(--bg-3,#1c1e17)',
      borderRadius: 6, fontSize: 12, cursor: 'pointer',
    }}
    onClick={() => togglePerm(perm)}
  >
    {/* ★ 修改：checkbox 替代固定图标 */}
    <input
      type="checkbox"
      checked={selectedPerms.has(perm)}
      onChange={() => togglePerm(perm)}
      style={{ accentColor: 'var(--accent,#cdf24b)' }}
    />
    <span style={{ color: 'var(--text)', fontFamily: 'var(--font-mono)', fontSize: 11 }}>
      {perm}
    </span>
    <span style={{ color: 'var(--text-faint)', marginLeft: 'auto', fontSize: 11 }}>
      {PERMISSION_DESC[perm as keyof typeof PERMISSION_DESC] || perm}
    </span>
  </div>
))}
```

**修改按钮**（第 561-568 行）：

```tsx
<div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
  <button className="btn" onClick={() => setPermDialog(null)} disabled={installing}>
    {t(locale, 'common.cancel')}
  </button>
  {/* ★ 新增：Allow Selected 按钮 */}
  <button
    className="btn"
    onClick={handlePermAllowSelected}
    disabled={installing || selectedPerms.size === 0}
  >
    {t(locale, 'workshop.permissionAllowSelected').replace('{count}', String(selectedPerms.size))}
  </button>
  <button className="btn btn-primary" onClick={handlePermAllowAll} disabled={installing}>
    {installing ? t(locale, 'common.loading') : t(locale, 'workshop.permissionAllowAll')}
  </button>
</div>
```

**i18n 补充**：在语言文件中添加：
```json
{
  "workshop": {
    "permissionAllowSelected": "Allow Selected ({count})"
  }
}
```

**验收**：
1. 权限行有 checkbox，可点击切换
2. "Allow Selected" 只授予勾选的权限
3. "Allow All" 行为不变
4. 至少选中一个权限才能点击 "Allow Selected"

---

### P1-4：已安装模块的权限无查看 UI

**根因**：ModuleDetails 组件不显示权限列表，且缺少 `module:getPermissions` IPC handler。

#### 修复方案

##### 1. 确认 IPC handler 已存在

`main.ts:303-307` 已有 `module:listPermissions` handler：
```typescript
ipcMain.handle('module:listPermissions', async (_event, moduleId: string) => {
  const mod = lazyLoad('permissionCenter');
  if (!mod) return [];
  return mod.listModulePermissions(moduleId);
});
```

`preload.ts:36` 已暴露 API：
```typescript
listPermissions: (moduleId: string) => ipcRenderer.invoke('module:listPermissions', moduleId),
```

##### 2. 修改 ModuleDetails 组件

**文件**：`src/components/shell/ShellLayout.tsx` — ModuleDetails 组件

```tsx
// ★ 新增 state
const [modulePerms, setModulePerms] = useState<string[]>([]);

// ★ 加载权限列表
useEffect(() => {
  if (selectedModule) {
    window.nativesAPI?.module?.listPermissions?.(selectedModule.id)
      .then((perms: string[]) => setModulePerms(perms || []))
      .catch(() => setModulePerms([]));
  }
}, [selectedModule]);

// 在 ModuleDetails 的 JSX 中添加权限展示区域：
{modulePerms.length > 0 && (
  <div style={{ marginTop: 16 }}>
    <h4 style={{ fontSize: 12, fontWeight: 600, color: 'var(--text-dim)', marginBottom: 8 }}>
      Permissions
    </h4>
    <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
      {modulePerms.map((perm) => (
        <div key={perm} style={{
          display: 'flex', alignItems: 'center', gap: 8,
          padding: '6px 8px', background: 'var(--bg-3,#1c1e17)',
          borderRadius: 4, fontSize: 11,
        }}>
          <span style={{ color: 'var(--accent,#cdf24b)' }}>✓</span>
          <span style={{ color: 'var(--text)', fontFamily: 'var(--font-mono)' }}>{perm}</span>
        </div>
      ))}
    </div>
  </div>
)}
```

**验收**：
1. 点击已安装模块，详情面板显示权限列表
2. 每个权限显示名称和授权状态

---

### P1-5：用户名引导流程缺失

**根因**：首次启动直接进入 Dashboard，无用户名设置引导。

#### 修复方案

##### 1. 新建 `src/components/onboarding/UsernameOnboarding.tsx`

```tsx
'use client';

import { useState, useCallback } from 'react';

interface UsernameOnboardingProps {
  onComplete: (username: string) => void;
}

export default function UsernameOnboarding({ onComplete }: UsernameOnboardingProps) {
  const [username, setUsername] = useState('');
  const [saving, setSaving] = useState(false);

  const handleSubmit = useCallback(async () => {
    const trimmed = username.trim();
    if (!trimmed) return;
    setSaving(true);
    try {
      await window.nativesAPI?.db?.set?.('settings:username', trimmed);
      onComplete(trimmed);
    } catch (err) {
      console.error('Failed to save username:', err);
    } finally {
      setSaving(false);
    }
  }, [username, onComplete]);

  return (
    <div style={{
      position: 'fixed', inset: 0,
      background: 'rgba(0,0,0,0.8)',
      display: 'flex', alignItems: 'center', justifyContent: 'center',
      zIndex: 100,
    }}>
      <div style={{
        background: 'var(--bg-2,#131410)',
        border: '1px solid var(--border,#262920)',
        borderRadius: 16, padding: 32, width: 380, maxWidth: '90vw',
        textAlign: 'center',
      }}>
        <h2 style={{ fontSize: 18, fontWeight: 600, color: 'var(--text)', margin: '0 0 8px' }}>
          Welcome to Natives
        </h2>
        <p style={{ fontSize: 13, color: 'var(--text-dim)', marginBottom: 24 }}>
          Choose a display name for your workspace.
        </p>
        <input
          type="text"
          value={username}
          onChange={(e) => setUsername(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && handleSubmit()}
          placeholder="Your name"
          autoFocus
          maxLength={32}
          style={{
            width: '100%', padding: '10px 14px',
            background: 'var(--bg-3,#1c1e17)',
            border: '1px solid var(--border,#262920)',
            borderRadius: 8, color: 'var(--text)', fontSize: 14,
            outline: 'none', marginBottom: 16,
          }}
        />
        <button
          className="btn btn-primary"
          onClick={handleSubmit}
          disabled={!username.trim() || saving}
          style={{ width: '100%' }}
        >
          {saving ? 'Saving...' : 'Get Started'}
        </button>
      </div>
    </div>
  );
}
```

##### 2. 在 ShellLayout.tsx 中添加首次启动检测

```tsx
import UsernameOnboarding from '@/components/onboarding/UsernameOnboarding';

// ★ 新增 state
const [needsOnboarding, setNeedsOnboarding] = useState<boolean | null>(null);

// ★ 检测是否需要引导
useEffect(() => {
  window.nativesAPI?.db?.get?.('settings:username').then((value: string | null) => {
    setNeedsOnboarding(!value);
  });
}, []);

// ★ 渲染引导组件
if (needsOnboarding === null) return null; // 加载中
if (needsOnboarding) {
  return <UsernameOnboarding onComplete={() => setNeedsOnboarding(false)} />;
}
```

**验收**：
1. 首次启动弹出用户名设置对话框
2. 输入用户名后写入 `settings:username`
3. 设置后不再弹出

---

### P1-6：错误分类器未接入通知系统

**根因**：`ShellLayout.tsx:209` 崩溃通知硬编码英文文本，未调用 `classifyError()` 获取操作提示。

#### 修复方案

**文件**：`src/components/shell/ShellLayout.tsx` 第 209-228 行

```typescript
manager.onCrash(moduleId, () => {
  console.error(`[Shell] Module ${moduleId} crashed`);
  setCrashedModules((prev) => {
    if (!prev.has(moduleId)) {
      // ★ 修复：使用错误分类器获取操作提示
      const classification = classifyError(new Error('Heartbeat timeout'));
      try {
        window.nativesAPI?.notification?.send?.(
          `Plugin ${moduleId} crashed`,
          classification.actionHint || 'The module stopped responding.',
          'error',
        );
      } catch { /* notification persistence is best-effort */ }
    }
    const next = new Set(prev);
    next.add(moduleId);
    return next;
  });
});
```

**验收**：
1. 模块崩溃时通知包含用户友好的操作提示
2. 不同错误类型显示不同的提示信息

---

## 🟡 P2 — 中（体验/架构）

---

### P2-1：环境变量删除用空字符串替代

**修复**：添加 `env:deleteVariable` IPC handler。

**文件**：`electron/main.ts`

```typescript
ipcMain.handle('env:deleteVariable', async (_event, profileId: string, key: string) => {
  try {
    const db = getDb();
    if (!db) return { ok: false, error: 'Database not available' };
    // 按 profile ID 和 key 删除
    const profile = lazyLoad('envInjector')?.getProfileById(profileId);
    if (!profile) return { ok: false, error: 'Profile not found' };
    db.prepare('DELETE FROM env_variables WHERE profile_id = ? AND key = ?').run(profile.id, key);
    return { ok: true };
  } catch (err) {
    return { ok: false, error: (err as Error).message };
  }
});
```

**文件**：`electron/preload.ts`

```typescript
deleteVariable: (profileId: string, key: string) =>
  ipcRenderer.invoke('env:deleteVariable', profileId, key),
```

**文件**：`src/app/settings/SettingsPage.tsx` 第 192 行

```typescript
// 旧：value 设为 ''
// 新：调用 delete API
await window.nativesAPI?.env?.deleteVariable?.(profileId, key);
```

---

### P2-2：XOR 加密 fallback 强度不足

**建议**：在 safeStorage 不可用时，使用 AES-256-GCM 替代 XOR。

**文件**：`src/lib/env-injector.ts` 的 `encrypt`/`decrypt` 函数

```typescript
function encrypt(text: string, encryptionKey: string): string {
  if (safeStorage && safeStorage.isEncryptionAvailable()) {
    return safeStorage.encryptString(text).toString('base64');
  }
  // ★ 替换 XOR 为 AES-256-GCM
  const crypto = require('crypto');
  const key = crypto.scryptSync(encryptionKey, 'natives-salt-v1', 32);
  const iv = crypto.randomBytes(16);
  const cipher = crypto.createCipheriv('aes-256-gcm', key, iv);
  let encrypted = cipher.update(text, 'utf8', 'base64');
  encrypted += cipher.final('base64');
  const authTag = cipher.getAuthTag().toString('base64');
  return `${iv.toString('base64')}:${authTag}:${encrypted}`;
}

function decrypt(encoded: string, encryptionKey: string): string {
  if (safeStorage && safeStorage.isEncryptionAvailable()) {
    return safeStorage.decryptString(Buffer.from(encoded, 'base64'));
  }
  // ★ 解密 AES-256-GCM
  const crypto = require('crypto');
  const [ivB64, authTagB64, data] = encoded.split(':');
  if (!ivB64 || !authTagB64 || !data) throw new Error('Invalid encrypted format');
  const key = crypto.scryptSync(encryptionKey, 'natives-salt-v1', 32);
  const iv = Buffer.from(ivB64, 'base64');
  const authTag = Buffer.from(authTagB64, 'base64');
  const decipher = crypto.createDecipheriv('aes-256-gcm', key, iv);
  decipher.setAuthTag(authTag);
  let decrypted = decipher.update(data, 'base64', 'utf8');
  decrypted += decipher.final('utf8');
  return decrypted;
}
```

---

### P2-3：模块更新仅刷新 DB 元数据

**建议**：在 `installer.ts` 的 `updateModule()` 中添加文件替换逻辑。

---

### P2-4：崩溃覆盖层文本硬编码英文

**文件**：`src/lib/iframe-manager.ts` 第 245 行

```typescript
// 旧：硬编码 "Plugin crashed. Click to reload."
// 新：接入 i18n
const crashText = typeof t === 'function' ? t('iframe.crashed') : 'Plugin crashed. Click to reload.';
```

---

### P2-5：通知面板仅 30 秒轮询

**文件**：`src/components/shell/NotificationPanel.tsx`

```typescript
// ★ 新增：监听 db-state-changed 事件实现即时通知
useEffect(() => {
  const api = window.nativesAPI;
  if (!api?.onDbStateChanged) return;
  const unsubscribe = api.onDbStateChanged((_event, channel) => {
    if (channel === 'notification:new') {
      loadNotifications(); // 立即刷新
    }
  });
  return unsubscribe;
}, [loadNotifications]);
```

---

### P2-6：状态持久化两套重复实现

**建议**：统一 `state-persistence.ts`（主进程）和 `iframe-manager.ts`（渲染进程）的实现，保留主进程版本作为单一数据源。

---

## ⚪ P3 — 低（后续迭代）

按 PRD 规划逐步实现，暂不提供详细代码方案。

---

## 执行顺序总结

```
P0-1 (终端输出)     ──┐
P0-2 (SQL 修复)       ├── 可并行，无依赖，共 ~2h
P0-3 (crypto 拆分)    ┤
P0-4 (Dashboard 权限) ┘
        │
        ▼
P1-2 (env 加密)       ──┐
P1-1 (SDK 补全)         ├── 可并行，共 ~4h
P1-3 (权限选择)         ┤
P1-5 (用户名引导)       ┤
P1-6 (错误分类)       ──┘
P1-4 (权限查看)       ── 依赖 P1-3 的 checkbox 组件
        │
        ▼
P2-* (体验优化)       ── 按优先级逐项处理，共 ~5h
```
