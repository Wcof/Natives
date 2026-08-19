/**
 * tauri/types-api — NativesAPI 大接口（ARCH-002 split）
 *
 * window.nativesAPI 的纯类型契约：全部 domain 的方法签名与返回值类型声明于此，
 * 实现由各 domain facade 装配（见 ./tauri-adapter.ts）。纯类型接口，无实现逻辑。
 */

import type { ProviderRoutingApi } from '@/types/provider-routing';

import type { RenderStatePayload } from './types-terminal';
import type { WriteGeneratedModuleResult } from './types-module';
import type {
  AppGrant,
  BrowserProfile,
  CreateLocalCreativeRequest,
  CreativeAppBrowserBounds,
  CreativeAppDeleteMutationResult,
  CreativeAppDeleteOptions,
  CreativeAppDockerStatus,
  CreativeAppGithubTokenStatus,
  CreativeAppInspectRequest,
  CreativeAppInspectResult,
  CreativeAppInstallRequest,
  CreativeAppLogEvent,
  CreativeAppMutationResult,
  CreativeAppOpenTarget,
  CreativeAppOperation,
  CreativeAppProgressEvent,
  CreativeAppProposal,
  CreativeAppProposalApproveResult,
  CreativeAppProposalPayload,
  CreativeAppProposalRejectResult,
  CreativeAppSummary,
  CreativeAppSurface,
  CreativeAppValidatedProposal,
  CreativeAppWindow,
  CreativeDraft,
  CreativeDraftPublishResult,
  CreativeGrantRequested,
  GrantEvent,
  LaunchPlan,
  LocalCreativeAiSettings,
  LocalCreativeConfig,
  LocalCreativeIssueCode,
  LocalProjectScanResult,
  OAuthAllowlistEntry,
  OAuthFlowResult,
  PackageManager,
  ProfileBinding,
  UpdateLocalCreativeRequest,
} from './types-creative-app';
import type {
  ProviderKeySummary,
  ProviderSummary,
  ProviderTestResult,
} from './types-provider';
import type { HarnessNotice, ProjectSummary } from './types-project';
import type {
  ExecutionEngineSettings,
  ExecutionEngineSnapshot,
  RuntimeDescriptor,
} from './types-execution';

export interface EnvVariableMetadata {
  key: string;
  has_value: boolean;
  masked: boolean;
}

export interface EnvProfileMetadata {
  id: number;
  name: string;
  is_default: number;
  created_at: string;
  variables: EnvVariableMetadata[];
}

export interface ScreenshotSaveAnnotatedRequest {
  sourcePath: string;
  dataUrl: string;
}

export interface ScreenshotSaveAnnotatedResult {
  path: string;
}

export type ReleaseAction =
  | 'update-version'
  | 'npm-install'
  | 'npm-build'
  | 'cargo-build-release'
  | 'git-commit'
  | 'git-tag'
  | 'git-push-branch'
  | 'git-push-tags';

export interface ReleaseStep {
  action: ReleaseAction;
  label: string;
  display: string;
}

export interface ReleasePlan {
  version: string;
  steps: ReleaseStep[];
}

export interface ReleasePreparation {
  version: string;
  updatedFiles: string[];
}

export interface ReleaseExecution {
  action: ReleaseAction;
  exitCode: number | null;
  stdout: string;
  stderr: string;
  success: boolean;
  alreadyComplete: boolean;
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
    ) => Promise<WriteGeneratedModuleResult>;
    rollback: (params: { moduleId: string; oldContent: string }) => Promise<void>;
  };
  /** Multi-source Personal Creations (internal + GitHub + local project). */
  creativeApp: {
    list: () => Promise<CreativeAppSummary[]>;
    start: (id: string) => Promise<CreativeAppMutationResult>;
    stop: (id: string) => Promise<CreativeAppMutationResult>;
    delete: (id: string, options?: CreativeAppDeleteOptions) => Promise<CreativeAppDeleteMutationResult>;
    getOpenTarget: (id: string) => Promise<CreativeAppOpenTarget>;
    inspectGithub: (request: CreativeAppInspectRequest) => Promise<CreativeAppInspectResult>;
    installGithub: (request: CreativeAppInstallRequest) => Promise<CreativeAppMutationResult>;
    operations: () => Promise<CreativeAppOperation[]>;
    getOperation: (id: number) => Promise<CreativeAppOperation>;
    cancelOperation: (id: number) => Promise<CreativeAppOperation>;
    onOperationChanged: (callback: (op: CreativeAppOperation) => void) => () => void;
    logs: (id: string, tail?: number, cursor?: number) => Promise<string>;
    reconcile: () => Promise<number>;
    githubTokenStatus: () => Promise<CreativeAppGithubTokenStatus>;
    githubTokenSet: (token: string) => Promise<CreativeAppGithubTokenStatus>;
    githubTokenClear: () => Promise<CreativeAppGithubTokenStatus>;
    dockerStatus: () => Promise<CreativeAppDockerStatus>;
    browserShow: (appId: string, url: string, bounds: CreativeAppBrowserBounds) => Promise<CreativeAppWindow>;
    browserSetBounds: (appId: string, bounds: CreativeAppBrowserBounds) => Promise<void>;
    browserBack: (appId: string) => Promise<void>;
    browserForward: (appId: string) => Promise<void>;
    browserReload: (appId: string) => Promise<void>;
    browserHide: (appId: string) => Promise<void>;
    browserClose: (appId: string) => Promise<void>;
    browserCurrent: (appId: string) => Promise<{ appId?: string | null; url?: string | null }>;
    // T08: BrowserProfile / grants / OAuth
    profileList: () => Promise<BrowserProfile[]>;
    profileCreate: (name: string) => Promise<BrowserProfile>;
    profileDelete: (profileId: string) => Promise<void>;
    profileBindings: () => Promise<ProfileBinding[]>;
    profileBind: (appId: string, profileId: string) => Promise<void>;
    profileUnbind: (appId: string) => Promise<void>;
    grantSet: (appId: string, kind: string, policy: string, path?: string | null) => Promise<AppGrant>;
    grantList: (appId: string) => Promise<AppGrant[]>;
    grantDelete: (grantId: string) => Promise<void>;
    grantEvents: (appId: string, limit?: number) => Promise<GrantEvent[]>;
    uploadFiles: (appId: string) => Promise<string[]>;
    clipboardRead: (appId: string) => Promise<string>;
    clipboardWrite: (appId: string, text: string) => Promise<void>;
    oauthDomains: (appId: string) => Promise<OAuthAllowlistEntry[]>;
    oauthStart: (appId: string, authorizeUrl: string) => Promise<OAuthFlowResult>;
    oauthCancel: (flowId: string) => Promise<void>;
    onGrantRequested: (callback: (event: CreativeGrantRequested) => void) => () => void;
    // CR-501: Surface / Endpoint / Window
    surfaceList: (applicationId: string) => Promise<CreativeAppSurface[]>;
    windowList: (applicationId: string) => Promise<CreativeAppWindow[]>;
    windowOpen: (
      applicationId: string,
      surfaceId: string,
      url: string,
      bounds: CreativeAppBrowserBounds,
    ) => Promise<CreativeAppWindow>;
    windowClose: (windowId: string) => Promise<void>;
    windowMinimize: (windowId: string) => Promise<void>;
    windowRestore: (windowId: string) => Promise<void>;
    // CR-1001/1002: Agent proposal gate
    proposalList: () => Promise<CreativeAppProposal[]>;
    proposalValidate: (proposal: CreativeAppProposalPayload) => Promise<CreativeAppValidatedProposal>;
    proposalApprove: (proposalId: string) => Promise<CreativeAppProposalApproveResult>;
    proposalReject: (proposalId: string) => Promise<CreativeAppProposalRejectResult>;
    onProgress: (callback: (event: CreativeAppProgressEvent) => void) => () => void;
    onLog: (callback: (event: CreativeAppLogEvent) => void) => () => void;
    inspectLocal: (request: { projectRoot: string; entryFile?: string }) => Promise<LocalProjectScanResult>;
    createLocal: (request: CreateLocalCreativeRequest) => Promise<CreativeAppSummary>;
    updateLocal: (request: UpdateLocalCreativeRequest) => Promise<CreativeAppSummary>;
    rescanLocal: (id: string) => Promise<LocalProjectScanResult>;
    restart: (id: string) => Promise<CreativeAppMutationResult>;
    resolveOrphan: (id: string, restart: boolean) => Promise<CreativeAppSummary>;
    getLocalLogs: (
      id: string,
      limit?: number,
    ) => Promise<Array<{ seq: number; tsMs: number; stream: string; text: string }>>;
    installLocalDependencies: (id: string) => Promise<CreativeAppSummary>;
    previewLocalDependencyInstall: (
      id: string,
    ) => Promise<{
      program: string;
      args: string[];
      packageManager: PackageManager;
      requiresConfirmation: true;
      display: string;
    }>;
    getLocalAiSettings: () => Promise<LocalCreativeAiSettings>;
    saveLocalAiSettings: (settings: LocalCreativeAiSettings) => Promise<LocalCreativeAiSettings>;
    previewLocalAi: (projectRoot: string) => Promise<{
      scan: LocalProjectScanResult;
      payloadPreview: unknown;
      settings: LocalCreativeAiSettings;
      requiresConfirmation: true;
    }>;
    analyzeLocalWithAi: (
      projectRoot: string,
      confirmed: boolean,
    ) => Promise<{
      scan: LocalProjectScanResult;
      aiPlan?: LaunchPlan | null;
      payloadPreview: unknown;
    }>;
    diagnoseLocalWithAi: (id: string) => Promise<{
      schemaVersion: number;
      issueCode: LocalCreativeIssueCode;
      summary: string;
      recoveryActions: string[];
    }>;
    getLocalConfig: (id: string) => Promise<LocalCreativeConfig>;
    pollLocalExits: () => Promise<number>;
  };
  /**
   * Draft lifecycle for the creation loop. Publishing is a host command, not a
   * model tool: the user's click is the authorization (ADR-0014, Section 9).
   */
  creativeDraft: {
    create: (request: {
      name: string;
      intent: string;
      conversationId?: string | null;
      originModuleId?: string | null;
    }) => Promise<CreativeDraft>;
    list: () => Promise<CreativeDraft[]>;
    get: (draftId: string) => Promise<CreativeDraft>;
    /** Current revision's HTML — what the preview pane and "continue" show. */
    read: (draftId: string) => Promise<{ draftId: string; html: string }>;
    /**
     * Link the draft to the conversation editing it. The conversation only
     * exists after the first message, so the link is made then — not at create.
     */
    bindConversation: (
      draftId: string,
      conversationId: string,
    ) => Promise<{ ok: boolean; draftId: string }>;
    /** Step the revision pointer back one: the user-facing "undo last change". */
    rollback: (draftId: string) => Promise<{ draftId: string; revision: number }>;
    publish: (request: {
      draftId: string;
      moduleId: string;
      name: string;
      permissions: string[];
    }) => Promise<CreativeDraftPublishResult>;
    delete: (draftId: string) => Promise<{ ok: boolean; draftId: string }>;
    /**
     * Sandbox preview URL, served with the same CSP as a module. Async because
     * the local HTTP port is resolved at runtime — never hardcode it.
     * The host resolves which revision to serve from the database pointer, so
     * this URL stays stable across generate / undo.
     */
    previewUrl: (draftId: string) => Promise<string>;
  };
  env: {
    getDefaultProfile: () => Promise<string>;
    listProfiles: () => Promise<EnvProfileMetadata[]>;
    createProfile: (name: string) => Promise<void>;
    deleteProfile: (name: string) => Promise<void>;
    setDefaultProfile: (name: string) => Promise<void>;
    setVariable: (profileId: string, key: string, value: string) => Promise<void>;
    deleteVariable: (profileId: string, key: string) => Promise<void>;
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
    listDirDetailed: (dirPath: string, options?: unknown) => Promise<{
      path: string;
      parent: string;
      entries: unknown[];
      project?: string | null;
    }>;
    readFile: (filePath: string) => Promise<unknown>;
    writeFileAtomic: (filePath: string, content: string, expectedMtime?: number) => Promise<unknown>;
    createEntry: (targetPath: string, type: string) => Promise<{ ok: boolean; error?: string }>;
    renameEntry: (oldPath: string, newPath: string) => Promise<{ ok: boolean; path?: string; error?: string }>;
    trashEntry: (filePath: string) => Promise<{ ok: boolean; error?: string }>;
    moveEntry: (from: string, to: string) => Promise<{ ok: boolean; path?: string; error?: string }>;
    copyEntry: (from: string, to: string) => Promise<{ ok: boolean; path?: string; error?: string }>;
    duplicateEntry: (filePath: string) => Promise<{ ok: boolean; path?: string; error?: string }>;
    stat: (filePath: string) => Promise<{ found: boolean; path?: string; isDir?: boolean; name?: string; kind?: string; size?: number; mtime?: number }>;
    importFiles: (sourcePaths: string[], destDir: string) => Promise<string[]>;
    recentFiles: (root: string) => Promise<unknown[]>;
    saveBlob: (dir: string, name: string, base64Data: string) => Promise<string>;
    convertFileSrc: (filePath: string) => string;
    // image_convert.rs：HEIC/TIFF 等 webview 不支持的格式 → 缓存 jpeg（仅 macOS）
    convertImagePreview: (filePath: string) => Promise<{ ok: boolean; jpegPath: string; cached: boolean }>;
    // locate.rs：终端路径定位链（直接 stat → 空格扩展 → 多根搜索 → spotlight）
    locate: (query: string, cwd?: string, roots?: string[]) => Promise<{ found: boolean; path?: string; isDir?: boolean; method?: string }>;
    verifyPaths: (candidates: string[]) => Promise<Array<{ path: string; exists: boolean; isDir: boolean }>>;
    trashEntries: (paths: string[]) => Promise<{ ok: boolean; trashed?: string[]; errors?: Array<{ path: string; error: string }>; count?: number }>;
    moveEntries: (paths: string[], destDir: string) => Promise<{ ok: boolean; moved?: string[]; errors?: Array<{ path: string; error: string }>; count?: number }>;
    copyEntries: (paths: string[], destDir: string) => Promise<{ ok: boolean; copied?: string[]; errors?: Array<{ path: string; error: string }>; count?: number }>;
    roots: () => Promise<Array<{ id: string; name: string; path: string }>>;
    openWith: (path: string, withApp?: 'default' | 'reveal' | 'terminal' | 'editor') => Promise<{ ok: boolean; with?: string }>;
    clipboardCopyFiles: (paths: string[]) => Promise<{ ok: boolean; count?: number }>;
    clipboardCopyImage: (filePath: string) => Promise<{ ok: boolean }>;
  };
  archive: {
    // 后端 ArchiveListing（archive.rs）：{ entries: [{name,size,isDir?}...], truncated }
    list: (archivePath: string) => Promise<{ entries: Array<{ name: string; size: number; isDir?: boolean }>; truncated: boolean }>;
    // archive_ops.rs：safe 解压（防 zip-slip/符号链接）与 zip 打包
    extract: (archivePath: string, destDir?: string) => Promise<{ ok: boolean; destPath: string; entryCount: number }>;
    compress: (paths: string[], destZipPath?: string) => Promise<{ ok: boolean; zipPath: string; entryCount: number }>;
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
    commit: (dirPath: string, message: string) => Promise<unknown>;
    push: (dirPath: string) => Promise<unknown>;
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
    saveAnnotated: (request: ScreenshotSaveAnnotatedRequest) => Promise<ScreenshotSaveAnnotatedResult>;
  };
  release: {
    inspect: (projectPath: string) => Promise<unknown>;
    prepare: (projectPath: string, version: string) => Promise<ReleasePreparation>;
    getSequence: (projectPath: string, version: string) => Promise<ReleasePlan>;
    execute: (projectPath: string, version: string, action: ReleaseAction) => Promise<ReleaseExecution>;
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
    getCached: (query: {
      preset: 'today' | '24h' | '7d' | '30d' | '90d' | 'custom';
      timeZone: string;
      projectPath: string | null;
      customStartMs?: number;
      customEndMs?: number;
    }) => Promise<{
      state: 'ready';
      metadata: { schemaVersion: number; generatedAtMs: number; coverageStartMs: number; coverageEndMs: number; timeZone: string };
      response: unknown;
    } | { state: 'missing'; metadata: null; response: null }>;
    sync: (request: {
      timeZone: string;
      currentView: {
        preset: 'today' | '24h' | '7d' | '30d' | '90d' | 'custom';
        timeZone: string;
        projectPath: string | null;
        customStartMs?: number;
        customEndMs?: number;
      };
    }) => Promise<{
      metadata: { schemaVersion: number; generatedAtMs: number; coverageStartMs: number; coverageEndMs: number; timeZone: string };
      response: unknown;
    }>;
    /** Optional ccusage enrichment (default disabled). */
    getCcusageEnabled: () => Promise<boolean>;
    setCcusageEnabled: (enabled: boolean) => Promise<boolean>;
    detectCcusage: () => Promise<string | null>;
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
      apiProtocol: string;
      displayName: string;
      websiteUrl: string;
      baseUrl: string;
      defaultModel: string;
      initialKey: { label: string; apiKey: string };
    }) => Promise<ProviderSummary>;
    delete: (providerId: string) => Promise<void>;
    updateDefaults: (input: { providerId: string; defaultModel: string }) => Promise<void>;
    addKey: (input: { providerId: string; label: string; apiKey: string }) => Promise<ProviderKeySummary>;
    testCandidate: (input: { providerType: string; apiProtocol?: string; baseUrl: string; apiKey: string; model: string }) => Promise<ProviderTestResult>;
    testKey: (input: { providerId: string; keyId: string; model?: string }) => Promise<ProviderTestResult>;
    discoverModels: (input: { providerType: string; apiProtocol?: string; baseUrl: string; apiKey: string }) => Promise<Array<{ id: string; displayName?: string }>>;
    discoverModelsSaved: (input: { providerId: string; keyId: string }) => Promise<Array<{ id: string; displayName?: string }>>;
    setPrimaryKey: (input: { providerId: string; keyId: string }) => Promise<void>;
    deleteKey: (input: { providerId: string; keyId: string }) => Promise<void>;
  };
  /** Optional until the Host routing commands are registered. */
  providerRouting?: ProviderRoutingApi;
  windowControls: {
    minimize: () => Promise<void>;
    maximize: () => Promise<void>;
    toggleFullscreen: () => Promise<void>;
    close: () => Promise<void>;
    isMaximized: () => Promise<boolean>;
    isFullscreen: () => Promise<boolean>;
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
  // T202/T302: legacy assistant_* CRUD interface retired (Daemon-canonical now).
  daemonSupervisor: {
    status: () => Promise<unknown>;
    ensure: () => Promise<unknown>;
    poll: () => Promise<unknown>;
    shutdown: () => Promise<unknown>;
  };
  // MIG-002: legacy executor_get_settings / executor_save_settings 已物理注销，
  // adapter 不再暴露旧写入口。执行引擎设置唯一权威 = executionEngine（V2）。
  /** 执行引擎设置 V2（唯一持久化权威 — A6 backend） */
  executionEngine: {
    /** legacyRuntimeId 仅用于一次性迁移（MIG-001），日常为 null。 */
    getSnapshot: (legacyRuntimeId?: string | null) => Promise<ExecutionEngineSnapshot>;
    saveSettings: (settings: ExecutionEngineSettings) => Promise<ExecutionEngineSettings>;
    detectRuntimes: () => Promise<RuntimeDescriptor[]>;
    getDiagnostics: () => Promise<Record<string, unknown>>;
  };
  /** Assistant in-process RPC (no daemon sidecar) */
  assistantV2: {
    request<T>(method: string, params?: unknown): Promise<T>;
    getStatus(): Promise<{ connected: boolean; error: string | null }>;
    subscribeHarness(
      listener: (notice: HarnessNotice) => void,
      options?: { cursor?: number; onError?: (error: unknown) => void },
    ): () => void;
  };
  /** Project directory management */
  project: {
    list(): Promise<ProjectSummary[]>;
    /** Soft-deleted (hidden) project paths — product decision 1. */
    listHidden(): Promise<string[]>;
    register(path: string): Promise<ProjectSummary>;
    rename(id: string, label: string): Promise<void>;
    remove(id: string): Promise<void>;
  };
  // macOS menubar popup (native_menubar_lifecycle: commands/menubar.rs).
  // Frozen contract: window label `menubar`, route `?surface=menubar`; these
  // commands validate the invoking window label in Rust.
  menubar: {
    toggle: () => Promise<void>;
    hide: () => Promise<void>;
    openMain: () => Promise<void>;
    openPersonalOverview: () => Promise<void>;
    quit: () => Promise<void>;
  };
  /** Dialog (file/directory picker) */
  dialog: {
    pickDirectory(): Promise<string | null>;
    pickFiles(): Promise<string[]>;
    saveFile(): Promise<string | null>;
  };
  /** Runtime 抽象层（Slice B） */
  runtime: {
    listAvailable: () => Promise<Array<{ id: string; displayName: string; available: boolean }>>;
    detectCli: () => Promise<{ claude_cli: boolean; codex_cli: boolean }>;
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
    /** 部分更新：缺字段后端保留现值；folderId 显式 null = 移出文件夹 */
    updateItem: (data: { id: string; folderId?: string | null; title?: string; description?: string; content?: string; sourceUrl?: string; status?: string; tagIds?: string[] }) => Promise<void>;
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
  /** Capability secrets (ADR-0016 决策 7) — Host 侧加密存储；list 绝不返回明文 */
  capabilitySecret: {
    set: (data: { kind: 'mcp_env' | 'mcp_bearer' | 'mcp_oauth_refresh'; ownerRef: string; keyName?: string; plaintext: string }) => Promise<{ id: string }>;
    delete: (id: string) => Promise<void>;
    list: (ownerRef: string) => Promise<Array<{ id: string; kind: string; keyName: string | null; createdAt: string }>>;
  };
  /** MCP OAuth 浏览器流 (ADR-0016 决策 7) — Host 侧 loopback + PKCE S256 */
  mcpOauth: {
    start: (data: { serverId: string; authorizeUrl: string; tokenUrl: string; clientId: string; scopes?: string[]; redirectPort?: number }) => Promise<{ ok: boolean; hasRefresh: boolean }>;
  };
  /** Provider OAuth 浏览器流 (ADR-0019 P3) — Host 侧 PKCE；Renderer 只拿安全 session 状态，绝不见 token */
  providerOauth: {
    start: (data: { providerId: string; platform?: string; authorizeUrl?: string; tokenUrl?: string; clientId?: string; scopes?: string[]; redirectPort?: number; identity?: string; accountName?: string; clientSecret?: string; projectId?: string }) => Promise<{ ok: boolean; accountId: string; state: string; hasRefresh: boolean }>;
    status: (data: { providerId: string }) => Promise<Array<{ id: string; name: string; platform: string; status: string; expiresAt: string | null; hasRefresh: boolean; projectId?: string | null; email?: string | null }>>;
    setProjectId: (data: { providerId: string; accountId: string; projectId: string }) => Promise<{ ok: boolean }>;
    disconnect: (data: { providerId: string; accountId: string }) => Promise<{ ok: boolean }>;
    refresh: (data: { providerId: string; accountId: string }) => Promise<{ ok: boolean; state: string; hasRefresh: boolean }>;
    deviceStart: (data: { providerId: string; platform?: string; clientId?: string; accountName?: string; identity?: string }) => Promise<{ ok: boolean; sessionId: string; userCode: string; verificationUri: string; verificationUriComplete: string | null; expiresIn: number; interval: number }>;
    devicePoll: (data: { sessionId: string }) => Promise<{ status: 'pending' | 'connected' | 'expired'; accountId?: string | null; state?: string | null; hasRefresh?: boolean | null; expiresIn?: number | null; interval?: number | null }>;
  };
  /** Job module（任务）— 契约 v1：8 个 job_* 命令，JSON snake_case；强类型见 src/lib/jobs-api.ts */
  jobs: {
    list: () => Promise<unknown>;
    get: (id: string) => Promise<unknown>;
    create: (payload: Record<string, unknown>) => Promise<unknown>;
    update: (payload: Record<string, unknown>) => Promise<unknown>;
    delete: (id: string) => Promise<unknown>;
    setEnabled: (id: string, enabled: boolean) => Promise<unknown>;
    runNow: (id: string) => Promise<unknown>;
    listRuns: (params: { job_id?: string; limit?: number; offset?: number }) => Promise<unknown>;
  };
}
