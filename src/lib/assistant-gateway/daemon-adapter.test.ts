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
