// Global type declarations for Natives2 — Tauri IPC types.
// The authoritative interface definition lives in src/lib/tauri-adapter.ts.
// This file augments Window.nativesAPI so the rest of the codebase can type-check.

export {};

declare global {
  interface Window {
    __nativesHttpPort?: never; // Removed — use getHttpPort() helper instead
    nativesAPI?: {
      themeReady: () => void;
      app: {
        version: () => Promise<string>;
      };
      db: {
        get: (key: string) => Promise<string | undefined>;
        set: (key: string, value: unknown) => Promise<{ ok: boolean }>;
        delete: (key: string) => Promise<{ ok: boolean }>;
        list: (prefix?: string) => Promise<string[]>;
      };
      terminal: {
        create: (profileId?: string, cols?: number, rows?: number) => Promise<{ sessionId: string; }>;
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
        onRenderState: (callback: (data: { sessionId: string; cursorX: number; cursorY: number; title: string | null; pwd: string | null; cols: number; rows: number }) => void) => () => void;
        onTitleChanged: (callback: (data: { sessionId: string; title: string }) => void) => () => void;
        onPwdChanged: (callback: (data: { sessionId: string; pwd: string }) => void) => () => void;
        onBell: (callback: (data: { sessionId: string }) => void) => () => void;
        renderState: (sessionId: string) => Promise<{ sessionId: string; cursorX: number; cursorY: number; title: string | null; pwd: string | null; cols: number; rows: number }>;
        recordStart: (sessionId: string, cols: number, rows: number) => Promise<void>;
        recordStop: (sessionId: string) => Promise<void>;
        recordList: () => Promise<unknown[]>;
        recordPlay: (id: string) => Promise<string>;
      };
      module: {
        scan: () => Promise<unknown[]>;
        install: (pathOrZip: string) => Promise<{ moduleId: string }>;
        readManifest: (source: string) => Promise<unknown>;
        grantPermission: (moduleId: string, permission: string) => Promise<void>;
        revokePermission: (moduleId: string, permission: string) => Promise<void>;
        listPermissions: (moduleId: string) => Promise<string[]>;
        getAuditLog: (moduleId?: string, limit?: number) => Promise<unknown[]>;
        approveAllPermissions: (moduleId: string) => Promise<void>;
        uninstall: (moduleId: string) => Promise<void>;
        list: () => Promise<unknown[]>;
        enable: (moduleId: string) => Promise<{ ok: boolean }>;
        disable: (moduleId: string) => Promise<{ ok: boolean }>;
        update: (moduleId: string) => Promise<{ ok: boolean }>;
      };
      env: {
        getVariables: (profileId: string) => Promise<{ key: string; value: string }[]>;
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
      plugins: {
        detect: (name: string) => Promise<string | null>;
        install: (name: string) => Promise<void>;
        uninstall: (name: string) => Promise<void>;
      };
      getTheme: () => Promise<string>;
      setTheme: (theme: string) => Promise<void>;
      getLocale: () => Promise<string>;
      setLocale: (locale: string) => Promise<void>;
      shell: {
        showItemInFolder: (filePath: string) => Promise<void>;
        openPath: (filePath: string) => Promise<void>;
      };
      notification: {
        send: (title: string, body: string, level?: string) => Promise<void>;
        list: (unreadOnly?: boolean) => Promise<unknown[]>;
        markRead: (id: number) => Promise<void>;
        markAllAsRead: () => Promise<void>;
      };
      fs: {
        listDir: (dirPath: string, options?: unknown) => Promise<unknown>;
        readFile: (filePath: string) => Promise<unknown>;
        writeFileAtomic: (filePath: string, content: string, expectedMtime?: number) => Promise<unknown>;
        createEntry: (targetPath: string, type: string) => Promise<{ ok: boolean }>;
        renameEntry: (oldPath: string, newPath: string) => Promise<{ ok: boolean }>;
        trashEntry: (filePath: string) => Promise<{ ok: boolean }>;
        moveEntry: (from: string, to: string) => Promise<{ ok: boolean }>;
        importFiles: (sourcePaths: string[], destDir: string) => Promise<void>;
        recentFiles: (root: string) => Promise<unknown[]>;
        saveBlob: (dir: string, name: string, base64Data: string) => Promise<string>;
        convertFileSrc?: (filePath: string) => string;
      };
      search: {
        grep: (query: string, root: string, options?: unknown) => Promise<unknown>;
        files: (query: string, root: string, options?: unknown) => Promise<unknown>;
        spotlight: (query: string, root: string) => Promise<unknown>;
      };
      git: {
        status: (dirPath: string) => Promise<unknown>;
        diff: (filePath: string) => Promise<unknown>;
      };
      disk: {
        usage: (dirPath: string) => Promise<unknown>;
        systemInfo: () => Promise<{
          total_bytes: number;
          used_bytes: number;
          available_bytes: number;
        }>;
      };
      thumbnail: {
        /** Generate a JPEG thumbnail at the given width, returning a base64-encoded string. */
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
        getMuted: () => Promise<unknown[]>;
        getDismissed: () => Promise<unknown[]>;
      };
      clipboard: {
        write: (text: string) => Promise<void>;
        read: () => Promise<string>;
      };
      usage: {
        refresh: () => Promise<{
          claude: import('./agent').ClaudeUsage | null;
          codex: import('./agent').CodexUsage | null;
          rtk: import('./agent').RtkUsage | null;
          history: import('./agent').UsageHistoryPoint[];
          modelStats: import('./agent').ModelStat[];
          sourceConfigured: boolean;
          sourceBreadcrumbs: string[];
          error?: string | null;
        }>;
      };
      windowControls: {
        minimize: () => Promise<void>;
        maximize: () => Promise<void>;
        close: () => Promise<void>;
        isMaximized: () => Promise<boolean>;
        tileWindow: (action: string) => Promise<void>;
      };
      openWidgetWindow: () => void;
      archive: {
        list: (archivePath: string) => Promise<unknown>;
      };
      bridge: {
        getHttpPort: () => Promise<number>;
        generateToken: (moduleId: string) => Promise<string>;
        validateToken: (token: string, moduleId: string) => Promise<boolean>;
      };
      state: {
        save: (moduleId: string, state: string) => Promise<void>;
        load: (moduleId: string) => Promise<string | null>;
        clear: (moduleId: string) => Promise<void>;
      };
      builtinTool: {
        list: () => Promise<Array<{ id: string; enabled: boolean; driver: string }>>;
        update: (id: string, enabled: boolean, driver: string) => Promise<void>;
        seed: (id: string, driver: string) => Promise<void>;
        detect: (driver: string) => Promise<boolean>;
        launch: (driver: string) => Promise<void>;
        ghosttyIsRunning: () => Promise<boolean>;
        ghosttyFocus: () => Promise<void>;
        ghosttyLaunch: () => Promise<void>;
        ghosttyVtAvailable: () => Promise<boolean>;
      };
    };
  }
}
