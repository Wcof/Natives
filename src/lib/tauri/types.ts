/**
 * tauri/types — 共享类型契约（ARCH-002）
 *
 * NativesAPI 与各 domain 数据类型统一声明于此；domain facade 与业务组件
 * 从这里取类型。wire-level 私有类型（Stored 与 Host 系列）与 normalize
 * 函数归 provider facade（./provider.ts），不暴露到 barrel 之外。
 */

import type { ProviderRoutingApi } from '@/types/provider-routing';

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

/**
 * Operation journal (batch 2 CR-201): every lifecycle mutation records a
 * durable operation row so the Renderer projects busy/error/retry from Host
 * facts instead of frontend booleans (CR-203).
 */
export type CreativeAppOperationKind = 'start' | 'stop' | 'restart' | 'delete' | 'install';
export type CreativeAppOperationPhase =
  | 'pending'
  | 'waiting'
  | 'running'
  | 'compensating'
  | 'succeeded'
  | 'failed'
  | 'compensated'
  | 'cancelled';

export interface CreativeAppOperation {
  id: number;
  applicationId?: string | null;
  runtimeInstanceId?: string | null;
  kind: CreativeAppOperationKind;
  phase: CreativeAppOperationPhase;
  actor: string;
  redactedInput?: string | null;
  errorCode?: string | null;
  errorMessage?: string | null;
  startedAt: string;
  finishedAt?: string | null;
  updatedAt: string;
}

/** Lifecycle mutation result: journaled operation id + current projection. */
export interface CreativeAppMutationResult {
  operationId: number;
  summary: CreativeAppSummary;
}

/** Delete mutation result (the app is gone, so there is no summary). */
export interface CreativeAppDeleteMutationResult {
  operationId: number;
  result: CreativeAppDeleteResult;
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
  runtimeId: string | null;
  appId: string;
  seq: number;
  tsMs: number;
  stream: 'stdout' | 'stderr' | 'system' | string;
  text: string;
}

// CR-501: Surface / Endpoint / Window
export interface CreativeAppSurface {
  id: string;
  applicationId: string;
  kind: string;
  label: string;
  title?: string | null;
  url?: string | null;
  boundsJson?: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface CreativeAppWindow {
  id: string;
  applicationId: string;
  surfaceId: string;
  runtimeInstanceId?: string | null;
  label: string;
  state: string;
  boundsJson?: string | null;
  createdAt: string;
  updatedAt: string;
}

// CR-1001/1002: Agent proposal (versioned, Host-gated).
//
// The wire field is `environmentKeys` (Rust serde camelCase). A `envKeys`
// twin silently drops the field — `protocol:check` forbids it. The proposal is
// referenced by its stable `proposalId` (Daemon-generated); approve/reject
// operate on that id and never re-send the executable body.

/** The proposal body the Host validates (no inbox bookkeeping). */
export interface CreativeAppProposalPayload {
  schemaVersion: number;
  kind: 'create' | 'start';
  ownership: 'managed' | 'attached' | 'remote';
  title: string;
  projectRoot: string;
  driver: CreativeAppProposedDriver;
  openPath: string;
  healthPath: string;
  environmentKeys: string[];
}

/** A pending inbox entry: the validated proposal flattened with bookkeeping. */
export interface CreativeAppProposal extends CreativeAppProposalPayload {
  proposalId: string;
  status: string;
  createdAt: string;
  updatedAt: string;
  runId: string;
  turnId?: string | null;
  toolCallId: string;
}

export type CreativeAppProposedDriver =
  | { kind: 'python'; schemaVersion: number; interpreter: string; entry: string; args: string[]; cwdRelative: string; environmentKeys: string[]; port: { mode: 'auto' | 'fixed'; value?: number | null }; openPath: string; healthPath: string; startupTimeoutMs: number; isVenv: boolean }
  | { kind: 'binary'; schemaVersion: number; executablePath: string; executableHash: string; approved: boolean; args: string[]; cwdRelative: string; environmentKeys: string[]; port: { mode: 'auto' | 'fixed'; value?: number | null }; openPath: string; healthPath: string; startupTimeoutMs: number }
  | { kind: 'staticHttp' }
  | { kind: 'compose'; command?: string[] | null; privileged: boolean };

export interface CreativeAppValidatedProposal {
  proposal: CreativeAppProposalPayload;
  redacted: string;
}

// Tagged approve/reject results — a repeat click on an already-decided
// proposal is an idempotent `already_decided` no-op, never an error and never
// a fake second success.
export type CreativeAppProposalApproveResult =
  | { status: 'approved'; proposalId: string; app: CreativeAppSummary }
  | { status: 'already_decided'; proposalId: string; currentStatus: string };

export type CreativeAppProposalRejectResult =
  | { status: 'rejected'; proposalId: string }
  | { status: 'already_decided'; proposalId: string; currentStatus: string };

// ── T08: BrowserProfile / grants / OAuth ─────────────────────────────

export interface BrowserProfile {
  id: string;
  name: string;
  platformStoreKey: string;
  isDefault: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface ProfileBinding {
  applicationId: string;
  profileId: string;
  updatedAt: string;
}

export interface AppGrant {
  id: string;
  applicationId: string;
  kind: string;
  policy: string;
  path?: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface GrantEvent {
  id: string;
  applicationId: string;
  kind: string;
  event: string;
  policy?: string | null;
  path?: string | null;
  createdAt: string;
}

export interface OAuthAllowlistEntry {
  id: string;
  applicationId: string;
  domain: string;
  createdAt: string;
}

export interface OAuthFlowResult {
  ok: boolean;
  callback: Record<string, string>;
}

/** Emitted by the Host when a download/window.open is denied so the UI can prompt. */
export interface CreativeGrantRequested {
  appId: string;
  kind: string;
  target: string;
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
  /** Explicit user authorization to run a blocked compose default (batch 8). */
  tradeApproval?: 'webserver' | 'dry_run';
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

// ── Execution Engine V2 (E2-01) ───────────────────────────────────────────
// Backend-derived truth: the snapshot is built by the Host from real runtime
// discovery + the daemon capability handshake (SETTINGS-002). Capability map
// keys/values mirror the daemon's advertised matrix; statuses are real probe
// output (ready | degraded | blocked | disabled | not_installed).

/** Runtime descriptor for the settings snapshot (Host-projected, real data). */
export interface RuntimeDescriptor {
  id: string;
  displayName: string;
  status: string;
  version?: string | null;
  authority: string;
  reasonCode: string;
  reason: string;
  capabilities: Record<string, string>;
  controllable: string[];
}

export interface ResolvedDefaultRuntime {
  runtimeId: string;
  source: string;
  fallbackUsed: boolean;
  reasonCode: string;
  reason: string;
}

/** ExecutionEngineSettingsV2 wire shape (camelCase). */
export interface ExecutionEngineSettings {
  schemaVersion: number;
  revision: number;
  defaultRuntime: string;
  externalUnavailablePolicy: string;
  native: { maxSteps: number; disabledTools: string[] };
  claudeCli: { enabled: boolean };
  codexCli: { enabled: boolean };
  diagnostics: { performanceTelemetry: boolean };
}

export interface ExecutionEngineSnapshot {
  settings: ExecutionEngineSettings;
  runtimes: RuntimeDescriptor[];
  resolvedDefault: ResolvedDefaultRuntime;
  defaultProvider?: unknown;
  diagnosticsSummary: Record<string, unknown>;
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
    inspectLocal: (request: { projectRoot: string }) => Promise<LocalProjectScanResult>;
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
