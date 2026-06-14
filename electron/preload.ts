import { contextBridge, ipcRenderer } from 'electron';

const nativesAPI = {
  // FOUC Guard — notify main process that theme is applied
  themeReady: () => ipcRenderer.send('theme-applied-ready'),

  // DB CRUD
  db: {
    get: (key: string) => ipcRenderer.invoke('db:get', key),
    set: (key: string, value: unknown) => ipcRenderer.invoke('db:set', key, value),
    delete: (key: string) => ipcRenderer.invoke('db:delete', key),
    list: (prefix?: string) => ipcRenderer.invoke('db:list', prefix),
  },

  // Terminal control
  terminal: {
    create: () => ipcRenderer.invoke('terminal:create'),
    write: (sessionId: string, data: string) => ipcRenderer.invoke('terminal:write', sessionId, data),
    resize: (sessionId: string, cols: number, rows: number) =>
      ipcRenderer.invoke('terminal:resize', sessionId, cols, rows),
    kill: (sessionId: string) => ipcRenderer.invoke('terminal:kill', sessionId),
  },

  // Module management
  module: {
    scan: () => ipcRenderer.invoke('module:scan'),
    install: (pathOrZip: string) => ipcRenderer.invoke('module:install', pathOrZip),
    uninstall: (moduleId: string) => ipcRenderer.invoke('module:uninstall', moduleId),
    list: () => ipcRenderer.invoke('module:list'),
  },

  // Environment
  env: {
    getVariables: (profileId: string) => ipcRenderer.invoke('env:getVariables', profileId),
    getDefaultProfile: () => ipcRenderer.invoke('env:getDefaultProfile'),
    listProfiles: () => ipcRenderer.invoke('env:listProfiles'),
    encrypt: (text: string) => ipcRenderer.invoke('env:encrypt', text),
    decrypt: (encrypted: string) => ipcRenderer.invoke('env:decrypt', encrypted),
  },

  // Theme
  getTheme: () => ipcRenderer.invoke('natives:getTheme'),
  setTheme: (theme: string) => ipcRenderer.invoke('natives:setTheme', theme),

  // Notifications
  notification: {
    send: (title: string, body: string, level?: string) =>
      ipcRenderer.invoke('notification:send', title, body, level || 'info'),
    list: (unreadOnly?: boolean) => ipcRenderer.invoke('notification:list', unreadOnly),
  },

  // Lifecycle
  onDbStateChanged: (callback: (_event: unknown, channel: string, data: unknown) => void) => {
    ipcRenderer.on('db-state-changed', callback);
    return () => ipcRenderer.removeListener('db-state-changed', callback);
  },
};

contextBridge.exposeInMainWorld('nativesAPI', nativesAPI);

export type NativesAPI = typeof nativesAPI;
