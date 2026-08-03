/**
 * Protocol v2 TypeScript types — single source for GUI.
 *
 * GENERATED-FROM: crates/assistant-protocol (Rust is the wire fact source).
 * Keep field names snake_case on the wire (serde default) where noted;
 * this module uses camelCase TS properties with explicit wire mappers only
 * at the adapter boundary. Do not hand-write parallel Conversation/Run/Event
 * types elsewhere — import from here.
 *
 * Sync check: `node scripts/check-protocol-sync.mjs`
 */

export const PROTOCOL_V2 = '2.0.0' as const;

// ─── Connection / Daemon ─────────────────────────────────

export type ConnectionState =
  | 'starting_daemon'
  | 'connecting'
  | 'connected'
  | 'reconnecting'
  | 'recovering'
  | 'offline'
  | 'incompatible'
  | 'fatal'
  | 'disconnected';

export type ErrorCategory =
  | 'validation'
  | 'auth'
  | 'not_found'
  | 'conflict'
  | 'rate_limited'
  | 'timeout'
  | 'internal'
  | 'provider'
  | 'network'
  | 'permission_denied'
  | 'unsupported'
  | 'extension';

export interface DaemonError {
  code: string;
  category: ErrorCategory;
  retryable: boolean;
  userMessageKey?: string;
  message: string;
  technicalMessage?: string;
  recoveryActions?: string[];
  correlationId?: string;
  details?: unknown;
}

export interface DaemonCapabilities {
  protocolVersion: string;
  methods: string[];
  providers: string[];
  tools: boolean;
  hooks: boolean;
  subagents: boolean;
  mcp: boolean;
  extensions: boolean;
  scheduler: boolean;
  eventReplay: boolean;
  credentialBroker: boolean;
  /** Per-runtime honest status (REQ-T03). */
  runtimes?: RuntimeCapability[];
}

export type RuntimeAvailability =
  | 'executable'
  | 'unavailable'
  | 'undetermined'
  | string;

export interface RuntimeCapability {
  id: string;
  displayName: string;
  status: RuntimeAvailability;
  reason?: string | null;
  methods?: string[];
}

// ─── Conversation / Message ──────────────────────────────

/** chat = plain Q&A; agent = tool-using; goal = long-running task chrome (pause/resume/delete). */
export type ConversationMode = 'chat' | 'agent' | 'goal';

export interface Conversation {
  id: string;
  mode: ConversationMode;
  projectId?: string | null;
  title: string;
  providerId: string;
  modelId: string;
  permissionProfileId?: string;
  createdAt: string;
  updatedAt: string;
  archivedAt?: string | null;
  pinned?: boolean;
  parentConversationId?: string | null;
}

export type MessageRole = 'system' | 'user' | 'assistant' | 'tool';

export interface Message {
  id: string;
  conversationId: string;
  parentMessageId?: string | null;
  role: MessageRole;
  status: string;
  inputTokens?: number;
  outputTokens?: number;
  createdAt: string;
  contentBlocks: ContentBlock[];
  runId?: string | null;
}

// ─── Content blocks ──────────────────────────────────────

export type ContentBlockType =
  | 'text'
  | 'reasoning'
  | 'image'
  | 'file_reference'
  | 'tool_call'
  | 'tool_result'
  | 'diff'
  | 'citation'
  | 'error'
  | 'permission'
  | 'ask_user'
  | 'plan'
  | 'subagent'
  | 'artifact'
  | 'compaction'
  | 'system_notice'
  | 'legacy';

export interface ContentBlock {
  type: ContentBlockType;
  text?: string;
  reasoning?: string;
  signature?: string;
  imageUrl?: string;
  mimeType?: string;
  altText?: string;
  filePath?: string;
  fileSize?: number;
  toolCallId?: string;
  toolName?: string;
  toolInput?: Record<string, unknown>;
  /** Streaming partial JSON args for tool_call_delta before full input is available. */
  toolPartialArgs?: string;
  /** True when live tool stdout was truncated for UI. */
  toolStreamTruncated?: boolean;
  toolStatus?: 'pending' | 'running' | 'completed' | 'failed' | 'rejected';
  toolOutput?: unknown;
  isError?: boolean;
  durationMs?: number;
  live?: boolean;
  locale?: string;
  citationUri?: string;
  citationTitle?: string;
  errorCode?: string;
  errorMessage?: string;
  retryable?: boolean;
  raw?: string;
  originalType?: string;
  /** Diff hunks summary */
  diffstat?: string;
  hunks?: Array<{ header: string; lines: string[] }>;
  planMarkdown?: string;
  permissionId?: string;
  artifactId?: string;
  subRunId?: string;
  question?: AskUserInteraction['question'];
  segmentId?: string;
  summary?: string;
  summaryStatus?: 'completed' | 'failed';
  /** compaction: token counts around a context compression (engine context_compressed). */
  beforeTokens?: number;
  afterTokens?: number;
  /**
   * system_notice/subagent: structured notice payload. The reducer stores data
   * only; the rendering layer owns localization/formatting (no hardcoded copy here).
   */
  noticeKind?:
    | 'generation_retry'
    | 'checkpoint_created'
    | 'checkpoint_committed'
    | 'checkpoint_rewound'
    | 'subagent_created'
    | string;
  noticeData?: Record<string, unknown>;
}

// ─── Run ─────────────────────────────────────────────────

export type RunStatus =
  | 'created'
  | 'queued'
  | 'preparing'
  | 'reasoning'
  | 'running'
  | 'waiting_permission'
  | 'waiting_user'
  | 'waiting_subagent'
  | 'cancelling'
  | 'completed'
  | 'failed'
  | 'cancelled'
  | 'interrupted'
  | 'background_watching';

export const TERMINAL_RUN_STATUSES: ReadonlySet<RunStatus> = new Set([
  'completed',
  'failed',
  'cancelled',
  'interrupted',
]);

export function isTerminalRunStatus(status: RunStatus | string): boolean {
  return TERMINAL_RUN_STATUSES.has(status as RunStatus);
}

export function isActiveRunStatus(status: RunStatus | string): boolean {
  return (
    status === 'preparing' ||
    status === 'reasoning' ||
    status === 'running' ||
    status === 'waiting_permission' ||
    status === 'waiting_user' ||
    status === 'waiting_subagent' ||
    status === 'cancelling' ||
    status === 'queued' ||
    status === 'background_watching'
  );
}

export interface Run {
  id: string;
  conversationId: string;
  status: RunStatus;
  parentRunId?: string | null;
  agentProfileId?: string | null;
  providerId: string;
  keyId?: string | null;
  modelId: string;
  permissionProfile: string;
  triggerMessageId?: string | null;
  startedAt?: string | null;
  finishedAt?: string | null;
  errorCode?: string | null;
  errorMessage?: string | null;
  stepCount?: number;
  maxSteps?: number;
  projectPath?: string | null;
  retryCount?: number;
  retryOfRunId?: string | null;
  retryOfTurnId?: string | null;
  continuedFromRunId?: string | null;
  branchId?: string | null;
  branchParentMessageId?: string | null;
  checkpointId?: string | null;
  resumeOfRunId?: string | null;
  createdAt?: string | null;
  lastEventSequence?: number;
  idempotencyKey?: string | null;
  background?: boolean;
  activity?: string | null;
}

// ─── Run events (wire: type + payload fields flattened) ──

export type RunEventType =
  | 'queued'
  | 'preparing'
  | 'started'
  | 'text_delta'
  | 'reasoning_delta'
  | 'tool_call_requested'
  | 'tool_call_prepared'
  | 'context_snapshot_committed'
  | 'tool_call_started'
  | 'tool_call_delta'
  | 'tool_call_completed'
  | 'tool_output_delta'
  | 'permission_requested'
  | 'permission_responded'
  | 'plan_mode_changed'
  | 'file_changed'
  | 'task_started'
  | 'task_updated'
  | 'task_completed'
  | 'usage_updated'
  | 'context_usage_updated'
  | 'context_compressed'
  | 'checkpoint_created'
  | 'checkpoint_committed'
  | 'checkpoint_rewound'
  | 'subagent_created'
  | 'subagent_completed'
  | 'subagent_failed'
  | 'progress'
  | 'generation_attempt_started'
  | 'generation_attempt_failed'
  | 'generation_attempt_discarded'
  | 'generation_attempt_committed'
  | 'completed'
  | 'failed'
  | 'cancelled'
  | 'interrupted'
  | 'artifact_created'
  | 'prompt_queue_updated'
  | 'interaction_requested'
  | 'interaction_resolved'
  /** Rust RunEventKind::InteractionResponded wire name (run_event.rs type_name). */
  | 'interaction_responded'
  | 'unknown';

export interface RunEvent {
  runId: string;
  sequence: number;
  timestamp: string;
  type: RunEventType | string;
  /** Event-specific fields (snake_case keys accepted at adapter boundary). */
  payload: Record<string, unknown>;
}

// ─── Interaction / Permission ────────────────────────────

/** Native grant scope; `session` is the current conversation across its runs. */
export type PermissionScope = 'once' | 'this_run' | 'session' | 'run' | 'project' | 'global';

export interface PermissionInteraction {
  kind: 'permission';
  id: string;
  runId: string;
  conversationId?: string;
  toolCallId: string;
  toolName: string;
  reason: string;
  input: Record<string, unknown>;
  permissionClass?: string;
  createdAt: string;
  cwd?: string;
  projectPath?: string;
}

export interface AskUserInteraction {
  kind: 'ask_user';
  id: string;
  runId: string;
  conversationId?: string;
  createdAt: string;
  question: {
    prompt: string;
    options?: Array<{ id: string; label: string; description?: string }>;
    multiSelect?: boolean;
    freeText?: boolean;
  };
}

export interface PlanApprovalInteraction {
  kind: 'plan_approval';
  id: string;
  runId: string;
  conversationId?: string;
  createdAt: string;
  title: string;
  planMarkdown: string;
}

export interface ConflictResolutionInteraction {
  kind: 'conflict_resolution';
  id: string;
  runId: string;
  conversationId?: string;
  createdAt: string;
  files: Array<{ path: string; base?: string; ours?: string; theirs?: string }>;
}

/** One-shot pool assignment before subagent tasks can start (daemon `subagent_assignment`). */
export type SubagentAssignmentMode = 'default' | 'random' | 'custom';

export interface SubagentRouteBinding {
  providerId: string;
  keyId: string;
  modelId: string;
}

/** Task row inside a batch assignment interaction. */
export interface SubagentAssignmentTask {
  callId: string;
  name: string;
  prompt?: string | null;
}

export interface SubagentAssignmentInteraction {
  kind: 'subagent_assignment';
  id: string;
  runId: string;
  conversationId?: string;
  createdAt: string;
  reason?: string;
  batchId?: string;
  parentConversationId?: string;
  parentRunId?: string;
  /** Default provider/key/model for mode=default. Missing → confirm disabled. */
  defaultBinding?: SubagentRouteBinding | null;
  /** Full task list for the batch (one custom row per task). */
  tasks?: SubagentAssignmentTask[];
}

export type InteractionRequest =
  | PermissionInteraction
  | AskUserInteraction
  | PlanApprovalInteraction
  | ConflictResolutionInteraction
  | SubagentAssignmentInteraction;

/** Wire session from `subagent.list`. */
export interface SubagentSession {
  id: string;
  parentConversationId: string;
  childConversationId: string;
  parentRunId?: string | null;
  taskCallId?: string | null;
  name: string;
  task: string;
  status: string;
  providerId: string;
  keyId: string;
  modelId: string;
  lastActivityAt?: string;
  closedAt?: string | null;
  error?: string | null;
  createdAt?: string;
  updatedAt?: string;
}

export interface SubagentRoutePolicy {
  parentConversationId: string;
  mode: string;
  bindings: SubagentRouteBinding[];
  createdAt?: string;
  updatedAt?: string;
}

export interface SubagentListResult {
  sessions: SubagentSession[];
  routePolicy?: SubagentRoutePolicy | null;
}

// ─── Prompt queue ────────────────────────────────────────

export type PromptQueueSource = 'user' | 'scheduler' | 'background_wake' | 'system';

export interface PromptQueueItem {
  id: string;
  conversationId: string;
  content: string;
  source: PromptQueueSource;
  createdAt: string;
  order: number;
  /** Client-only optimistic id until server assigns real id. */
  clientTempId?: string;
  attachments?: AttachmentRef[];
}

// ─── Artifacts / tasks / children ────────────────────────

export interface Artifact {
  id: string;
  runId: string;
  path: string;
  label?: string;
  kind: string;
  size: number;
  mimeType?: string;
  createdAt?: string;
  staleReason?: string;
}

export interface ChildRunSummary {
  id: string;
  parentRunId: string;
  status: RunStatus | string;
  task?: string;
  agentProfileId?: string | null;
  providerId?: string;
  modelId?: string;
  keyLabel?: string;
  startedAt?: string | null;
  finishedAt?: string | null;
  toolCount?: number;
  tokenCount?: number;
  errorCount?: number;
  background?: boolean;
  projectPath?: string | null;
  worktree?: string | null;
}

export type BackgroundTaskKind = 'subagent' | 'terminal' | 'monitor' | 'scheduler' | 'other';

export interface BackgroundTask {
  id: string;
  kind: BackgroundTaskKind;
  runId?: string;
  conversationId?: string;
  title: string;
  status: string;
  createdAt: string;
  error?: string;
  /** Latest output / result snippet when advertised by task.list. */
  output?: string | null;
}

export interface ContextUsage {
  conversationId: string;
  usedTokens: number;
  maxTokens: number;
  messageTokens?: number;
  toolDefinitionTokens?: number;
  memoryTokens?: number;
  attachmentTokens?: number;
  compactionCount?: number;
}

export interface AttachmentRef {
  path: string;
  name?: string;
  mimeType?: string;
  size?: number;
}

export interface FileChange {
  path: string;
  changeType: string;
  runId?: string;
}

// ─── Snapshot ────────────────────────────────────────────

export interface ConversationSnapshot {
  conversation: Conversation;
  messages: Message[];
  runs: Run[];
  activeRunId?: string | null;
  eventsByRun?: Record<string, RunEvent[]>;
  interactions?: InteractionRequest[];
  promptQueue?: PromptQueueItem[];
  children?: ChildRunSummary[];
  artifacts?: Artifact[];
  tasks?: BackgroundTask[];
  contextUsage?: ContextUsage | null;
  capabilities?: DaemonCapabilities | null;
  messagePageInfo?: { hasMore: boolean; nextCursor: { createdAt: string; id: string } | null };
}

// ─── RPC methods (subset of Rust ALL_METHODS + GUI needs) ─

export type AssistantMethod =
  | 'daemon.getCapabilities'
  | 'daemon.getStatus'
  | 'daemon.ping'
  | 'provider.list'
  | 'provider.discoverModels'
  | 'provider.test'
  | 'conversation.create'
  | 'conversation.list'
  | 'conversation.listPage'
  | 'conversation.get'
  | 'conversation.update'
  | 'conversation.getMessages'
  | 'conversation.getMessagesPage'
  | 'conversation.appendMessage'
  | 'conversation.rename'
  | 'conversation.update_model'
  | 'conversation.update_permission'
  | 'conversation.archive'
  | 'conversation.delete'
  | 'conversation.fork'
  | 'conversation.getContextUsage'
  | 'run.create'
  | 'run.start'
  | 'run.cancel'
  | 'run.retry'
  | 'run.continue'
  | 'run.resume'
  | 'run.subscribe'
  | 'run.replay'
  | 'run.list'
  | 'run.getEvents'
  | 'run.listChildren'
  | 'run.finish'
  | 'run.getActivity'
  | 'run.rewind'
  | 'run.rewindPreview'
  | 'workspace.restore'
  | 'workspace.restorePreview'
  | 'task.wait'
  | 'permission.respond'
  | 'permission.listPending'
  | 'interaction.listPending'
  | 'interaction.respond'
  | 'promptQueue.list'
  | 'promptQueue.enqueue'
  | 'promptQueue.update'
  | 'promptQueue.remove'
  | 'promptQueue.reorder'
  | 'promptQueue.sendNow'
  | 'promptQueue.interject'
  | 'tool.list'
  | 'agent.list'
  | 'subagent.list'
  | 'subagent.touch'
  | 'subagent.switchRoute'
  | 'extension.list'
  | 'extension.enable'
  | 'mcp.list'
  | 'mcp.start'
  | 'mcp.stop'
  | 'mcp.call'
  | 'mcp.liveness'
  | 'mcp.reconnect'
  | 'mcp.auth.set'
  | 'mcp.auth.status'
  | 'mcp.auth.clear'
  | 'mcp.auth.oauthStart'
  | 'mcp.auth.oauthCallback'
  | 'mcp.resources.list'
  | 'mcp.resources.read'
  | 'mcp.resources.templates.list'
  | 'mcp.prompts.list'
  | 'mcp.prompts.get'
  | 'mcp.roots.list'
  | 'mcp.notifications.list'
  | 'memory.search'
  | 'memory.add'
  | 'skill.list'
  | 'artifact.list'
  | 'artifact.open'
  | 'artifact.reveal'
  | 'task.list'
  | 'task.cancel'
  | 'scheduler.list'
  | 'scheduler.create'
  | 'scheduler.update'
  | 'scheduler.delete'
  | 'scheduler.history'
  | 'scheduler.tick'
  | 'extension.list'
  | 'extension.enable'
  | 'engine.rateLimit.get'
  | 'engine.rateLimit.update'
  | 'engine.rateLimit.acquire'
  | 'engine.rateLimit.cooldown'
  | 'capability.skill.list'
  | 'capability.skill.get'
  | 'capability.skill.update'
  | 'capability.skill.import'
  | 'capability.skill.delete'
  | 'capability.skill.rescan'
  | 'capability.mcp.list'
  | 'capability.mcp.get'
  | 'capability.mcp.create'
  | 'capability.mcp.update'
  | 'capability.mcp.delete'
  | 'capability.mcp.importJson'
  | 'capability.mcp.hub.search'
  | 'capability.mcp.hub.get'
  | 'capability.mcp.hub.install'
  | 'capability.expert.list'
  | 'capability.expert.get'
  | 'capability.expert.create'
  | 'capability.expert.update'
  | 'capability.expert.delete'
  | 'capability.expert.importMd'
  | 'capability.expert.exportMd'
  | 'capability.team.list'
  | 'capability.team.get'
  | 'capability.team.create'
  | 'capability.team.update'
  | 'capability.team.delete'
  | 'conversation.updateCapabilities'
  | 'conversation.getCapabilities'
  // Harness control plane (Native execution engine). Read surface first, then
  // the Draft → Validate → Diff → Publish → Rollback lifecycle, bindings, and
  // the per-Run evidence lookup.
  | 'harness.overview'
  | 'harness.topology'
  | 'harness.workspace.get'
  | 'harness.template.list'
  | 'harness.hook.catalog'
  | 'harness.profile.list'
  | 'harness.profile.get'
  | 'harness.profile.create'
  | 'harness.profile.archive'
  | 'harness.draft.get'
  | 'harness.draft.save'
  | 'harness.draft.validate'
  | 'harness.draft.diff'
  | 'harness.draft.review'
  | 'harness.draft.simulate'
  | 'harness.draft.publish'
  | 'harness.version.list'
  | 'harness.version.rollback'
  | 'harness.binding.get'
  | 'harness.binding.set'
  | 'harness.run.getSnapshot'
  | 'harness.audit.list'
  | 'harness.prompt.preview'
  | 'harness.source.list'
  | 'harness.source.acknowledgeDrift'
  | 'harness.external.inspect'
  | 'harness.subscribe'
  | 'harness.trace.list'
  | 'harness.audit.export'
  | 'project.identity.register'
  | 'project.identity.list';

/**
 * Capability library selection carried on run.start / conversation rows
 * (ADR-0016). All fields optional; absent = legacy behaviour.
 */
export interface CapabilitySelection {
  skills?: string[];
  mcp_servers?: string[];
  expert_id?: string | null;
  team_id?: string | null;
}
