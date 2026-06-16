import { contextBridge, ipcRenderer } from 'electron';

const nativesAPI = {
  // FOUC Guard — notify main process that theme is applied
  themeReady: () => ipcRenderer.send('theme-applied-ready'),

  // App metadata
  app: {
    version: () => ipcRenderer.invoke('app:version'),
  },

  // DB CRUD
  db: {
    get: (key: string) => ipcRenderer.invoke('db:get', key),
    set: (key: string, value: unknown) => ipcRenderer.invoke('db:set', key, value),
    delete: (key: string) => ipcRenderer.invoke('db:delete', key),
    list: (prefix?: string) => ipcRenderer.invoke('db:list', prefix),
  },

  // Terminal control
  terminal: {
    create: (profileId?: string) => ipcRenderer.invoke('terminal:create', profileId),
    write: (sessionId: string, data: string) => ipcRenderer.invoke('terminal:write', sessionId, data),
    resize: (sessionId: string, cols: number, rows: number) =>
      ipcRenderer.invoke('terminal:resize', sessionId, cols, rows),
    kill: (sessionId: string) => ipcRenderer.invoke('terminal:kill', sessionId),
    cwd: (sessionId: string) => ipcRenderer.invoke('terminal:cwd', sessionId),
  },

  // Module management
  module: {
    scan: () => ipcRenderer.invoke('module:scan'),
    install: (pathOrZip: string) => ipcRenderer.invoke('module:install', pathOrZip),
    readManifest: (source: string) => ipcRenderer.invoke('module:readManifest', source),
    grantPermission: (moduleId: string, permission: string) => ipcRenderer.invoke('module:grantPermission', moduleId, permission),
    revokePermission: (moduleId: string, permission: string) => ipcRenderer.invoke('module:revokePermission', moduleId, permission),
    listPermissions: (moduleId: string) => ipcRenderer.invoke('module:listPermissions', moduleId),
    getAuditLog: (moduleId?: string, limit?: number) => ipcRenderer.invoke('module:getAuditLog', moduleId, limit),
    approveAllPermissions: (moduleId: string) => ipcRenderer.invoke('module:approveAllPermissions', moduleId),
    uninstall: (moduleId: string) => ipcRenderer.invoke('module:uninstall', moduleId),
    list: () => ipcRenderer.invoke('module:list'),
    enable: (moduleId: string) => ipcRenderer.invoke('module:enable', moduleId),
    disable: (moduleId: string) => ipcRenderer.invoke('module:disable', moduleId),
    update: (moduleId: string) => ipcRenderer.invoke('module:update', moduleId),
  },

  // Environment
  env: {
    getVariables: (profileId: string) => ipcRenderer.invoke('env:getVariables', profileId),
    getDefaultProfile: () => ipcRenderer.invoke('env:getDefaultProfile'),
    listProfiles: () => ipcRenderer.invoke('env:listProfiles'),
    createProfile: (name: string) => ipcRenderer.invoke('env:createProfile', name),
    deleteProfile: (name: string) => ipcRenderer.invoke('env:deleteProfile', name),
    setDefaultProfile: (name: string) => ipcRenderer.invoke('env:setDefaultProfile', name),
    setVariable: (profileId: string, key: string, value: string) =>
      ipcRenderer.invoke('env:setVariable', profileId, key, value),
    deleteVariable: (profileId: string, key: string) =>
      ipcRenderer.invoke('env:deleteVariable', profileId, key),
    encrypt: (text: string) => ipcRenderer.invoke('env:encrypt', text),
    decrypt: (encrypted: string) => ipcRenderer.invoke('env:decrypt', encrypted),
  },

  // Theme
  getTheme: () => ipcRenderer.invoke('natives:getTheme'),
  setTheme: (theme: string) => ipcRenderer.invoke('natives:setTheme', theme),

  // Locale
  getLocale: () => ipcRenderer.invoke('natives:getLocale'),
  setLocale: (locale: string) => ipcRenderer.invoke('natives:setLocale', locale),

  // Notifications
  notification: {
    send: (title: string, body: string, level?: string) =>
      ipcRenderer.invoke('notification:send', title, body, level || 'info'),
    list: (unreadOnly?: boolean) => ipcRenderer.invoke('notification:list', unreadOnly),
    markRead: (id: number) => ipcRenderer.invoke('notification:markRead', id),
    markAllAsRead: () => ipcRenderer.invoke('notification:markAllRead'),
  },

  // === File Manager IPC ===
  fs: {
    listDir: (dirPath: string, options?: any) => ipcRenderer.invoke('fs:listDir', dirPath, options),
    readFile: (filePath: string) => ipcRenderer.invoke('fs:readFile', filePath),
    writeFileAtomic: (filePath: string, content: string, expectedMtime?: number) =>
      ipcRenderer.invoke('fs:writeFileAtomic', filePath, content, expectedMtime),
    createEntry: (targetPath: string, type: string) => ipcRenderer.invoke('fs:createEntry', targetPath, type),
    renameEntry: (oldPath: string, newPath: string) => ipcRenderer.invoke('fs:renameEntry', oldPath, newPath),
    trashEntry: (filePath: string) => ipcRenderer.invoke('fs:trashEntry', filePath),
    moveEntry: (from: string, to: string) => ipcRenderer.invoke('fs:moveEntry', from, to),
    importFiles: (sourcePaths: string[], destDir: string) => ipcRenderer.invoke('fs:importFiles', sourcePaths, destDir),
    recentFiles: (root: string) => ipcRenderer.invoke('fs:recentFiles', root),
  },

  // === Archive IPC ===
  archive: {
    list: (archivePath: string) => ipcRenderer.invoke('archive:list', archivePath),
  },

  // === Search IPC ===
  search: {
    grep: (query: string, root: string, options?: any) => ipcRenderer.invoke('search:grep', query, root, options),
    files: (query: string, root: string, options?: any) => ipcRenderer.invoke('search:files', query, root, options),
    spotlight: (query: string, root: string) => ipcRenderer.invoke('search:spotlight', query, root),
  },

  // === State Persistence IPC (US16) ===
  state: {
    save: (moduleId: string, state: string) => ipcRenderer.invoke('state:save', moduleId, state),
    load: (moduleId: string) => ipcRenderer.invoke('state:load', moduleId),
    clear: (moduleId: string) => ipcRenderer.invoke('state:clear', moduleId),
  },

  // === Git IPC ===
  git: {
    status: (dirPath: string) => ipcRenderer.invoke('git:status', dirPath),
    diff: (filePath: string) => ipcRenderer.invoke('git:diff', filePath),
  },

  // === Disk Usage IPC ===
  disk: {
    usage: (dirPath: string) => ipcRenderer.invoke('disk:usage', dirPath),
  },

  // === Thumbnail IPC ===
  thumbnail: {
    generate: (filePath: string, width: number) => ipcRenderer.invoke('thumbnail:generate', filePath, width),
  },

  // === Agent IPC ===
  agent: {
    scanProjects: () => ipcRenderer.invoke('agent:scanProjects'),
    getSessions: (projectPath: string) => ipcRenderer.invoke('agent:getSessions', projectPath),
    scanSkills: () => ipcRenderer.invoke('agent:scanSkills'),
    detectStatus: (output: string, exitCode?: number) => ipcRenderer.invoke('agent:detectStatus', output, exitCode),
  },

  // Skills management
  skills: {
    enable: (path: string) => ipcRenderer.invoke('skills:enable', path),
    disable: (path: string) => ipcRenderer.invoke('skills:disable', path),
    getDeactivatedPath: (path: string) => ipcRenderer.invoke('skills:getDeactivatedPath', path),
    uninstall: (path: string) => ipcRenderer.invoke('skills:uninstall', path),
  },

  // Lifecycle
  onDbStateChanged: (callback: (_event: unknown, channel: string, data: unknown) => void) => {
    ipcRenderer.on('db-state-changed', callback);
    return () => ipcRenderer.removeListener('db-state-changed', callback);
  },

  // === Phase 3: Screenshot ===
  screenshot: {
    watch: (callback: (filePath: string) => void) => {
      const channel = 'screenshot:detected';
      const handler = (_event: unknown, filePath: string) => callback(filePath);
      ipcRenderer.on(channel, handler);
      ipcRenderer.send('screenshot:start-watching');
      return () => {
        ipcRenderer.send('screenshot:stop-watching');
        ipcRenderer.removeListener(channel, handler);
      };
    },
    saveAnnotated: (dataUrl: string, targetPath?: string) =>
      ipcRenderer.invoke('screenshot:save-annotated', dataUrl, targetPath),
  },

  // === Phase 3: Release Wizard ===
  release: {
    inspect: (projectPath: string) => ipcRenderer.invoke('release:inspect', projectPath),
    prepare: (projectPath: string, version: string) => ipcRenderer.invoke('release:prepare', projectPath, version),
    getSequence: (projectPath: string, version: string) => ipcRenderer.invoke('release:getSequence', projectPath, version),
    execute: (projectPath: string, command: string) => ipcRenderer.invoke('release:execute', projectPath, command),
  },

  // === Phase 3: Update Checker ===
  update: {
    check: () => ipcRenderer.invoke('update:check'),
    mute: (version: string) => ipcRenderer.invoke('update:mute', version),
    getMuted: () => ipcRenderer.invoke('update:getMuted'),
  },

  // Clipboard
  clipboard: {
    write: (text: string) => ipcRenderer.invoke('clipboard:write', text),
    read: () => ipcRenderer.invoke('clipboard:read'),
  },

  // Usage tracking
  usage: {
    refresh: () => ipcRenderer.invoke('usage:refresh'),
  },
};

contextBridge.exposeInMainWorld('nativesAPI', nativesAPI);

export type NativesAPI = typeof nativesAPI;
