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
  | 'tool_call_started'
  | 'tool_call_delta'
  | 'tool_call_completed'
  | 'permission_requested'
  | 'permission_responded'
  | 'file_changed'
  | 'usage_updated'
  | 'context_compressed'
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

export type PermissionScope = 'once' | 'run' | 'project' | 'global';

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

export type InteractionRequest =
  | PermissionInteraction
  | AskUserInteraction
  | PlanApprovalInteraction
  | ConflictResolutionInteraction;

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
  | 'conversation.get'
  | 'conversation.update'
  | 'conversation.getMessages'
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
  | 'run.subscribe'
  | 'run.replay'
  | 'run.list'
  | 'run.getEvents'
  | 'run.listChildren'
  | 'run.finish'
  | 'run.getActivity'
  | 'run.rewind'
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
  | 'tool.list'
  | 'agent.list'
  | 'subagent.list'
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
  | 'extension.enable';
