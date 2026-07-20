/**
 * Imperative controller: user intents → Gateway → dispatch.
 * No React; safe for tests.
 */
import type { AssistantGateway } from '@/lib/assistant-gateway';
import type { AttachmentRef, Run, RunEvent } from '@/lib/assistant-protocol';
import { isActiveRunStatus, mapWireRun } from '@/lib/assistant-protocol';
import type { AssistantWorkspaceState, WorkspaceAction } from './state';

export type Dispatch = (action: WorkspaceAction) => void;

export interface SendResult {
  queued: boolean;
  runId?: string;
  promptQueueItemId?: string;
}

export async function connectWorkspace(
  gateway: AssistantGateway,
  dispatch: Dispatch,
): Promise<void> {
  dispatch({ type: 'connection/set', connection: 'connecting', error: null });
  try {
    await gateway.connect();
    const caps = gateway.getCapabilities ? await gateway.getCapabilities() : null;
    dispatch({ type: 'capabilities/set', capabilities: caps });
    dispatch({ type: 'connection/set', connection: 'connected', reconnectAttempts: 0 });
  } catch (err) {
    dispatch({
      type: 'connection/set',
      connection: 'offline',
      error: err instanceof Error ? err.message : String(err),
    });
    throw err;
  }
}

export async function loadConversations(
  gateway: AssistantGateway,
  dispatch: Dispatch,
): Promise<void> {
  const list = await gateway.request<Array<Record<string, unknown> | import('@/lib/assistant-protocol').Conversation>>(
    'conversation.list',
    { include_archived: false },
  );
  // Accept already-mapped Conversation objects from fixture or wire shapes
  const { mapWireConversation } = await import('@/lib/assistant-protocol');
  const conversations = (Array.isArray(list) ? list : []).map((c) => {
    if (c && typeof c === 'object' && 'providerId' in c) {
      return c as import('@/lib/assistant-protocol').Conversation;
    }
    return mapWireConversation(c as Record<string, unknown>);
  });
  dispatch({ type: 'conversations/replace', conversations });
}

export async function openConversation(
  gateway: AssistantGateway,
  dispatch: Dispatch,
  conversationId: string,
): Promise<void> {
  dispatch({ type: 'conversations/setActive', id: conversationId });
  const snapshot = await gateway.getSnapshot(conversationId);
  dispatch({ type: 'snapshot/apply', snapshot });
}

/** Consume run events into the store; on gap set recovering and replay. */
export async function subscribeRun(
  gateway: AssistantGateway,
  dispatch: Dispatch,
  getState: () => AssistantWorkspaceState,
  runId: string,
  afterSequence: number,
  signal?: { aborted: boolean },
): Promise<void> {
  let last = afterSequence;
  try {
    for await (const event of gateway.subscribe(runId, afterSequence)) {
      if (signal?.aborted) return;
      const state = getState();
      const prevLast = state.lastSequenceByRun[runId] ?? last;
      if (event.sequence > prevLast + 1 && prevLast > 0) {
        dispatch({ type: 'recovering/set', runId, recovering: true });
        const missing = await gateway.request<RunEvent[]>('run.getEvents', {
          run_id: runId,
          after_sequence: prevLast,
        });
        dispatch({ type: 'event/replay', runId, events: missing });
        last = getState().lastSequenceByRun[runId] ?? event.sequence;
        continue;
      }
      dispatch({ type: 'event/apply', event });
      last = event.sequence;
      if (
        event.type === 'completed' ||
        event.type === 'failed' ||
        event.type === 'interrupted'
      ) {
        return;
      }
    }
  } catch (err) {
    dispatch({
      type: 'disconnect/soft',
    });
    dispatch({
      type: 'connection/set',
      connection: 'reconnecting',
      error: err instanceof Error ? err.message : String(err),
    });
    throw err;
  }
}

export async function resolveProjectPath(options: {
  explicit?: string | null;
  conversationProjectId?: string | null;
  /** When true, last-resort readActiveProject via nativesAPI (desktop only). */
  tryActiveProject?: boolean;
}): Promise<string | null> {
  const fromExplicit = options.explicit?.trim() || null;
  if (fromExplicit) return fromExplicit;
  const fromConv = options.conversationProjectId?.trim() || null;
  if (fromConv) return fromConv;
  if (options.tryActiveProject === false) return null;
  try {
    const { readActiveProject } = await import('@/lib/active-project');
    const api =
      typeof window !== 'undefined'
        ? (window as unknown as { nativesAPI?: { db?: { get?: (k: string) => Promise<unknown> } } })
            .nativesAPI
        : undefined;
    const path = await readActiveProject(api);
    return path?.trim() || null;
  } catch {
    return null;
  }
}

export async function sendOrQueue(
  gateway: AssistantGateway,
  dispatch: Dispatch,
  state: AssistantWorkspaceState,
  params: {
    conversationId: string;
    content: string;
    providerId: string;
    modelId: string;
    attachments?: AttachmentRef[];
    /** Explicit project root from workbench (preferred). */
    projectPath?: string | null;
    /** Force immediate send (cancel current first) */
    forceImmediate?: boolean;
  },
): Promise<SendResult> {
  const {
    conversationId,
    content,
    providerId,
    modelId,
    attachments = [],
    projectPath: projectPathParam,
    forceImmediate,
  } = params;
  const runId = state.activeRunByConversation[conversationId];
  const run = runId ? state.runs[runId] : null;
  const busy = run && isActiveRunStatus(run.status);
  const conversation = state.conversations[conversationId];

  if (busy && !forceImmediate) {
    const clientTempId = `pq-temp-${Date.now()}`;
    dispatch({
      type: 'promptQueue/optimisticEnqueue',
      item: {
        id: clientTempId,
        clientTempId,
        conversationId,
        content,
        source: 'user',
        createdAt: new Date().toISOString(),
        order: (state.promptQueues[conversationId] ?? []).length,
        attachments,
      },
    });
    const serverItem = await gateway.request<{
      id: string;
      content: string;
      conversationId?: string;
      source?: string;
      createdAt?: string;
      order?: number;
    }>('promptQueue.enqueue', {
      conversation_id: conversationId,
      content,
      client_temp_id: clientTempId,
      attachments,
    });
    dispatch({
      type: 'promptQueue/reassociate',
      conversationId,
      clientTempId,
      serverItem: {
        id: serverItem.id,
        conversationId,
        content: serverItem.content ?? content,
        source: 'user',
        createdAt: serverItem.createdAt ?? new Date().toISOString(),
        order: serverItem.order ?? 0,
      },
    });
    dispatch({ type: 'composer/clear', conversationId });
    return { queued: true, promptQueueItemId: serverItem.id };
  }

  if (busy && forceImmediate && run) {
    await gateway.request('run.cancel', { run_id: run.id });
  }

  dispatch({
    type: 'messages/appendOptimistic',
    message: {
      id: `pending-user-${Date.now()}`,
      conversationId,
      role: 'user',
      status: 'sending',
      createdAt: new Date().toISOString(),
      contentBlocks: [{ type: 'text', text: content }],
    },
  });

  // project_path: workbench explicit → conversation.projectId → active project.
  // Never invent daemon process cwd. Fixture conversations carry projectId.
  const projectPath = await resolveProjectPath({
    explicit: projectPathParam,
    conversationProjectId: conversation?.projectId ?? null,
    tryActiveProject: true,
  });
  if (!projectPath) {
    throw new Error(
      'project_path is required: select a project directory before starting a run',
    );
  }

  const started = await gateway.request<Run | Record<string, unknown>>('run.start', {
    conversation_id: conversationId,
    provider_id: providerId,
    model_id: modelId,
    permission_profile: conversation?.permissionProfileId ?? 'ask',
    content,
    attachments: attachments.map((a) => ({
      path: a.path,
      name: a.name,
      mime_type: a.mimeType,
      mimeType: a.mimeType,
      size: a.size,
    })),
    project_path: projectPath,
  });

  const mapped: Run =
    started && typeof started === 'object' && 'providerId' in started
      ? (started as Run)
      : mapWireRun(started as Record<string, unknown>);

  // Prefer host/daemon status; only promote bare "queued" to "preparing" so the
  // UI does not sit on "排队中" while the engine is already spinning up.
  const nextStatus =
    mapped.status === 'queued' || mapped.status === 'created' ? 'preparing' : mapped.status;
  dispatch({ type: 'run/upsert', run: { ...mapped, status: nextStatus } });
  dispatch({ type: 'composer/clear', conversationId });
  return { queued: false, runId: mapped.id };
}

export async function respondPermission(
  gateway: AssistantGateway,
  dispatch: Dispatch,
  requestId: string,
  approved: boolean,
  scope: string,
  runId?: string,
): Promise<void> {
  await gateway.request('permission.respond', {
    request_id: requestId,
    approved,
    scope,
    run_id: runId,
  });
  // Optimistic remove; server event is source of truth when it arrives
  dispatch({ type: 'interaction/remove', id: requestId });
}

export async function cancelRun(
  gateway: AssistantGateway,
  dispatch: Dispatch,
  runId: string,
): Promise<void> {
  await gateway.request('run.cancel', { run_id: runId });
  // Do not invent terminal status — wait for interrupted event
}

export async function retryRun(
  gateway: AssistantGateway,
  dispatch: Dispatch,
  runId: string,
): Promise<string> {
  const run = await gateway.request<Run | Record<string, unknown>>('run.retry', { run_id: runId });
  const id = typeof run === 'object' && run && 'id' in run ? String(run.id) : runId;
  if (run && typeof run === 'object' && 'providerId' in run) {
    dispatch({ type: 'run/upsert', run: run as Run });
  }
  return id;
}
