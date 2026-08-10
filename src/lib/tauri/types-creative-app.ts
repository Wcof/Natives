/**
 * tauri/types-creative-app — Creative App domain 共享类型（ARCH-002 split）
 *
 * multi-source Personal Creations（Catalog / GitHub / 本地工程）、T08
 * BrowserProfile / grants / OAuth、CR-501 Surface / Window、
 * CR-1001/1002 Agent proposal 以及 creativeDraft 的 wire 类型统一声明于此；
 * creative facade（./creative.ts）与业务组件从这里取类型。
 */

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
