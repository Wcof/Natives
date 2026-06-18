import { app, BrowserWindow, ipcMain, safeStorage, shell as electronShell, Notification, session } from 'electron';
import * as path from 'path';
import * as fs from 'fs';
import * as os from 'os';

let mainWindow: BrowserWindow | null = null;

const isDev = process.env.NODE_ENV === 'development' || !app.isPackaged;

// ── Window State Persistence（对标 fanbox — 窗口位置/大小持久化）──
const WINDOW_STATE_PATH = path.join(app.getPath('userData'), 'window-state.json');

interface WindowState {
  x?: number;
  y?: number;
  width?: number;
  height?: number;
  maximized?: boolean;
}

function loadWindowState(): WindowState {
  try {
    if (fs.existsSync(WINDOW_STATE_PATH)) {
      return JSON.parse(fs.readFileSync(WINDOW_STATE_PATH, 'utf-8'));
    }
  } catch { /* ignore corrupt file */ }
  return {};
}

function saveWindowState(): void {
  if (!mainWindow || mainWindow.isDestroyed()) return;
  try {
    const bounds = mainWindow.getBounds();
    const state: WindowState = {
      x: bounds.x,
      y: bounds.y,
      width: bounds.width,
      height: bounds.height,
      maximized: mainWindow.isMaximized(),
    };
    fs.writeFileSync(WINDOW_STATE_PATH, JSON.stringify(state, null, 2), 'utf-8');
  } catch { /* best-effort, non-fatal */ }
}

let saveWindowStateTimer: ReturnType<typeof setTimeout> | null = null;
function debouncedSaveWindowState(): void {
  if (saveWindowStateTimer) clearTimeout(saveWindowStateTimer);
  saveWindowStateTimer = setTimeout(saveWindowState, 400);
}

function createWindow(): void {
  const saved = loadWindowState();
  mainWindow = new BrowserWindow({
    width: saved.width || 1280,
    height: saved.height || 800,
    x: saved.x,
    y: saved.y,
    show: false, // FOUC guard
    transparent: true,
    frame: false,
    hasShadow: true,
    resizable: true,
    minWidth: 420,
    minHeight: 450,
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
    },
  });

  // 恢复最大化状态
  if (saved.maximized) {
    mainWindow.maximize();
  }

  // 窗口移动/调整大小时持久化（debounced 400ms，对标 fanbox）
  mainWindow.on('move', debouncedSaveWindowState);
  mainWindow.on('resize', debouncedSaveWindowState);

  // ── 后台通知轮询（对标 CodePilot — 窗口隐藏时显示原生系统通知）──
  // 当窗口隐藏/最小化时，轮询未读通知并显示 OS 原生 Notification 横幅。
  // 窗口恢复可见时自动停止（渲染层接管轮询）。
  let bgNotifTimer: ReturnType<typeof setInterval> | null = null;
  const shownNotifIds = new Set<number>();

  function startBgNotifPolling(): void {
    if (bgNotifTimer) return; // 已在轮询
    bgNotifTimer = setInterval(() => {
      try {
        const notifMod = lazyLoad('bridgeNotif');
        if (!notifMod) return;
        const unread = notifMod.getNotifications({ unreadOnly: true });
        for (const n of unread) {
          if (shownNotifIds.has(n.id)) continue;
          shownNotifIds.add(n.id);
          // 显示原生系统通知
          if (Notification.isSupported()) {
            const osNotif = new Notification({
              title: n.title || 'Natives',
              body: n.body || '',
              silent: false,
            });
            osNotif.on('click', () => {
              // 点击通知 → 聚焦主窗口
              if (mainWindow && !mainWindow.isDestroyed()) {
                mainWindow.show();
                mainWindow.focus();
              }
            });
            osNotif.show();
          }
        }
      } catch { /* non-fatal */ }
    }, 5000);
  }

  function stopBgNotifPolling(): void {
    if (bgNotifTimer) {
      clearInterval(bgNotifTimer);
      bgNotifTimer = null;
    }
  }

  mainWindow.on('hide', startBgNotifPolling);
  mainWindow.on('minimize', startBgNotifPolling);
  mainWindow.on('show', stopBgNotifPolling);
  mainWindow.on('restore', stopBgNotifPolling);

  // ── External Link Handler（对标 fanbox + CodePilot — 防止 iframe 打开新窗口）──
  // 拦截 window.open 和 <a target="_blank">，http/https 用系统浏览器打开，
  // 其他协议拒绝。防止模块 iframe 创建不受控的 Electron 窗口。
  mainWindow.webContents.setWindowOpenHandler(({ url }) => {
    if (url.startsWith('http://') || url.startsWith('https://')) {
      electronShell.openExternal(url);
    }
    return { action: 'deny' };
  });

  // 防止导航离开应用（will-navigate guard）
  // 模块 iframe 或意外脚本不应让主窗口跳转到外部 URL。
  mainWindow.webContents.on('will-navigate', (event, url) => {
    const parsed = new URL(url);
    // 允许 localhost（开发服务器）和 file://（生产模式）
    if (parsed.hostname === 'localhost' || parsed.hostname === '127.0.0.1' || parsed.protocol === 'file:') return;
    event.preventDefault();
    if (url.startsWith('http://') || url.startsWith('https://')) {
      electronShell.openExternal(url);
    }
  });

  // ── Loading Splash（对标 CodePilot — 启动时立即显示加载动画）──
  // 避免用户看到空白窗口。加载完成后由 theme-applied-ready 替换。
  const LOADING_HTML = `data:text/html;charset=utf-8,${encodeURIComponent(`
    <html><head><style>
      body { margin:0; display:flex; align-items:center; justify-content:center;
             height:100vh; background:transparent; font-family:-apple-system,sans-serif; }
      .spinner { width:32px; height:32px; border:3px solid rgba(255,255,255,0.15);
                 border-top-color:rgba(255,255,255,0.6); border-radius:50%;
                 animation:spin .8s linear infinite; }
      @keyframes spin { to { transform:rotate(360deg); } }
    </style></head><body><div class="spinner"></div></body></html>
  `)}`;

  mainWindow.loadURL(LOADING_HTML);

  // FOUC: show window fallback — prevent permanent black screen
  const showTimeout = setTimeout(() => {
    if (mainWindow && !mainWindow.isVisible()) {
      console.warn('[Main] FOUC timeout — forcing window show');
      mainWindow.show();
    }
  }, 10_000);

  // 加载真实 URL（splash 替换）
  const loadRealUrl = (): void => {
    if (!mainWindow || mainWindow.isDestroyed()) return;
    if (isDev) {
      mainWindow.loadURL('http://localhost:3000').catch((err) => {
        console.error('[Main] Failed to load dev URL:', err);
        if (mainWindow) mainWindow.show();
      });
    } else {
      mainWindow.loadFile(path.join(__dirname, '..', '.next', 'standalone', 'index.html')).catch((err) => {
        console.error('[Main] Failed to load production build:', err);
        if (mainWindow) mainWindow.show();
      });
    }
  };
  // 短延迟让 splash 先渲染，再加载真实内容
  setTimeout(loadRealUrl, 150);

  // FOUC: wait for renderer to apply theme before showing
  ipcMain.on('theme-applied-ready', () => {
    clearTimeout(showTimeout);
    if (mainWindow && !mainWindow.isVisible()) {
      mainWindow.show();
    }
  });

  mainWindow.on('closed', () => {
    clearTimeout(showTimeout);
    saveWindowState(); // 关闭前保存窗口状态
    mainWindow = null;
  });
}

// ── Dynamic imports for module implementations ──

let db: typeof import('../src/main/database') | null = null;
let shell: typeof import('../src/main/shell') | null = null;
let moduleManager: typeof import('../src/main/module-manager') | null = null;
let installer: typeof import('../src/main/installer') | null = null;
let envInjector: typeof import('../src/lib/env-injector') | null = null;
let bridgeNotif: typeof import('../src/main/bridge-notification') | null = null;
let bridgeIPC: typeof import('../src/main/bridge-ipc') | null = null;
let bridgeHost: typeof import('../src/main/bridge-host') | null = null;
let httpServer: typeof import('../src/main/http-server') | null = null;
let iframeSandbox: typeof import('../src/lib/token-manager') | null = null;
let screenshot: typeof import('../src/main/screenshot') | null = null;
let releaseWizard: typeof import('../src/main/release-wizard') | null = null;
let updateChecker: typeof import('../src/main/update-checker') | null = null;
let fileManager: typeof import('../src/main/file-manager') | null = null;
let searchEngine: typeof import('../src/lib/search-engine') | null = null;
let gitModule: typeof import('../src/main/git') | null = null;
let diskUsage: typeof import('../src/main/disk-usage') | null = null;
let thumbModule: typeof import('../src/main/thumbnail') | null = null;
let agentStatus: typeof import('../src/main/agent-status') | null = null;
let sessionScanner: typeof import('../src/main/session-scanner') | null = null;
let skillsManager: typeof import('../src/main/skills-manager') | null = null;
let fileWatcher: typeof import('../src/main/file-watcher') | null = null;
let clipboard: typeof import('../src/main/clipboard') | null = null;
let usageTracker: typeof import('../src/main/usage-tracker') | null = null;
let permissionCenter: typeof import('../src/main/permission-center') | null = null;
let statePersistence: typeof import('../src/lib/state-persistence') | null = null;
let archiveModule: typeof import('../src/main/archive') | null = null;
let recentFilesModule: typeof import('../src/main/recent-files') | null = null;

function lazyLoad(module: string): any {
  switch (module) {
    case 'db':
      if (!db) {
        try {
          const m = require('../src/main/database');
          m.initDb();
          db = m;
        } catch (e) {
          console.warn('[Main] Database not available:', e);
        }
      }
      return db;
    case 'shell':
      if (!shell) shell = require('../src/main/shell');
      return shell;
    case 'moduleManager':
      if (!moduleManager) moduleManager = require('../src/main/module-manager');
      return moduleManager;
    case 'installer':
      if (!installer) installer = require('../src/main/installer');
      return installer;
    case 'envInjector':
      if (!envInjector) envInjector = require('../src/lib/env-injector');
      return envInjector;
    case 'bridgeNotif':
      if (!bridgeNotif) bridgeNotif = require('../src/main/bridge-notification');
      return bridgeNotif;
    case 'bridgeIPC':
      if (!bridgeIPC) bridgeIPC = require('../src/main/bridge-ipc');
      return bridgeIPC;
    case 'bridgeHost':
      if (!bridgeHost) bridgeHost = require('../src/main/bridge-host');
      return bridgeHost;
    case 'httpServer':
      if (!httpServer) httpServer = require('../src/main/http-server');
      return httpServer;
    case 'iframeSandbox':
      if (!iframeSandbox) iframeSandbox = require('../src/lib/token-manager');
      return iframeSandbox;
    case 'screenshot':
      if (!screenshot) screenshot = require('../src/main/screenshot');
      return screenshot;
    case 'releaseWizard':
      if (!releaseWizard) releaseWizard = require('../src/main/release-wizard');
      return releaseWizard;
    case 'updateChecker':
      if (!updateChecker) updateChecker = require('../src/main/update-checker');
      return updateChecker;
    case 'fileManager':
      if (!fileManager) fileManager = require('../src/main/file-manager');
      return fileManager;
    case 'searchEngine':
      if (!searchEngine) searchEngine = require('../src/lib/search-engine');
      return searchEngine;
    case 'gitModule':
      if (!gitModule) gitModule = require('../src/main/git');
      return gitModule;
    case 'diskUsage':
      if (!diskUsage) diskUsage = require('../src/main/disk-usage');
      return diskUsage;
    case 'thumbModule':
      if (!thumbModule) thumbModule = require('../src/main/thumbnail');
      return thumbModule;
    case 'agentStatus':
      if (!agentStatus) agentStatus = require('../src/main/agent-status');
      return agentStatus;
    case 'sessionScanner':
      if (!sessionScanner) sessionScanner = require('../src/main/session-scanner');
      return sessionScanner;
    case 'skillsManager':
      if (!skillsManager) skillsManager = require('../src/main/skills-manager');
      return skillsManager;
    case 'fileWatcher':
      if (!fileWatcher) fileWatcher = require('../src/main/file-watcher');
      return fileWatcher;
    case 'clipboard':
      if (!clipboard) clipboard = require('../src/main/clipboard');
      return clipboard;
    case 'usageTracker':
      if (!usageTracker) usageTracker = require('../src/main/usage-tracker');
      return usageTracker;
    case 'permissionCenter':
      if (!permissionCenter) permissionCenter = require('../src/main/permission-center');
      return permissionCenter;
    case 'statePersistence':
      if (!statePersistence) statePersistence = require('../src/lib/state-persistence');
      return statePersistence;
    case 'archive':
      if (!archiveModule) archiveModule = require('../src/main/archive');
      return archiveModule;
    case 'recentFiles':
      if (!recentFilesModule) recentFilesModule = require('../src/main/recent-files');
      return recentFilesModule;
  }
}

// ── Single-Instance Lock（对标 CodePilot — 防止多实例 DB 冲突）──
const gotLock = app.requestSingleInstanceLock();
if (!gotLock) {
  app.quit();
} else {
  app.on('second-instance', () => {
    // 第二个实例启动时，聚焦主窗口
    if (mainWindow) {
      if (mainWindow.isMinimized()) mainWindow.restore();
      mainWindow.focus();
    }
  });
}

// ── Startup ──

// ── Shell Environment Inheritance（对标 CodePilot — Dock 启动时获取完整 env）──
// 从 Dock/Finder 启动时 process.env 极简（无 Homebrew PATH、nvm、API keys 等）。
// 通过 login shell 获取用户完整环境并注入到 process.env。
async function inheritShellEnv(): Promise<void> {
  if (process.platform !== 'darwin') return; // 仅 macOS
  // 如果已有 HOME 和完整 PATH，跳过（从终端启动时不需要）
  if (process.env.PATH && process.env.PATH.includes('/usr/local/bin')) return;

  try {
    const { execFile } = require('child_process');
    const shell = process.env.SHELL || '/bin/zsh';
    const output: string = await new Promise((resolve, reject) => {
      execFile(shell, ['-ilc', 'env'], {
        encoding: 'utf-8',
        timeout: 10_000,
        env: {}, // 空 env 强制 login shell 加载所有 profile
      }, (err: Error | null, stdout: string) => {
        if (err) reject(err);
        else resolve(stdout);
      });
    });

    // 解析 KEY=VALUE 行，合并到 process.env（不覆盖已有的）
    for (const line of output.split('\n')) {
      const eqIdx = line.indexOf('=');
      if (eqIdx <= 0) continue;
      const key = line.slice(0, eqIdx);
      const val = line.slice(eqIdx + 1);
      if (!process.env[key]) {
        process.env[key] = val;
      }
    }
    console.log('[Main] Shell env inherited from login shell');
  } catch (err) {
    console.warn('[Main] Failed to inherit shell env (non-fatal):', err);
  }
}

// ── System Proxy Detection（对标 CodePilot — 支持 Clash/Surge 等中国 VPN 工具）──
// macOS 系统级代理（System Settings > Network > Proxies）不在 shell env 中。
// 使用 Chromium 的 resolveProxy API 检测并注入到 process.env。
async function resolveSystemProxy(): Promise<void> {
  // 如果已有代理设置，跳过
  if (process.env.HTTP_PROXY || process.env.HTTPS_PROXY || process.env.http_proxy || process.env.https_proxy) return;

  try {
    const proxyList = await session.defaultSession.resolveProxy('https://registry.npmjs.org');
    if (!proxyList || proxyList === 'DIRECT') return;

    // Chromium 返回有序列表: "PROXY host:port; SOCKS5 host:port; DIRECT"
    for (const entry of proxyList.split(';')) {
      const trimmed = entry.trim();
      if (!trimmed || trimmed === 'DIRECT') continue;

      const httpMatch = trimmed.match(/^(?:PROXY|HTTPS)\s+([\w.-]+:\d+)$/i);
      if (httpMatch) {
        process.env.HTTP_PROXY = `http://${httpMatch[1]}`;
        process.env.HTTPS_PROXY = `http://${httpMatch[1]}`;
        console.log('[Main] System proxy detected:', process.env.HTTPS_PROXY);
        return;
      }

      const socksMatch = trimmed.match(/^SOCKS5?\s+([\w.-]+:\d+)$/i);
      if (socksMatch) {
        process.env.HTTP_PROXY = `socks5://${socksMatch[1]}`;
        process.env.HTTPS_PROXY = `socks5://${socksMatch[1]}`;
        console.log('[Main] System SOCKS proxy detected:', process.env.HTTPS_PROXY);
        return;
      }
    }
  } catch (err) {
    console.warn('[Main] Failed to resolve system proxy (non-fatal):', err);
  }
}

// ── Env Sanitization（对标 CodePilot — 防止 __NEXT_PRIVATE_* 泄漏到子进程）──
// Next.js 运行时设置的 __NEXT_PRIVATE_* 变量如果泄漏到其他 Next.js 项目，
// 会导致那些项目跳过自身配置加载。
function sanitizedProcessEnv(): Record<string, string> {
  const env: Record<string, string> = {};
  for (const [key, val] of Object.entries(process.env)) {
    if (key.startsWith('__NEXT_PRIVATE_')) continue;
    if (val !== undefined) env[key] = val;
  }
  return env;
}

function initializeServices(): void {
  // Initialize database
  lazyLoad('db');

  // Start HTTP server with bridge handler
  const http = lazyLoad('httpServer');
  const bridge = lazyLoad('bridgeHost');
  const sandbox = lazyLoad('iframeSandbox');

  if (http && bridge) {
    // Wire bridge handler to HTTP server
    http.setBridgeHandler(bridge.handleBridgeRequest);

    // Wire token verifier
    if (sandbox) {
      http.setTokenVerifier((moduleId: string, token: string) => {
        return sandbox.validateSessionToken(token, moduleId);
      });
    }

    http.startServer().then((port: number) => {
      console.log(`[Main] HTTP server started on port ${port}`);
    }).catch((err: Error) => {
      console.error('[Main] Failed to start HTTP server:', err);
    });
  }

  // Auto-scan modules on startup (US13)
  const mm = lazyLoad('moduleManager');
  if (mm) {
    try {
      mm.syncModulesToDb();
      console.log('[Main] Modules synced to database on startup');
    } catch (err) {
      console.warn('[Main] Failed to sync modules on startup:', err);
    }
  }
}

// ── IPC Channel Registrations ──

// App version — single source of truth from package.json (replaces the
// hardcoded '0.1.0' that drifted out of sync). See ISSUE-2.
ipcMain.handle('app:version', () => {
  try {
    // eslint-disable-next-line @typescript-eslint/no-require-imports
    return require('../package.json').version as string;
  } catch {
    return '0.0.0';
  }
});

// DB CRUD — uses IPC sender context, not hardcoded moduleId
ipcMain.handle('db:get', (event, key: string) => {
  const mod = lazyLoad('db');
  if (!mod) return { error: 'Database not available' };
  const value = mod.dbGet('__renderer__', key);
  return { value };
});
ipcMain.handle('db:set', (event, key: string, value: unknown) => {
  const mod = lazyLoad('db');
  if (!mod) return { ok: false, error: 'Database not available' };
  try {
    mod.dbSet('__renderer__', key, String(value));
    mainWindow?.webContents.send('db-state-changed', 'module_data', { key });
    return { ok: true };
  } catch (err) {
    return { ok: false, error: (err as Error).message };
  }
});
ipcMain.handle('db:delete', (event, key: string) => {
  const mod = lazyLoad('db');
  if (!mod) return { ok: false, error: 'Database not available' };
  mod.dbDelete('__renderer__', key);
  mainWindow?.webContents.send('db-state-changed', 'module_data', { key });
  return { ok: true };
});
ipcMain.handle('db:list', (event, prefix?: string) => {
  const mod = lazyLoad('db');
  return mod ? mod.dbList('__renderer__', prefix) : [];
});

// Terminal control
ipcMain.handle('terminal:create', async (_event, profileId?: string) => {
  const mod = lazyLoad('shell');
  if (!mod) return { sessionId: null, error: 'Shell not available' };

  const env: Record<string, string> = {};
  const sessionId = await mod.createSession(env, profileId);

  // ★ 专用终端 IPC 通道（对标 CodePilot — 避免 db-state-changed 的事件风暴）
  // 同时发送到专用通道和旧 db-state-changed 通道（向后兼容）
  mod.onData(sessionId, (data: string) => {
    mainWindow?.webContents.send('terminal:data', { sessionId, data });
    mainWindow?.webContents.send('db-state-changed', 'terminal:data', { sessionId, data });
  });
  mod.onExit(sessionId, (exitCode: number) => {
    mainWindow?.webContents.send('terminal:exit', { sessionId, exitCode });
    mainWindow?.webContents.send('db-state-changed', 'terminal:exit', { sessionId, exitCode });
  });

  return { sessionId };
});
ipcMain.handle('terminal:write', (_event, sessionId: string, data: string) => {
  const mod = lazyLoad('shell');
  if (mod) mod.write(sessionId, data);
});
ipcMain.handle('terminal:resize', (_event, sessionId: string, cols: number, rows: number) => {
  const mod = lazyLoad('shell');
  if (mod) mod.resize(sessionId, cols, rows);
});
ipcMain.handle('terminal:kill', (_event, sessionId: string) => {
  const mod = lazyLoad('shell');
  if (mod) mod.killSession(sessionId);
});

// Archive
ipcMain.handle('archive:list', (_event, archivePath: string) => {
  try {
    const mod = lazyLoad('archive');
    if (!mod) return { entries: [], truncated: false, totalSize: 0 };
    return mod.listArchive(archivePath);
  } catch (err) {
    return { entries: [], truncated: false, totalSize: 0, error: (err as Error).message };
  }
});

// Terminal CWD detection (macOS — uses lsof)
// 对标 fanbox：GUI 启动的 app 不继承 shell locale，lsof 对 CJK 路径输出 \xe8 字节。
// 修复：传 LC_ALL=en_US.UTF-8 + \xNN 解码回退。
ipcMain.handle('terminal:cwd', async (_event, sessionId: string) => {
  const mod = lazyLoad('shell');
  if (!mod) return { ok: false };
  const sessions = mod.getActiveSessions();
  const session = sessions.find((s: { id: string }) => s.id === sessionId);
  if (!session?.pid) return { ok: false };
  try {
    const { execSync } = require('child_process');
    const output = execSync(
      `lsof -p ${session.pid} -Fn 2>/dev/null | grep '^n/' | tail -1 | sed 's/^n//'`,
      { encoding: 'utf8', timeout: 3000, env: { ...process.env, LC_ALL: 'en_US.UTF-8' } }
    ).trim();
    if (!output) return { ok: false };
    // \xNN 解码回退（lsof 可能输出原始字节而非 UTF-8）
    // 对标 fanbox decodeLsofPath：收集字节到 Buffer 再整体 UTF-8 解码
    // String.fromCharCode 对多字节 CJK（如 用=\xe7\x94\xa8）会产出乱码
    let decoded = output;
    if (/\\x[0-9a-fA-F]{2}/.test(output)) {
      const parts = output.split(/(\\x[0-9a-fA-F]{2})/);
      const raw: number[] = [];
      for (const part of parts) {
        const m = part.match(/^\\x([0-9a-fA-F]{2})$/);
        if (m) {
          raw.push(parseInt(m[1]!, 16));
        } else {
          for (let j = 0; j < part.length; j++) raw.push(part.charCodeAt(j));
        }
      }
      decoded = Buffer.from(raw).toString('utf8');
    }
    return { ok: true, cwd: decoded };
  } catch {
    return { ok: false };
  }
});

// Module management
ipcMain.handle('module:scan', () => {
  const mod = lazyLoad('moduleManager');
  if (!mod) return [];
  mod.syncModulesToDb();
  return mod.scanModules();
});
ipcMain.handle('module:install', (_event, pathOrZip: string) => {
  const mod = lazyLoad('installer');
  return mod ? mod.installModule(pathOrZip) : { success: false, error: 'Installer not available' };
});
ipcMain.handle('module:readManifest', (_event, source: string) => {
  const mod = lazyLoad('installer');
  if (!mod) return { error: 'Installer not available' };
  return mod.readManifestFromSource(source);
});
ipcMain.handle('module:grantPermission', async (_event, moduleId: string, permission: string) => {
  const mod = lazyLoad('permissionCenter');
  if (!mod) return { ok: false, error: 'Permission center not available' };
  try {
    mod.grantPermissionWithAudit(moduleId, permission, 'IPC grant');
    return { ok: true };
  } catch (err) {
    return { ok: false, error: (err as Error).message };
  }
});
ipcMain.handle('module:revokePermission', async (_event, moduleId: string, permission: string) => {
  const mod = lazyLoad('permissionCenter');
  if (!mod) return { ok: false, error: 'Permission center not available' };
  try {
    mod.revokePermissionWithAudit(moduleId, permission, 'IPC revoke');
    return { ok: true };
  } catch (err) {
    return { ok: false, error: (err as Error).message };
  }
});
ipcMain.handle('module:listPermissions', async (_event, moduleId: string) => {
  const mod = lazyLoad('permissionCenter');
  if (!mod) return [];
  return mod.listModulePermissions(moduleId);
});
ipcMain.handle('module:getAuditLog', async (_event, moduleId?: string, limit?: number) => {
  const mod = lazyLoad('permissionCenter');
  if (!mod) return [];
  return mod.getAuditLog(moduleId, limit);
});
ipcMain.handle('module:approveAllPermissions', async (_event, moduleId: string) => {
  const mod = lazyLoad('permissionCenter');
  if (!mod) return { ok: false, error: 'Permission center not available' };
  try {
    mod.approveAllPermissions(moduleId, 'Approved on module activation');
    return { ok: true };
  } catch (err) {
    return { ok: false, error: (err as Error).message };
  }
});
ipcMain.handle('module:uninstall', (_event, moduleId: string) => {
  const mod = lazyLoad('installer');
  return mod ? mod.uninstallModule(moduleId) : { success: false, error: 'Installer not available' };
});
ipcMain.handle('module:list', () => {
  const mod = lazyLoad('installer');
  return mod ? mod.getInstalledModules() : [];
});
ipcMain.handle('module:enable', (_event, moduleId: string) => {
  const mod = lazyLoad('installer');
  if (!mod) return { success: false, error: 'Installer not available' };
  mod.enableModule(moduleId);
  return { success: true };
});
ipcMain.handle('module:disable', (_event, moduleId: string) => {
  const mod = lazyLoad('installer');
  if (!mod) return { success: false, error: 'Installer not available' };
  mod.disableModule(moduleId);
  return { success: true };
});
ipcMain.handle('module:update', (_event, moduleId: string) => {
  const mod = lazyLoad('installer');
  return mod ? mod.updateModule(moduleId) : { success: false, error: 'Installer not available' };
});

// Environment injection
ipcMain.handle('env:getVariables', async (_event, profileId: string) => {
  const mod = lazyLoad('envInjector');
  return mod ? await mod.getVariables(profileId) : {};
});
ipcMain.handle('env:getDefaultProfile', () => {
  const mod = lazyLoad('envInjector');
  return mod ? mod.getDefaultProfile() : null;
});
ipcMain.handle('env:listProfiles', () => {
  const mod = lazyLoad('envInjector');
  return mod ? mod.listProfiles() : [];
});
ipcMain.handle('env:createProfile', (_event, name: string) => {
  const mod = lazyLoad('envInjector');
  if (mod) mod.createProfile(name);
  return { ok: true };
});
ipcMain.handle('env:deleteProfile', (_event, name: string) => {
  const mod = lazyLoad('envInjector');
  if (mod) mod.deleteProfile(name);
  return { ok: true };
});
ipcMain.handle('env:setDefaultProfile', (_event, name: string) => {
  const mod = lazyLoad('envInjector');
  if (mod) mod.setDefaultProfile(name);
  return { ok: true };
});
ipcMain.handle('env:setVariable', async (_event, profileName: string, key: string, value: string) => {
  try {
    const mod = lazyLoad('envInjector');
    if (!mod) return { ok: false, error: 'Env injector not available' };
    await mod.setVariable(profileName, key, value);
    return { ok: true };
  } catch (err) {
    return { ok: false, error: (err as Error).message };
  }
});
ipcMain.handle('env:deleteVariable', async (_event, profileName: string, key: string) => {
  try {
    const mod = lazyLoad('envInjector');
    if (!mod) return { ok: false, error: 'Env injector not available' };
    const db = lazyLoad('db')?.getDb?.();
    if (!db) return { ok: false, error: 'Database not available' };
    const profile = db.prepare('SELECT id FROM env_profiles WHERE name = ?').get(profileName) as { id: number } | undefined;
    if (!profile) return { ok: false, error: 'Profile not found' };
    db.prepare('DELETE FROM env_variables WHERE profile_id = ? AND key = ?').run(profile.id, key);
    return { ok: true };
  } catch (err) {
    return { ok: false, error: (err as Error).message };
  }
});

// Notifications
ipcMain.handle('notification:send', (_event, title: string, body: string, level: string) => {
  const mod = lazyLoad('bridgeNotif');
  if (mod) mod.sendNotification('__system__', title, body, level as 'info' | 'warning' | 'error');
  return { ok: true };
});
ipcMain.handle('notification:list', (_event, unreadOnly?: boolean) => {
  const mod = lazyLoad('bridgeNotif');
  return mod ? mod.getNotifications({ unreadOnly }) : [];
});
ipcMain.handle('notification:markRead', (_event, notificationId: number) => {
  const mod = lazyLoad('bridgeNotif');
  if (mod) mod.markAsRead(notificationId);
  return { ok: true };
});
ipcMain.handle('notification:markAllRead', (_event) => {
  const mod = lazyLoad('bridgeNotif');
  if (mod) mod.markAllAsRead();
  return { ok: true };
});

// Theme
ipcMain.handle('natives:getTheme', () => {
  const mod = lazyLoad('bridgeHost');
  return mod ? mod.getTheme().theme : 'editorial';
});
ipcMain.handle('natives:setTheme', (_event, theme: string) => {
  const mod = lazyLoad('bridgeHost');
  if (mod) mod.setTheme(theme);
  // Broadcast theme change to renderer
  if (mainWindow) {
    mainWindow.webContents.send('theme-changed', theme);
  }
  return { ok: true };
});

// Locale
ipcMain.handle('natives:getLocale', () => {
  const mod = lazyLoad('bridgeHost');
  return mod ? mod.getLocale().locale : 'zh-CN';
});
ipcMain.handle('natives:setLocale', (_event, locale: string) => {
  const mod = lazyLoad('bridgeHost');
  if (mod) mod.setLocale(locale);
  return { ok: true };
});

// Environment variable encryption via safeStorage
ipcMain.handle('env:encrypt', (_event, plaintext: string) => {
  try {
    if (!safeStorage.isEncryptionAvailable()) {
      return { error: 'Encryption not available on this platform' };
    }
    const encrypted = safeStorage.encryptString(plaintext);
    return { data: encrypted.toString('base64') };
  } catch (e) {
    console.error('[Main] env:encrypt failed:', e);
    return { error: 'Encryption failed' };
  }
});
ipcMain.handle('env:decrypt', (_event, encryptedBase64: string) => {
  try {
    if (!safeStorage.isEncryptionAvailable()) {
      return { error: 'Encryption not available on this platform' };
    }
    const buffer = Buffer.from(encryptedBase64, 'base64');
    const decrypted = safeStorage.decryptString(buffer);
    return { data: decrypted };
  } catch (e) {
    console.error('[Main] env:decrypt failed:', e);
    return { error: 'Decryption failed' };
  }
});

// Clipboard
ipcMain.handle('clipboard:write', (_event, text: string) => {
  const mod = lazyLoad('clipboard');
  if (mod) return mod.copyToClipboard(text);
  // Fallback: use Electron clipboard directly
  try {
    const { clipboard } = require('electron');
    clipboard.writeText(text);
    return true;
  } catch { return false; }
});
ipcMain.handle('clipboard:read', () => {
  try {
    const { clipboard } = require('electron');
    return clipboard.readText();
  } catch { return ''; }
});

// ════════════════════════════════════════
// State Persistence IPC (US16)
// ════════════════════════════════════════

ipcMain.handle('state:save', async (_event, moduleId: string, stateJson: string) => {
  try {
    const mod = lazyLoad('statePersistence');
    if (mod) {
      mod.saveModuleState(moduleId, JSON.parse(stateJson));
      return { ok: true };
    }
    // Fallback: direct db write
    const dbMod = lazyLoad('db');
    if (dbMod) {
      dbMod.dbSet('__system__', `_state:${moduleId}`, stateJson);
      return { ok: true };
    }
    return { ok: false, error: 'State persistence not available' };
  } catch (err) {
    return { ok: false, error: (err as Error).message };
  }
});

ipcMain.handle('state:load', async (_event, moduleId: string) => {
  try {
    const mod = lazyLoad('statePersistence');
    if (mod) {
      const state = mod.loadModuleState(moduleId);
      return state ? JSON.stringify(state) : null;
    }
    // Fallback: direct db read
    const dbMod = lazyLoad('db');
    if (dbMod) {
      const val = dbMod.dbGet('__system__', `_state:${moduleId}`);
      return val || null;
    }
    return null;
  } catch {
    return null;
  }
});

ipcMain.handle('state:clear', async (_event, moduleId: string) => {
  try {
    const mod = lazyLoad('statePersistence');
    if (mod) {
      mod.clearModuleState(moduleId);
      return { ok: true };
    }
    const dbMod = lazyLoad('db');
    if (dbMod) {
      dbMod.dbDelete('__system__', `_state:${moduleId}`);
      return { ok: true };
    }
    return { ok: false };
  } catch {
    return { ok: false };
  }
});

// ════════════════════════════════════════
// Usage Tracking IPC (US72-74)
// ════════════════════════════════════════

// 结果缓存（对标 fanbox 的 30s 缓存，避免频繁磁盘 I/O）
let usageCache: { at: number; data: unknown } | null = null;
const USAGE_CACHE_TTL = 30_000; // 30 秒

// stats-cache.json 文件元数据缓存（跳过未变更文件的重解析）
let statsCacheMeta: { size: number; mtimeMs: number; data: unknown } | null = null;

ipcMain.handle('usage:refresh', async () => {
  const mod = lazyLoad('usageTracker');
  if (!mod) return { claude: null, rtk: null, error: 'Usage tracker not available' };

  // 检查结果缓存
  if (usageCache && Date.now() - usageCache.at < USAGE_CACHE_TTL) {
    return usageCache.data;
  }

  const result: { claude: unknown; rtk: unknown; codex: string; error?: string } = {
    claude: null,
    rtk: null,
    codex: 'Not available',
  };

  // Claude Usage — parse from ~/.claude/stats-cache.json（真实数据源）
  // 使用 mtime+size 检查，文件未变更时跳过重解析
  try {
    const os = require('os');
    const path = require('path');
    const fs = require('fs');
    const statsPath = path.join(os.homedir(), '.claude', 'stats-cache.json');
    if (fs.existsSync(statsPath)) {
      const st = fs.statSync(statsPath);
      if (statsCacheMeta && statsCacheMeta.size === st.size && statsCacheMeta.mtimeMs === st.mtimeMs) {
        result.claude = statsCacheMeta.data;
      } else {
        const raw = JSON.parse(fs.readFileSync(statsPath, 'utf-8'));
        result.claude = mod.parseClaudeStatsCache(raw);
        statsCacheMeta = { size: st.size, mtimeMs: st.mtimeMs, data: result.claude };
      }
      const dbMod = lazyLoad('db');
      if (dbMod) {
        dbMod.dbSet('__system__', 'usage:claude', JSON.stringify(result.claude));
      }
    }
  } catch (err) {
    console.warn('[Main] usage:refresh claude error:', err);
  }

  // RTK Usage — execute `rtk gain --history`（异步，不阻塞主进程）
  try {
    const { exec } = require('child_process');
    const output: string = await new Promise((resolve, reject) => {
      exec('rtk gain --history', { encoding: 'utf-8', timeout: 5000 }, (err: Error | null, stdout: string) => {
        if (err) reject(err);
        else resolve(stdout);
      });
    });
    result.rtk = mod.parseRtkUsage(output);
    const dbMod = lazyLoad('db');
    if (dbMod) {
      dbMod.dbSet('__system__', 'usage:rtk', JSON.stringify(result.rtk));
    }
  } catch (err) {
    console.warn('[Main] usage:refresh rtk error:', err);
  }

  // 更新结果缓存
  usageCache = { at: Date.now(), data: result };
  return result;
});

// Skills management — 使用 skills-manager 的 symlink-safe toggleSkill（对标 fanbox）
ipcMain.handle('skills:enable', async (_event, skillPath: string) => {
  try {
    const skillsMod = lazyLoad('skillsManager');
    if (!skillsMod) return { success: false, error: 'Skills manager not available' };
    await skillsMod.toggleSkill(skillPath, true);
    return { success: true };
  } catch (err) {
    return { success: false, error: (err as Error).message };
  }
});
ipcMain.handle('skills:disable', async (_event, skillPath: string) => {
  try {
    const skillsMod = lazyLoad('skillsManager');
    if (!skillsMod) return { success: false, error: 'Skills manager not available' };
    await skillsMod.toggleSkill(skillPath, false);
    return { success: true };
  } catch (err) {
    return { success: false, error: (err as Error).message };
  }
});
ipcMain.handle('skills:getDeactivatedPath', async (_event, skillPath: string) => {
  const skillsMod = lazyLoad('skillsManager');
  if (!skillsMod) return null;
  return skillsMod.getDeactivatedPath(skillPath);
});

ipcMain.handle('skills:uninstall', async (_event, skillPath: string) => {
  try {
    const fs = require('fs');
    const path = require('path');
    const srcDir = path.dirname(skillPath);
    if (fs.existsSync(srcDir)) {
      fs.rmSync(srcDir, { recursive: true, force: true });
    }
    // Also remove _disabled/ copy if exists
    const skillsMod = lazyLoad('skillsManager');
    if (skillsMod) {
      const deactivatedPath = skillsMod.getDeactivatedPath(skillPath);
      if (deactivatedPath) {
        const disabledDir = path.dirname(deactivatedPath);
        if (fs.existsSync(disabledDir)) {
          fs.rmSync(disabledDir, { recursive: true, force: true });
        }
      }
    }
    return { success: true };
  } catch (err) {
    return { success: false, error: (err as Error).message };
  }
});

// ════════════════════════════════════════
// Phase 3: Screenshot IPC
// ════════════════════════════════════════

let screenshotWatcher: (() => void) | null = null;

ipcMain.on('screenshot:start-watching', () => {
  const mod = lazyLoad('screenshot');
  if (!mod) return;
  // Avoid duplicate watchers
  if (screenshotWatcher) screenshotWatcher();
  screenshotWatcher = mod.watchScreenshotDir((filePath: string) => {
    mainWindow?.webContents.send('screenshot:detected', filePath);
  });
});

ipcMain.on('screenshot:stop-watching', () => {
  if (screenshotWatcher) {
    screenshotWatcher();
    screenshotWatcher = null;
  }
});

ipcMain.handle('screenshot:save-annotated', (_event, dataUrl: string, targetPath?: string) => {
  const mod = lazyLoad('screenshot');
  if (!mod) return { success: false, error: 'Screenshot module not available' };
  try {
    const result = mod.saveAnnotatedImage(dataUrl, targetPath);
    return { success: true, path: result };
  } catch (err) {
    return { success: false, error: (err as Error).message };
  }
});

// ════════════════════════════════════════
// Phase 3: Release Wizard IPC
// ════════════════════════════════════════

ipcMain.handle('release:inspect', async (_event, projectPath: string) => {
  const mod = lazyLoad('releaseWizard');
  if (!mod) return { error: 'Release wizard not available' };
  try {
    return await mod.inspectProject(projectPath);
  } catch (err) {
    return { error: (err as Error).message };
  }
});

ipcMain.handle('release:prepare', async (_event, projectPath: string, version: string) => {
  const mod = lazyLoad('releaseWizard');
  if (!mod) return { success: false, error: 'Release wizard not available' };
  try {
    await mod.prepareRelease(projectPath, version);
    return { success: true };
  } catch (err) {
    return { success: false, error: (err as Error).message };
  }
});

ipcMain.handle('release:getSequence', (_event, projectPath: string, version: string) => {
  const mod = lazyLoad('releaseWizard');
  if (!mod) return { steps: [] };
  return mod.getCommandSequence(projectPath, version);
});

ipcMain.handle('release:execute', async (_event, projectPath: string, command: string) => {
  // Execute release command via shell
  const shellMod = lazyLoad('shell');
  if (!shellMod) return { success: false, error: 'Shell not available' };
  try {
    const { execSync } = require('child_process');
    const output = execSync(command, { cwd: projectPath, encoding: 'utf-8', timeout: 120000 });
    return { success: true, output: output.trim() };
  } catch (err) {
    const error = err as { stdout?: string; stderr?: string; message?: string };
    return {
      success: false,
      error: error.message || 'Command failed',
      output: (error.stdout || error.stderr || '').toString().trim(),
    };
  }
});

// ════════════════════════════════════════
// Phase 3: Update Checker IPC
// ════════════════════════════════════════

ipcMain.handle('update:check', async () => {
  const mod = lazyLoad('updateChecker');
  if (!mod) return null;
  try {
    const pkg = require('../package.json');
    const currentVersion = pkg.version || '0.1.0';
    const result = await mod.checkForUpdate(currentVersion, 'Wcof', 'Natives');
    return result;
  } catch {
    return null;
  }
});

ipcMain.handle('update:mute', (_event, version: string) => {
  const mod = lazyLoad('updateChecker');
  if (mod) mod.muteVersion(version);
  return { ok: true };
});

ipcMain.handle('update:getMuted', () => {
  const mod = lazyLoad('updateChecker');
  return mod ? mod.getMutedVersions() : [];
});

// ════════════════════════════════════════
// File System IPC (Phase 2)
// ════════════════════════════════════════

ipcMain.handle('fs:listDir', async (_event, dirPath: string, options?: any) => {
  const mod = lazyLoad('fileManager');
  if (!mod) return [];
  try { return await mod.listDir(dirPath, options); }
  catch (err) { console.error('[Main] fs:listDir error:', err); return []; }
});

ipcMain.handle('fs:readFile', async (_event, filePath: string) => {
  const mod = lazyLoad('fileManager');
  if (!mod) return null;
  try { return await mod.readFile(filePath); }
  catch (err) { console.error('[Main] fs:readFile error:', err); return null; }
});

ipcMain.handle('fs:writeFileAtomic', async (_event, filePath: string, content: string, expectedMtime?: number) => {
  const mod = lazyLoad('fileManager');
  if (!mod) return { ok: false, error: 'File manager not available' };
  try { await mod.writeFileAtomic(filePath, content, expectedMtime); return { ok: true }; }
  catch (err) { return { ok: false, error: (err as Error).message }; }
});

ipcMain.handle('fs:createEntry', async (_event, targetPath: string, type: string) => {
  const mod = lazyLoad('fileManager');
  if (!mod) return { ok: false, error: 'File manager not available' };
  try { await mod.createEntry(targetPath, type); return { ok: true }; }
  catch (err) { return { ok: false, error: (err as Error).message }; }
});

ipcMain.handle('fs:renameEntry', async (_event, oldPath: string, newPath: string) => {
  const mod = lazyLoad('fileManager');
  if (!mod) return { ok: false, error: 'File manager not available' };
  try { await mod.renameEntry(oldPath, newPath); return { ok: true }; }
  catch (err) { return { ok: false, error: (err as Error).message }; }
});

ipcMain.handle('fs:trashEntry', async (_event, filePath: string) => {
  const mod = lazyLoad('fileManager');
  if (!mod) return { ok: false, error: 'File manager not available' };
  try { await mod.trashEntry(filePath); return { ok: true }; }
  catch (err) { return { ok: false, error: (err as Error).message }; }
});

ipcMain.handle('fs:moveEntry', async (_event, from: string, to: string) => {
  const mod = lazyLoad('fileManager');
  if (!mod) return { ok: false, error: 'File manager not available' };
  try { await mod.moveEntry(from, to); return { ok: true }; }
  catch (err) { return { ok: false, error: (err as Error).message }; }
});

ipcMain.handle('fs:importFiles', async (_event, sourcePaths: string[], destDir: string) => {
  const mod = lazyLoad('fileManager');
  if (!mod) return { ok: false, error: 'File manager not available' };
  try { const count = await mod.importFiles(sourcePaths, destDir); return { ok: true, count }; }
  catch (err) { return { ok: false, error: (err as Error).message }; }
});

ipcMain.handle('fs:recentFiles', async (_event, root: string) => {
  const mod = lazyLoad('recentFiles');
  if (!mod) return [];
  try { return await mod.getRecentModifiedFiles(root); }
  catch (err) { console.error('[Main] fs:recentFiles error:', err); return []; }
});

// ════════════════════════════════════════
// Search IPC (Phase 2)
// ════════════════════════════════════════

ipcMain.handle('search:grep', async (_event, query: string, root: string, options?: any) => {
  const mod = lazyLoad('searchEngine');
  if (!mod) return [];
  try { return await mod.grepContent(query, root, options); }
  catch (err) { console.error('[Main] search:grep error:', err); return []; }
});

ipcMain.handle('search:files', async (_event, query: string, root: string, options?: any) => {
  const mod = lazyLoad('searchEngine');
  if (!mod) return [];
  try { return await mod.searchFiles(query, root, options); }
  catch (err) { console.error('[Main] search:files error:', err); return []; }
});

ipcMain.handle('search:spotlight', async (_event, query: string, root: string) => {
  const mod = lazyLoad('searchEngine');
  if (!mod) return [];
  try { return await mod.spotlightSearch(query, root); }
  catch (err) { console.error('[Main] search:spotlight error:', err); return []; }
});

// ════════════════════════════════════════
// File Watcher IPC（对标 fanbox — 激活 file-watcher.ts 的多目录监听）
// ════════════════════════════════════════

const fileWatchers = new Map<string, () => void>(); // dir -> stop function

function startFileWatch(dir: string): void {
  if (fileWatchers.has(dir) || !dir) return;
  try {
    const mod = lazyLoad('fileWatcher');
    if (!mod) return;
    const stop = mod.startFileWatcher(dir, (event: any) => {
      if (mainWindow && !mainWindow.isDestroyed()) {
        mainWindow.webContents.send('fs:changed', event);
      }
    });
    fileWatchers.set(dir, stop);
  } catch { /* 无权限等，跳过 */ }
}

ipcMain.handle('fs:watch-set', (_event, { dirs }: { dirs: string[] }) => {
  const want = new Set((dirs || []).filter(Boolean));
  // 关闭不再需要的
  for (const [dir, stop] of fileWatchers) {
    if (!want.has(dir)) { try { stop(); } catch { /* */ } fileWatchers.delete(dir); }
  }
  // 启动新增的
  for (const dir of want) startFileWatch(dir);
  return { ok: true, count: fileWatchers.size };
});

ipcMain.handle('fs:watch', (_event, { dir }: { dir: string }) => {
  // 兼容旧单目录接口
  for (const [d, stop] of fileWatchers) {
    if (d !== dir) { try { stop(); } catch { /* */ } fileWatchers.delete(d); }
  }
  startFileWatch(dir);
  return { ok: true };
});

// ════════════════════════════════════════
// Git IPC (Phase 2)
// ════════════════════════════════════════

ipcMain.handle('git:status', async (_event, dirPath: string) => {
  const mod = lazyLoad('gitModule');
  if (!mod) return null;
  try { return await mod.getGitStatus(dirPath); }
  catch (err) { console.error('[Main] git:status error:', err); return null; }
});

ipcMain.handle('git:diff', async (_event, filePath: string) => {
  const mod = lazyLoad('gitModule');
  if (!mod) return null;
  try { return await mod.getGitDiff(filePath); }
  catch (err) { console.error('[Main] git:diff error:', err); return null; }
});

// ════════════════════════════════════════
// Disk Usage IPC (Phase 2)
// ════════════════════════════════════════

ipcMain.handle('disk:usage', async (_event, dirPath: string) => {
  const mod = lazyLoad('diskUsage');
  if (!mod) return [];
  try { return await mod.getDiskUsage(dirPath); }
  catch (err) { console.error('[Main] disk:usage error:', err); return []; }
});

// ════════════════════════════════════════
// Thumbnail IPC (Phase 2)
// ════════════════════════════════════════

ipcMain.handle('thumbnail:generate', async (_event, filePath: string, width: number) => {
  const mod = lazyLoad('thumbModule');
  if (!mod) return null;
  try {
    const result = await mod.generateThumb(filePath, width);
    if (!result) return null;
    return { buffer: result.buffer.toString('base64'), contentType: result.contentType, cached: result.cached };
  } catch (err) { console.error('[Main] thumbnail:generate error:', err); return null; }
});

// ════════════════════════════════════════
// Agent IPC (Phase 2)
// ════════════════════════════════════════

ipcMain.handle('agent:detectStatus', (_event, output: string, exitCode?: number) => {
  const mod = lazyLoad('agentStatus');
  if (!mod) return 'unknown';
  return mod.detectAgentStatus(output, exitCode);
});

// Agent scanning: scanProjects, getSessions, scanSkills
// These delegate to session-scanner + skills-manager and use file system walking
ipcMain.handle('agent:scanProjects', async () => {
  const mod = lazyLoad('sessionScanner');
  if (!mod) return [];
  try {
    // Walk common agent project dirs (~/.claude/projects, ~/.codex/projects, etc.)
    const { promises: fsp } = require('fs') as typeof import('fs');
    const possibleDirs = [
      require('path').join(require('os').homedir(), '.claude', 'projects'),
      require('path').join(require('os').homedir(), '.codex', 'projects'),
    ];
    const projects: string[] = [];
    for (const dir of possibleDirs) {
      try {
        const entries = await fsp.readdir(dir, { withFileTypes: true });
        for (const entry of entries) {
          if (entry.isDirectory()) {
            projects.push(require('path').join(dir, entry.name));
          }
        }
      } catch { /* dir may not exist */ }
    }
    return projects;
  } catch { return []; }
});

ipcMain.handle('agent:getSessions', async (_event, projectPath: string) => {
  const mod = lazyLoad('sessionScanner');
  if (!mod) return [];
  try {
    const { promises: fsp } = require('fs') as typeof import('fs');
    const sessionsDir = require('path').join(projectPath, '.claude', 'sessions');
    const files = await fsp.readdir(sessionsDir);
    const sessions: any[] = [];
    for (const file of files.slice(0, 50)) { // limit to 50 sessions
      if (!file.endsWith('.jsonl')) continue;
      try {
        const content = await fsp.readFile(require('path').join(sessionsDir, file), 'utf-8');
        const lines = content.split('\n').filter(Boolean);
        const session = mod.parseClaudeSessionFile(file.replace('.jsonl', ''), projectPath, lines);
        sessions.push(session);
      } catch { /* skip unreadable files */ }
    }
    return sessions;
  } catch { return []; }
});

ipcMain.handle('agent:scanSkills', async () => {
  const mod = lazyLoad('skillsManager');
  if (!mod) return [];
  try {
    const { promises: fsp } = require('fs') as typeof import('fs');
    const path = require('path') as typeof import('path');
    const homedir = require('os').homedir();

    // 扫描所有 SKILL_SOURCES 声明的来源（对标 fanbox 的 5 源扫描）
    const skillDirs: { dir: string; source: import('../src/types/agent').SkillSource }[] = [
      { dir: path.join(homedir, '.claude', 'skills'), source: '~/.claude/skills' },
      { dir: path.join(homedir, '.codex', 'skills'), source: '~/.codex/skills' },
      { dir: path.join(homedir, '.agents', 'skills'), source: '~/.agents/skills' },
    ];

    // project/.claude/skills — 扫描最近活跃的项目目录
    try {
      const projectsDir = path.join(homedir, '.claude', 'projects');
      const projectEntries = await fsp.readdir(projectsDir, { withFileTypes: true });
      for (const pe of projectEntries) {
        if (!pe.isDirectory()) continue;
        const projectSkillsDir = path.join(projectsDir, pe.name, '.claude', 'skills');
        try {
          await fsp.access(projectSkillsDir);
          skillDirs.push({ dir: projectSkillsDir, source: 'project/.claude/skills' });
        } catch { /* no skills dir in this project */ }
      }
    } catch { /* ~/.claude/projects may not exist */ }

    const skills: any[] = [];
    const seenPaths = new Set<string>(); // 去重（跨源可能指向同一技能）

    for (const { dir, source } of skillDirs) {
      try {
        const entries = await fsp.readdir(dir, { withFileTypes: true });
        for (const entry of entries) {
          if (!entry.isDirectory()) continue;
          const skillPath = path.join(dir, entry.name, 'SKILL.md');
          try {
            // 去重：跳过已扫描过的路径
            const realPath = await fsp.realpath(skillPath).catch(() => skillPath);
            if (seenPaths.has(realPath)) continue;
            seenPaths.add(realPath);

            const content = await fsp.readFile(skillPath, 'utf-8');
            const health = mod.checkSkillHealth(content);
            // Extract description from frontmatter（支持 BOM 和 CRLF）
            let description = '';
            const clean = content.replace(/^﻿/, '');
            const descMatch = clean.match(/^---\r?\n([\s\S]*?)\r?\n---/);
            if (descMatch) {
              const descLine = descMatch[1]?.split('\n').find((l: string) => l.startsWith('description:'));
              if (descLine) description = descLine.split(':').slice(1).join(':').trim().replace(/^["']|["']$/g, '');
            }
            // 检查是否已禁用（_disabled/ 目录）
            const deactivatedPath = mod.getDeactivatedPath(skillPath);
            const enabled = !await fsp.access(deactivatedPath).then(() => true).catch(() => false);

            skills.push({
              name: mod.getSkillNameFromPath(skillPath),
              description,
              source,
              path: skillPath,
              enabled,
              health: { ok: health.ok, issues: health.issues },
              triggerCount: 0,
              lastTriggered: undefined,
            });
          } catch { /* skill without SKILL.md */ }
        }
      } catch { /* dir may not exist */ }
    }

    // 合并真实会话统计（不造假，缺数据为 0）
    try {
      const { getSkillStats } = require('../src/main/skill-stats');
      const stats = await getSkillStats();
      for (const s of skills) {
        const st = stats[s.name];
        s.triggerCount = st?.count ?? 0;
        s.lastTriggered = st?.lastTriggered;
      }
    } catch { /* stats unavailable, keep triggerCount at default */ }
    return skills;
  } catch { return []; }
});

// ── Shell IPC (Finder reveal, open in editor, terminal open in dir) ──

ipcMain.handle('shell:showItemInFolder', async (_event, filePath: string) => {
  try {
    electronShell.showItemInFolder(filePath);
    return { ok: true };
  } catch (err) {
    return { ok: false, error: (err as Error).message };
  }
});

ipcMain.handle('shell:openPath', async (_event, filePath: string) => {
  try {
    const err = await electronShell.openPath(filePath);
    return { ok: !err, error: err || undefined };
  } catch (err) {
    return { ok: false, error: (err as Error).message };
  }
});

ipcMain.handle('terminal:openInDir', async (_event, dirPath: string) => {
  try {
    const mod = lazyLoad('shell');
    if (!mod) return { sessionId: null, error: 'Shell not available' };
    const env: Record<string, string> = {};
    const sessionId = await mod.createSession(env);
    if (sessionId) {
      mod.write(sessionId, `cd "${dirPath}"\r`);
    }
    return { sessionId };
  } catch (err) {
    return { sessionId: null, error: (err as Error).message };
  }
});

if (gotLock) {
app.whenReady().then(async () => {
  // Version-based cache clearing
  try {
    const versionFile = path.join(app.getPath('userData'), 'last-version.txt');
    const currentVersion = app.getVersion();
    const lastVersion = fs.existsSync(versionFile) ? fs.readFileSync(versionFile, 'utf-8').trim() : '';
    if (lastVersion && lastVersion !== currentVersion) {
      const { session } = require('electron');
      await session.defaultSession.clearCache();
      await session.defaultSession.clearStorageData({ storages: ['cachestorage', 'serviceworkers'] });
    }
    fs.writeFileSync(versionFile, currentVersion, 'utf-8');
  } catch { /* non-fatal */ }

  // Widget window (secondary transparent floating window)
  let widgetWindow: BrowserWindow | null = null;

  ipcMain.on('open-widget-window', () => {
    if (widgetWindow) {
      widgetWindow.focus();
      return;
    }

    widgetWindow = new BrowserWindow({
      width: 500,
      height: 620,
      transparent: true,
      frame: false,
      resizable: false,
      hasShadow: true,
      webPreferences: {
        preload: path.join(__dirname, 'preload.js'),
        contextIsolation: true,
        nodeIntegration: false,
      },
    });

    if (isDev) {
      widgetWindow.loadURL('http://localhost:3000/?mode=widget');
    } else {
      widgetWindow.loadURL(`file://${path.join(__dirname, '..', '.next', 'standalone', 'index.html')}?mode=widget`);
    }

    widgetWindow.on('closed', () => {
      widgetWindow = null;
    });
  });

  // 日志轮转（对标 CodePilot — 50MB 上限，防止磁盘无限增长）
  try {
    const logRotate = require('../src/main/log-rotate');
    logRotate.interceptConsole();
  } catch { /* non-fatal */ }

  // 先继承 shell 环境（从 Dock 启动时关键），再检测系统代理，最后初始化服务
  await inheritShellEnv();
  await resolveSystemProxy();

  // ── Localhost Proxy Bypass（对标 fanbox — 防止内部 HTTP 服务被代理拦截）──
  // Clash/Surge 等工具可能劫持 localhost 请求，导致模块 iframe 加载失败。
  try {
    await session.defaultSession.setProxy({
      mode: 'system',
      proxyBypassRules: 'localhost;127.0.0.1;[::1]',
    });
  } catch { /* non-fatal */ }

  createWindow();
  initializeServices();

  // ── Window Controls（frame:false 下自定义交通灯按钮的 IPC）──
  ipcMain.handle('window:minimize', () => {
    if (mainWindow && !mainWindow.isDestroyed()) mainWindow.minimize();
  });
  ipcMain.handle('window:maximize', () => {
    if (!mainWindow || mainWindow.isDestroyed()) return;
    if (mainWindow.isMaximized()) mainWindow.unmaximize();
    else mainWindow.maximize();
  });
  ipcMain.handle('window:close', () => {
    if (mainWindow && !mainWindow.isDestroyed()) mainWindow.close();
  });
  ipcMain.handle('window:isMaximized', () => {
    return mainWindow ? mainWindow.isMaximized() : false;
  });
});

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') {
    const dbMod = lazyLoad('db');
    if (dbMod) {
      try {
        dbMod.getDb().pragma('wal_checkpoint(TRUNCATE)');
      } catch { /* DB may not be initialized */ }
      dbMod.closeDb();
    }
    app.quit();
  }
});

app.on('activate', () => {
  if (!mainWindow || mainWindow.isDestroyed()) {
    createWindow();
  }
});

// ── Quit Confirmation（对标 fanbox — 有终端运行时确认退出）──
let isQuitting = false;

app.on('before-quit', async (event) => {
  // 已确认退出，直接清理
  if (isQuitting) {
    // 0. 关闭日志流
    try {
      const logRotate = require('../src/main/log-rotate');
      logRotate.close();
    } catch { /* non-fatal */ }

    // 1. Kill all terminal sessions
    try {
      const shellMod = lazyLoad('shell');
      if (shellMod) {
        const sessions = shellMod.getActiveSessions();
        for (const sess of sessions) {
          try { shellMod.killSession(sess.id); } catch { /* already dead */ }
        }
      }
    } catch { /* shell module not loaded */ }

    // 2. SQLite WAL checkpoint + close
    const dbMod = lazyLoad('db');
    if (dbMod) {
      try { dbMod.getDb().pragma('wal_checkpoint(TRUNCATE)'); } catch { /* */ }
      dbMod.closeDb();
    }
    return;
  }

  // 检查是否有活跃终端
  try {
    const shellMod = lazyLoad('shell');
    if (shellMod) {
      const sessions = shellMod.getActiveSessions();
      if (sessions.length > 0) {
        event.preventDefault();
        const { dialog } = require('electron');
        const result = await dialog.showMessageBox(mainWindow!, {
          type: 'warning',
          buttons: ['Quit', 'Cancel'],
          defaultId: 1,
          cancelId: 1,
          title: 'Active Terminal Sessions',
          message: `${sessions.length} terminal session(s) still running`,
          detail: 'Quitting will terminate running agent tasks. Quit anyway?',
        });
        if (result.response === 0) {
          isQuitting = true;
          app.quit();
        }
        return;
      }
    }
  } catch { /* shell module not loaded, proceed with quit */ }

  // 无活跃终端，直接退出
  isQuitting = true;

  // 2. SQLite WAL checkpoint + close
  const dbMod = lazyLoad('db');
  if (dbMod) {
    try {
      dbMod.getDb().pragma('wal_checkpoint(TRUNCATE)');
    } catch { /* DB may not be initialized */ }
    dbMod.closeDb();
  }
});

// ── Crash Breadcrumbs（对标 CodePilot 的 uncaughtExceptionMonitor） ──
// 使用 uncaughtExceptionMonitor 而非 uncaughtException —— 不抑制默认退出行为
const CRASH_DIR = path.join(os.homedir(), '.natives');
const CRASH_FILE = path.join(CRASH_DIR, 'crash-breadcrumbs.jsonl');

function logCrashBreadcrumb(type: string, detail: Record<string, unknown>): void {
  try {
    if (!fs.existsSync(CRASH_DIR)) fs.mkdirSync(CRASH_DIR, { recursive: true });
    const entry = JSON.stringify({
      ts: new Date().toISOString(),
      type,
      pid: process.pid,
      mem: process.memoryUsage(),
      ...detail,
    });
    fs.appendFileSync(CRASH_FILE, entry + '\n');
  } catch { /* best-effort, non-fatal */ }
}

process.on('uncaughtExceptionMonitor', (err) => {
  logCrashBreadcrumb('uncaughtException', {
    name: err?.name,
    message: String(err?.message ?? '').slice(0, 200),
    stack: String(err?.stack ?? '').slice(0, 500),
  });
});

// Electron-specific crash events
app.on('child-process-gone', (_event, details) => {
  logCrashBreadcrumb('child-process-gone', {
    reason: details.reason,
    name: details.name,
    serviceName: (details as any).serviceName,
  });
});

app.on('render-process-gone', (_event, webContents, details) => {
  logCrashBreadcrumb('render-process-gone', {
    reason: details.reason,
    url: webContents?.getURL?.()?.slice(0, 200),
  });
});
} // end if (gotLock)
