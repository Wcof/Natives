/**
 * DaemonAssistantAdapter — production path via Tauri nativesAPI.assistantV2.
 * GUI still only talks to AssistantGateway; this file is the sole place that
 * may touch window.nativesAPI for assistant execution.
 *
 * Persistent live stream (RunWatchStreamV2, docs/contracts/STREAM-CONTRACT-V2.md)
 * is the PRIMARY path: the host watch bridge (`run_watch_start`/`stop` + the
 * `run-watch-frame` Tauri event) streams durable + live frames, and the adapter
 * tracks dual cursors (lastDurableSequence / lastLiveSequence) that never share
 * a sequence namespace. Live deltas are transient and never advance the durable
 * projection watermark. The persistent stream is the single streaming contract
 * (STREAM-CONTRACT-V2); the legacy `run.subscribe` long-poll fallback is retired
 * (MIG-004).
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
  applyProjectionEvent,
  createProjectionState,
} from '@/lib/assistant-protocol';
import type { AssistantGateway } from './gateway';
import { cmd, subscribe } from '@/lib/tauri/core';

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

// ─── RunWatchStreamV2 host watch bridge ────────────────────────────────────

/** Tauri event emitted by the host watch bridge once per frame. */
const WATCH_FRAME_EVENT = 'run-watch-frame';

/** One RunWatchStreamV2 frame as delivered by the host bridge. */
export interface WatchFrame {
  frame_type: 'event' | 'heartbeat' | 'resync_required';
  lane?: 'durable' | 'live';
  run_id?: string;
  durable_sequence?: number | null;
  live_sequence?: number | null;
  event_type?: string;
  payload?: unknown;
  timestamp?: string;
  reason?: string;
}

/** `run-watch-frame` event payload. */
export interface WatchFrameEvent {
  run_id: string;
  frame: WatchFrame;
}

export interface WatchStartResult {
  ok: boolean;
  error?: string;
}

/**
 * Result of `run_watch_state` — the host-side dual-cursor recovery source.
 */
export interface WatchStateResult {
  active: boolean;
  lastDurableSequence?: number;
  lastLiveSequence?: number;
  terminal?: boolean;
  error?: string | null;
}

/**
 * Host watch bridge seam. The Renderer never talks to the UDS socket directly
 * — only Tauri commands (`run_watch_start`/`run_watch_stop`/`run_watch_state`)
 * and the `run-watch-frame` event.
 */
export interface HostWatchBridge {
  start(
    runId: string,
    afterDurableSequence: number,
    afterLiveSequence: number,
  ): Promise<WatchStartResult>;
  stop(runId: string): Promise<void>;
  /**
   * Query the host's current watch state. Optional so injected test bridges
   * don't have to implement it; when absent the adapter keeps its local dual
   * cursors (which already track durable/live on every consumed frame).
   */
  state?(runId: string): Promise<WatchStateResult>;
  listen(listener: (frame: WatchFrame) => void): () => void;
}

/**
 * Tauri v2 JS contract: invoke args are camelCase and Tauri maps them to the
 * Rust command's snake_case parameters (`run_id`, `after_durable_sequence`,
 * `after_live_sequence`). Passing snake_case keys from JS leaves the params
 * unmapped, so the watch commands silently miss their arguments.
 */
export function buildWatchStartParams(
  runId: string,
  afterDurableSequence: number,
  afterLiveSequence: number,
): Record<string, unknown> {
  return { runId, afterDurableSequence, afterLiveSequence };
}

export function buildWatchStopParams(runId: string): Record<string, unknown> {
  return { runId };
}

export function buildWatchStateParams(runId: string): Record<string, unknown> {
  return { runId };
}

/**
 * Tauri command invoker：与 `@/lib/tauri/core` 的 `cmd` 同签名（泛型返回），
 * 供默认 bridge 注入与契约测试捕获 exact command/payload。
 */
export type TauriInvokeFn = <T = unknown>(
  command: string,
  args?: Record<string, unknown>,
) => Promise<T>;

/**
 * Build the default Tauri-hosted bridge (lazy; tests inject a fake instead).
 *
 * 审计收口 #2：invoker 与 frame-subscribe 可注入，默认走真实 `cmd`/`subscribe`，
 * 这样测试能捕获 exact command name 与 exact camelCase payload，而不只是测
 * params helper。
 */
export function createTauriWatchBridge(
  invoker: TauriInvokeFn = cmd,
  subscribeFrames: (listener: (frame: WatchFrame) => void) => () => void = (listener) =>
    subscribe<WatchFrameEvent>(WATCH_FRAME_EVENT, (payload) => {
      listener(payload.frame);
    }),
  isBrowser: boolean = typeof window !== 'undefined',
): HostWatchBridge | null {
  if (!isBrowser) return null;
  return {
    async start(runId, afterDurableSequence, afterLiveSequence) {
      try {
        const result = await invoker<{ ok?: boolean; error?: string }>(
          'run_watch_start',
          buildWatchStartParams(runId, afterDurableSequence, afterLiveSequence),
        );
        return { ok: Boolean(result.ok), error: result.error };
      } catch (err) {
        return { ok: false, error: err instanceof Error ? err.message : String(err) };
      }
    },
    async stop(runId) {
      try {
        await invoker('run_watch_stop', buildWatchStopParams(runId));
      } catch {
        // best-effort unsubscribe
      }
    },
    async state(runId) {
      try {
        const result = await invoker<{
          active?: boolean;
          lastDurableSequence?: number;
          lastLiveSequence?: number;
          terminal?: boolean;
          error?: string | null;
        }>('run_watch_state', buildWatchStateParams(runId));
        return {
          active: Boolean(result.active),
          lastDurableSequence: result.lastDurableSequence ?? 0,
          lastLiveSequence: result.lastLiveSequence ?? 0,
          terminal: Boolean(result.terminal),
          error: result.error ?? null,
        };
      } catch {
        // run_watch_state unavailable (older host) — adapter keeps local cursors.
        return {
          active: false,
          lastDurableSequence: 0,
          lastLiveSequence: 0,
          terminal: false,
          error: null,
        };
      }
    },
    listen(listener) {
      return subscribeFrames(listener);
    },
  };
}

/**
 * Synthetic live-sequence step. Live deltas are mapped to
 * `durableAnchor + fraction` so the single-sequence renderer reducer accepts
 * them without advancing the effective durable watermark (floor on reconnect).
 */
const LIVE_SEQUENCE_STEP = 0.000001;
const LIVE_SEQUENCE_MAX_FRACTION = 0.999999;

/** Max consecutive persistent-stream failures before falling back / giving up. */
const MAX_WATCH_RECONNECTS = 3;

/** Thrown when the persistent stream cannot be (re)established. */
export class WatchStreamUnavailableError extends Error {
  readonly runId: string;
  constructor(runId: string, reason: string) {
    super(`run.watch persistent stream unavailable for ${runId}: ${reason}`);
    this.name = 'WatchStreamUnavailableError';
    this.runId = runId;
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/** Flatten the internally-tagged RunEventKind payload (drop the `type` key). */
function flattenPayload(raw: unknown): Record<string, unknown> {
  const payload: Record<string, unknown> = {};
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return payload;
  for (const [k, v] of Object.entries(raw as Record<string, unknown>)) {
    if (k === 'type') continue;
    payload[k] = v;
  }
  return payload;
}

export interface DaemonAdapterOptions {
  /** Inject for tests. */
  requestFn?: RequestFn;
  pollIntervalMs?: number;
  /** Inject the persistent-stream bridge for tests. */
  watchBridge?: HostWatchBridge;
}

export class DaemonAssistantAdapter implements AssistantGateway {
  private requestFn: RequestFn | null;
  private connected = false;
  private pollIntervalMs: number;
  private abortControllers = new Map<string, AbortController>();
  private projectionRecovery = new Map<
    string,
    ReturnType<typeof createProjectionState>['recovery']
  >();
  /** Per-run ephemeral live cursor, kept across reconnects. */
  private liveCursorByRun = new Map<string, number>();
  private watchBridge: HostWatchBridge | null;

  constructor(options: DaemonAdapterOptions = {}) {
    this.requestFn = options.requestFn ?? null;
    this.pollIntervalMs = options.pollIntervalMs ?? 400;
    this.watchBridge = options.watchBridge ?? null;
  }

  private resolveRequest(): RequestFn {
    const fn = this.requestFn ?? getAssistantV2Request();
    if (!fn) throw new Error('assistantV2 not available');
    return fn;
  }

  private resolveWatchBridge(): HostWatchBridge | null {
    if (this.watchBridge) return this.watchBridge;
    return createTauriWatchBridge();
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

  getProjectionRecovery(runId: string) {
    return this.projectionRecovery.get(runId);
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

  /**
   * Persistent live stream is the single streaming contract (STREAM-CONTRACT-V2).
   * The legacy `run.subscribe` long-poll fallback is retired (MIG-004): when the
   * persistent stream is unavailable the error propagates to the caller.
   */
  async *subscribe(runId: string, afterSequence: number): AsyncIterable<RunEvent> {
    const controller = new AbortController();
    this.abortControllers.set(runId, controller);
    try {
      const bridge = this.resolveWatchBridge();
      if (!bridge) {
        throw new WatchStreamUnavailableError(runId, 'no watch bridge available');
      }
      yield* this.readPersistentStream(
        runId,
        bridge,
        controller.signal,
        Math.floor(afterSequence),
        this.liveCursorByRun.get(runId) ?? 0,
      );
    } finally {
      if (this.abortControllers.get(runId) === controller) {
        this.abortControllers.delete(runId);
      }
    }
  }

  /**
   * Persistent stream reader: consumes RunWatchStreamV2 frames from the host
   * bridge, maintains dual cursors (durable / live — never merged), reconnects
   * by cursor on unexpected closure, and clean-closes on a terminal durable
   * event. Live deltas are buffered and flushed just before the next durable
   * fact; a durable MessageCompleted/ToolCallCompleted clears the transient
   * live state for that unit.
   */
  private async *readPersistentStream(
    runId: string,
    bridge: HostWatchBridge,
    signal: AbortSignal,
    initialDurable: number,
    initialLive: number,
  ): AsyncIterable<RunEvent> {
    let durableSeq = initialDurable;
    let liveSeq = initialLive;
    let liveStaleUpTo = 0;
    let liveFraction = 0;
    let projection = createProjectionState(runId, initialDurable);
    let liveBuffer: RunEvent[] = [];
    let consecutiveFailures = 0;
    let firstStart = true;

    while (!signal.aborted) {
      // The Renderer projection is the durable replay authority. Host watch
      // state may report frames emitted before this listener existed, so it
      // can never advance the durable skip cursor.
      const recovered = await this.recoverCursors(bridge, runId, durableSeq, liveSeq);
      durableSeq = recovered.durable;
      liveSeq = recovered.live;
      const source = this.createFrameSource(bridge, runId, signal);
      let startFailed: WatchStartResult | null = null;
      let started = false;
      // Progress = at least one event-bearing frame since the last (re)start.
      // Heartbeats / resync frames alone are NOT progress: a stream that only
      // ever delivers `resync_required` and never an event must exhaust the
      // reconnect budget instead of looping forever.
      let sawEvent = false;
      try {
        // Register before start: the host can synchronously replay or emit a
        // terminal frame while `start` is still resolving.
        const result = await bridge
          .start(runId, durableSeq, liveSeq)
          .catch(() => ({ ok: false as const, error: 'watch_start_failed' }));
        if (!result.ok) {
          startFailed = result;
        } else {
          started = true;
          firstStart = false;
          for (;;) {
            const frame = await source.next();
            if (frame === 'closed') break;
            const out = this.consumeWatchFrame(frame, runId, {
              durableSeq,
              liveSeq,
              liveStaleUpTo,
              liveFraction,
              projection,
              liveBuffer,
            });
            durableSeq = out.state.durableSeq;
            liveSeq = out.state.liveSeq;
            liveStaleUpTo = out.state.liveStaleUpTo;
            liveFraction = out.state.liveFraction;
            projection = out.state.projection;
            liveBuffer = out.state.liveBuffer;

            if (out.events.length > 0) sawEvent = true;
            for (const ev of out.events) {
              this.projectionRecovery.set(runId, projection.recovery);
              yield ev;
            }
            if (out.resyncLive) {
              // Live buffer lost → drop transient live state; continue durable-only.
              liveBuffer = [];
              liveSeq = 0;
              liveFraction = 0;
            }
            if (out.terminal) {
              await bridge.stop(runId).catch(() => {});
              this.liveCursorByRun.set(runId, liveSeq);
              return;
            }
            if (out.streamClosed) {
              break; // reconnect by durable/live cursor
            }
          }
        }
      } finally {
        source.dispose();
        if (started) await bridge.stop(runId).catch(() => {});
      }
      if (startFailed) {
        // An immediate rejection (embedded mode / old daemon) is an availability
        // failure. Mid-stream reconnect failures retry a bounded number of times.
        if (firstStart || consecutiveFailures >= MAX_WATCH_RECONNECTS) {
          throw new WatchStreamUnavailableError(runId, startFailed.error ?? 'unknown');
        }
        consecutiveFailures += 1;
        await sleep(150 * consecutiveFailures);
        continue;
      }
      if (signal.aborted) {
        this.liveCursorByRun.set(runId, liveSeq);
        return;
      }
      // Bounded reconnect (product decision 4): a stream that keeps closing
      // without delivering any EVENT counts as a failure; after the budget is
      // exhausted the caller reconciles (run.getActivity → run.cancel). A
      // stream that delivered events (daemon restart) resets the counter.
      if (sawEvent) {
        consecutiveFailures = 0;
      } else {
        consecutiveFailures += 1;
        if (consecutiveFailures >= MAX_WATCH_RECONNECTS) {
          throw new WatchStreamUnavailableError(
            runId,
            `stream closed ${consecutiveFailures} times without progress`,
          );
        }
      }
      await sleep(200);
    }
    this.liveCursorByRun.set(runId, liveSeq);
  }

  /**
   * Recover the dual cursors from the host watch state before (re)starting the
   * stream. The local durable cursor is the only replay cursor: it advances
   * after a durable frame has been projected through this adapter. The Host
   * cursor records emission, not Renderer consumption, and must not skip
   * durable history. Its live cursor may still discard transient deltas.
   */
  private async recoverCursors(
    bridge: HostWatchBridge,
    runId: string,
    localDurable: number,
    localLive: number,
  ): Promise<{ durable: number; live: number }> {
    if (!bridge.state) return { durable: localDurable, live: localLive };
    try {
      const st = await bridge.state(runId);
      if (st.active) {
        return {
          durable: localDurable,
          live: Math.max(localLive, st.lastLiveSequence ?? 0),
        };
      }
    } catch {
      // run_watch_state unavailable — keep the local dual cursors.
    }
    return { durable: localDurable, live: localLive };
  }

  /**
   * Wait for the next frame on the host event stream. `'closed'` means the
   * signal was aborted (or the stream ended without a terminal durable event).
   */
  private createFrameSource(bridge: HostWatchBridge, runId: string, signal: AbortSignal) {
    const queue: WatchFrame[] = [];
    let waiter: (() => void) | null = null;
    const wake = () => {
      const w = waiter;
      waiter = null;
      w?.();
    };
    const onAbort = () => wake();
    signal.addEventListener('abort', onAbort);
    const unlisten = bridge.listen((frame) => {
      if (frame.run_id != null && frame.run_id !== runId) return;
      queue.push(frame);
      wake();
    });
    return {
      dispose() {
        signal.removeEventListener('abort', onAbort);
        unlisten();
      },
      async next(): Promise<WatchFrame | 'closed'> {
        for (;;) {
          if (signal.aborted) return 'closed';
          if (queue.length > 0) return queue.shift()!;
          await new Promise<void>((resolve) => {
            waiter = resolve;
          });
        }
      },
    };
  }

  /**
   * Translate one RunWatchStreamV2 frame into renderer events while
   * maintaining the dual durable/live cursors.
   */
  private consumeWatchFrame(
    frame: WatchFrame,
    runId: string,
    state: {
      durableSeq: number;
      liveSeq: number;
      liveStaleUpTo: number;
      liveFraction: number;
      projection: ReturnType<typeof createProjectionState>;
      liveBuffer: RunEvent[];
    },
  ): {
    state: {
      durableSeq: number;
      liveSeq: number;
      liveStaleUpTo: number;
      liveFraction: number;
      projection: ReturnType<typeof createProjectionState>;
      liveBuffer: RunEvent[];
    };
    events: RunEvent[];
    terminal: boolean;
    resyncLive: boolean;
    streamClosed: boolean;
  } {
    const next = { ...state, liveBuffer: [...state.liveBuffer] };
    const events: RunEvent[] = [];
    const out = { state: next, events, terminal: false, resyncLive: false, streamClosed: false };

    if (frame.frame_type === 'heartbeat') {
      if (typeof frame.durable_sequence === 'number') {
        next.durableSeq = Math.max(next.durableSeq, frame.durable_sequence);
      }
      if (typeof frame.live_sequence === 'number') {
        next.liveSeq = Math.max(next.liveSeq, frame.live_sequence);
      }
      return out;
    }

    if (frame.frame_type === 'resync_required') {
      // A live-lane resync means some ephemeral deltas are unrecoverable; the
      // durable lane is never resynced from the live bus. A durable-lane
      // resync / stream_closed signals the Renderer to reconnect by cursor.
      if (frame.lane === 'live' || frame.reason === 'live_buffer_gap') {
        out.resyncLive = true;
      } else {
        out.streamClosed = true;
      }
      return out;
    }

    if (frame.frame_type !== 'event') return out;

    const liveSequence =
      typeof frame.live_sequence === 'number' ? frame.live_sequence : null;

    if (frame.lane === 'live') {
      if (liveSequence != null) {
        next.liveSeq = Math.max(next.liveSeq, liveSequence);
        // Deltas for an already-completed message/tool are stale — drop them.
        if (liveSequence <= next.liveStaleUpTo) return out;
      }
      const synthetic =
        next.durableSeq +
        Math.min(LIVE_SEQUENCE_MAX_FRACTION, next.liveFraction + LIVE_SEQUENCE_STEP);
      next.liveFraction = synthetic - next.durableSeq;
      const event: RunEvent = {
        runId,
        sequence: synthetic,
        timestamp: frame.timestamp ?? new Date().toISOString(),
        type: frame.event_type ?? 'unknown',
        payload: flattenPayload(frame.payload),
      };
      next.liveBuffer = [...next.liveBuffer, event];
      return out;
    }

    // Durable event: flush buffered live deltas first so live facts for a unit
    // arrive before their durable completion.
    for (const liveEvent of next.liveBuffer) {
      events.push(liveEvent);
    }
    next.liveBuffer = [];

    const durableSequence =
      typeof frame.durable_sequence === 'number' ? frame.durable_sequence : 0;
    next.durableSeq = Math.max(next.durableSeq, durableSequence);
    next.liveFraction = 0;

    const event: RunEvent = {
      runId,
      sequence: durableSequence,
      timestamp: frame.timestamp ?? new Date().toISOString(),
      type: frame.event_type ?? 'unknown',
      payload: flattenPayload(frame.payload),
    };
    events.push(event);
    // Durable facts advance the durable projection watermark (live never does).
    next.projection = applyProjectionEvent(next.projection, event);

    const type = event.type;
    if (isTerminalEventType(type)) {
      out.terminal = true;
    }
    // MessageCompleted / ToolCallCompleted clear transient live state: any
    // buffered or late live deltas for the completed unit are now obsolete.
    if (type === 'message_completed' || type === 'tool_call_completed') {
      next.liveBuffer = [];
      next.liveStaleUpTo = Math.max(next.liveStaleUpTo, next.liveSeq);
    }
    return out;
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
