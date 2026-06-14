import { app, BrowserWindow, ipcMain } from 'electron';
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

// ── IPC Channel Registrations (placeholder) ──

// DB CRUD
ipcMain.handle('db:get', (_event, key: string) => {
  return { key };
});
ipcMain.handle('db:set', (_event, key: string, value: unknown) => {
  return { key, value };
});
ipcMain.handle('db:delete', (_event, key: string) => {
  return { key };
});
ipcMain.handle('db:list', (_event, prefix?: string) => {
  return { prefix };
});

// Terminal control
ipcMain.handle('terminal:create', () => {
  return { sessionId: 'placeholder' };
});
ipcMain.handle('terminal:write', (_event, _sessionId: string, _data: string) => {
  return;
});
ipcMain.handle('terminal:resize', (_event, _sessionId: string, _cols: number, _rows: number) => {
  return;
});
ipcMain.handle('terminal:kill', (_event, _sessionId: string) => {
  return;
});

// Module management
ipcMain.handle('module:scan', () => {
  return [];
});
ipcMain.handle('module:install', (_event, _pathOrZip: string) => {
  return { success: false };
});
ipcMain.handle('module:uninstall', (_event, _moduleId: string) => {
  return { success: false };
});

// Environment injection
ipcMain.handle('env:getVariables', (_event, _profileId: string) => {
  return {};
});
ipcMain.handle('env:getDefaultProfile', () => {
  return null;
});

// ── Theme ready IPC ──
ipcMain.handle('natives:getTheme', () => {
  return 'terminal-volt';
});

app.whenReady().then(createWindow);

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
