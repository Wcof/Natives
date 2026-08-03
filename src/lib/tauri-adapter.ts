/**
 * Tauri adapter — replaces electron/preload.ts
 *
 * Maps window.nativesAPI methods to Tauri invoke calls.
 * Every method matches the original Electron IPC contract exactly.
 * Unimplemented commands throw "not implemented" errors, never fake success.
 */

import { invoke, convertFileSrc } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { unwrapAssistantRpc, type AssistantRpcEnvelope } from './assistant-rpc';
import { classifyError } from './error-classifier';
import { getHttpPort } from './natives-http-port';
import type { ProviderRoutingApi } from '@/types/provider-routing';
import type { ProviderRouteBinding, ProviderRoutingSettings } from '@/types/provider-routing';

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

/** write_generated_module result — ok/moduleId always present */
export interface WriteGeneratedModuleResult {
  moduleId: string;
  ok: boolean;
  oldContent?: string | null;
  newContent?: string;
  contractId?: string;
  contentHash?: string;
}

// ── Creative App (multi-source Personal Creations) ──

export type CreativeAppSource = 'internal' | 'external_github' | 'local_project';
export type CreativeAppRuntime =
  | 'workshop_static'
  | 'docker_compose'
  | 'docker_run'
  | 'local_static'
  | 'node_dev_server';
export type CreativeAppState =
  | 'available'
  | 'disabled'
  | 'installing'
  | 'installed_stopped'
  | 'starting'
  | 'running'
  | 'stopping'
  | 'runtime_unavailable'
  | 'install_failed'
  | 'start_failed'
  | 'deleting'
  | 'delete_failed'
  | 'cleanup_failed'
  | 'orphaned';

export type LocalProjectKind =
  | 'html'
  | 'vite'
  | 'vue'
  | 'vue_vite'
  | 'vite_other'
  | 'unknown';

export type LaunchMode = 'smart' | 'custom';
export type PackageManager = 'npm' | 'pnpm' | 'yarn';

export type LocalCreativeIssueCode =
  | 'path_missing'
  | 'environment_missing'
  | 'dependencies_missing'
  | 'port_conflict'
  | 'config_invalid'
  | 'ai_error'
  | 'start_unhealthy'
  | 'orphaned_process';

export interface CreativeAppStatusDetail {
  code: LocalCreativeIssueCode;
  message: string;
  recoveryActions?: string[];
}

export interface LocalProjectSummary {
  projectRoot: string;
  projectKind: LocalProjectKind;
  launchMode: LaunchMode;
  packageManager?: PackageManager;
  deviceId: string;
  deviceName: string;
  /** Host should open the app GUI after a successful start. */
  autoOpen: boolean;
}

export interface CreativeAppActions {
  canOpen: boolean;
  canStart: boolean;
  canStop: boolean;
  canDelete: boolean;
  canRetry: boolean;
}

export interface CreativeAppSummary {
  id: string;
  /** Unified identity across Catalog / assistant card / detail / preview. */
  applicationId: string;
  /** Active runtime instance id, when the app currently has one. */
  runtimeInstanceId?: string;
  source: CreativeAppSource;
  runtime: CreativeAppRuntime;
  title: string;
  description?: string;
  icon?: string;
  version: string;
  state: CreativeAppState;
  openUrl?: string;
  repositoryUrl?: string;
  lastError?: string;
  statusDetail?: CreativeAppStatusDetail;
  localProject?: LocalProjectSummary;
  actions: CreativeAppActions;
}

/**
 * A draft is what an idea lives in before it becomes a module: no contract_id,
 * no `modules` row, no sidebar entry until the user publishes it (ADR-0014).
 * The state machine mirrors `src/lib/creative-draft.ts`.
 */
export interface CreativeDraft {
  draftId: string;
  name: string;
  /** The user's original one-sentence request. */
  intent: string;
  conversationId?: string | null;
  /** Absent for a brand-new draft; set when continuing an existing module. */
  originModuleId?: string | null;
  currentRevision: number;
  state: 'drafting' | 'generating' | 'ready' | 'publishing' | 'published' | 'archived';
  createdAt: string;
  updatedAt: string;
}

export interface CreativeDraftPublishResult {
  ok: true;
  draftId: string;
  moduleId: string;
  contractId: string;
  contentHash: string;
  /** Previous module HTML, present only when overwriting — powers one-click rollback. */
  oldContent?: string | null;
}

export interface CreativeAppDeleteOptions {
  removeVolumes?: boolean;
  removeImages?: boolean;
}

export interface CreativeAppDeleteResult {
  ok: boolean;
  warnings: string[];
}

export type CreativeAppOpenTarget =
  | { kind: 'workshop_module'; moduleId: string }
  | { kind: 'local_url'; url: string; appId: string };

export interface CreativeAppInspectRequest {
  repositoryUrl: string;
  token?: string | null;
  saveToken?: boolean;
  oneClick?: boolean;
  releaseTag?: string | null;
}

export interface CreativeAppInstallRequest {
  repositoryUrl: string;
  releaseTag: string;
  releaseId?: number | null;
  candidateId: string;
  token?: string | null;
  hostPort?: number | null;
  openPath?: string | null;
  healthPath?: string | null;
  service?: string | null;
  env?: Array<{ key: string; value: string }>;
  confirmBindMounts?: boolean;
}

export interface CreativeAppEnvRequirement {
  key: string;
  required: boolean;
  secret: boolean;
}

export interface CreativeAppInstallCandidate {
  id: string;
  runtime: CreativeAppRuntime;
  confidence: number;
  title: string;
  description: string;
  primaryAsset: string;
  service?: string | null;
  image?: string | null;
  suggestedHostPort?: number | null;
  containerPort?: number | null;
  openPath: string;
  healthPath?: string | null;
  envRequirements: CreativeAppEnvRequirement[];
  riskSummary: string[];
  hardBlockers: string[];
  requiresManual: boolean;
}

export interface CreativeAppReleaseTagInfo {
  tag: string;
  releaseId: number;
  isPrerelease: boolean;
}

export interface CreativeAppInspectResult {
  repositoryUrl: string;
  owner: string;
  repo: string;
  releaseTag: string;
  releaseId?: number | null;
  isPrerelease: boolean;
  candidates: CreativeAppInstallCandidate[];
  oneClickEligible: boolean;
  oneClickCandidateId?: string | null;
  warnings: string[];
  blockers: string[];
  availableTags: CreativeAppReleaseTagInfo[];
}

export interface CreativeAppGithubTokenStatus {
  configured: boolean;
  masked?: string | null;
}

export interface CreativeAppDockerStatus {
  available: boolean;
  version?: string | null;
  composeAvailable: boolean;
  composeVersion?: string | null;
  error?: string | null;
}

export interface CreativeAppBrowserBounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

export type CreativeAppProgressStage =
  | 'inspecting_release'
  | 'downloading_assets'
  | 'pulling_image'
  | 'creating'
  | 'starting'
  | 'health_check'
  | 'ready'
  | 'failed'
  | 'stopped'
  | 'installing_dependencies';

export interface CreativeAppProgressEvent {
  appId: string;
  stage: CreativeAppProgressStage | string;
  message: string;
}

export interface CreativeAppLogEvent {
  appId: string;
  seq: number;
  tsMs: number;
  stream: 'stdout' | 'stderr' | 'system' | string;
  text: string;
}

export interface LaunchPlan {
  schemaVersion: 1;
  source: 'rule' | 'user' | 'ai';
  projectKind: LocalProjectKind;
  runtime: 'static_http' | 'node_dev_server' | 'docker_compose';
  program: 'internal' | 'npm' | 'pnpm' | 'yarn' | 'node';
  cwdRelative: string;
  script?: string;
  entryFile?: string;
  scriptRunner?: 'vite' | 'vue_cli' | 'node';
  args: string[];
  environmentKeys: string[];
  port: { mode: 'auto' | 'fixed'; value?: number };
  openPath: string;
  healthPath: string;
  startupTimeoutMs: number;
  autoOpen: boolean;
  confidence?: number;
  reason: string;
  /** Compose detail when runtime is docker_compose (batch 5). */
  compose?: ComposePlanDetail;
}

export interface ComposePlanDetail {
  composeFile: string;
  projectSeed: string;
  service?: string;
  command: string[];
  healthPath: string;
  hostPort?: number;
}

export interface LocalToolVersions {
  node?: string;
  npm?: string;
  pnpm?: string;
  yarn?: string;
}

export interface LocalProjectScanResult {
  projectRoot: string;
  projectKind: LocalProjectKind;
  packageManager?: PackageManager;
  packageManagerChoices: PackageManager[];
  scripts: string[];
  preferredScript?: string;
  hasNodeModules: boolean;
  dependenciesMissing: boolean;
  toolVersions: LocalToolVersions;
  risks: string[];
  blockers: string[];
  rulePlan?: LaunchPlan;
  /** Non-web manifests detected (docker-compose, dockerfile, python, makefile). */
  extraManifests: string[];
  treeSample: string[];
  existingId?: string;
}

export interface CreateLocalCreativeRequest {
  projectRoot: string;
  title: string;
  description?: string;
  icon?: string;
  launchMode: LaunchMode;
  launchPlan?: LaunchPlan;
  env?: Array<{ key: string; value: string }>;
  autoOpen?: boolean;
  startupTimeoutMs?: number;
}

export interface UpdateLocalCreativeRequest {
  id: string;
  title?: string;
  description?: string;
  icon?: string;
  launchMode?: LaunchMode;
  launchPlan?: LaunchPlan;
  env?: Array<{ key: string; value: string }>;
  envUpsert?: Array<{ key: string; value: string }>;
  envRemoveKeys?: string[];
  autoOpen?: boolean;
  startupTimeoutMs?: number;
  projectRoot?: string;
}

export interface LocalCreativeAiSettings {
  enabled: boolean;
  providerId?: string | null;
  model?: string | null;
  mode: string;
  userConsented: boolean;
  timeoutMs: number;
}

export interface LocalCreativeConfig {
  summary: CreativeAppSummary;
  launchPlan: LaunchPlan;
  envKeys: string[];
  dependencyInstall?: {
    program: string;
    args: string[];
    packageManager: PackageManager;
    requiresConfirmation: boolean;
    display: string;
  };
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
  lastError: string | null;
  createdAt: string;
}

export interface ProviderSummary {
  id: string;
  providerType: string;
  apiProtocol: string;
  displayName: string;
  websiteUrl: string;
  baseUrl: string;
  defaultModel: string | null;
  primaryKeyId: string | null;
  keys: ProviderKeySummary[];
  models?: Array<{ id: string; displayName?: string | null }>;
}

export interface ProviderTestResult {
  success: boolean;
  status: ProviderKeySummary['status'];
  testedAt: string;
  errorCode: string | null;
  userMessage: string | null;
}

interface StoredProviderKey extends Omit<ProviderKeySummary, 'lastError'> {
  lastErrorMessage: string | null;
}

interface StoredProvider {
  id: string;
  presetName: string;
  apiProtocol?: string;
  name: string;
  websiteUrl: string;
  baseUrl: string;
  defaultModel: string | null;
  primaryKeyId: string | null;
  keys: StoredProviderKey[];
  models?: Array<{ id: string; displayName?: string | null }>;
}

interface StoredProviderTestResult {
  success: boolean;
  error: string | null;
}

function normalizeProviderKey(key: StoredProviderKey): ProviderKeySummary {
  const { lastErrorMessage, ...rest } = key;
  return { ...rest, lastError: lastErrorMessage };
}

function normalizeProvider(provider: StoredProvider): ProviderSummary {
  return {
    id: provider.id,
    providerType: provider.presetName,
    apiProtocol: provider.apiProtocol ?? provider.presetName,
    displayName: provider.name,
    websiteUrl: provider.websiteUrl,
    baseUrl: provider.baseUrl,
    defaultModel: provider.defaultModel,
    primaryKeyId: provider.primaryKeyId,
    keys: provider.keys.map(normalizeProviderKey),
    models: provider.models ?? [],
  };
}

type HostRoutingSettings = { enabled: boolean; localEnabled: boolean; localPort: number; rectifier: { enabled?: boolean }; globalProxy: { enabled?: boolean; url?: string } };
type HostLocalRoutingTokenIssued = { token: string };
type HostRouteBinding = { id: string; position: number; providerId: string; credentialKind: 'api_key' | 'sub2api_pool'; credentialId: string | null; modelId: string; enabled: boolean; createdAt: string; updatedAt: string };

function normalizeRoutingSettings(settings: HostRoutingSettings): ProviderRoutingSettings {
  return {
    enabled: settings.enabled,
    loopbackEnabled: settings.localEnabled,
    loopbackPort: settings.localPort,
    rectifierEnabled: settings.rectifier.enabled === true,
    outboundProxyEnabled: settings.globalProxy.enabled === true,
    outboundProxyUrl: typeof settings.globalProxy.url === 'string' ? settings.globalProxy.url : null,
  };
}

function normalizeRouteBinding(binding: HostRouteBinding): ProviderRouteBinding {
  return {
    id: binding.id,
    providerId: binding.providerId,
    modelId: binding.modelId,
    credential: binding.credentialKind === 'api_key' && binding.credentialId ? { kind: 'api_key', keyId: binding.credentialId } : { kind: 'sub2api_pool' },
    priority: binding.position,
    enabled: binding.enabled,
  };
}

/**
 * Map a stored provider-test payload into UI-facing fields.
 * Non-throwing failures still need classification so AddProvider / ProviderDetail
 * do not dump raw diagnostic strings (protocol=/http_status=/request_id=…).
 */
export function normalizeProviderTest(result: StoredProviderTestResult): ProviderTestResult {
  if (result.success) {
    return {
      success: true,
      status: 'valid',
      testedAt: new Date().toISOString(),
      errorCode: null,
      userMessage: null,
    };
  }

  const raw = result.error ?? 'Provider test failed';
  const classified = classifyError(raw);
  const lower = raw.toLowerCase();
  const rateLimited =
    classified.category === 'RATE_LIMITED' ||
    lower.includes('http_status=429') ||
    lower.includes('rate limited') ||
    lower.includes('too many requests');

  return {
    success: false,
    status: rateLimited ? 'rate_limited' : 'invalid',
    testedAt: new Date().toISOString(),
    errorCode: rateLimited ? 'RATE_LIMITED' : classified.category,
    userMessage: classified.userMessage,
  };
}

export interface ProjectSummary {
  id: string;
  path: string;
  label: string;
  conversationCount: number;
  exists: boolean;
  lastOpenedAt: string;
}

export interface HarnessNotice {
  cursor: number;
  kind: 'published' | 'binding_changed' | 'source_drift' | 'trace_updated' | 'reset_required';
  profile_id?: string | null;
  run_id?: string | null;
  created_at?: string;
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
    start: (id: string) => Promise<CreativeAppSummary>;
    stop: (id: string) => Promise<CreativeAppSummary>;
    delete: (id: string, options?: CreativeAppDeleteOptions) => Promise<CreativeAppDeleteResult>;
    getOpenTarget: (id: string) => Promise<CreativeAppOpenTarget>;
    inspectGithub: (request: CreativeAppInspectRequest) => Promise<CreativeAppInspectResult>;
    installGithub: (request: CreativeAppInstallRequest) => Promise<CreativeAppSummary>;
    logs: (id: string, tail?: number) => Promise<string>;
    reconcile: () => Promise<number>;
    githubTokenStatus: () => Promise<CreativeAppGithubTokenStatus>;
    githubTokenSet: (token: string) => Promise<CreativeAppGithubTokenStatus>;
    githubTokenClear: () => Promise<CreativeAppGithubTokenStatus>;
    dockerStatus: () => Promise<CreativeAppDockerStatus>;
    browserShow: (appId: string, url: string, bounds: CreativeAppBrowserBounds) => Promise<void>;
    browserSetBounds: (bounds: CreativeAppBrowserBounds) => Promise<void>;
    browserBack: () => Promise<void>;
    browserForward: () => Promise<void>;
    browserReload: () => Promise<void>;
    browserHide: () => Promise<void>;
    browserClose: () => Promise<void>;
    browserCurrent: () => Promise<{ appId?: string | null; url?: string | null }>;
    onProgress: (callback: (event: CreativeAppProgressEvent) => void) => () => void;
    onLog: (callback: (event: CreativeAppLogEvent) => void) => () => void;
    inspectLocal: (request: { projectRoot: string }) => Promise<LocalProjectScanResult>;
    createLocal: (request: CreateLocalCreativeRequest) => Promise<CreativeAppSummary>;
    updateLocal: (request: UpdateLocalCreativeRequest) => Promise<CreativeAppSummary>;
    rescanLocal: (id: string) => Promise<LocalProjectScanResult>;
    restart: (id: string) => Promise<CreativeAppSummary>;
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
  assistant: {
    listSessions: (params: { projectId: string | null }) => Promise<unknown>;
    getMessages: (sessionId: string) => Promise<unknown>;
    createSession: (params: { projectId: string | null; title: string; modelId: string; providerId: string }) => Promise<unknown>;
    deleteSession: (sessionId: string) => Promise<void>;
    saveMessage: (params: { sessionId: string; role: string; content: string; status: string; tokenCount: number; toolCalls?: string; toolResult?: string }) => Promise<unknown>;
    updateMessageStatus: (params: { messageId: string; status: string; toolResult?: string }) => Promise<void>;
    updateSessionTitle: (params: { sessionId: string; title: string }) => Promise<void>;
    updateSessionModel: (params: { sessionId: string; modelId: string; providerId: string }) => Promise<void>;
  };
  daemonSupervisor: {
    status: () => Promise<unknown>;
    ensure: () => Promise<unknown>;
    poll: () => Promise<unknown>;
    shutdown: () => Promise<unknown>;
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
    subscribeHarness(
      listener: (notice: HarnessNotice) => void,
      options?: { cursor?: number; onError?: (error: unknown) => void },
    ): () => void;
  };
  /** Project directory management */
  project: {
    list(): Promise<ProjectSummary[]>;
    register(path: string): Promise<ProjectSummary>;
    rename(id: string, label: string): Promise<void>;
    remove(id: string): Promise<void>;
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
    setCapabilityEnabled: (name: string, enabled: boolean) => Promise<void>;
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
      cmd<WriteGeneratedModuleResult>('write_generated_module', {
        moduleId,
        name,
        htmlContent,
        permissions,
      }),
    rollback: (params: { moduleId: string; oldContent: string }) =>
      cmd('rollback_module', params),
  },

  // Creative App (multi-source)
  creativeApp: {
    list: () => cmd<CreativeAppSummary[]>('creative_app_list'),
    start: (id: string) => cmd<CreativeAppSummary>('creative_app_start', { id }),
    stop: (id: string) => cmd<CreativeAppSummary>('creative_app_stop', { id }),
    delete: (id: string, options?: CreativeAppDeleteOptions) =>
      cmd<CreativeAppDeleteResult>('creative_app_delete', { id, options }),
    getOpenTarget: (id: string) =>
      cmd<CreativeAppOpenTarget>('creative_app_get_open_target', { id }),
    inspectGithub: (request: CreativeAppInspectRequest) =>
      cmd<CreativeAppInspectResult>('creative_app_inspect_github', { request }),
    installGithub: (request: CreativeAppInstallRequest) =>
      cmd<CreativeAppSummary>('creative_app_install_github', { request }),
    logs: (id: string, tail?: number) => cmd<string>('creative_app_logs', { id, tail }),
    reconcile: () => cmd<number>('creative_app_reconcile'),
    githubTokenStatus: () =>
      cmd<CreativeAppGithubTokenStatus>('creative_app_github_token_status'),
    githubTokenSet: (token: string) =>
      cmd<CreativeAppGithubTokenStatus>('creative_app_github_token_set', { token }),
    githubTokenClear: () =>
      cmd<CreativeAppGithubTokenStatus>('creative_app_github_token_clear'),
    dockerStatus: () => cmd<CreativeAppDockerStatus>('creative_app_docker_status'),
    browserShow: (appId: string, url: string, bounds: CreativeAppBrowserBounds) =>
      cmd('creative_app_browser_show', { appId, url, bounds }),
    browserSetBounds: (bounds: CreativeAppBrowserBounds) =>
      cmd('creative_app_browser_set_bounds', { bounds }),
    browserBack: () => cmd('creative_app_browser_back'),
    browserForward: () => cmd('creative_app_browser_forward'),
    browserReload: () => cmd('creative_app_browser_reload'),
    browserHide: () => cmd('creative_app_browser_hide'),
    browserClose: () => cmd('creative_app_browser_close'),
    browserCurrent: () =>
      cmd<{ appId?: string | null; url?: string | null }>('creative_app_browser_current'),
    onProgress: (callback) => {
      const unlisten = listen<CreativeAppProgressEvent>('creative-app-progress', (event) => {
        callback(event.payload);
      });
      return () => {
        unlisten.then((fn) => fn());
      };
    },
    onLog: (callback) => {
      const unlisten = listen<CreativeAppLogEvent>('creative-app-log', (event) => {
        callback(event.payload);
      });
      return () => {
        unlisten.then((fn) => fn());
      };
    },
    inspectLocal: (request: { projectRoot: string }) =>
      cmd<LocalProjectScanResult>('creative_app_inspect_local', { request }),
    createLocal: (request: CreateLocalCreativeRequest) =>
      cmd<CreativeAppSummary>('creative_app_create_local', { request }),
    updateLocal: (request: UpdateLocalCreativeRequest) =>
      cmd<CreativeAppSummary>('creative_app_update_local', { request }),
    rescanLocal: (id: string) =>
      cmd<LocalProjectScanResult>('creative_app_rescan_local', { id }),
    restart: (id: string) => cmd<CreativeAppSummary>('creative_app_restart', { id }),
    resolveOrphan: (id: string, restart: boolean) =>
      cmd<CreativeAppSummary>('creative_app_resolve_orphan', { id, restart }),
    getLocalLogs: (id: string, limit?: number) =>
      cmd<Array<{ seq: number; tsMs: number; stream: string; text: string }>>(
        'creative_app_get_local_logs',
        { id, limit },
      ),
    installLocalDependencies: (id: string) =>
      cmd<CreativeAppSummary>('creative_app_install_local_dependencies', { id }),
    previewLocalDependencyInstall: (id: string) =>
      cmd<{
        program: string;
        args: string[];
        packageManager: PackageManager;
        requiresConfirmation: true;
        display: string;
      }>('creative_app_preview_local_dependency_install', { id }),
    getLocalAiSettings: () =>
      cmd<LocalCreativeAiSettings>('creative_app_get_local_ai_settings'),
    saveLocalAiSettings: (settings: LocalCreativeAiSettings) =>
      cmd<LocalCreativeAiSettings>('creative_app_save_local_ai_settings', { settings }),
    previewLocalAi: (projectRoot: string) =>
      cmd<{
        scan: LocalProjectScanResult;
        payloadPreview: unknown;
        settings: LocalCreativeAiSettings;
        requiresConfirmation: true;
      }>('creative_app_preview_local_ai', { projectRoot }),
    analyzeLocalWithAi: (projectRoot: string, confirmed: boolean) =>
      cmd<{
        scan: LocalProjectScanResult;
        aiPlan?: LaunchPlan | null;
        payloadPreview: unknown;
      }>('creative_app_analyze_local_with_ai', { projectRoot, confirmed }),
    diagnoseLocalWithAi: (id: string) =>
      cmd<{
        schemaVersion: number;
        issueCode: LocalCreativeIssueCode;
        summary: string;
        recoveryActions: string[];
      }>('creative_app_diagnose_local_with_ai', { id }),
    getLocalConfig: (id: string) =>
      cmd<LocalCreativeConfig>('creative_app_get_local_config', { id }),
    pollLocalExits: () => cmd<number>('creative_app_poll_local_exits'),
  },

  // Creative drafts — the creation loop before a module exists
  creativeDraft: {
    create: (request) => cmd<CreativeDraft>('create_creative_draft', request),
    list: () => cmd<CreativeDraft[]>('list_creative_drafts'),
    get: (draftId: string) => cmd<CreativeDraft>('get_creative_draft', { draftId }),
    read: (draftId: string) =>
      cmd<{ draftId: string; html: string }>('read_creative_draft', { draftId }),
    bindConversation: (draftId: string, conversationId: string) =>
      cmd<{ ok: boolean; draftId: string }>('bind_creative_draft_conversation', {
        draftId,
        conversationId,
      }),
    rollback: (draftId: string) =>
      cmd<{ draftId: string; revision: number }>('rollback_creative_draft', { draftId }),
    publish: (request) =>
      cmd<CreativeDraftPublishResult>('publish_creative_draft', request),
    delete: (draftId: string) =>
      cmd<{ ok: boolean; draftId: string }>('delete_creative_draft', { draftId }),
    previewUrl: async (draftId: string) => {
      const port = await getHttpPort();
      return `http://localhost:${port}/drafts/${draftId}/`;
    },
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
  setLocale: async (locale: string) => {
    window.dispatchEvent(new CustomEvent('locale-changed', { detail: locale }));
    await cmd('set_locale', { locale });
  },

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
    listDirDetailed: (dirPath: string, options?: unknown) =>
      cmd('fs_list_dir_detailed', { dirPath, options }),
    readFile: (filePath: string) => cmd('fs_read_file', { filePath }),
    writeFileAtomic: (filePath: string, content: string, expectedMtime?: number) =>
      cmd('fs_write_file_atomic', { filePath, content, expectedMtime }),
    createEntry: async (targetPath: string, type: string) => {
      try {
        // Backend accepts "file" | "dir" | "folder". Tauri maps camelCase → snake_case.
        const entryType = type === 'folder' ? 'folder' : type;
        await cmd('fs_create_entry', { targetPath, entryType });
        return { ok: true };
      } catch (e: any) {
        return { ok: false, error: e?.message || String(e) };
      }
    },
    renameEntry: async (oldPath: string, newPath: string) => {
      try {
        const path = await cmd<string>('fs_rename_entry', { oldPath, newPath });
        return { ok: true, path };
      } catch (e: any) {
        return { ok: false, error: e?.message || String(e) };
      }
    },
    trashEntry: async (filePath: string) => {
      try {
        await cmd('fs_trash_entry', { filePath });
        return { ok: true };
      } catch (e: any) {
        return { ok: false, error: e?.message || String(e) };
      }
    },
    moveEntry: async (from: string, to: string) => {
      try {
        const path = await cmd<string>('fs_move_entry', { from, to });
        return { ok: true, path };
      } catch (e: any) {
        return { ok: false, error: e?.message || String(e) };
      }
    },
    copyEntry: async (from: string, to: string) => {
      try {
        const path = await cmd<string>('fs_copy_entry', { from, to });
        return { ok: true, path };
      } catch (e: any) {
        return { ok: false, error: e?.message || String(e) };
      }
    },
    duplicateEntry: async (filePath: string) => {
      try {
        const path = await cmd<string>('fs_duplicate_entry', { filePath });
        return { ok: true, path };
      } catch (e: any) {
        return { ok: false, error: e?.message || String(e) };
      }
    },
    stat: (filePath: string) => cmd('fs_stat', { filePath }),
    importFiles: (sourcePaths: string[], destDir: string) =>
      cmd('fs_import_files', { sourcePaths, destDir }),
    recentFiles: (root: string) => cmd('fs_recent_files', { root }),
    saveBlob: (dir: string, name: string, base64Data: string) =>
      cmd('fs_save_blob', { dir, name, base64Data }),
    convertFileSrc: (filePath: string) => convertFileSrc(filePath),
    convertImagePreview: (filePath: string) => cmd('fs_convert_image_preview', { filePath }),
    locate: (query: string, cwd?: string, roots?: string[]) => cmd('fs_locate', { query, cwd, roots }),
    verifyPaths: (candidates: string[]) => cmd('fs_verify_paths', { candidates }),
    trashEntries: async (paths: string[]) => {
      try {
        return await cmd('fs_trash_entries', { paths });
      } catch (e: any) {
        return { ok: false, errors: [{ path: '', error: e?.message || String(e) }] };
      }
    },
    moveEntries: async (paths: string[], destDir: string) => {
      try {
        return await cmd('fs_move_entries', { paths, destDir });
      } catch (e: any) {
        return { ok: false, errors: [{ path: '', error: e?.message || String(e) }] };
      }
    },
    copyEntries: async (paths: string[], destDir: string) => {
      try {
        return await cmd('fs_copy_entries', { paths, destDir });
      } catch (e: any) {
        return { ok: false, errors: [{ path: '', error: e?.message || String(e) }] };
      }
    },
    roots: () => cmd('fs_roots'),
    openWith: (path: string, withApp: 'default' | 'reveal' | 'terminal' | 'editor' = 'default') =>
      cmd('fs_open_with', { path, with: withApp }),
    clipboardCopyFiles: async (paths: string[]) => {
      try {
        return await cmd('fs_clipboard_copy_files', { paths });
      } catch (e: any) {
        return { ok: false, count: 0 };
      }
    },
    clipboardCopyImage: async (filePath: string) => {
      try {
        return await cmd('fs_clipboard_copy_image', { filePath });
      } catch (e: any) {
        return { ok: false };
      }
    },
  },

  // Archive
  archive: {
    list: (archivePath: string) => cmd('archive_list', { archivePath }),
    extract: (archivePath: string, destDir?: string) => cmd('fs_extract_archive', { archivePath, destDir }),
    compress: (paths: string[], destZipPath?: string) => cmd('fs_compress_entries', { paths, destZipPath }),
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
    commit: (dirPath: string, message: string) => cmd('git_commit', { dirPath, message }),
    push: (dirPath: string) => cmd('git_push', { dirPath }),
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
    getCached: (query: {
      preset: 'today' | '24h' | '7d' | '30d' | '90d' | 'custom';
      timeZone: string;
      projectPath: string | null;
      customStartMs?: number;
      customEndMs?: number;
    }) => cmd('usage_get_cached', { query }),
    sync: (request: {
      timeZone: string;
      currentView: {
        preset: 'today' | '24h' | '7d' | '30d' | '90d' | 'custom';
        timeZone: string;
        projectPath: string | null;
        customStartMs?: number;
        customEndMs?: number;
      };
    }) => cmd('usage_sync', { request }),
    getCcusageEnabled: () => cmd<boolean>('usage_get_ccusage_enabled'),
    setCcusageEnabled: (enabled: boolean) =>
      cmd<boolean>('usage_set_ccusage_enabled', { enabled }),
    detectCcusage: () => cmd<string | null>('usage_detect_ccusage'),
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
    pickFiles: async () => {
      try {
        const { open } = await import('@tauri-apps/plugin-dialog');
        const selected = await open({ directory: false, multiple: true });
        if (!selected) return [];
        return Array.isArray(selected) ? selected : [selected];
      } catch {
        return [];
      }
    },
    saveFile: async () => {
      try {
        const { save } = await import('@tauri-apps/plugin-dialog');
        const selected = await save();
        return selected as string | null;
      } catch {
        return null;
      }
    },
  },

  // Provider (unified API — single source of truth)
  provider: {
    list: () => cmd<StoredProvider[]>('list_providers').then(providers => providers.map(normalizeProvider)),
    create: (input: { providerType: string; apiProtocol: string; displayName: string; websiteUrl: string; baseUrl: string; defaultModel: string; initialKey: { label: string; apiKey: string } }) =>
      cmd<StoredProvider>('add_provider', { input }).then(normalizeProvider),
    delete: (providerId: string) => cmd('delete_provider', { providerId }),
    updateDefaults: (input: { providerId: string; defaultModel: string }) =>
      cmd('provider_update_defaults', { input }),
    addKey: (input: { providerId: string; label: string; apiKey: string }) =>
      cmd<StoredProviderKey>('add_provider_key', { input }).then(normalizeProviderKey),
    testCandidate: (input: { providerType: string; apiProtocol?: string; baseUrl: string; apiKey: string; model: string }) =>
      cmd<StoredProviderTestResult>('test_provider_raw', { input }).then(normalizeProviderTest),
    testKey: (input: { providerId: string; keyId: string; model?: string }) =>
      cmd<StoredProviderTestResult>('provider_test', { input }).then(normalizeProviderTest),
    discoverModels: (input: { providerType: string; apiProtocol?: string; baseUrl: string; apiKey: string }) =>
      cmd<Array<{ id: string; displayName?: string }>>('provider_discover_models', { input }),
    discoverModelsSaved: (input: { providerId: string; keyId: string }) =>
      cmd<Array<{ id: string; displayName?: string }>>('provider_discover_models_saved', { input }),
    setPrimaryKey: (input: { providerId: string; keyId: string }) =>
      cmd('provider_set_primary_key', { input }),
    deleteKey: (input: { providerId: string; keyId: string }) =>
      cmd('delete_provider_key', { input }),
  },

  providerRouting: {
    getSettings: () => cmd<HostRoutingSettings>('provider_routing_get_settings').then(normalizeRoutingSettings),
    saveSettings: (settings) => cmd<HostRoutingSettings>('provider_routing_update_settings', {
      input: {
        enabled: settings.enabled,
        localEnabled: settings.loopbackEnabled,
        localPort: settings.loopbackPort,
        rectifier: { enabled: settings.rectifierEnabled },
        globalProxy: { enabled: settings.outboundProxyEnabled, url: settings.outboundProxyUrl },
      },
    }).then(normalizeRoutingSettings),
    rotateLoopbackToken: () => cmd<HostLocalRoutingTokenIssued>('provider_routing_rotate_local_token').then((result) => result.token),
    listBindings: () => cmd<HostRouteBinding[]>('provider_routing_list_bindings').then((bindings) => bindings.map(normalizeRouteBinding)),
    saveBindings: (bindings) => cmd<HostRouteBinding[]>('provider_routing_update_bindings', {
      bindings: bindings.map((binding, position) => ({
        id: binding.id,
        position,
        providerId: binding.providerId,
        credentialKind: binding.credential.kind,
        credentialId: binding.credential.kind === 'api_key' ? binding.credential.keyId : null,
        modelId: binding.modelId,
        enabled: binding.enabled,
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      })),
    }).then((bindings) => bindings.map(normalizeRouteBinding)),
    listSub2ApiAccounts: (providerId) => cmd<Array<{ id: string; providerId: string; name: string; platform: string; accountType: string; concurrency: number; priority: number; expiresAt: string | null; status: 'active' | 'paused' | 'expired' | 'invalid' }>>('provider_accounts_list', { providerId }).then((accounts) => accounts.map((account) => ({ ...account, email: null }))),
    previewSub2ApiImport: ({ providerId, content }) => cmd<{ accounts: Array<{ index: number; name: string; platform: string; accountType: string; action: 'create' | 'update' | 'skip' | 'reject'; error: string | null }> }>('provider_accounts_preview_import', { providerId, source: content }).then((preview) => ({
      items: preview.accounts.map((account) => ({ ...account, email: null, reason: account.error })),
      rejected: preview.accounts.filter((account) => account.action === 'reject').length,
    })),
    commitSub2ApiImport: ({ providerId, content }) => cmd<{ created: number; updated: number; skipped: number; failed: unknown[] }>('provider_accounts_commit_import', { request: { providerId, source: content } }).then((result) => ({ ...result, failed: result.failed.length })),
    deleteSub2ApiAccounts: ({ providerId, accountIds }) => cmd<{ deleted: string[]; notFound: string[]; failed: string[] }>('provider_accounts_batch_delete', { request: { providerId, accountIds } }),
    createSub2ApiPool: ({ name }) => cmd<string>('provider_accounts_create_pool', { input: { name } }),
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
  },

  // Sidecar supervisor (production UDS health; no silent embedded fallback)
  daemonSupervisor: {
    status: () => cmd('daemon_supervisor_status'),
    ensure: () => cmd('daemon_supervisor_ensure'),
    poll: () => cmd('daemon_supervisor_poll'),
    shutdown: () => cmd('daemon_supervisor_shutdown'),
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
    setCapabilityEnabled: (name: string, enabled: boolean) =>
      cmd('runtime_set_capability_enabled', { name, enabled }),
  },

  // Job module（任务）— 契约 v1：入参 JSON snake_case，与后端命令面一致
  jobs: {
    list: () => cmd('job_list'),
    get: (id: string) => cmd('job_get', { id }),
    create: (payload: Record<string, unknown>) => cmd('job_create', payload),
    update: (payload: Record<string, unknown>) => cmd('job_update', payload),
    delete: (id: string) => cmd('job_delete', { id }),
    setEnabled: (id: string, enabled: boolean) => cmd('job_set_enabled', { id, enabled }),
    runNow: (id: string) => cmd('job_run_now', { id }),
    listRuns: (params: { job_id?: string; limit?: number; offset?: number }) =>
      cmd('job_runs_list', params as Record<string, unknown>),
  },

  // Window Controls
  windowControls: {
    minimize: () => cmd('window_minimize'),
    maximize: () => cmd('window_maximize'),
    toggleFullscreen: () => cmd('window_toggle_fullscreen'),
    close: () => cmd('window_close'),
    isMaximized: () => cmd('window_is_maximized'),
    isFullscreen: () => cmd('window_is_fullscreen'),
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
      id: string; folderId?: string | null; title?: string; description?: string;
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

  // ── Capability secrets (ADR-0016 决策 7) ──
  capabilitySecret: {
    set: (data: { kind: 'mcp_env' | 'mcp_bearer' | 'mcp_oauth_refresh'; ownerRef: string; keyName?: string; plaintext: string }) =>
      cmd<{ id: string }>('capability_secret_set', { input: data }),
    delete: (id: string) => cmd<void>('capability_secret_delete', { id }),
    list: (ownerRef: string) =>
      cmd<Array<{ id: string; kind: string; keyName: string | null; createdAt: string }>>('capability_secret_list', { ownerRef }),
  },

  // ── MCP OAuth 浏览器流 (ADR-0016 决策 7) ──
  mcpOauth: {
    start: (data: { serverId: string; authorizeUrl: string; tokenUrl: string; clientId: string; scopes?: string[]; redirectPort?: number }) =>
      cmd<{ ok: boolean; hasRefresh: boolean }>('mcp_oauth_start', { input: data }),
  },

  // Assistant in-process RPC (no daemon sidecar)
  assistantV2: {
    request: <T>(method: string, params?: unknown): Promise<T> =>
      cmd<AssistantRpcEnvelope<T>>('assistant_rpc_request', { method, params: params ?? null })
        .then(unwrapAssistantRpc),
    getStatus: (): Promise<{ connected: boolean; error: string | null }> =>
      cmd<{ connected: boolean; error: string | null }>('assistant_status'),
    subscribeHarness: (
      listener: (notice: HarnessNotice) => void,
      options?: { cursor?: number; onError?: (error: unknown) => void },
    ): (() => void) => {
      let stopped = false;
      let cursor = options?.cursor ?? 0;
      const run = async () => {
        while (!stopped) {
          try {
            const page = await nativesAPI.assistantV2.request<{
              notices: HarnessNotice[];
              next_cursor: number;
              reset_required?: boolean;
            }>('harness.subscribe', { cursor, wait_ms: 25_000, limit: 100 });
            if (stopped) break;
            for (const notice of page.notices) listener(notice);
            cursor = page.next_cursor;
          } catch (error) {
            if (stopped) break;
            options?.onError?.(error);
            await new Promise((resolve) => window.setTimeout(resolve, 1_000));
          }
        }
      };
      void run();
      return () => { stopped = true; };
    },
  },

  // Project directory management
  project: {
    list: (): Promise<ProjectSummary[]> => cmd<ProjectSummary[]>('project_list'),
    register: (path: string): Promise<ProjectSummary> => cmd<ProjectSummary>('project_register', { path }),
    rename: (id: string, label: string): Promise<void> => cmd<void>('project_rename', { id, label }),
    remove: (id: string): Promise<void> => cmd<void>('project_remove', { id }),
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
