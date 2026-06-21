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
    decrypt: (encrypted: string) => Promise<string>;
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
    refresh: () => Promise<unknown>;
  };
  codegraph: {
    read: () => Promise<unknown>;
    rtkGain: () => Promise<unknown>;
  };
  provider: {
    list: () => Promise<unknown>;
    add: (data: {
      presetName: string;
      name: string;
      websiteUrl: string;
      baseUrl: string;
      keys: { label: string; apiKey: string }[];
    }) => Promise<unknown>;
    delete: (id: string) => Promise<void>;
    addKey: (data: { providerId: string; label: string; apiKey: string }) => Promise<unknown>;
    deleteKey: (id: string) => Promise<void>;
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
    decrypt: (encrypted: string) => cmd('env_decrypt', { encrypted }),
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
    refresh: () => cmd('usage_refresh'),
  },

  // CodeGraph
  codegraph: {
    read: () => cmd('read_codegraph'),
    rtkGain: () => cmd('rtk_gain'),
  },

  // Provider
  provider: {
    list: () => cmd('list_providers'),
    add: (data: { presetName: string; name: string; websiteUrl: string; baseUrl: string; keys: { label: string; apiKey: string }[] }) =>
      cmd('add_provider', data),
    delete: (id: string) => cmd('delete_provider', { id }),
    addKey: (data: { providerId: string; label: string; apiKey: string }) =>
      cmd('add_provider_key', data),
    deleteKey: (id: string) => cmd('delete_provider_key', { id }),
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
};

// Expose to window (replaces contextBridge.exposeInMainWorld)
if (typeof window !== 'undefined') {
  (window as unknown as { nativesAPI: NativesAPI }).nativesAPI = nativesAPI;
}

export default nativesAPI;
