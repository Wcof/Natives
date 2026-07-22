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
  mapWireArtifact,
  mapWireCapabilities,
  mapWireConversation,
  mapWireMessage,
  mapWireRun,
  mapWireRunEvent,
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
            // If the terminal event's sequence is already covered, re-stamp it
            // as next so the reducer will accept it (it drops seq <= last).
            const event =
              recovered.sequence > seq
                ? recovered
                : {
                    ...recovered,
                    sequence: seq + 1,
                    payload: { ...recovered.payload, replayed: true },
                  };
            yield event;
            return;
          }
          // Run is terminal on the server with no recoverable event: synthesize
          // a terminal from run.get so the UI can leave the live bubble.
          const synthesized = await this.synthesizeTerminalFromRun(runId, seq);
          if (synthesized) {
            yield synthesized;
          }
          return;
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
      const anyTerminal = events.filter((e) => isTerminalEventType(e.type));
      return anyTerminal.length > 0 ? anyTerminal[anyTerminal.length - 1]! : null;
    } catch {
      return null;
    }
  }

  private async synthesizeTerminalFromRun(
    runId: string,
    afterSequence: number,
  ): Promise<RunEvent | null> {
    try {
      const listed = (await this.resolveRequest()('run.list', {
        run_id: runId,
        limit: 50,
      })) as unknown;
      const runs = Array.isArray(listed)
        ? listed
        : Array.isArray((listed as { runs?: unknown[] } | null)?.runs)
          ? (listed as { runs: unknown[] }).runs
          : [];
      const raw = runs.find((r) => {
        const rec = (r ?? {}) as Record<string, unknown>;
        return String(rec.id ?? '') === runId;
      }) as Record<string, unknown> | undefined;
      if (!raw) return null;
      const status = String(raw.status ?? '');
      if (
        status !== 'completed' &&
        status !== 'failed' &&
        status !== 'cancelled' &&
        status !== 'interrupted'
      ) {
        return null;
      }
      const type =
        status === 'completed'
          ? 'completed'
          : status === 'cancelled'
            ? 'cancelled'
            : status === 'interrupted'
              ? 'interrupted'
              : 'failed';
      return {
        runId,
        sequence: afterSequence + 1,
        timestamp: String(raw.finished_at ?? raw.finishedAt ?? new Date().toISOString()),
        type,
        payload: {
          reason: status,
          code: raw.error_code ?? raw.errorCode ?? (type === 'failed' ? 'RUN_TERMINAL' : undefined),
          error: raw.error_message ?? raw.errorMessage ?? undefined,
          source: 'run.list',
        },
      };
    } catch {
      return null;
    }
  }

  async getSnapshot(conversationId: string): Promise<ConversationSnapshot> {
    const fn = this.resolveRequest();
    const [convRaw, messagesRaw, runsRaw] = await Promise.all([
      fn('conversation.get', { id: conversationId }).catch(() => null),
      fn('conversation.getMessages', { conversation_id: conversationId }),
      fn('run.list', { conversation_id: conversationId, limit: 20 }),
    ]);

    const conversation = convRaw
      ? mapWireConversation(convRaw as Record<string, unknown>)
      : mapWireConversation({ id: conversationId, title: 'Conversation' });

    const messages = (Array.isArray(messagesRaw) ? messagesRaw : []).map((m) =>
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

    let artifacts: ConversationSnapshot['artifacts'] = [];
    if (active) {
      try {
        const raw = (await fn('artifact.list', { run_id: active.id })) as unknown[];
        artifacts = (Array.isArray(raw) ? raw : []).map((a) =>
          mapWireArtifact((a ?? {}) as Record<string, unknown>),
        );
      } catch {
        artifacts = [];
      }
    }

    let interactions: ConversationSnapshot['interactions'] = [];
    try {
      const raw = (await fn('permission.listPending', { conversation_id: conversationId })) as unknown[];
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

    return {
      conversation,
      messages,
      runs,
      activeRunId: active?.id ?? null,
      eventsByRun,
      artifacts,
      interactions,
      capabilities: await this.getCapabilities(),
    };
  }
}
