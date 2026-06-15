import { app, BrowserWindow, ipcMain, safeStorage } from 'electron';
import * as path from 'path';

let mainWindow: BrowserWindow | null = null;

const isDev = process.env.NODE_ENV === 'development' || !app.isPackaged;

function createWindow(): void {
  mainWindow = new BrowserWindow({
    width: 1280,
    height: 800,
    minWidth: 900,
    minHeight: 600,
    show: false, // FOUC guard
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
    },
  });

  if (isDev) {
    mainWindow.loadURL('http://localhost:3000');
    mainWindow.webContents.openDevTools();
  } else {
    mainWindow.loadFile(path.join(__dirname, '..', '.next', 'standalone', 'index.html'));
  }

  // FOUC: wait for renderer to apply theme before showing
  ipcMain.on('theme-applied-ready', () => {
    if (mainWindow && !mainWindow.isVisible()) {
      mainWindow.show();
    }
  });

  mainWindow.on('closed', () => {
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
  }
}

// ── Startup ──

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

  // ★ P0-1: Forward PTY output to renderer via db-state-changed IPC
  mod.onData(sessionId, (data: string) => {
    mainWindow?.webContents.send('db-state-changed', 'terminal:data', { sessionId, data });
  });
  mod.onExit(sessionId, (exitCode: number) => {
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
      { encoding: 'utf8', timeout: 3000 }
    ).trim();
    return output ? { ok: true, cwd: output } : { ok: false };
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
  return mod ? mod.getTheme().theme : 'terminal-volt';
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

ipcMain.handle('usage:refresh', async () => {
  const mod = lazyLoad('usageTracker');
  if (!mod) return { claude: null, rtk: null, error: 'Usage tracker not available' };

  const result: { claude: unknown; rtk: unknown; codex: string; error?: string } = {
    claude: null,
    rtk: null,
    codex: 'Not available',
  };

  // Claude Usage — parse from ~/.claude/usage.json
  try {
    const os = require('os');
    const path = require('path');
    const fs = require('fs');
    const usagePath = path.join(os.homedir(), '.claude', 'usage.json');
    if (fs.existsSync(usagePath)) {
      const raw = JSON.parse(fs.readFileSync(usagePath, 'utf-8'));
      result.claude = mod.parseClaudeUsage(raw);
      const dbMod = lazyLoad('db');
      if (dbMod) {
        dbMod.dbSet('__system__', 'usage:claude', JSON.stringify(result.claude));
      }
    }
  } catch (err) {
    console.warn('[Main] usage:refresh claude error:', err);
  }

  // RTK Usage — execute `rtk gain --history`
  try {
    const { execSync } = require('child_process');
    const output = execSync('rtk gain --history', { encoding: 'utf-8', timeout: 5000 });
    result.rtk = mod.parseRtkUsage(output);
    const dbMod = lazyLoad('db');
    if (dbMod) {
      dbMod.dbSet('__system__', 'usage:rtk', JSON.stringify(result.rtk));
    }
  } catch (err) {
    console.warn('[Main] usage:refresh rtk error:', err);
  }

  return result;
});

// Skills management
ipcMain.handle('skills:enable', async (_event, skillPath: string) => {
  try {
    const skillsMod = lazyLoad('skillsManager');
    if (!skillsMod) return { success: false, error: 'Skills manager not available' };
    // Move from _disabled/ back to original location
    const deactivatedPath = skillsMod.getDeactivatedPath(skillPath);
    const fs = require('fs');
    // If the skill is currently in _disabled/, move it back
    if (deactivatedPath && fs.existsSync(deactivatedPath)) {
      const targetDir = deactivatedPath.replace('_disabled/skills/', 'skills/').replace(/\/SKILL\.md$/, '');
      if (!fs.existsSync(targetDir)) {
        fs.mkdirSync(targetDir, { recursive: true });
      }
      fs.cpSync(deactivatedPath, skillPath, { recursive: true, force: true });
      fs.rmSync(deactivatedPath.substring(0, deactivatedPath.lastIndexOf('SKILL.md')), { recursive: true, force: true });
    }
    return { success: true };
  } catch (err) {
    return { success: false, error: (err as Error).message };
  }
});
ipcMain.handle('skills:disable', async (_event, skillPath: string) => {
  try {
    const skillsMod = lazyLoad('skillsManager');
    if (!skillsMod) return { success: false, error: 'Skills manager not available' };
    const deactivatedPath = skillsMod.getDeactivatedPath(skillPath);
    const fs = require('fs');
    const path = require('path');
    if (deactivatedPath) {
      const targetDir = path.dirname(deactivatedPath);
      if (!fs.existsSync(targetDir)) {
        fs.mkdirSync(targetDir, { recursive: true });
      }
      // Move the entire skill directory to _disabled/
      const srcDir = path.dirname(skillPath);
      fs.cpSync(srcDir, targetDir, { recursive: true, force: true });
      fs.rmSync(srcDir, { recursive: true, force: true });
    }
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
    const skillDirs = [
      path.join(homedir, '.atomcode', 'skills'),
      path.join(homedir, '.claude', 'skills'),
    ];
    const skills: any[] = [];
    for (const dir of skillDirs) {
      try {
        const entries = await fsp.readdir(dir, { withFileTypes: true });
        for (const entry of entries) {
          if (!entry.isDirectory()) continue;
          const skillPath = path.join(dir, entry.name, 'SKILL.md');
          try {
            const content = await fsp.readFile(skillPath, 'utf-8');
            const health = mod.checkSkillHealth(content);
            skills.push({
              name: mod.getSkillNameFromPath(skillPath),
              path: skillPath,
              source: dir.includes('.atomcode') ? 'atomcode' : 'claude',
              ...health,
            });
          } catch { /* skill without SKILL.md */ }
        }
      } catch { /* dir may not exist */ }
    }
    return skills;
  } catch { return []; }
});

app.whenReady().then(() => {
  createWindow();
  initializeServices();
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

app.on('before-quit', () => {
  const dbMod = lazyLoad('db');
  if (dbMod) {
    try {
      dbMod.getDb().pragma('wal_checkpoint(TRUNCATE)');
    } catch { /* DB may not be initialized */ }
    dbMod.closeDb();
  }
});
