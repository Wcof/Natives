/**
 * Tauri adapter — replaces electron/preload.ts
 *
 * Maps window.nativesAPI methods to Tauri invoke calls.
 * Every method matches the original Electron IPC contract exactly.
 * Unimplemented commands throw "not implemented" errors, never fake success.
 */

import { invoke, convertFileSrc } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

// ── Ghostty render state payload (feature gate ghostty-vt) ──

/** Payload emitted by terminal:render-state event */
export interface RenderStatePayload {
  sessionId: string;
  cursorX: number;
  cursorY: number;
  title: string | null;
  pwd: string | null;
  cols: number;
  rows: number;
}

// --- Types matching the Electron preload contract ---

// Provider domain types
export interface ProviderKeySummary {
  id: string;
  providerId: string;
  label: string;
  maskedKey: string;
  isPrimary: boolean;
  isActive: boolean;
  status: 'untested' | 'valid' | 'invalid' | 'rate_limited' | 'unavailable';
  lastTestedAt: string | null;
  lastErrorCode: string | null;
  lastErrorMessage: string | null;
}

export interface ProviderSummary {
  id: string;
  providerType: string;
  displayName: string;
  websiteUrl: string;
  baseUrl: string;
  defaultModel: string | null;
  primaryKeyId: string | null;
  keys: ProviderKeySummary[];
}

export interface ProviderTestResult {
  success: boolean;
  status: ProviderKeySummary['status'];
  testedAt: string;
  errorCode: string | null;
  userMessage: string | null;
}

export interface ProjectSummary {
  id: string;
  path: string;
  label: string;
  conversationCount: number;
  exists: boolean;
}

export interface NativesAPI {
  themeReady: () => void;
  app: { version: () => Promise<string> };
  db: {
    get: (key: string) => Promise<unknown>;
    set: (key: string, value: unknown) => Promise<void>;
    delete: (key: string) => Promise<void>;
    list: (prefix?: string) => Promise<unknown[]>;
  };
  terminal: {
    create: (profileId?: string, cols?: number, rows?: number) => Promise<{ sessionId: string }>;
    write: (sessionId: string, data: string) => Promise<void>;
    resize: (sessionId: string, cols: number, rows: number) => Promise<void>;
    kill: (sessionId: string) => Promise<void>;
    cwd: (sessionId: string) => Promise<{ cwd: string; source: string }>;
    proc: (sessionId: string) => Promise<{ processName: string; pid: number }>;
    sessionState: (sessionId: string) => Promise<{
      sessionId: string; cols: number; rows: number;
      title: string; cwd: string; foregroundProcess: string;
      pid: number; status: string;
    }>;
    listSessions: () => Promise<Array<{
      sessionId: string; cols: number; rows: number;
      title: string; cwd: string; foregroundProcess: string;
      pid: number; status: string;
    }>>;
    onData: (callback: (data: { sessionId: string; data: string }) => void) => () => void;
    onExit: (callback: (data: { sessionId: string; exitCode: number }) => void) => () => void;
    onRenderState: (callback: (data: RenderStatePayload) => void) => () => void;
    onTitleChanged: (callback: (data: { sessionId: string; title: string }) => void) => () => void;
    onPwdChanged: (callback: (data: { sessionId: string; pwd: string }) => void) => () => void;
    onBell: (callback: (data: { sessionId: string }) => void) => () => void;
    renderState: (sessionId: string) => Promise<RenderStatePayload>;
    /** Terminal recording (asciinema v2 .cast) */
    recordStart: (sessionId: string, cols: number, rows: number) => Promise<void>;
    recordStop: (sessionId: string) => Promise<void>;
    recordList: () => Promise<unknown[]>;
    recordPlay: (id: string) => Promise<string>;
    recordExport: (id: string, format: string) => Promise<{ ok: boolean; path: string; format: string; fellBack?: string }>;
    recordPrune: () => Promise<void>;
  };
  builtinTool: {
    list: () => Promise<Array<{ id: string; enabled: boolean; driver: string }>>;
    update: (id: string, enabled: boolean, driver: string) => Promise<void>;
    seed: (id: string, driver: string) => Promise<void>;
    detect: (driver: string) => Promise<boolean>;
    launch: (driver: string) => Promise<void>;
    ghosttyIsRunning: () => Promise<boolean>;
    ghosttyFocus: () => Promise<void>;
    ghosttyLaunch: (configPath?: string) => Promise<void>;
    ghosttySyncTheme: () => Promise<string>;
    ghosttyVtAvailable: () => Promise<boolean>;
  };
  module: {
    scan: () => Promise<unknown[]>;
    install: (pathOrZip: string) => Promise<unknown>;
    readManifest: (source: string) => Promise<unknown>;
    grantPermission: (moduleId: string, permission: string) => Promise<void>;
    revokePermission: (moduleId: string, permission: string) => Promise<void>;
    listPermissions: (moduleId: string) => Promise<string[]>;
    getAuditLog: (moduleId?: string, limit?: number) => Promise<unknown[]>;
    approveAllPermissions: (moduleId: string) => Promise<void>;
    uninstall: (moduleId: string) => Promise<void>;
    list: () => Promise<unknown[]>;
    enable: (moduleId: string) => Promise<void>;
    disable: (moduleId: string) => Promise<void>;
    update: (moduleId: string) => Promise<void>;
    writeGenerated: (
      moduleId: string,
      name: string,
      htmlContent: string,
      permissions: string[],
    ) => Promise<{ moduleId: string; ok: boolean }>;
    rollback: (params: { moduleId: string; oldContent: string }) => Promise<void>;
  };
  env: {
    getVariables: (profileId: string) => Promise<unknown>;
    getDefaultProfile: () => Promise<string>;
    listProfiles: () => Promise<string[]>;
    createProfile: (name: string) => Promise<void>;
    deleteProfile: (name: string) => Promise<void>;
    setDefaultProfile: (name: string) => Promise<void>;
    setVariable: (profileId: string, key: string, value: string) => Promise<void>;
    deleteVariable: (profileId: string, key: string) => Promise<void>;
    encrypt: (text: string) => Promise<string>;
    };
  getTheme: () => Promise<string>;
  setTheme: (theme: string) => Promise<void>;
  shell: {
    showItemInFolder: (filePath: string) => Promise<void>;
    openPath: (filePath: string) => Promise<void>;
  };
  getLocale: () => Promise<string>;
  setLocale: (locale: string) => Promise<void>;
  notification: {
    send: (title: string, body: string, level?: string) => Promise<void>;
    list: (unreadOnly?: boolean) => Promise<unknown[]>;
    markRead: (id: number) => Promise<void>;
    markAllAsRead: () => Promise<void>;
  };
  fs: {
    listDir: (dirPath: string, options?: unknown) => Promise<unknown>;
    readFile: (filePath: string) => Promise<string>;
    writeFileAtomic: (filePath: string, content: string, expectedMtime?: number) => Promise<void>;
    createEntry: (targetPath: string, type: string) => Promise<{ ok: boolean }>;
    renameEntry: (oldPath: string, newPath: string) => Promise<{ ok: boolean }>;
    trashEntry: (filePath: string) => Promise<{ ok: boolean }>;
    moveEntry: (from: string, to: string) => Promise<void>;
    importFiles: (sourcePaths: string[], destDir: string) => Promise<void>;
    recentFiles: (root: string) => Promise<unknown[]>;
    saveBlob: (dir: string, name: string, base64Data: string) => Promise<string>;
    convertFileSrc: (filePath: string) => string;
  };
  archive: {
    list: (archivePath: string) => Promise<unknown[]>;
  };
  search: {
    grep: (query: string, root: string, options?: unknown) => Promise<unknown>;
    files: (query: string, root: string, options?: unknown) => Promise<unknown>;
    spotlight: (query: string, root: string) => Promise<unknown>;
  };
  state: {
    save: (moduleId: string, state: string) => Promise<void>;
    load: (moduleId: string) => Promise<string | null>;
    clear: (moduleId: string) => Promise<void>;
  };
  git: {
    status: (dirPath: string) => Promise<unknown>;
    diff: (filePath: string) => Promise<string>;
  };
  disk: {
    usage: (dirPath: string) => Promise<unknown>;
    systemInfo: () => Promise<unknown>;
    systemMetrics: () => Promise<{ cpuUsage: number; memoryUsedBytes: number; memoryTotalBytes: number }>;
  };
  thumbnail: {
    generate: (filePath: string, width: number) => Promise<string>;
  };
  agent: {
    scanProjects: () => Promise<unknown[]>;
    getSessions: (projectPath: string) => Promise<unknown[]>;
    scanSkills: () => Promise<unknown>;
    detectStatus: (output: string, exitCode?: number) => Promise<unknown>;
  };
  skills: {
    enable: (path: string) => Promise<void>;
    disable: (path: string) => Promise<void>;
    getDeactivatedPath: (path: string) => Promise<string>;
    uninstall: (path: string) => Promise<void>;
  };
  onDbStateChanged: (callback: (event: unknown, channel: string, data: unknown) => void) => () => void;
  screenshot: {
    watch: (callback: (filePath: string) => void) => () => void;
    saveAnnotated: (dataUrl: string, targetPath?: string) => Promise<string>;
  };
  release: {
    inspect: (projectPath: string) => Promise<unknown>;
    prepare: (projectPath: string, version: string) => Promise<unknown>;
    getSequence: (projectPath: string, version: string) => Promise<unknown>;
    execute: (projectPath: string, command: string) => Promise<unknown>;
  };
  update: {
    check: () => Promise<unknown>;
    mute: (version: string) => Promise<void>;
    dismiss: (version: string) => Promise<void>;
    getMuted: () => Promise<string[]>;
    getDismissed: () => Promise<string[]>;
  };
  clipboard: {
    write: (text: string) => Promise<void>;
    read: () => Promise<string>;
  };
  usage: {
    refresh: (params: { startMs: number; endMs: number; force: boolean; includeComparison: boolean; timeZone: string }) => Promise<unknown>;
  };
  codegraph: {
    read: () => Promise<unknown>;
    rtkGain: () => Promise<unknown>;
  };
  // Provider (unified API)
  provider: {
    list: () => Promise<ProviderSummary[]>;
    create: (input: {
      providerType: string;
      displayName: string;
      websiteUrl: string;
      baseUrl: string;
      defaultModel: string;
      initialKey: { label: string; apiKey: string };
    }) => Promise<ProviderSummary>;
    delete: (providerId: string) => Promise<void>;
    updateDefaults: (input: { providerId: string; defaultModel: string }) => Promise<void>;
    addKey: (input: { providerId: string; label: string; apiKey: string }) => Promise<ProviderKeySummary>;
    testCandidate: (input: { providerType: string; baseUrl: string; apiKey: string; model: string }) => Promise<ProviderTestResult>;
    testKey: (input: { providerId: string; keyId: string }) => Promise<ProviderTestResult>;
    discoverModels: (input: { providerType: string; baseUrl: string; apiKey: string }) => Promise<Array<{ id: string; displayName?: string }>>;
    setPrimaryKey: (input: { providerId: string; keyId: string }) => Promise<void>;
    deleteKey: (input: { providerId: string; keyId: string }) => Promise<void>;
  };
  windowControls: {
    minimize: () => Promise<void>;
    maximize: () => Promise<void>;
    close: () => Promise<void>;
    isMaximized: () => Promise<boolean>;
    tileWindow: (action: string) => Promise<void>;
  };
  openWidgetWindow: () => void;
  bridge: {
    getHttpPort: () => Promise<number>;
    generateToken: (moduleId: string) => Promise<string>;
    validateToken: (token: string, moduleId: string) => Promise<boolean>;
  };
  fsWatch: {
    start: (path: string) => Promise<void>;
    stop: (path: string) => Promise<void>;
    stopAll: () => Promise<void>;
    list: () => Promise<string[]>;
    onChange: (callback: (event: { path: string; kind: string }) => void) => () => void;
  };
  htmlPreview: {
    prepare: (htmlPath: string) => Promise<{ content: string; fsBase: string; serverPort: number }>;
  };
  lidGuard: {
    set: (on: boolean) => Promise<void>;
    status: () => Promise<{ sleepDisabled: boolean; terminalCount: number }>;
  };
  wechat: {
    env: () => Promise<{ target: string; cwd: string; persona: string; state: string; connected: boolean }>;
    login: () => Promise<{ qrcode: string; qrcode_img_content: string; state: string }>;
    pollLogin: (qrcode: string, verifyCode?: string) => Promise<{ state: string; error?: string }>;
    disconnect: () => Promise<{ ok: boolean }>;
    check: () => Promise<{ ok: boolean; state: string }>;
    send: (text: string) => Promise<{ ok: boolean; cid: string }>;
    setTarget: (target: string) => Promise<void>;
    setCwd: (dir: string) => Promise<void>;
    setPersona: (persona: string) => Promise<void>;
    detectAgents: () => Promise<{ claude: boolean; codex: boolean }>;
    status: () => Promise<{ state: string; connected: boolean; target: string; cwd: string }>;
  };
  plugins: {
    detect: (name: string) => Promise<string | null>;
    install: (name: string) => Promise<void>;
    uninstall: (name: string) => Promise<void>;
  };
  assistant: {
    listSessions: (params: { projectId: string | null }) => Promise<unknown>;
    getMessages: (sessionId: string) => Promise<unknown>;
    createSession: (params: { projectId: string | null; title: string; modelId: string; providerId: string }) => Promise<unknown>;
    deleteSession: (sessionId: string) => Promise<void>;
    saveMessage: (params: { sessionId: string; role: string; content: string; status: string; tokenCount: number; toolCalls?: string; toolResult?: string }) => Promise<unknown>;
    updateMessageStatus: (params: { messageId: string; status: string; toolResult?: string }) => Promise<void>;
    updateSessionTitle: (params: { sessionId: string; title: string }) => Promise<void>;
    updateSessionModel: (params: { sessionId: string; modelId: string; providerId: string }) => Promise<void>;
    streamChat: (params: { sessionId: string; model: string; messages: Array<{ role: string; content: string }> }) => Promise<void>;
    cancelStream: (sessionId: string) => Promise<void>;
  };
  /** 执行引擎设置（PRD 3.4） */
  executorSettings: {
    get: () => Promise<{ enabledTools: Record<string, boolean>; maxSelfHeal: number; maxSteps?: number }>;
    save: (settings: { enabledTools: Record<string, boolean>; maxSelfHeal: number; maxSteps?: number }) => Promise<void>;
  };
  /** Assistant in-process RPC (no daemon sidecar) */
  assistantV2: {
    request<T>(method: string, params?: unknown): Promise<T>;
    getStatus(): Promise<{ connected: boolean; error: string | null }>;
  };
  /** Project directory management */
  project: {
    list(): Promise<ProjectSummary[]>;
    register(path: string): Promise<ProjectSummary>;
  };
  /** Dialog (file/directory picker) */
  dialog: {
    pickDirectory(): Promise<string | null>;
  };
  /** Runtime 抽象层（Slice B） */
  runtime: {
    listAvailable: () => Promise<Array<{ id: string; displayName: string; available: boolean }>>;
    detectCli: () => Promise<{ claude_cli: boolean; codex_cli: boolean }>;
    listCatalog: () => Promise<unknown>;
    setCapabilityEnabled: (name: string, enabled: boolean) => Promise<void>;
  };
  /** Task Scheduler（Slice J） */
  scheduler: {
    listTasks: () => Promise<Array<{
      id: string; name: string; prompt: string; scheduleType: string;
      scheduleValue: string; enabled: boolean; lastStatus: string | null;
      consecutiveErrors: number; nextRun: string;
    }>>;
  };
  /** Library (fanbox clone — G4) */
  library: {
    listFolders: () => Promise<unknown>;
    createFolder: (data: { name: string; parentId?: string }) => Promise<unknown>;
    updateFolder: (data: { id: string; name: string }) => Promise<void>;
    deleteFolder: (id: string, moveItems: boolean) => Promise<void>;
    listTags: () => Promise<unknown>;
    createTag: (data: { name: string; color: string }) => Promise<unknown>;
    deleteTag: (id: string) => Promise<void>;
    listItems: (filter: { folderId?: string; tagId?: string; keyword?: string; status?: string; itemType?: string; limit?: number; offset?: number }) => Promise<unknown>;
    getItem: (id: string) => Promise<unknown>;
    createItem: (data: { folderId?: string; title: string; description?: string; content?: string; sourceUrl?: string; itemType?: string; status?: string; tagIds?: string[] }) => Promise<unknown>;
    updateItem: (data: { id: string; folderId?: string; title: string; description?: string; content?: string; sourceUrl?: string; status?: string; tagIds?: string[] }) => Promise<void>;
    deleteItem: (id: string) => Promise<void>;
    batchTag: (data: { itemIds: string[]; tagIds: string[] }) => Promise<void>;
    batchMove: (data: { itemIds: string[]; folderId?: string }) => Promise<void>;
    batchDelete: (data: { itemIds: string[] }) => Promise<void>;
    getStats: () => Promise<unknown>;
  };
  /** Subagent (G8) */
  subagent: {
    list: () => Promise<unknown>;
    get: (id: string) => Promise<unknown>;
    create: (data: { name: string; role?: string; instructions?: string; tools?: string; providerId?: string; providerKeyId?: string; modelId?: string; fallbackEnabled?: boolean; maxRuns?: number }) => Promise<unknown>;
    update: (data: { id: string; name: string; role?: string; instructions?: string; tools?: string; providerId?: string; providerKeyId?: string; modelId?: string; fallbackEnabled?: boolean; maxRuns?: number; enabled?: boolean }) => Promise<void>;
    delete: (id: string) => Promise<void>;
    run: (data: { subagentId: string; inputText: string }) => Promise<unknown>;
    listRuns: (subagentId: string) => Promise<unknown>;
    resolveBinding: (subagentId: string) => Promise<unknown>;
  };
}

// --- Helper: invoke with error mapping ---

async function cmd<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (err) {
    const msg = typeof err === 'string' ? err : String(err);
    throw new Error(`Tauri command failed: ${command} — ${msg}`);
  }
}

// --- The adapter ---

const nativesAPI: NativesAPI = {
  // FOUC Guard
  themeReady: () => {
    // Tauri: emit event to signal theme readiness
    invoke('theme_ready_signal').catch(() => {
      // Graceful — window show is controlled by Tauri, not Electron
    });
  },

  // App
  app: {
    version: () => cmd<string>('app_version'),
  },

  // DB
  db: {
    get: async (key: string) => {
      const res = await cmd<{ value: unknown } | unknown>('db_get', { key });
      return res && typeof res === 'object' && 'value' in (res as object) ? (res as { value: unknown }).value : res;
    },
    set: (key: string, value: unknown) => cmd('db_set', { key, value }),
    delete: (key: string) => cmd('db_delete', { key }),
    list: (prefix?: string) => cmd('db_list', { prefix }),
  },

  // Terminal
  terminal: {
    create: async (profileId?: string, cols?: number, rows?: number) => {
      const sessionId = await cmd<string>('terminal_create', { profileId, cols, rows });
      return { sessionId } as any;
    },
    write: (sessionId: string, data: string) => cmd('terminal_write', { sessionId, data }),
    resize: (sessionId: string, cols: number, rows: number) =>
      cmd('terminal_resize', { sessionId, cols, rows }),
    kill: (sessionId: string) => cmd('terminal_kill', { sessionId }),
    cwd: async (sessionId: string) => {
      const result = await cmd<{ cwd: string; source: string }>('terminal_cwd', { sessionId });
      return result as any;
    },
    proc: (sessionId: string) => cmd<{ processName: string; pid: number }>('terminal_proc', { sessionId }),
    sessionState: (sessionId: string) => cmd<{
      sessionId: string; cols: number; rows: number;
      title: string; cwd: string; foregroundProcess: string;
      pid: number; status: string;
    }>('terminal_session_state', { sessionId }),
    listSessions: () => cmd<Array<{
      sessionId: string; cols: number; rows: number;
      title: string; cwd: string; foregroundProcess: string;
      pid: number; status: string;
    }>>('terminal_list_sessions'),
    onData: (callback) => {
      const unlisten = listen<{ sessionId: string; data: string }>('terminal:data', (event) => {
        callback(event.payload);
      });
      return () => { unlisten.then((fn) => fn()); };
    },
    onExit: (callback) => {
      const unlisten = listen<{ sessionId: string; exitCode: number }>('terminal:exit', (event) => {
        callback(event.payload);
      });
      return () => { unlisten.then((fn) => fn()); };
    },
    onRenderState: (callback) => {
      const unlisten = listen<RenderStatePayload>('terminal:render-state', (event) => {
        callback(event.payload);
      });
      return () => { unlisten.then((fn) => fn()); };
    },
    onTitleChanged: (callback) => {
      const unlisten = listen<{ sessionId: string; title: string }>('terminal:title-changed', (event) => {
        callback(event.payload);
      });
      return () => { unlisten.then((fn) => fn()); };
    },
    onPwdChanged: (callback) => {
      const unlisten = listen<{ sessionId: string; pwd: string }>('terminal:pwd-changed', (event) => {
        callback(event.payload);
      });
      return () => { unlisten.then((fn) => fn()); };
    },
    onBell: (callback) => {
      const unlisten = listen<{ sessionId: string }>('terminal:bell', (event) => {
        callback(event.payload);
      });
      return () => { unlisten.then((fn) => fn()); };
    },
    renderState: (sessionId: string) => cmd<RenderStatePayload>('terminal_render_state', { sessionId }),
    recordStart: (sessionId: string, cols: number, rows: number) =>
      cmd('terminal_record_start', { sessionId, cols, rows }),
    recordStop: (sessionId: string) => cmd('terminal_record_stop', { sessionId }),
    recordList: () => cmd('terminal_record_list'),
    recordPlay: (id: string) => cmd<string>('terminal_record_play', { id }),
    recordExport: (id: string, format: string) =>
      cmd<{ ok: boolean; path: string; format: string; fellBack?: string }>('terminal_record_export', { id, format }),
    recordPrune: () => cmd('terminal_record_prune'),
  },

  // Builtin Tool Registry
  builtinTool: {
    list: () => cmd<Array<{ id: string; enabled: boolean; driver: string }>>('builtin_tool_list'),
    update: (id: string, enabled: boolean, driver: string) =>
      cmd('builtin_tool_update', { id, enabled, driver }),
    seed: (id: string, driver: string) => cmd('builtin_tool_seed', { id, driver }),
    detect: (driver: string) => cmd<boolean>('builtin_tool_detect', { driver }),
    launch: (driver: string) => cmd('builtin_tool_launch', { driver }),
    ghosttyIsRunning: () => cmd<boolean>('builtin_tool_ghostty_is_running'),
    ghosttyFocus: () => cmd('builtin_tool_ghostty_focus'),
    ghosttyLaunch: (configPath?: string) => cmd('builtin_tool_ghostty_launch', { config_path: configPath }),
    ghosttySyncTheme: () => cmd<string>('builtin_tool_ghostty_sync_theme'),
    ghosttyVtAvailable: () => cmd<boolean>('ghostty_vt_available'),
  },

  // Module
  module: {
    scan: () => cmd('module_scan'),
    install: (pathOrZip: string) => cmd('module_install', { pathOrZip }),
    readManifest: (source: string) => cmd('module_read_manifest', { source }),
    grantPermission: (moduleId: string, permission: string) =>
      cmd('module_grant_permission', { moduleId, permission }),
    revokePermission: (moduleId: string, permission: string) =>
      cmd('module_revoke_permission', { moduleId, permission }),
    listPermissions: (moduleId: string) => cmd('module_list_permissions', { moduleId }),
    getAuditLog: (moduleId?: string, limit?: number) =>
      cmd('module_get_audit_log', { moduleId, limit }),
    approveAllPermissions: (moduleId: string) =>
      cmd('module_approve_all_permissions', { moduleId }),
    uninstall: (moduleId: string) => cmd('module_uninstall', { moduleId }),
    list: () => cmd('module_list'),
    enable: (moduleId: string) => cmd('module_enable', { moduleId }),
    disable: (moduleId: string) => cmd('module_disable', { moduleId }),
    update: (moduleId: string, source?: string) => cmd('module_update', { moduleId, source }),
    writeGenerated: (
      moduleId: string,
      name: string,
      htmlContent: string,
      permissions: string[],
    ) =>
      cmd<{ moduleId: string; ok: boolean }>('write_generated_module', {
        moduleId,
        name,
        htmlContent,
        permissions,
      }),
    rollback: (params: { moduleId: string; oldContent: string }) =>
      cmd('rollback_module', params),
  },

  // Environment
  env: {
    getVariables: (profileId: string) => cmd('env_get_variables', { profileId }),
    getDefaultProfile: () => cmd('env_get_default_profile'),
    listProfiles: () => cmd('env_list_profiles'),
    createProfile: (name: string) => cmd('env_create_profile', { name }),
    deleteProfile: (name: string) => cmd('env_delete_profile', { name }),
    setDefaultProfile: (name: string) => cmd('env_set_default_profile', { name }),
    setVariable: (profileId: string, key: string, value: string) =>
      cmd('env_set_variable', { profileId, key, value }),
    deleteVariable: (profileId: string, key: string) =>
      cmd('env_delete_variable', { profileId, key }),
    encrypt: (text: string) => cmd('env_encrypt', { text }),
  },

  // Theme
  getTheme: () => cmd('get_theme'),
  setTheme: (theme: string) => cmd('set_theme', { theme }),

  // Shell
  shell: {
    showItemInFolder: (filePath: string) => cmd('show_item_in_folder', { path: filePath }),
    openPath: (filePath: string) => cmd('open_path', { path: filePath }),
  },

  // Locale
  getLocale: () => cmd('get_locale'),
  setLocale: (locale: string) => cmd('set_locale', { locale }),

  // Notifications
  notification: {
    send: (title: string, body: string, level?: string) =>
      cmd('notification_send', { title, body, level: level || 'info' }),
    list: (unreadOnly?: boolean) => cmd('notification_list', { unreadOnly }),
    markRead: (id: number) => cmd('notification_mark_read', { id }),
    markAllAsRead: () => cmd('notification_mark_all_read'),
  },

  // File System
  fs: {
    listDir: (dirPath: string, options?: unknown) => cmd('fs_list_dir', { dirPath, options }),
    readFile: (filePath: string) => cmd('fs_read_file', { filePath }),
    writeFileAtomic: (filePath: string, content: string, expectedMtime?: number) =>
      cmd('fs_write_file_atomic', { filePath, content, expectedMtime }),
    createEntry: async (targetPath: string, type: string) => {
      await cmd('fs_create_entry', { targetPath, type });
      return { ok: true } as any;
    },
    renameEntry: async (oldPath: string, newPath: string) => {
      await cmd('fs_rename_entry', { oldPath, newPath });
      return { ok: true } as any;
    },
    trashEntry: async (filePath: string) => {
      await cmd('fs_trash_entry', { filePath });
      return { ok: true } as any;
    },
    moveEntry: (from: string, to: string) => cmd('fs_move_entry', { from, to }),
    importFiles: (sourcePaths: string[], destDir: string) =>
      cmd('fs_import_files', { sourcePaths, destDir }),
    recentFiles: (root: string) => cmd('fs_recent_files', { root }),
    saveBlob: (dir: string, name: string, base64Data: string) =>
      cmd('fs_save_blob', { dir, name, base64Data }),
    convertFileSrc: (filePath: string) => convertFileSrc(filePath),
  },

  // Archive
  archive: {
    list: (archivePath: string) => cmd('archive_list', { archivePath }),
  },

  // Search
  search: {
    grep: (query: string, root: string, options?: unknown) =>
      cmd('search_grep', { query, root, options }),
    files: (query: string, root: string, options?: unknown) =>
      cmd('search_files', { query, root, options }),
    spotlight: (query: string, root: string) =>
      cmd('search_spotlight', { query, root }),
  },

  // State Persistence
  state: {
    save: (moduleId: string, state: string) => cmd('state_save', { moduleId, state }),
    load: (moduleId: string) => cmd('state_load', { moduleId }),
    clear: (moduleId: string) => cmd('state_clear', { moduleId }),
  },

  // Git
  git: {
    status: (dirPath: string) => cmd('git_status', { dirPath }),
    diff: (filePath: string) => cmd('git_diff', { filePath }),
  },

  // Disk
  disk: {
    usage: (dirPath: string) => cmd('disk_usage', { dirPath }),
    systemInfo: () => cmd('disk_system_info'),
    systemMetrics: () => cmd('system_metrics'),
  },

  // Thumbnail
  thumbnail: {
    generate: (filePath: string, width: number) =>
      cmd('thumbnail_generate', { filePath, width }),
  },

  // Agent
  agent: {
    scanProjects: () => cmd('agent_scan_projects'),
    getSessions: (projectPath: string) => cmd('agent_get_sessions', { projectPath }),
    scanSkills: () => cmd('agent_scan_skills'),
    detectStatus: (output: string, exitCode?: number) =>
      cmd('agent_detect_status', { output, exitCode }),
  },

  // Skills
  skills: {
    enable: (path: string) => cmd('skills_enable', { path }),
    disable: (path: string) => cmd('skills_disable', { path }),
    getDeactivatedPath: (path: string) => cmd('skills_get_deactivated_path', { path }),
    uninstall: (path: string) => cmd('skills_uninstall', { path }),
  },

  // DB State Changed event
  onDbStateChanged: (callback) => {
    const unlisten = listen<{ channel: string; data: unknown }>('db-state-changed', (event) => {
      callback(event, event.payload.channel, event.payload.data);
    });
    return () => { unlisten.then((fn) => fn()); };
  },

  // Screenshot
  screenshot: {
    watch: (callback) => {
      const unlisten = listen<string>('screenshot:detected', (event) => {
        callback(event.payload);
      });
      // Start watching
      invoke('screenshot_start_watching').catch(() => {});
      return () => {
        invoke('screenshot_stop_watching').catch(() => {});
        unlisten.then((fn) => fn());
      };
    },
    saveAnnotated: (dataUrl: string, targetPath?: string) =>
      cmd('screenshot_save_annotated', { dataUrl, targetPath }),
  },

  // Release
  release: {
    inspect: (projectPath: string) => cmd('release_inspect', { projectPath }),
    prepare: (projectPath: string, version: string) =>
      cmd('release_prepare', { projectPath, version }),
    getSequence: (projectPath: string, version: string) =>
      cmd('release_get_sequence', { projectPath, version }),
    execute: (projectPath: string, command: string) =>
      cmd('release_execute', { projectPath, command }),
  },

  // Update
  update: {
    check: () => cmd('update_check'),
    mute: (version: string) => cmd('update_mute', { version }),
    dismiss: (version: string) => cmd('update_dismiss', { version }),
    getMuted: () => cmd('update_get_muted'),
    getDismissed: () => cmd('update_get_dismissed'),
  },

  // Clipboard
  clipboard: {
    write: (text: string) => cmd('clipboard_write', { text }),
    read: () => cmd('clipboard_read'),
  },

  // Usage
  usage: {
    refresh: (params: { startMs: number; endMs: number; force: boolean; includeComparison: boolean; timeZone: string }) =>
      cmd('usage_refresh', { request: params }),
  },

  // CodeGraph
  codegraph: {
    read: () => cmd('read_codegraph'),
    rtkGain: () => cmd('rtk_gain'),
  },

  // Dialog （文件/目录选择，经 Tauri dialog plugin）
  dialog: {
    pickDirectory: async () => {
      try {
        const { open } = await import('@tauri-apps/plugin-dialog');
        const selected = await open({ directory: true, multiple: false });
        return selected as string | null;
      } catch {
        return null;
      }
    },
  },

  // Provider (unified API — single source of truth)
  provider: {
    list: () => cmd<ProviderSummary[]>('list_providers'),
    create: (input: { providerType: string; displayName: string; websiteUrl: string; baseUrl: string; defaultModel: string; initialKey: { label: string; apiKey: string } }) =>
      cmd<ProviderSummary>('add_provider', input),
    delete: (providerId: string) => cmd('delete_provider', { id: providerId }),
    updateDefaults: (input: { providerId: string; defaultModel: string }) =>
      cmd('assistant_provider_update_defaults', { input }),
    addKey: (input: { providerId: string; label: string; apiKey: string }) =>
      cmd<ProviderKeySummary>('add_provider_key', input),
    testCandidate: (input: { providerType: string; baseUrl: string; apiKey: string; model: string }) =>
      cmd<ProviderTestResult>('test_provider_raw', { input }),
    testKey: (input: { providerId: string; keyId: string }) =>
      cmd<ProviderTestResult>('assistant_provider_test_key', { input }),
    discoverModels: (input: { providerType: string; baseUrl: string; apiKey: string }) =>
      cmd<Array<{ id: string; displayName?: string }>>('provider_discover_models', { input }),
    setPrimaryKey: (input: { providerId: string; keyId: string }) =>
      cmd('assistant_provider_set_primary_key', { input }),
    deleteKey: (input: { providerId: string; keyId: string }) =>
      cmd('assistant_provider_delete_key', { input }),
  },

  // Assistant
  assistant: {
    listSessions: (params: { projectId: string | null }) =>
      cmd('assistant_list_sessions', params),
    getMessages: (sessionId: string) =>
      cmd('assistant_get_messages', { sessionId }),
    createSession: (params: { projectId: string | null; title: string; modelId: string; providerId: string }) =>
      cmd('assistant_create_session', params),
    deleteSession: (sessionId: string) =>
      cmd('assistant_delete_session', { sessionId }),
    saveMessage: (params: { sessionId: string; role: string; content: string; status: string; tokenCount: number; toolCalls?: string; toolResult?: string }) =>
      cmd('assistant_save_message', params),
    updateMessageStatus: (params: { messageId: string; status: string; toolResult?: string }) =>
      cmd('assistant_update_message_status', params),
    updateSessionTitle: (params: { sessionId: string; title: string }) =>
      cmd('assistant_update_session_title', params),
    updateSessionModel: (params: { sessionId: string; modelId: string; providerId: string }) =>
      cmd('assistant_update_session_model', params),
    streamChat: (params: { sessionId: string; model: string; messages: Array<{ role: string; content: string }> }) =>
      cmd('stream_chat', { input: params }),
    cancelStream: (sessionId: string) =>
      cmd('cancel_stream', { sessionId }),
  },

  // Execution Engine settings（PRD 3.4）
  executorSettings: {
    get: () => cmd('executor_get_settings'),
    save: (settings: { enabledTools: Record<string, boolean>; maxSelfHeal: number; maxSteps?: number }) =>
      cmd('executor_save_settings', { settings }),
  },

  // Runtime abstraction（Slice B）
  runtime: {
    listAvailable: () => cmd('runtime_list_available'),
    detectCli: () => cmd('runtime_detect_cli'),
    listCatalog: () => cmd('runtime_list_catalog'),
    setCapabilityEnabled: (name: string, enabled: boolean) =>
      cmd('runtime_set_capability_enabled', { name, enabled }),
  },

  // Task Scheduler（Slice J）
  scheduler: {
    listTasks: () => cmd('scheduler_list_tasks'),
  },

  // Window Controls
  windowControls: {
    minimize: () => cmd('window_minimize'),
    maximize: () => cmd('window_maximize'),
    close: () => cmd('window_close'),
    isMaximized: () => cmd('window_is_maximized'),
    tileWindow: (action: string) => cmd('window_tile', { action }),
  },

  // Widget window
  openWidgetWindow: () => {
    invoke('open_widget_window').catch(() => {});
  },

  // Bridge / Security
  bridge: {
    getHttpPort: () => cmd<number>('get_http_port'),
    generateToken: (moduleId: string) => cmd<string>('generate_token', { moduleId }),
    validateToken: (token: string, moduleId: string) => cmd<boolean>('validate_token', { token, moduleId }),
  },

  // FsWatch — file system change notifications
  fsWatch: {
    start: (path: string) => cmd<void>('fs_watch_start', { path }),
    stop: (path: string) => cmd<void>('fs_watch_stop', { path }),
    stopAll: () => cmd<void>('fs_watch_stop_all'),
    list: () => cmd<string[]>('fs_watch_list'),
    onChange: (callback: (event: { path: string; kind: string }) => void) => {
      let unlisten: (() => void) | null = null;
      listen<{ path: string; kind: string }>('fs-watch-change', (e) => callback(e.payload))
        .then((fn) => { unlisten = fn; })
        .catch(() => {});
      return () => { unlisten?.(); };
    },
  },

  // HtmlPreview — sandboxed HTML preview with local resource rewriting
  htmlPreview: {
    prepare: (htmlPath: string) => cmd<{ content: string; fsBase: string; serverPort: number }>('html_preview_prepare', { htmlPath }),
  },

  // LidGuard — prevent macOS sleep while terminals are active
  lidGuard: {
    set: (on: boolean) => cmd<void>('lid_guard_set', { on }),
    status: () => cmd<{ sleepDisabled: boolean; terminalCount: number }>('lid_guard_status'),
  },

  // WeChat ClawBot
  wechat: {
    env: () => cmd<{ target: string; cwd: string; persona: string; state: string; connected: boolean }>('wechat_env'),
    login: () => cmd<{ qrcode: string; qrcode_img_content: string; state: string }>('wechat_login'),
    pollLogin: (qrcode: string, verifyCode?: string) => cmd<{ state: string; error?: string }>('wechat_poll_login', { qrcode, verifyCode }),
    disconnect: () => cmd<{ ok: boolean }>('wechat_disconnect'),
    check: () => cmd<{ ok: boolean; state: string }>('wechat_check'),
    send: (text: string) => cmd<{ ok: boolean; cid: string }>('wechat_send', { text }),
    setTarget: (target: string) => cmd('wechat_set_target', { target }),
    setCwd: (dir: string) => cmd('wechat_set_cwd', { dir }),
    setPersona: (persona: string) => cmd('wechat_set_persona', { persona }),
    detectAgents: () => cmd<{ claude: boolean; codex: boolean }>('wechat_detect_agents'),
    status: () => cmd<{ state: string; connected: boolean; target: string; cwd: string }>('wechat_status'),
  },
  // Plugins
  plugins: {
    detect: (name: string) => cmd<string | null>('plugin_detect', { name }),
    install: (name: string) => cmd<void>('plugin_install', { name }),
    uninstall: (name: string) => cmd<void>('plugin_uninstall', { name }),
  },

  // ── Library (fanbox clone — G4) ──
  library: {
    listFolders: () => cmd('library_list_folders'),
    createFolder: (data: { name: string; parentId?: string }) =>
      cmd('library_create_folder', { input: data }),
    updateFolder: (data: { id: string; name: string }) =>
      cmd('library_update_folder', { input: data }),
    deleteFolder: (id: string, moveItems: boolean) =>
      cmd('library_delete_folder', { id, moveItems }),
    listTags: () => cmd('library_list_tags'),
    createTag: (data: { name: string; color: string }) =>
      cmd('library_create_tag', { input: data }),
    deleteTag: (id: string) =>
      cmd('library_delete_tag', { id }),
    listItems: (filter: {
      folderId?: string; tagId?: string; keyword?: string;
      status?: string; itemType?: string; limit?: number; offset?: number;
    }) => cmd('library_list_items', { filter }),
    getItem: (id: string) =>
      cmd('library_get_item', { id }),
    createItem: (data: {
      folderId?: string; title: string; description?: string;
      content?: string; sourceUrl?: string; itemType?: string;
      status?: string; tagIds?: string[];
    }) => cmd('library_create_item', { input: data }),
    updateItem: (data: {
      id: string; folderId?: string; title: string; description?: string;
      content?: string; sourceUrl?: string; status?: string; tagIds?: string[];
    }) => cmd('library_update_item', { input: data }),
    deleteItem: (id: string) =>
      cmd('library_delete_item', { id }),
    batchTag: (data: { itemIds: string[]; tagIds: string[] }) =>
      cmd('library_batch_tag', { input: data }),
    batchMove: (data: { itemIds: string[]; folderId?: string }) =>
      cmd('library_batch_move', { input: data }),
    batchDelete: (data: { itemIds: string[] }) =>
      cmd('library_batch_delete', { input: data }),
    getStats: () => cmd('library_get_stats'),
  },

  // ── Subagent (G8) ──
  subagent: {
    list: () => cmd('subagent_list'),
    get: (id: string) => cmd('subagent_get', { id }),
    create: (data: {
      name: string; role?: string; instructions?: string; tools?: string;
      providerId?: string; providerKeyId?: string; modelId?: string; fallbackEnabled?: boolean; maxRuns?: number;
    }) => cmd('subagent_create', { input: data }),
    update: (data: {
      id: string; name: string; role?: string; instructions?: string; tools?: string;
      providerId?: string; providerKeyId?: string; modelId?: string; fallbackEnabled?: boolean;
      maxRuns?: number; enabled?: boolean;
    }) => cmd('subagent_update', { input: data }),
    delete: (id: string) => cmd('subagent_delete', { id }),
    run: (data: { subagentId: string; inputText: string }) =>
      cmd('subagent_run', { input: data }),
    listRuns: (subagentId: string) =>
      cmd('subagent_list_runs', { subagentId }),
    resolveBinding: (subagentId: string) =>
      cmd('subagent_resolve_binding', { subagentId }),
  },

  // Assistant in-process RPC (no daemon sidecar)
  assistantV2: {
    request: <T>(method: string, params?: unknown): Promise<T> =>
      cmd<T>('assistant_rpc_request', { method, params: params ?? null }),
    getStatus: (): Promise<{ connected: boolean; error: string | null }> =>
      cmd<{ connected: boolean; error: string | null }>('assistant_status'),
  },

  // Project directory management
  project: {
    list: (): Promise<ProjectSummary[]> => cmd<ProjectSummary[]>('project_list'),
    register: (path: string): Promise<ProjectSummary> => cmd<ProjectSummary>('project_register', { path }),
  },
};

// Expose to window (replaces contextBridge.exposeInMainWorld)
if (typeof window !== 'undefined') {
  (window as unknown as { nativesAPI: NativesAPI }).nativesAPI = nativesAPI;
  // Expose cmd helper so DaemonClient and other modules can invoke
  // Tauri commands without importing @tauri-apps/api/core directly.
  (window as any).__nativesCmd = cmd;
}

export default nativesAPI;
