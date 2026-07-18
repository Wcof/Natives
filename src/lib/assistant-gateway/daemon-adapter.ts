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
        // Prefer run.subscribe with long-poll push_wait (Protocol v2), fall back to getEvents.
        let events: RunEvent[] = [];
        try {
          const sub = (await this.resolveRequest()('run.subscribe', {
            run_id: runId,
            after_sequence: seq,
            wait_ms: Math.max(this.pollIntervalMs, 800),
            mode: 'push',
          })) as { events?: unknown[]; terminal?: boolean };
          const raw = Array.isArray(sub?.events) ? sub.events : [];
          events = raw.map((e) => mapWireRunEvent((e ?? {}) as Record<string, unknown>));
        } catch {
          const raw = (await this.resolveRequest()('run.getEvents', {
            run_id: runId,
            after_sequence: seq,
          })) as unknown[];
          events = (Array.isArray(raw) ? raw : []).map((e) =>
            mapWireRunEvent((e ?? {}) as Record<string, unknown>),
          );
        }
        for (const event of events) {
          if (event.sequence <= seq) continue;
          seq = event.sequence;
          yield event;
          if (
            event.type === 'completed' ||
            event.type === 'failed' ||
            event.type === 'interrupted' ||
            event.type === 'cancelled'
          ) {
            return;
          }
        }
        if (events.length === 0) {
          await new Promise((r) => setTimeout(r, this.pollIntervalMs));
        }
      }
    } finally {
      this.abortControllers.delete(runId);
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
    const runs = (Array.isArray(runsRaw) ? runsRaw : []).map((r) =>
      mapWireRun((r ?? {}) as Record<string, unknown>),
    );

    const eventsByRun: Record<string, RunEvent[]> = {};
    const active = runs.find((r) =>
      ['running', 'preparing', 'waiting_permission', 'waiting_subagent', 'queued'].includes(r.status),
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
      activeRunId: active?.id ?? runs[0]?.id ?? null,
      eventsByRun,
      artifacts,
      interactions,
      capabilities: await this.getCapabilities(),
    };
  }
}
