/**
 * Tauri adapter — replaces electron/preload.ts
 *
 * Maps window.nativesAPI methods to Tauri invoke calls.
 * Every method matches the original Electron IPC contract exactly.
 * Unimplemented commands throw "not implemented" errors, never fake success.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

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
    create: (profileId?: string) => Promise<string>;
    write: (sessionId: string, data: string) => Promise<void>;
    resize: (sessionId: string, cols: number, rows: number) => Promise<void>;
    kill: (sessionId: string) => Promise<void>;
    cwd: (sessionId: string) => Promise<string>;
    onData: (callback: (data: { sessionId: string; data: string }) => void) => () => void;
    onExit: (callback: (data: { sessionId: string; exitCode: number }) => void) => () => void;
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
    createEntry: (targetPath: string, type: string) => Promise<void>;
    renameEntry: (oldPath: string, newPath: string) => Promise<void>;
    trashEntry: (filePath: string) => Promise<void>;
    moveEntry: (from: string, to: string) => Promise<void>;
    importFiles: (sourcePaths: string[], destDir: string) => Promise<void>;
    recentFiles: (root: string) => Promise<unknown[]>;
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
  };
  thumbnail: {
    generate: (filePath: string, width: number) => Promise<string>;
  };
  agent: {
    scanProjects: () => Promise<unknown[]>;
    getSessions: (projectPath: string) => Promise<unknown[]>;
    scanSkills: () => Promise<unknown[]>;
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
    getMuted: () => Promise<string[]>;
  };
  clipboard: {
    write: (text: string) => Promise<void>;
    read: () => Promise<string>;
  };
  usage: {
    refresh: () => Promise<unknown>;
  };
  windowControls: {
    minimize: () => Promise<void>;
    maximize: () => Promise<void>;
    close: () => Promise<void>;
    isMaximized: () => Promise<boolean>;
  };
  openWidgetWindow: () => void;
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
    create: (profileId?: string) => cmd('terminal_create', { profileId }),
    write: (sessionId: string, data: string) => cmd('terminal_write', { sessionId, data }),
    resize: (sessionId: string, cols: number, rows: number) =>
      cmd('terminal_resize', { sessionId, cols, rows }),
    kill: (sessionId: string) => cmd('terminal_kill', { sessionId }),
    cwd: (sessionId: string) => cmd('terminal_cwd', { sessionId }),
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
    update: (moduleId: string) => cmd('module_update', { moduleId }),
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
    createEntry: (targetPath: string, type: string) =>
      cmd('fs_create_entry', { targetPath, type }),
    renameEntry: (oldPath: string, newPath: string) =>
      cmd('fs_rename_entry', { oldPath, newPath }),
    trashEntry: (filePath: string) => cmd('fs_trash_entry', { filePath }),
    moveEntry: (from: string, to: string) => cmd('fs_move_entry', { from, to }),
    importFiles: (sourcePaths: string[], destDir: string) =>
      cmd('fs_import_files', { sourcePaths, destDir }),
    recentFiles: (root: string) => cmd('fs_recent_files', { root }),
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
    getMuted: () => cmd('update_get_muted'),
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

  // Window Controls
  windowControls: {
    minimize: () => cmd('window_minimize'),
    maximize: () => cmd('window_maximize'),
    close: () => cmd('window_close'),
    isMaximized: () => cmd('window_is_maximized'),
  },

  // Widget window
  openWidgetWindow: () => {
    invoke('open_widget_window').catch(() => {});
  },
};

// Expose to window (replaces contextBridge.exposeInMainWorld)
if (typeof window !== 'undefined') {
  (window as unknown as { nativesAPI: NativesAPI }).nativesAPI = nativesAPI;
}

export default nativesAPI;
