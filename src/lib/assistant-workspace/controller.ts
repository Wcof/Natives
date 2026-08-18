/**
 * Imperative controller: user intents → Gateway → dispatch.
 * No React; safe for tests.
 */
import type { AssistantGateway } from '@/lib/assistant-gateway';
import type { AttachmentRef, CapabilitySelection, Run, RunEvent } from '@/lib/assistant-protocol';
import {
  AuthoritativeEventMissing,
  isActiveRunStatus,
  mapWireRun,
} from '@/lib/assistant-protocol';
import { getConversationCapabilities } from './capability-admin';
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
  const listRaw = await gateway.request<unknown>('conversation.listPage', { limit: 100 }).catch(() =>
    gateway.request<unknown>('conversation.list', { include_archived: false }),
  );
  const list = Array.isArray(listRaw)
    ? listRaw
    : ((listRaw as { conversations?: unknown[] } | null)?.conversations ?? []);
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
  // Best-effort context usage when advertised (Phase 3).
  try {
    const caps = gateway.getCapabilities ? await gateway.getCapabilities() : null;
    // Hydrate ADR-0016 capability selection when the daemon advertises it.
    if (caps?.methods?.includes('conversation.getCapabilities')) {
      try {
        const selection = await getConversationCapabilities(gateway, conversationId);
        dispatch({ type: 'capabilitySelection/set', conversationId, selection });
      } catch {
        /* optional */
      }
    }
    if (caps?.methods?.includes('conversation.getContextUsage')) {
      const usage = await gateway.request<Record<string, unknown>>(
        'conversation.getContextUsage',
        { conversation_id: conversationId },
      );
      if (usage && typeof usage === 'object') {
        const used = Number(
          (usage as { usedTokens?: number; used_tokens?: number }).usedTokens ??
            (usage as { used_tokens?: number }).used_tokens ??
            0,
        );
        const max = Number(
          (usage as { maxTokens?: number; max_tokens?: number }).maxTokens ??
            (usage as { max_tokens?: number }).max_tokens ??
            128000,
        );
        dispatch({
          type: 'snapshot/apply',
          snapshot: {
            conversation: snapshot.conversation,
            messages: snapshot.messages ?? [],
            runs: snapshot.runs ?? [],
            contextUsage: {
              conversationId,
              usedTokens: used,
              maxTokens: max,
            },
          },
        });
      }
    }
  } catch {
    /* optional */
  }
}

/** Consume run events into the store; on gap set recovering and replay. */
function isImmediateEvent(type: string): boolean {
  return (
    type === 'completed' ||
    type === 'failed' ||
    type === 'interrupted' ||
    type === 'cancelled' ||
    type === 'permission_requested' ||
    type === 'interaction_requested' ||
    type === 'error'
  );
}

export async function subscribeRun(
  gateway: AssistantGateway,
  dispatch: Dispatch,
  getState: () => AssistantWorkspaceState,
  runId: string,
  afterSequence: number,
  signal?: { aborted: boolean },
): Promise<void> {
  let last = afterSequence;
  let sawTerminal = false;
  const pending: RunEvent[] = [];
  let raf: number | null = null;

  const flush = () => {
    raf = null;
    if (pending.length === 0) return;
    const batch = pending.splice(0, pending.length);
    if (batch.length === 1) {
      dispatch({ type: 'event/apply', event: batch[0]! });
    } else {
      dispatch({ type: 'event/applyBatch', events: batch });
    }
  };

  const scheduleFlush = () => {
    if (raf != null) return;
    if (typeof requestAnimationFrame === 'function') {
      raf = requestAnimationFrame(flush) as unknown as number;
    } else {
      // Node / tests: microtask batch.
      raf = 1;
      queueMicrotask(() => {
        raf = null;
        flush();
      });
    }
  };

  const applyEvent = (event: RunEvent, immediate: boolean) => {
    if (immediate) {
      // Drain any queued stream deltas first so order is preserved.
      flush();
      dispatch({ type: 'event/apply', event });
      return;
    }
    pending.push(event);
    scheduleFlush();
  };

  try {
    for await (const event of gateway.subscribe(runId, afterSequence)) {
      if (signal?.aborted) {
        flush();
        return;
      }
      const state = getState();
      const prevLast = state.lastSequenceByRun[runId] ?? last;
      if (event.sequence > prevLast + 1 && prevLast > 0) {
        flush();
        // Sequence gap: run-level recovering only. Do not leave the global
        // assistant connection on "recovering" (banner would look like a disconnect).
        dispatch({ type: 'recovering/set', runId, recovering: true });
        dispatch({
          type: 'projection/recovery',
          runId,
          recovery: { kind: 'incomplete', lastSequence: prevLast, reason: 'gap' },
        });
        if (getState().connection === 'recovering') {
          dispatch({
            type: 'connection/set',
            connection: 'connected',
            error: null,
          });
        }
        if (typeof console !== 'undefined' && typeof console.debug === 'function') {
          console.debug('[assistant] sequence gap recovery', {
            runId,
            lastSequence: prevLast,
            liveSequence: event.sequence,
            reason: 'sequence_gap',
          });
        }
        const missing = await gateway.request<RunEvent[]>('run.getEvents', {
          run_id: runId,
          after_sequence: prevLast,
        });
        dispatch({ type: 'event/replay', runId, events: missing });
        last = getState().lastSequenceByRun[runId] ?? event.sequence;
        // Do not skip the live event after replay — apply if still new.
        if (event.sequence <= last) continue;
      }
      const immediate = isImmediateEvent(event.type);
      applyEvent(event, immediate);
      last = event.sequence;
      // Any successful live event clears a real reconnect banner / backoff count.
      // Sequence gaps only touch recoveringRuns (run-level), never global connection.
      const conn = getState().connection;
      if (conn === 'reconnecting' || conn === 'recovering' || conn === 'offline') {
        dispatch({
          type: 'connection/set',
          connection: 'connected',
          error: null,
          reconnectAttempts: 0,
        });
      }
      if (
        event.type === 'completed' ||
        event.type === 'failed' ||
        event.type === 'interrupted' ||
        event.type === 'cancelled'
      ) {
        sawTerminal = true;
        flush();
        // Clear any soft-recover banner now that we have a real terminal.
        if (getState().recoveringRuns[runId]) {
          dispatch({ type: 'recovering/set', runId, recovering: false });
        }
        return;
      }
    }
    flush();
    // Subscribe iterator ended without a terminal event.
    // Quiet empty end is normal when the adapter is still mid-poll and the
    // workbench will resubscribe. Do **not** flip connection/recovering banners
    // — that showed permanent "正在重连 / 正在恢复事件" during normal answers.
    // Workbench soft-resubscribes from lastSequence while the run stays active.
    if (!sawTerminal && !signal?.aborted) {
      if (typeof console !== 'undefined' && typeof console.debug === 'function') {
        console.debug('[assistant] subscribe quiet end', {
          runId,
          lastSequence: last,
          reason: 'iterator_ended_without_terminal',
        });
      }
      // intentionally no-op on connection state
    }
  } catch (err) {
    flush();
    if (err instanceof AuthoritativeEventMissing) {
      const recovery = gateway.getProjectionRecovery?.(runId) ?? {
        kind: 'incomplete' as const,
        lastSequence: last,
        reason: 'authoritative_event_missing' as const,
      };
      dispatch({ type: 'projection/recovery', runId, recovery });
      dispatch({ type: 'recovering/set', runId, recovering: false });
      return;
    }
    const run = getState().runs[runId];
    if (run && isActiveRunStatus(run.status)) {
      // Run-level recovering only; global reconnecting is set below for transport errors.
      dispatch({ type: 'recovering/set', runId, recovering: true });
    }
    // Real transport / IPC / daemon-unavailable errors surface as reconnecting.
    const message = err instanceof Error ? err.message : String(err);
    if (typeof console !== 'undefined' && typeof console.debug === 'function') {
      console.debug('[assistant] subscribe transport error', {
        runId,
        lastSequence: last,
        reason: 'transport_error',
        error: message,
      });
    }
    // Product decision 4 (审计收口 #4)：单 run watch 失败是 run 级关注点，
    // 只进 run 自己的折叠栏（runErrors/recovering），绝不写全局 connection。
    // 全局 Banner 只反映 daemon 级 starting/offline/fatal/incompatible；
    // 预算耗尽后由调用方 reconcileExhaustedRun 做权威对账（run.cancel 写回）。
    dispatch({ type: 'run/error/set', runId, error: message });
    throw err;
  }
}

/** Shared i18n key set when a run's reconnect budget is exhausted. */
export const RUN_WATCH_EXHAUSTED_KEY = 'assistant.runWatchExhausted';

/**
 * Reconcile a run after its reconnect budget is exhausted: query the daemon's
 * authoritative run snapshot (`run.getActivity`) and, if the run is still
 * active, cancel it authoritatively (`run.cancel`). The run's fold shows the
 * exhausted state; the global banner stays generic.
 *
 * 审计收口 #4：getActivity / cancel 返回的权威 Run 必须 upsert 回 store，
 * 并清 recovering/transport error；同 run cancel 恰好一次（cancelRun 内部
 * 已 upsert 终态，随后订阅停止，无终态 event 消费者的问题被消除）。
 */
export async function reconcileExhaustedRun(
  gateway: AssistantGateway,
  dispatch: Dispatch,
  runId: string,
): Promise<void> {
  let authoritative: Record<string, unknown> | null = null;
  try {
    const activity = await gateway.request<Record<string, unknown>>('run.getActivity', {
      run_id: runId,
    });
    authoritative = activity ?? null;
  } catch {
    // Daemon unreachable: keep the run recovering; the global reconnect flow
    // (connectWorkspace) owns daemon-level connectivity.
    dispatch({ type: 'run/error/set', runId, error: RUN_WATCH_EXHAUSTED_KEY });
    return;
  }
  const status = authoritative && typeof authoritative === 'object' ? String((authoritative as { status?: unknown }).status ?? '') : '';
  if (authoritative && 'id' in (authoritative as object)) {
    const run = 'providerId' in (authoritative as object)
      ? (authoritative as unknown as Run)
      : { ...authoritative, id: String((authoritative as { id?: unknown }).id) };
    if ('status' in run) {
      // 权威快照写回 store；终态即终止订阅，清 recovering。
      dispatch({ type: 'run/upsert', run: run as Run });
    }
  }
  if (isActiveRunStatus(status)) {
    // 权威 cancel：cancelRun 返回的权威 Run 也会被 upsert（恰好一次终态）。
    await cancelRun(gateway, dispatch, runId);
    dispatch({ type: 'run/error/set', runId, error: RUN_WATCH_EXHAUSTED_KEY });
    // 权威终态写回后，run 级 recovering/transport 状态必须清除。
    dispatch({ type: 'recovering/set', runId, recovering: false });
  } else {
    // Already terminal — nothing to cancel; clear transient run error.
    dispatch({ type: 'recovering/set', runId, recovering: false });
    dispatch({ type: 'run/error/set', runId, error: null });
  }
}

export async function resolveProjectPath(options: {
  explicit?: string | null;
  /** Stable ProjectIdentity UUID from the conversation, never a path itself. */
  conversationProjectId?: string | null;
  gateway?: AssistantGateway;
  /** When true, last-resort readActiveProject via nativesAPI (desktop only). */
  tryActiveProject?: boolean;
}): Promise<string | null> {
  const fromExplicit = options.explicit?.trim() || null;
  if (fromExplicit) return fromExplicit;
  const fromConv = options.conversationProjectId?.trim() || null;
  if (fromConv) {
    if (!options.gateway) return null;
    const response = await options.gateway.request<unknown>('project.identity.list', {});
    const items =
      response && typeof response === 'object' && Array.isArray((response as { items?: unknown }).items)
        ? (response as { items: unknown[] }).items
        : [];
    const identity = items.find(
      (item): item is { project_id?: unknown; canonical_path?: unknown } =>
        !!item &&
        typeof item === 'object' &&
        (item as { project_id?: unknown }).project_id === fromConv,
    );
    const canonicalPath =
      identity && typeof identity.canonical_path === 'string'
        ? identity.canonical_path.trim()
        : '';
    return canonicalPath || null;
  }
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
    /** Optional reasoning / effort level (REQ-E04). */
    effort?: string | null;
    /** Optional runtime id: native | claude_cli | … (REQ-T01). */
    runtimeId?: string | null;
    /**
     * Surface selector. The daemon resolves the tool allowlist from this, which
     * is how the creator workbench gets the draft tools and nothing else.
     */
    agentProfileId?: string | null;
    /**
     * ADR-0016 selection override (temp→real promotion happens in the same
     * tick as send, before the store re-render lands). `undefined` = read the
     * conversation's selection from state.
     */
    capabilitySelection?: CapabilitySelection | null;
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
    effort,
    runtimeId,
    agentProfileId,
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
    try {
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
    } catch (err) {
      // Drop optimistic queue row so the prompt-queue chrome does not stick open.
      const remaining = (state.promptQueues[conversationId] ?? []).filter(
        (item) => item.id !== clientTempId && item.clientTempId !== clientTempId,
      );
      dispatch({ type: 'promptQueue/set', conversationId, items: remaining });
      throw err;
    }
  }

  if (busy && forceImmediate && run) {
    await gateway.request('run.cancel', { run_id: run.id });
  }

  // Resolve project BEFORE optimistic UI so a missing project does not leave
  // a permanent "正在思考" live bubble with no run to complete.
  const projectPath = await resolveProjectPath({
    explicit: projectPathParam,
    conversationProjectId: conversation?.projectId ?? null,
    gateway,
    tryActiveProject: true,
  });
  if (!projectPath) {
    throw new Error(
      'project_path is required: select a project directory before starting a run',
    );
  }

  // ADR-0016: conversation-level capability selection rides on run.start.
  // Absent / empty selection ⇒ omit the field entirely (None = legacy behaviour).
  const capabilitySelection: CapabilitySelection | null =
    params.capabilitySelection !== undefined
      ? params.capabilitySelection
      : state.capabilitySelectionByConversation[conversationId] ?? null;
  const hasCapabilitySelection =
    capabilitySelection != null &&
    ((capabilitySelection.skills?.length ?? 0) > 0 ||
      (capabilitySelection.mcp_servers?.length ?? 0) > 0 ||
      Boolean(capabilitySelection.expert_id) ||
      Boolean(capabilitySelection.team_id));

  const optimisticUserId = `pending-user-${Date.now()}`;
  dispatch({
    type: 'messages/appendOptimistic',
    message: {
      id: optimisticUserId,
      conversationId,
      role: 'user',
      status: 'sending',
      createdAt: new Date().toISOString(),
      contentBlocks: [{ type: 'text', text: content }],
    },
  });

  let started: Run | Record<string, unknown>;
  try {
    started = await gateway.request<Run | Record<string, unknown>>('run.start', {
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
      ...(effort && effort.trim() ? { effort: effort.trim() } : {}),
      ...(runtimeId && runtimeId.trim() ? { runtime_id: runtimeId.trim() } : {}),
      ...(agentProfileId && agentProfileId.trim()
        ? { agent_profile_id: agentProfileId.trim() }
        : {}),
      ...(hasCapabilitySelection ? { capability_selection: capabilitySelection } : {}),
    });
  } catch (err) {
    dispatch({ type: 'messages/remove', id: optimisticUserId, conversationId });
    dispatch({ type: 'run/clearActive', conversationId });
    throw err;
  }

  // Prefer daemon_run_id when present so subscribe/getEvents hit engine id.
  const wire =
    started && typeof started === 'object'
      ? (started as Record<string, unknown>)
      : {};
  const mappedBase: Run =
    started && typeof started === 'object' && 'providerId' in started
      ? (started as Run)
      : mapWireRun(wire);
  const daemonIdRaw = wire.daemon_run_id ?? wire.daemonRunId;
  const preferredId =
    typeof daemonIdRaw === 'string' && daemonIdRaw.trim()
      ? daemonIdRaw.trim()
      : mappedBase.id;
  const mapped: Run = {
    ...mappedBase,
    id: preferredId || mappedBase.id,
    conversationId: mappedBase.conversationId || conversationId,
    startedAt: mappedBase.startedAt ?? new Date().toISOString(),
  };
  if (!mapped.id) {
    dispatch({ type: 'messages/remove', id: optimisticUserId, conversationId });
    throw new Error('run.start returned no run id');
  }
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
  // 审计收口 #4：run.cancel 返回权威 Run，必须 upsert 回 store（清
  // recovering/transport error 的最终权威终态），不能丢弃终态。
  const result = await gateway.request<Run | Record<string, unknown>>('run.cancel', {
    run_id: runId,
  });
  if (result && typeof result === 'object' && 'id' in result) {
    const run =
      'providerId' in result ? (result as Run) : { ...(result as Record<string, unknown>), id: String(result.id) };
    if ('status' in run) {
      dispatch({ type: 'run/upsert', run: run as Run });
    }
  }
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
