// Global type declarations for Natives

export {};

declare global {
  // HTTP server port injected by main process
  var __nativesHttpPort: number | undefined;

  interface Window {
    __nativesHttpPort?: number;
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
        create: (profileId?: string) => Promise<{ sessionId: string | null; error?: string }>;
        write: (sessionId: string, data: string) => Promise<void>;
        resize: (sessionId: string, cols: number, rows: number) => Promise<void>;
        kill: (sessionId: string) => Promise<void>;
        cwd: (sessionId: string) => Promise<{ ok: boolean; cwd?: string }>;
      };
      archive: {
        list: (archivePath: string) => Promise<{ entries: Array<{ name: string; size: number; isDir: boolean }>; truncated: boolean; totalSize: number; error?: string }>;
      };
      module: {
        scan: () => Promise<unknown[]>;
        install: (pathOrZip: string) => Promise<{ success: boolean; moduleId?: string; error?: string }>;
        readManifest: (source: string) => Promise<{ manifest?: { id: string; name: string; permissions: string[] }; error?: string }>;
        grantPermission: (moduleId: string, permission: string) => Promise<{ ok: boolean; error?: string }>;
        revokePermission: (moduleId: string, permission: string) => Promise<{ ok: boolean; error?: string }>;
        listPermissions: (moduleId: string) => Promise<Array<{ module_id: string; permission: string; granted: number }>>;
        getAuditLog: (moduleId?: string, limit?: number) => Promise<Array<{ id: number; module_id: string; permission: string; action: string; granted: number; reason: string | null; created_at: string }>>;
        approveAllPermissions: (moduleId: string) => Promise<{ ok: boolean; error?: string }>;
        uninstall: (moduleId: string) => Promise<{ success: boolean; error?: string }>;
        list: () => Promise<unknown[]>;
        enable: (moduleId: string) => Promise<{ success: boolean; error?: string }>;
        disable: (moduleId: string) => Promise<{ success: boolean; error?: string }>;
        update: (moduleId: string) => Promise<{ success: boolean; error?: string }>;
      };
      env: {
        getVariables: (profileId: string) => Promise<Record<string, string>>;
        getDefaultProfile: () => Promise<unknown>;
        listProfiles: () => Promise<unknown[]>;
        createProfile: (name: string) => Promise<{ success: boolean; error?: string }>;
        deleteProfile: (name: string) => Promise<{ success: boolean; error?: string }>;
        setDefaultProfile: (name: string) => Promise<{ ok: boolean }>;
        setVariable: (profileId: string, key: string, value: string) => Promise<{ ok: boolean }>;
        deleteVariable: (profileId: string, key: string) => Promise<{ ok: boolean; error?: string }>;
        encrypt: (text: string) => Promise<string>;
        decrypt: (encrypted: string) => Promise<string>;
      };
      getTheme: () => Promise<string>;
      setTheme: (theme: string) => Promise<void>;
      getLocale: () => Promise<string>;
      setLocale: (locale: string) => Promise<void>;
      notification: {
        send: (title: string, body: string, level?: string) => Promise<{ ok: boolean }>;
        list: (unreadOnly?: boolean) => Promise<unknown[]>;
        markRead: (id: number) => Promise<{ ok: boolean }>;
        markAllAsRead: () => Promise<{ ok: boolean }>;
      };
      onDbStateChanged: (callback: (event: unknown, channel: string, data: unknown) => void) => () => void;

      // File Manager
      fs: {
        listDir: (dirPath: string, options?: { sortBy?: string; sortDir?: string; showHidden?: boolean }) => Promise<import('./file').FileEntry[]>;
        readFile: (filePath: string) => Promise<{ content: string; truncated: boolean; size: number; encoding: string } | null>;
        writeFileAtomic: (filePath: string, content: string, expectedMtime?: number) => Promise<{ ok: boolean; error?: string }>;
        createEntry: (targetPath: string, type: string) => Promise<{ ok: boolean; error?: string }>;
        renameEntry: (oldPath: string, newPath: string) => Promise<{ ok: boolean; error?: string }>;
        trashEntry: (filePath: string) => Promise<{ ok: boolean; error?: string }>;
        moveEntry: (from: string, to: string) => Promise<{ ok: boolean; error?: string }>;
        importFiles: (sourcePaths: string[], destDir: string) => Promise<{ ok: boolean; count?: number; error?: string }>;
        recentFiles: (root: string) => Promise<Array<{ path: string; mtime: number; size: number }>>;
      };

      // Search
      search: {
        grep: (query: string, root: string, options?: { maxResults?: number; fileExtensions?: string[] }) => Promise<import('./file').ContentSearchResult[]>;
        files: (query: string, root: string, options?: { maxResults?: number; includeDirs?: boolean }) => Promise<import('./file').SearchResult[]>;
        spotlight: (query: string, root: string) => Promise<import('./file').ContentSearchResult[]>;
      };

      // Git
      git: {
        status: (dirPath: string) => Promise<import('./file').GitStatus | null>;
        diff: (filePath: string) => Promise<string | null>;
      };

      // Disk Usage
      disk: {
        usage: (dirPath: string) => Promise<import('./file').DiskUsageItem[]>;
      };

      // Thumbnail
      thumbnail: {
        generate: (filePath: string, width: number) => Promise<{ buffer: string; contentType: string; cached: boolean } | null>;
      };

      // Agent
      agent: {
        scanProjects: () => Promise<string[]>;
        getSessions: (projectPath: string) => Promise<import('./agent').AgentSession[]>;
        scanSkills: () => Promise<import('./agent').SkillInfo[]>;
        detectStatus: (output: string, exitCode?: number) => Promise<string>;
      };

      // Phase 3: Screenshot
      screenshot: {
        watch: (callback: (filePath: string) => void) => () => void;
        saveAnnotated: (dataUrl: string, targetPath?: string) => Promise<{ success: boolean; path?: string; error?: string }>;
      };

      // Phase 3: Release Wizard
      release: {
        inspect: (projectPath: string) => Promise<{
          hasPackageJson: boolean;
          hasGit: boolean;
          isClean: boolean;
          hasChangelog: boolean;
          hasGhCli: boolean;
          currentVersion: string;
          changelogHasUnreleased: boolean;
          error?: string;
        }>;
        prepare: (projectPath: string, version: string) => Promise<{ success: boolean; error?: string }>;
        getSequence: (projectPath: string, version: string) => Promise<{
          steps: Array<{ name: string; command: string; description: string; enabled: boolean }>;
        }>;
        execute: (projectPath: string, command: string) => Promise<{ success: boolean; output?: string; error?: string }>;
      };

      // Phase 3: Update Checker
      update: {
        check: () => Promise<{
          latestVersion: string;
          releaseUrl: string;
          publishedAt: string;
          body: string;
        } | null>;
        mute: (version: string) => Promise<{ ok: boolean }>;
        getMuted: () => Promise<Array<{ version: string; mutedAt: number }>>;
      };

      // Clipboard
      clipboard: {
        write: (text: string) => Promise<boolean>;
        read: () => Promise<string>;
      };

      // Skills management
      skills: {
        enable: (path: string) => Promise<{ success: boolean; error?: string }>;
        disable: (path: string) => Promise<{ success: boolean; error?: string }>;
        getDeactivatedPath: (path: string) => Promise<string | null>;
        uninstall: (path: string) => Promise<{ success: boolean; error?: string }>;
      };

      // Usage tracking
      usage: {
        refresh: () => Promise<{
          claude: import('./agent').ClaudeUsage | null;
          rtk: import('./agent').RtkUsage | null;
          codex: string;
          error?: string;
        }>;
      };

      // Window controls
      windowControls: {
        minimize: () => Promise<void>;
        maximize: () => Promise<void>;
        close: () => Promise<void>;
        isMaximized: () => Promise<boolean>;
      };

      // Widget window launcher
      openWidgetWindow: () => void;

      // State persistence (US16)
      state: {
        save: (moduleId: string, state: string) => Promise<{ ok: boolean; error?: string }>;
        load: (moduleId: string) => Promise<string | null>;
        clear: (moduleId: string) => Promise<{ ok: boolean }>;
      };
    };
  }
}
