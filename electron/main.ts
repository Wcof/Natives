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
let iframeSandbox: typeof import('../src/lib/iframe-sandbox') | null = null;

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
      if (!iframeSandbox) iframeSandbox = require('../src/lib/iframe-sandbox');
      return iframeSandbox;
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
        return sandbox.verifyToken(moduleId, token);
      });
    }

    http.startServer().then((port: number) => {
      console.log(`[Main] HTTP server started on port ${port}`);
    }).catch((err: Error) => {
      console.error('[Main] Failed to start HTTP server:', err);
    });
  }
}

// ── IPC Channel Registrations ──

// DB CRUD — uses IPC sender context, not hardcoded moduleId
ipcMain.handle('db:get', (event, key: string) => {
  const mod = lazyLoad('db');
  // For renderer process, use a special namespace
  return mod ? mod.dbGet('__renderer__', key) : { error: 'Database not available' };
});
ipcMain.handle('db:set', (event, key: string, value: unknown) => {
  const mod = lazyLoad('db');
  if (mod) mod.dbSet('__renderer__', key, String(value));
  return { ok: true };
});
ipcMain.handle('db:delete', (event, key: string) => {
  const mod = lazyLoad('db');
  if (mod) mod.dbDelete('__renderer__', key);
  return { ok: true };
});
ipcMain.handle('db:list', (event, prefix?: string) => {
  const mod = lazyLoad('db');
  return mod ? mod.dbList('__renderer__', prefix) : [];
});

// Terminal control
ipcMain.handle('terminal:create', () => {
  const mod = lazyLoad('shell');
  if (!mod) return { sessionId: null, error: 'Shell not available' };

  // Inject default env profile if available
  const envMod = lazyLoad('envInjector');
  let env: Record<string, string> = {};
  if (envMod) {
    const defaultProfile = envMod.getDefaultProfile();
    if (defaultProfile) {
      env = envMod.getVariables(defaultProfile.name);
    }
  }

  const sessionId = mod.createSession(env);
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
  return mod ? mod.enableModule(moduleId) : { success: false };
});
ipcMain.handle('module:disable', (_event, moduleId: string) => {
  const mod = lazyLoad('installer');
  return mod ? mod.disableModule(moduleId) : { success: false };
});

// Environment injection
ipcMain.handle('env:getVariables', (_event, profileId: string) => {
  const mod = lazyLoad('envInjector');
  return mod ? mod.getVariables(profileId) : {};
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
ipcMain.handle('env:setVariable', (_event, profileName: string, key: string, value: string) => {
  const mod = lazyLoad('envInjector');
  if (mod) mod.setVariable(profileName, key, value);
  return { ok: true };
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

app.whenReady().then(() => {
  createWindow();
  initializeServices();
});

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') {
    app.quit();
  }
});

app.on('activate', () => {
  if (BrowserWindow.getAllWindows().length === 0) {
    createWindow();
  }
});
