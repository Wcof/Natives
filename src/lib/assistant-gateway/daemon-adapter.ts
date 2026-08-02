/**
 * DaemonAssistantAdapter — production path via Tauri nativesAPI.assistantV2.
 * GUI still only talks to AssistantGateway; this file is the sole place that
 * may touch window.nativesAPI for assistant execution.
 */
import type {
  AssistantMethod,
  ConversationSnapshot,
  DaemonCapabilities,
  RunEvent,
} from '@/lib/assistant-protocol';
import {
  mapWireArtifactList,
  mapWireCapabilities,
  mapWireConversation,
  mapWireMessage,
  mapWireRun,
  mapWireRunEvent,
  AuthoritativeEventMissing,
  applyProjectionEvent,
  createProjectionState,
} from '@/lib/assistant-protocol';
import type { AssistantGateway } from './gateway';

type RequestFn = (method: string, params?: unknown) => Promise<unknown>;

function isTerminalEventType(type: string): boolean {
  return (
    type === 'completed' ||
    type === 'failed' ||
    type === 'interrupted' ||
    type === 'cancelled'
  );
}

function getAssistantV2Request(): RequestFn | null {
  if (typeof window === 'undefined') return null;
  const api = window.nativesAPI?.assistantV2;
  if (!api?.request) return null;
  return (method, params) => api.request(method, params);
}

export interface DaemonAdapterOptions {
  /** Inject for tests. */
  requestFn?: RequestFn;
  pollIntervalMs?: number;
}

export class DaemonAssistantAdapter implements AssistantGateway {
  private requestFn: RequestFn | null;
  private connected = false;
  private pollIntervalMs: number;
  private abortControllers = new Map<string, AbortController>();

  constructor(options: DaemonAdapterOptions = {}) {
    this.requestFn = options.requestFn ?? null;
    this.pollIntervalMs = options.pollIntervalMs ?? 400;
  }

  private resolveRequest(): RequestFn {
    const fn = this.requestFn ?? getAssistantV2Request();
    if (!fn) throw new Error('assistantV2 not available');
    return fn;
  }

  async connect(): Promise<void> {
    const fn = this.resolveRequest();
    try {
      await fn('daemon.ping', {});
      this.connected = true;
    } catch (err) {
      this.connected = false;
      throw err;
    }
  }

  async disconnect(): Promise<void> {
    for (const c of this.abortControllers.values()) c.abort();
    this.abortControllers.clear();
    this.connected = false;
  }

  async getCapabilities(): Promise<DaemonCapabilities | null> {
    try {
      const raw = (await this.resolveRequest()('daemon.getCapabilities', {})) as Record<string, unknown>;
      return mapWireCapabilities(raw ?? {});
    } catch {
      return null;
    }
  }

  async request<T>(method: AssistantMethod, params?: unknown): Promise<T> {
    return (await this.resolveRequest()(method, params)) as T;
  }

  async *subscribe(runId: string, afterSequence: number): AsyncIterable<RunEvent> {
    const controller = new AbortController();
    this.abortControllers.set(runId, controller);
    let seq = afterSequence;
    let projection = createProjectionState(runId, afterSequence);
    try {
      while (!controller.signal.aborted) {
        let events: RunEvent[] = [];
        let terminal = false;
        try {
          const sub = (await this.resolveRequest()('run.subscribe', {
            run_id: runId,
            after_sequence: seq,
            wait_ms: Math.max(this.pollIntervalMs, 800),
            mode: 'push',
          })) as { events?: unknown[]; terminal?: boolean };
          const raw = Array.isArray(sub?.events) ? sub.events : [];
          events = raw.map((e) => mapWireRunEvent((e ?? {}) as Record<string, unknown>));
          terminal = Boolean(sub?.terminal);
        } catch {
          // Fallback path: one-shot getEvents + run status (no push wait).
          const raw = (await this.resolveRequest()('run.getEvents', {
            run_id: runId,
            after_sequence: seq,
          })) as unknown;
          const list = Array.isArray(raw)
            ? raw
            : Array.isArray((raw as { events?: unknown[] } | null)?.events)
              ? (raw as { events: unknown[] }).events
              : [];
          events = list.map((e) => mapWireRunEvent((e ?? {}) as Record<string, unknown>));
          terminal = await this.isRunTerminal(runId);
        }

        let sawTerminalEvent = false;
        for (const event of events) {
          if (event.sequence <= seq) continue;
          seq = event.sequence;
          projection = applyProjectionEvent(projection, event);
          yield event;
          if (isTerminalEventType(event.type)) {
            sawTerminalEvent = true;
            return;
          }
        }

        if (sawTerminalEvent) return;

        // Server says the run is terminal but we have no new events after `seq`
        // (client missed the terminal event, or after_sequence already past it).
        // Replay from 0 once to recover the real terminal event; never exit the
        // stream as "ended" while the UI still thinks the run is active — that
        // used to flip ConnectionBanner into permanent "正在重连".
        if (terminal) {
          const recovered = await this.recoverTerminalEvent(runId, seq);
          if (recovered) {
            projection = applyProjectionEvent(projection, recovered);
            if (recovered.sequence <= seq) throw new AuthoritativeEventMissing(runId);
            seq = recovered.sequence;
            yield recovered;
            return;
          }
          // A terminal DB status without its authoritative event is an
          // incomplete projection, not permission to fabricate a sequence.
          throw new AuthoritativeEventMissing(runId);
        }

        // Still active: keep polling. Do not exit after N empty rounds — long
        // tool runs / slow providers are valid, and exiting only causes a fake
        // reconnect loop in the workbench.
        await new Promise((r) => setTimeout(r, this.pollIntervalMs));
      }
    } finally {
      this.abortControllers.delete(runId);
    }
  }

  private async isRunTerminal(runId: string): Promise<boolean> {
    try {
      // Prefer a cheap events scan for a terminal kind already on disk.
      const raw = (await this.resolveRequest()('run.getEvents', {
        run_id: runId,
        after_sequence: 0,
      })) as unknown;
      const list = Array.isArray(raw)
        ? raw
        : Array.isArray((raw as { events?: unknown[] } | null)?.events)
          ? (raw as { events: unknown[] }).events
          : [];
      const events = list.map((e) => mapWireRunEvent((e ?? {}) as Record<string, unknown>));
      if (events.some((e) => isTerminalEventType(e.type))) return true;

      // Fall back to run.list status (run.get is not a public RPC).
      const listed = (await this.resolveRequest()('run.list', {
        run_id: runId,
        limit: 50,
      })) as unknown;
      const runs = Array.isArray(listed)
        ? listed
        : Array.isArray((listed as { runs?: unknown[] } | null)?.runs)
          ? (listed as { runs: unknown[] }).runs
          : [];
      const row = runs.find((r) => {
        const rec = (r ?? {}) as Record<string, unknown>;
        return String(rec.id ?? '') === runId;
      }) as Record<string, unknown> | undefined;
      if (!row) return false;
      const status = String(row.status ?? '');
      return (
        status === 'completed' ||
        status === 'failed' ||
        status === 'cancelled' ||
        status === 'interrupted'
      );
    } catch {
      return false;
    }
  }

  /** Find the latest terminal event at-or-after `afterSequence` (replay from 0 if needed). */
  private async recoverTerminalEvent(
    runId: string,
    afterSequence: number,
  ): Promise<RunEvent | null> {
    try {
      const raw = (await this.resolveRequest()('run.getEvents', {
        run_id: runId,
        after_sequence: 0,
      })) as unknown;
      const list = Array.isArray(raw)
        ? raw
        : Array.isArray((raw as { events?: unknown[] } | null)?.events)
          ? (raw as { events: unknown[] }).events
          : [];
      const events = list.map((e) => mapWireRunEvent((e ?? {}) as Record<string, unknown>));
      const terminals = events.filter(
        (e) => e.sequence > afterSequence && isTerminalEventType(e.type),
      );
      if (terminals.length > 0) return terminals[terminals.length - 1]!;
      // If afterSequence already past the terminal, still surface the last terminal
      // so the controller can apply it (reducer ignores seq <= last unless restamped).
      return null;
    } catch {
      return null;
    }
  }

  async getSnapshot(conversationId: string): Promise<ConversationSnapshot> {
    const fn = this.resolveRequest();
    const [convRaw, messagesRaw, runsRaw] = await Promise.all([
      fn('conversation.get', { id: conversationId }).catch(() => null),
      fn('conversation.getMessagesPage', { conversation_id: conversationId, limit: 100 }).catch(() => fn('conversation.getMessages', { conversation_id: conversationId })),
      fn('run.list', { conversation_id: conversationId, limit: 20 }),
    ]);

    const conversation = convRaw
      ? mapWireConversation(convRaw as Record<string, unknown>)
      : mapWireConversation({ id: conversationId, title: 'Conversation' });

    const messageRows = Array.isArray(messagesRaw)
      ? messagesRaw
      : Array.isArray((messagesRaw as { messages?: unknown[] } | null)?.messages)
        ? (messagesRaw as { messages: unknown[] }).messages
        : [];
    const pageRaw = messagesRaw as { nextCursor?: { createdAt?: string; id?: string } | null };
    const messages = messageRows.map((m) =>
      mapWireMessage((m ?? {}) as Record<string, unknown>),
    );
    const runRows = Array.isArray(runsRaw)
      ? runsRaw
      : Array.isArray((runsRaw as { runs?: unknown[] } | null)?.runs)
        ? (runsRaw as { runs: unknown[] }).runs
        : [];
    const runs = runRows.map((r) =>
      mapWireRun((r ?? {}) as Record<string, unknown>),
    );

    const eventsByRun: Record<string, RunEvent[]> = {};
    const active = runs.find((r) =>
      [
        'running',
        'preparing',
        'reasoning',
        'waiting_permission',
        'waiting_user',
        'waiting_subagent',
        'queued',
        'cancelling',
      ].includes(String(r.status)),
    );

    await Promise.all(
      runs.slice(0, 5).map(async (run) => {
        try {
          const raw = (await fn('run.getEvents', { run_id: run.id, after_sequence: 0 })) as unknown[];
          eventsByRun[run.id] = (Array.isArray(raw) ? raw : []).map((e) =>
            mapWireRunEvent((e ?? {}) as Record<string, unknown>),
          );
        } catch {
          eventsByRun[run.id] = [];
        }
      }),
    );

    // Load artifacts for recent main runs + any child runs we already know about.
    // Prefer the active run first, then remaining recent runs (including completed)
    // so finished sessions still recover file/artifact history after refresh.
    const artifactRunIds: string[] = [];
    if (active) artifactRunIds.push(active.id);
    for (const run of runs.slice(0, 8)) {
      if (!artifactRunIds.includes(run.id)) artifactRunIds.push(run.id);
    }
    // Collect child run ids from loaded events (subagent_created).
    for (const events of Object.values(eventsByRun)) {
      for (const event of events) {
        if (event.type !== 'subagent_created') continue;
        const subId = String(
          event.payload.sub_run_id ?? event.payload.subRunId ?? event.payload.child_run_id ?? '',
        );
        if (subId && !artifactRunIds.includes(subId)) artifactRunIds.push(subId);
      }
    }

    const artifacts: NonNullable<ConversationSnapshot['artifacts']> = [];
    await Promise.all(
      artifactRunIds.slice(0, 12).map(async (runId) => {
        try {
          const raw = await fn('artifact.list', { run_id: runId });
          for (const a of mapWireArtifactList(raw)) {
            artifacts.push(a.runId ? a : { ...a, runId });
          }
        } catch {
          /* optional */
        }
      }),
    );

    let interactions: ConversationSnapshot['interactions'] = [];
    try {
      const raw = (await fn('permission.listPending', {
        conversation_id: conversationId,
      })) as unknown[];
      if (Array.isArray(raw)) {
        interactions = raw.map((item) => {
          const r = (item ?? {}) as Record<string, unknown>;
          return {
            kind: 'permission' as const,
            id: String(r.id ?? r.permission_id ?? ''),
            runId: String(r.run_id ?? r.runId ?? ''),
            conversationId,
            toolCallId: String(r.tool_call_id ?? r.toolCallId ?? ''),
            toolName: String(r.tool_name ?? r.toolName ?? 'tool'),
            reason: String(r.reason ?? ''),
            input: (r.input ?? r.args ?? {}) as Record<string, unknown>,
            createdAt: String(r.created_at ?? r.createdAt ?? new Date().toISOString()),
          };
        });
      }
    } catch {
      // Method may be unimplemented on older daemons.
    }

    // Also pull pending interactions (subagent_assignment / ask_user / plan).
    try {
      const raw = (await fn('interaction.listPending', {
        conversation_id: conversationId,
      })) as unknown;
      const list = Array.isArray(raw)
        ? raw
        : raw && typeof raw === 'object' && Array.isArray((raw as { interactions?: unknown[] }).interactions)
          ? (raw as { interactions: unknown[] }).interactions
          : [];
      for (const item of list) {
        if (!item || typeof item !== 'object') continue;
        const r = item as Record<string, unknown>;
        const kind = String(r.kind ?? '');
        const id = String(r.id ?? r.interaction_id ?? r.interactionId ?? '');
        if (!id) continue;
        if (interactions.some((i) => i.id === id)) continue;
        if (kind === 'subagent_assignment') {
          const payload =
            r.payload && typeof r.payload === 'object'
              ? (r.payload as Record<string, unknown>)
              : r;
          const defaultRaw =
            payload.default_binding && typeof payload.default_binding === 'object'
              ? (payload.default_binding as Record<string, unknown>)
              : payload.defaultBinding && typeof payload.defaultBinding === 'object'
                ? (payload.defaultBinding as Record<string, unknown>)
                : null;
          const defaultBinding = defaultRaw
            ? {
                providerId: String(
                  defaultRaw.provider_id ?? defaultRaw.providerId ?? '',
                ),
                keyId: String(defaultRaw.key_id ?? defaultRaw.keyId ?? ''),
                modelId: String(defaultRaw.model_id ?? defaultRaw.modelId ?? ''),
              }
            : null;
          const tasks = Array.isArray(payload.tasks)
            ? payload.tasks
                .filter(
                  (item): item is Record<string, unknown> =>
                    Boolean(item) && typeof item === 'object',
                )
                .map((task, index) => ({
                  callId: String(task.call_id ?? task.callId ?? `task-${index}`),
                  name: String(
                    task.name ?? task.prompt ?? task.task ?? `Task ${index + 1}`,
                  ),
                  prompt:
                    task.prompt != null
                      ? String(task.prompt)
                      : task.task != null
                        ? String(task.task)
                        : null,
                }))
            : undefined;
          interactions.push({
            kind: 'subagent_assignment',
            id,
            runId: String(
              r.run_id ?? r.runId ?? payload.parent_run_id ?? payload.run_id ?? '',
            ),
            conversationId: String(
              r.conversation_id ??
                r.conversationId ??
                payload.parent_conversation_id ??
                payload.conversation_id ??
                conversationId,
            ),
            createdAt: String(r.created_at ?? r.createdAt ?? new Date().toISOString()),
            reason: payload.reason != null ? String(payload.reason) : undefined,
            batchId:
              payload.batch_id != null || payload.batchId != null
                ? String(payload.batch_id ?? payload.batchId)
                : undefined,
            parentConversationId:
              payload.parent_conversation_id != null || payload.parentConversationId != null
                ? String(payload.parent_conversation_id ?? payload.parentConversationId)
                : undefined,
            parentRunId:
              payload.parent_run_id != null || payload.parentRunId != null
                ? String(payload.parent_run_id ?? payload.parentRunId)
                : undefined,
            defaultBinding,
            tasks,
          });
        } else if (kind === 'tool_permission' || kind === 'permission') {
          interactions.push({
            kind: 'permission',
            id,
            runId: String(r.run_id ?? r.runId ?? ''),
            conversationId,
            toolCallId: String(r.tool_call_id ?? r.toolCallId ?? ''),
            toolName: String(r.tool_name ?? r.toolName ?? 'tool'),
            reason: String(r.reason ?? ''),
            input: (r.input ?? r.args ?? {}) as Record<string, unknown>,
            createdAt: String(r.created_at ?? r.createdAt ?? new Date().toISOString()),
          });
        }
      }
    } catch {
      /* optional */
    }

    return {
      conversation,
      messages,
      runs,
      activeRunId: active?.id ?? null,
      eventsByRun,
      messagePageInfo: Array.isArray(messagesRaw)
        ? { hasMore: false, nextCursor: null }
        : {
            hasMore: Boolean(pageRaw.nextCursor),
            nextCursor: pageRaw.nextCursor?.createdAt && pageRaw.nextCursor.id
              ? { createdAt: pageRaw.nextCursor.createdAt, id: pageRaw.nextCursor.id }
              : null,
          },
      artifacts,
      interactions,
      capabilities: await this.getCapabilities(),
    };
  }
}
