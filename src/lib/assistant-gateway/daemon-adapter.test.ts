/**
 * DaemonAssistantAdapter — inject requestFn (no window.nativesAPI required).
 */
import assert from 'node:assert/strict';
import test from 'node:test';
import { DaemonAssistantAdapter } from './daemon-adapter';
import type { HostWatchBridge, WatchFrame } from './daemon-adapter';

/** In-memory host watch bridge for tests. */
function makeTestBridge() {
  let listener: ((frame: WatchFrame) => void) | null = null;
  const started: Array<{ runId: string; durable: number; live: number }> = [];
  const stopped: string[] = [];
  const bridge: HostWatchBridge = {
    async start(runId, afterDurableSequence, afterLiveSequence) {
      started.push({ runId, durable: afterDurableSequence, live: afterLiveSequence });
      return { ok: true };
    },
    async stop(runId) {
      stopped.push(runId);
    },
    listen(fn) {
      listener = fn;
      return () => {
        listener = null;
      };
    },
  };
  return {
    bridge,
    started,
    stopped,
    emit(frame: WatchFrame) {
      listener?.(frame);
    },
  };
}

const DURABLE = (runId: string, durableSequence: number, eventType: string, payload: Record<string, unknown> = {}): WatchFrame => ({
  frame_type: 'event',
  lane: 'durable',
  run_id: runId,
  durable_sequence: durableSequence,
  live_sequence: null,
  event_type: eventType,
  payload: { type: eventType, ...payload },
  timestamp: new Date().toISOString(),
});

const LIVE = (runId: string, liveSequence: number, eventType: string, payload: Record<string, unknown> = {}): WatchFrame => ({
  frame_type: 'event',
  lane: 'live',
  run_id: runId,
  durable_sequence: null,
  live_sequence: liveSequence,
  event_type: eventType,
  payload: { type: eventType, ...payload },
  timestamp: new Date().toISOString(),
});

test('connect pings daemon and marks connected', async () => {
  const calls: Array<{ method: string; params?: unknown }> = [];
  const adapter = new DaemonAssistantAdapter({
    requestFn: async (method, params) => {
      calls.push({ method, params });
      if (method === 'daemon.ping') return { ok: true };
      throw new Error(`unexpected ${method}`);
    },
  });
  await adapter.connect();
  assert.equal(calls[0]?.method, 'daemon.ping');
});

test('connect fails closed when assistantV2 missing and no inject', async () => {
  const adapter = new DaemonAssistantAdapter({});
  await assert.rejects(() => adapter.connect(), /assistantV2 not available/);
});

test('request never invents streamChat path — only injected methods', async () => {
  const methods: string[] = [];
  const adapter = new DaemonAssistantAdapter({
    requestFn: async (method) => {
      methods.push(method);
      if (method === 'run.start') {
        return {
          id: 'run-x',
          conversation_id: 'c',
          status: 'running',
          provider_id: 'openai',
          model_id: 'm',
        };
      }
      return {};
    },
  });
  await adapter.request('run.start', {
    conversation_id: 'c',
    content: 'hi',
    project_path: '/tmp/p',
  });
  assert.deepEqual(methods, ['run.start']);
  assert.ok(!methods.some((m) => m.toLowerCase().includes('stream')));
});

test('getSnapshot accepts daemon run.list envelope', async () => {
  const adapter = new DaemonAssistantAdapter({
    requestFn: async (method) => {
      if (method === 'conversation.get') return { id: 'c', title: 'C' };
      if (method === 'conversation.getMessages') return [];
      if (method === 'run.list') {
        return {
          runs: [{
            id: 'r',
            conversation_id: 'c',
            status: 'completed',
            provider_id: 'openai',
            model_id: 'm',
          }],
        };
      }
      if (method === 'run.getEvents') return [];
      throw new Error(method);
    },
  });

  const snapshot = await adapter.getSnapshot('c');
  assert.equal(snapshot.runs[0]?.id, 'r');
});

test('stream_subscribe_has_no_fixed_poll_delay', async () => {
  // A3/§5: the active stream must not sleep on a fixed client-side poll
  // interval — the daemon blocks server-side up to wait_ms and the loop
  // continues immediately on an empty window. Source-level regression: no
  // fixed setTimeout(pollIntervalMs) remains in the subscribe loop.
  const src = DaemonAssistantAdapter.toString();
  assert.ok(
    !/setTimeout\([^)]*pollIntervalMs/.test(src),
    'subscribe loop must not use a fixed setTimeout(pollIntervalMs)',
  );
});

// ─── RunWatchStreamV2 persistent stream (primary path) ────────────────────

test('subscribe uses persistent stream: durable + live frames, terminal clean close', async () => {
  const bridge = makeTestBridge();
  const adapter = new DaemonAssistantAdapter({
    requestFn: async (method) => {
      if (method === 'daemon.ping') return { ok: true };
      throw new Error(`unexpected ${method}`);
    },
    watchBridge: bridge.bridge,
  });
  await adapter.connect();

  const types: string[] = [];
  const seqs: number[] = [];
  const collect = (async () => {
    for await (const e of adapter.subscribe('r1', 0)) {
      types.push(e.type);
      seqs.push(e.sequence);
    }
  })();
  await new Promise((r) => setTimeout(r, 10));

  bridge.emit(DURABLE('r1', 1, 'started'));
  bridge.emit(LIVE('r1', 101, 'text_delta', { text: 'hi' }));
  bridge.emit(DURABLE('r1', 2, 'message_completed', { message_id: 'm1' }));
  bridge.emit(DURABLE('r1', 3, 'completed', { reason: 'ok' }));
  await collect;

  assert.deepEqual(types, ['started', 'text_delta', 'message_completed', 'completed']);
  // Live deltas are anchored below the durable watermark (dual cursor).
  assert.ok(seqs.some((s) => s > 1 && s < 2), 'live delta is durably anchored');
  assert.ok(seqs.includes(3), 'terminal durable event yielded');
  assert.equal(bridge.started.length, 1);
  assert.equal(bridge.started[0]?.durable, 0);
  assert.equal(bridge.started[0]?.live, 0);
  assert.ok(bridge.stopped.includes('r1'), 'watch stopped after terminal');
});

test('subscribe reconnects by durable/live cursor after stream_closed', async () => {
  const bridge = makeTestBridge();
  const adapter = new DaemonAssistantAdapter({
    requestFn: async (method) => {
      if (method === 'daemon.ping') return { ok: true };
      throw new Error(`unexpected ${method}`);
    },
    watchBridge: bridge.bridge,
  });
  await adapter.connect();

  const types: string[] = [];
  const collect = (async () => {
    for await (const e of adapter.subscribe('r2', 5)) {
      types.push(e.type);
    }
  })();
  await new Promise((r) => setTimeout(r, 10));

  bridge.emit(DURABLE('r2', 6, 'started'));
  bridge.emit(LIVE('r2', 201, 'text_delta', { text: 'a' }));
  bridge.emit(DURABLE('r2', 7, 'message_completed', { message_id: 'm1' }));
  // Stream dies; adapter must reconnect by the tracked cursors.
  bridge.emit({
    frame_type: 'resync_required',
    run_id: 'r2',
    lane: 'durable',
    reason: 'stream_closed',
  });
  // Give the adapter time to tear down and restart the watch by cursor.
  await new Promise((r) => setTimeout(r, 400));

  bridge.emit(LIVE('r2', 202, 'text_delta', { text: 'b' }));
  bridge.emit(DURABLE('r2', 8, 'completed', { reason: 'ok' }));
  await collect;

  assert.ok(types.includes('text_delta'));
  assert.ok(types.includes('completed'));
  assert.equal(bridge.started.length, 2, 'restarted after stream_closed');
  const restart = bridge.started[1]!;
  assert.equal(restart.durable, 7, 'reconnect uses durable cursor');
  assert.equal(restart.live, 201, 'reconnect uses live cursor');
});

test('subscribe propagates WatchStreamUnavailableError when bridge start rejects', async () => {
  // MIG-004: the legacy `run.subscribe` long-poll fallback is retired. An
  // unavailable persistent stream must surface as an error, never silently
  // switch to a second contract.
  const methods: string[] = [];
  const adapter = new DaemonAssistantAdapter({
    requestFn: async (method) => {
      methods.push(method);
      if (method === 'daemon.ping') return { ok: true };
      throw new Error(method);
    },
    watchBridge: {
      async start() {
        return { ok: false, error: 'embedded no stream' };
      },
      async stop() {},
      listen() {
        return () => {};
      },
    },
  });
  await adapter.connect();
  await assert.rejects(
    async () => {
      for await (const _e of adapter.subscribe('r3', 0)) {
        // no events expected
      }
    },
    /run\.watch persistent stream unavailable for r3/,
  );
  assert.ok(
    !methods.includes('run.subscribe'),
    'legacy run.subscribe must not be invoked as a fallback',
  );
});

test('message_completed clears transient live state (stale live delta dropped)', async () => {
  const bridge = makeTestBridge();
  const adapter = new DaemonAssistantAdapter({
    requestFn: async (method) => {
      if (method === 'daemon.ping') return { ok: true };
      throw new Error(`unexpected ${method}`);
    },
    watchBridge: bridge.bridge,
  });
  await adapter.connect();

  const types: string[] = [];
  const collect = (async () => {
    for await (const e of adapter.subscribe('r4', 0)) {
      types.push(e.type);
    }
  })();
  await new Promise((r) => setTimeout(r, 10));

  bridge.emit(LIVE('r4', 301, 'text_delta', { text: 'hello' }));
  bridge.emit(DURABLE('r4', 1, 'message_completed', { message_id: 'm1' }));
  // A trailing live delta that belongs to the already-completed message
  // (live_sequence <= the delivered live watermark at completion) is dropped.
  bridge.emit(LIVE('r4', 301, 'text_delta', { text: 'stale' }));
  bridge.emit(DURABLE('r4', 2, 'completed', { reason: 'ok' }));
  await collect;

  const textDeltas = types.filter((t) => t === 'text_delta');
  assert.equal(textDeltas.length, 1, 'stale live delta cleared by message_completed');
  assert.ok(types.includes('completed'));
});

