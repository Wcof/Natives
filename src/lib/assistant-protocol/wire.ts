/**
 * Wire (snake_case) ↔ TS (camelCase) mappers at the adapter boundary only.
 */
import type {
  Artifact,
  ContentBlock,
  Conversation,
  DaemonCapabilities,
  Message,
  Run,
  RunEvent,
  RunStatus,
} from './types';

function str(v: unknown, fallback = ''): string {
  return v == null ? fallback : String(v);
}

function optStr(v: unknown): string | null | undefined {
  if (v === undefined) return undefined;
  if (v === null) return null;
  return String(v);
}

function num(v: unknown): number | undefined {
  return typeof v === 'number' && Number.isFinite(v) ? v : undefined;
}

export function mapWireCapabilities(raw: Record<string, unknown>): DaemonCapabilities {
  return {
    protocolVersion: str(raw.protocol_version ?? raw.protocolVersion, '2.0.0'),
    methods: Array.isArray(raw.methods) ? raw.methods.map(String) : [],
    providers: Array.isArray(raw.providers) ? raw.providers.map(String) : [],
    tools: Boolean(raw.tools),
    hooks: Boolean(raw.hooks),
    subagents: Boolean(raw.subagents),
    mcp: Boolean(raw.mcp),
    extensions: Boolean(raw.extensions),
    scheduler: Boolean(raw.scheduler),
    eventReplay: Boolean(raw.event_replay ?? raw.eventReplay),
    credentialBroker: Boolean(raw.credential_broker ?? raw.credentialBroker),
  };
}

export function mapWireConversation(raw: Record<string, unknown>): Conversation {
  return {
    id: str(raw.id),
    mode: (raw.mode === 'chat' ? 'chat' : 'agent') as Conversation['mode'],
    projectId: optStr(raw.project_id ?? raw.projectId),
    title: str(raw.title, 'Untitled'),
    providerId: str(raw.provider_id ?? raw.providerId),
    modelId: str(raw.model_id ?? raw.modelId),
    permissionProfileId: str(raw.permission_profile_id ?? raw.permissionProfileId ?? 'ask') || 'ask',
    createdAt: str(raw.created_at ?? raw.createdAt, new Date().toISOString()),
    updatedAt: str(raw.updated_at ?? raw.updatedAt, new Date().toISOString()),
    archivedAt: optStr(raw.archived_at ?? raw.archivedAt),
    pinned: Boolean(raw.pinned),
    parentConversationId: optStr(raw.parent_conversation_id ?? raw.parentConversationId),
  };
}

function mapWireBlock(block: Record<string, unknown>): ContentBlock {
  const type = str(block.type, 'legacy') as ContentBlock['type'];
  const content = (block.content ?? block) as Record<string, unknown>;
  switch (type) {
    case 'text':
      return { type: 'text', text: str(content.text ?? content.content) };
    case 'reasoning':
      return {
        type: 'reasoning',
        reasoning: str(content.text ?? content.reasoning ?? content.content),
        durationMs: num(content.duration_ms ?? content.durationMs),
      };
    case 'tool_call':
      return {
        type: 'tool_call',
        toolCallId: str(content.tool_call_id ?? content.id ?? content.toolCallId),
        toolName: str(content.tool_name ?? content.name ?? content.toolName),
        toolInput: (content.input ?? content.arguments ?? {}) as Record<string, unknown>,
        toolStatus: (content.status ?? 'pending') as ContentBlock['toolStatus'],
      };
    case 'tool_result':
      return {
        type: 'tool_result',
        toolCallId: str(content.tool_call_id ?? content.toolCallId),
        toolOutput: content.output ?? content.result,
        isError: Boolean(content.is_error ?? content.isError),
        durationMs: num(content.duration_ms ?? content.durationMs),
      };
    case 'image':
      return {
        type: 'image',
        mimeType: str(content.mime_type ?? content.mimeType, 'image/png'),
        imageUrl: str(content.data ?? content.imageUrl),
        altText: str(content.alt_text ?? content.altText),
      };
    case 'file_reference':
      return {
        type: 'file_reference',
        filePath: str(content.path ?? content.file_path ?? content.filePath),
        fileSize: num(content.size ?? content.file_size ?? content.fileSize),
        mimeType: str(content.mime_type ?? content.mimeType, 'application/octet-stream'),
      };
    case 'error':
      return {
        type: 'error',
        errorCode: str(content.code ?? content.error_code ?? content.errorCode),
        errorMessage: str(content.message ?? content.error_message ?? content.errorMessage),
        retryable: Boolean(content.retryable),
      };
    case 'citation':
      return {
        type: 'citation',
        citationUri: str(content.uri ?? content.url ?? content.citationUri),
        citationTitle: content.title ? str(content.title) : undefined,
      };
    default:
      return {
        type: 'legacy',
        originalType: type,
        raw: JSON.stringify(content, null, 2),
      };
  }
}

export function mapWireMessage(raw: Record<string, unknown>): Message {
  const blocksRaw = (raw.content_blocks ?? raw.contentBlocks ?? []) as unknown[];
  return {
    id: str(raw.id),
    conversationId: str(raw.conversation_id ?? raw.conversationId),
    parentMessageId: optStr(raw.parent_message_id ?? raw.parentMessageId),
    role: (str(raw.role, 'assistant') as Message['role']),
    status: str(raw.status, 'complete'),
    inputTokens: num(raw.input_tokens ?? raw.inputTokens),
    outputTokens: num(raw.output_tokens ?? raw.outputTokens),
    createdAt: str(raw.created_at ?? raw.createdAt, new Date().toISOString()),
    contentBlocks: blocksRaw.map((b) => mapWireBlock((b ?? {}) as Record<string, unknown>)),
    runId: optStr(raw.run_id ?? raw.runId),
  };
}

export function mapWireRun(raw: Record<string, unknown>): Run {
  return {
    id: str(raw.id),
    conversationId: str(raw.conversation_id ?? raw.conversationId),
    status: str(raw.status, 'created') as RunStatus,
    parentRunId: optStr(raw.parent_run_id ?? raw.parentRunId),
    agentProfileId: optStr(raw.agent_profile_id ?? raw.agentProfileId),
    providerId: str(raw.provider_id ?? raw.providerId),
    keyId: optStr(raw.key_id ?? raw.keyId),
    modelId: str(raw.model_id ?? raw.modelId),
    permissionProfile: str(raw.permission_profile ?? raw.permissionProfile, 'ask'),
    triggerMessageId: optStr(raw.trigger_message_id ?? raw.triggerMessageId),
    startedAt: optStr(raw.started_at ?? raw.startedAt),
    finishedAt: optStr(raw.finished_at ?? raw.finishedAt),
    errorCode: optStr(raw.error_code ?? raw.errorCode),
    errorMessage: optStr(raw.error_message ?? raw.errorMessage),
    stepCount: num(raw.step_count ?? raw.stepCount),
    maxSteps: num(raw.max_steps ?? raw.maxSteps),
    projectPath: optStr(raw.project_path ?? raw.projectPath),
    retryCount: num(raw.retry_count ?? raw.retryCount) ?? 0,
    createdAt: optStr(raw.created_at ?? raw.createdAt),
    lastEventSequence: num(raw.last_event_sequence ?? raw.lastEventSequence) ?? 0,
    idempotencyKey: optStr(raw.idempotency_key ?? raw.idempotencyKey),
    background: Boolean(raw.background),
    activity: optStr(raw.activity),
  };
}

/** Normalize daemon events: type may be top-level or nested; payload may be flat. */
export function mapWireRunEvent(raw: Record<string, unknown>): RunEvent {
  const type = str(raw.type ?? raw.event_type ?? raw.eventType, 'unknown');
  let payload: Record<string, unknown> = {};
  if (raw.payload && typeof raw.payload === 'object' && !Array.isArray(raw.payload)) {
    payload = { ...(raw.payload as Record<string, unknown>) };
  } else {
    // Flattened wire (serde flatten): copy non-envelope keys
    for (const [k, v] of Object.entries(raw)) {
      if (['run_id', 'runId', 'sequence', 'timestamp', 'type', 'event_type', 'eventType'].includes(k)) continue;
      payload[k] = v;
    }
  }
  return {
    runId: str(raw.run_id ?? raw.runId),
    sequence: Number(raw.sequence ?? 0),
    timestamp: str(raw.timestamp ?? raw.emitted_at ?? raw.emittedAt, new Date().toISOString()),
    type,
    payload,
  };
}

export function mapWireArtifact(raw: Record<string, unknown>): Artifact {
  return {
    id: str(raw.id),
    runId: str(raw.run_id ?? raw.runId),
    path: str(raw.path),
    label: raw.label != null ? str(raw.label) : undefined,
    kind: str(raw.kind, 'file'),
    size: num(raw.size) ?? 0,
    mimeType: raw.mime_type != null || raw.mimeType != null ? str(raw.mime_type ?? raw.mimeType) : undefined,
    createdAt: raw.created_at != null || raw.createdAt != null ? str(raw.created_at ?? raw.createdAt) : undefined,
    staleReason: raw.stale_reason != null || raw.staleReason != null ? str(raw.stale_reason ?? raw.staleReason) : undefined,
  };
}
