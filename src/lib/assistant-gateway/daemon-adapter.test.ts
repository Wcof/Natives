/**
 * DaemonAssistantAdapter — inject requestFn (no window.nativesAPI required).
 */
import assert from 'node:assert/strict';
import test from 'node:test';
import { DaemonAssistantAdapter } from './daemon-adapter';

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

test('subscribe uses run.subscribe then yields terminal completed', async () => {
  let seqCalls = 0;
  const adapter = new DaemonAssistantAdapter({
    pollIntervalMs: 10,
    requestFn: async (method, params) => {
      if (method === 'daemon.ping') return { ok: true };
      if (method === 'run.subscribe') {
        seqCalls += 1;
        const p = params as { after_sequence?: number };
        const after = Number(p.after_sequence ?? 0);
        if (after >= 2) return { events: [] };
        return {
          events: [
            {
              run_id: 'r1',
              sequence: 1,
              timestamp: new Date().toISOString(),
              type: 'text_delta',
              payload: { text: 'hi' },
            },
            {
              run_id: 'r1',
              sequence: 2,
              timestamp: new Date().toISOString(),
              type: 'completed',
              payload: { reason: 'ok' },
            },
          ],
        };
      }
      if (method === 'run.getEvents') return [];
      throw new Error(`unexpected ${method}`);
    },
  });
  await adapter.connect();
  const types: string[] = [];
  for await (const e of adapter.subscribe('r1', 0)) {
    types.push(e.type);
  }
  assert.ok(types.includes('text_delta') || types.includes('completed'));
  assert.ok(types.includes('completed'));
  assert.ok(seqCalls >= 1);
});

test('subscribe falls back to run.getEvents when subscribe fails', async () => {
  const adapter = new DaemonAssistantAdapter({
    pollIntervalMs: 10,
    requestFn: async (method) => {
      if (method === 'daemon.ping') return { ok: true };
      if (method === 'run.subscribe') throw new Error('unsupported');
      if (method === 'run.getEvents') {
        return [
          {
            run_id: 'r2',
            sequence: 1,
            timestamp: new Date().toISOString(),
            type: 'completed',
            payload: { reason: 'ok' },
          },
        ];
      }
      throw new Error(method);
    },
  });
  await adapter.connect();
  const events = [];
  for await (const e of adapter.subscribe('r2', 0)) {
    events.push(e);
  }
  assert.equal(events.length, 1);
  assert.equal(events[0]?.type, 'completed');
});

test('subscribe recovers terminal when server marks terminal without new events', async () => {
  let subCalls = 0;
  const adapter = new DaemonAssistantAdapter({
    pollIntervalMs: 5,
    requestFn: async (method, params) => {
      if (method === 'daemon.ping') return { ok: true };
      if (method === 'run.subscribe') {
        subCalls += 1;
        const after = Number((params as { after_sequence?: number }).after_sequence ?? 0);
        if (after === 0) {
          return {
            events: [
              {
                run_id: 'r-term',
                sequence: 1,
                timestamp: new Date().toISOString(),
                type: 'text_delta',
                text: 'hi',
              },
            ],
            terminal: false,
          };
        }
        // After seq 1: server is terminal but event list after_sequence is empty
        // (the bug that caused permanent reconnect banner).
        return { events: [], terminal: true };
      }
      if (method === 'run.getEvents') {
        return [
          {
            run_id: 'r-term',
            sequence: 1,
            timestamp: new Date().toISOString(),
            type: 'text_delta',
            text: 'hi',
          },
          {
            run_id: 'r-term',
            sequence: 2,
            timestamp: new Date().toISOString(),
            type: 'completed',
            reason: 'stop',
          },
        ];
      }
      if (method === 'run.get') {
        return { id: 'r-term', status: 'completed' };
      }
      if (method === 'run.list') {
        return { runs: [{ id: 'r-term', status: 'completed' }] };
      }
      throw new Error(`unexpected ${method}`);
    },
  });
  await adapter.connect();
  const types: string[] = [];
  for await (const e of adapter.subscribe('r-term', 0)) {
    types.push(e.type);
  }
  assert.ok(types.includes('text_delta'));
  assert.ok(types.includes('completed'), `expected completed, got ${types.join(',')}`);
  assert.ok(subCalls >= 1);
});

test('subscribe rejects terminal status without an authoritative event', async () => {
  const adapter = new DaemonAssistantAdapter({
    pollIntervalMs: 5,
    requestFn: async (method) => {
      if (method === 'daemon.ping') return { ok: true };
      if (method === 'run.subscribe') {
        return { events: [], terminal: true };
      }
      if (method === 'run.getEvents') return [];
      if (method === 'run.list') {
        return {
          runs: [
            {
              id: 'r-syn',
              status: 'failed',
              error_code: 'NO_CREDENTIALS',
              error_message: 'No credentials',
            },
          ],
        };
      }
      throw new Error(method);
    },
  });
  await adapter.connect();
  await assert.rejects(
    async () => {
      for await (const _event of adapter.subscribe('r-syn', 0)) {
        // A terminal DB row is not a substitute for an authoritative event.
      }
    },
    /authoritative terminal event missing for run r-syn/,
  );
  assert.deepEqual(adapter.getProjectionRecovery('r-syn'), {
    kind: 'incomplete',
    lastSequence: 0,
    reason: 'authoritative_event_missing',
  });
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
