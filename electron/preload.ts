import { contextBridge, ipcRenderer } from 'electron';

const nativesAPI = {
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
  },

  // Environment
  env: {
    getVariables: (profileId: string) => ipcRenderer.invoke('env:getVariables', profileId),
    getDefaultProfile: () => ipcRenderer.invoke('env:getDefaultProfile'),
  },

  // Theme
  getTheme: () => ipcRenderer.invoke('natives:getTheme'),

  // Lifecycle
  onThemeApplied: (callback: () => void) => {
    ipcRenderer.on('theme-applied-ready', callback);
    return () => ipcRenderer.removeListener('theme-applied-ready', callback);
  },
  onDbStateChanged: (callback: (_event: unknown, channel: string, data: unknown) => void) => {
    ipcRenderer.on('db-state-changed', callback);
    return () => ipcRenderer.removeListener('db-state-changed', callback);
  },
};

contextBridge.exposeInMainWorld('nativesAPI', nativesAPI);

export type NativesAPI = typeof nativesAPI;
