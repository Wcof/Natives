import assert from 'node:assert/strict';
import test from 'node:test';
import {
  cancelRun,
  resolveProjectPath,
  respondPermission,
  retryRun,
  sendOrQueue,
  subscribeRun,
} from './controller';
import { FixtureAssistantAdapter } from '../assistant-gateway/fixture-adapter';
import type { AssistantGateway } from '../assistant-gateway/gateway';
import type { AssistantMethod } from '../assistant-protocol';
import { goldenPermission, goldenTextStream } from '../assistant-fixtures/golden';
import { createInitialWorkspaceState, workspaceReducer } from './reducer';

test('resolveProjectPath prefers explicit then conversation then null without window', async () => {
  assert.equal(
    await resolveProjectPath({
      explicit: ' /proj ',
      conversationProjectId: '/other',
      tryActiveProject: false,
    }),
    '/proj',
  );
  assert.equal(
    await resolveProjectPath({
      explicit: null,
      conversationProjectId: '/from-conv',
      tryActiveProject: false,
    }),
    '/from-conv',
  );
  assert.equal(
    await resolveProjectPath({
      explicit: null,
      conversationProjectId: null,
      tryActiveProject: false,
    }),
    null,
  );
});

test('sendOrQueue throws only when no project can be resolved', async () => {
  const adapter = new FixtureAssistantAdapter({
    id: 'no-project',
    conversations: [
      {
        id: 'c-empty',
        mode: 'agent',
        title: 'No project',
        providerId: 'openai',
        modelId: 'gpt-4o',
        projectId: null,
        createdAt: 't',
        updatedAt: 't',
      },
    ],
    eventsByRun: { __next__: [] },
  });
  await adapter.connect();
  let state = createInitialWorkspaceState();
  state = workspaceReducer(state, {
    type: 'conversations/upsert',
    conversation: {
      id: 'c-empty',
      mode: 'agent',
      title: 'No project',
      providerId: 'openai',
      modelId: 'gpt-4o',
      projectId: null,
      createdAt: 't',
      updatedAt: 't',
    },
  });
  const dispatch = (a: import('./state').WorkspaceAction) => {
    state = workspaceReducer(state, a);
  };
  await assert.rejects(
    () =>
      sendOrQueue(adapter, dispatch, state, {
        conversationId: 'c-empty',
        content: 'x',
        providerId: 'openai',
        modelId: 'gpt-4o',
      }),
    /project_path is required/,
  );
});

test('sendOrQueue accepts explicit projectPath over conversation', async () => {
  const adapter = new FixtureAssistantAdapter(goldenTextStream);
  await adapter.connect();
  let state = createInitialWorkspaceState();
  const dispatch = (a: import('./state').WorkspaceAction) => {
    state = workspaceReducer(state, a);
  };
  await adapter.connect();
  // load conv with projectId from fixture list
  const list = await adapter.request<Array<{ id: string; projectId?: string }>>('conversation.list', {});
  for (const c of list) {
    state = workspaceReducer(state, {
      type: 'conversations/upsert',
      conversation: {
        id: c.id,
        mode: 'agent',
        title: 't',
        providerId: 'openai',
        modelId: 'gpt-4o',
        projectId: c.projectId ?? '/tmp/project',
        createdAt: 't',
        updatedAt: 't',
      },
    });
  }
  const result = await sendOrQueue(adapter, dispatch, state, {
    conversationId: 'conv-1',
    content: 'hi',
    providerId: 'openai',
    modelId: 'gpt-4o',
    projectPath: '/explicit/path',
  });
  assert.ok(result.runId);
});

test('sendOrQueue forwards conversation permission profile to daemon run.start', async () => {
  const seen: Record<string, unknown>[] = [];
  const adapter: AssistantGateway = {
    async connect() {},
    async disconnect() {},
    async request<T>(method: AssistantMethod, params?: unknown): Promise<T> {
      if (method === 'run.start') {
        seen.push(params as Record<string, unknown>);
        return {
          id: 'run-1',
          conversation_id: 'conv-1',
          status: 'running',
          provider_id: 'openai',
          model_id: 'gpt-4o',
          permission_profile: 'readonly',
        } as T;
      }
      return {} as T;
    },
    async *subscribe() {},
    async getSnapshot() {
      throw new Error('unused');
    },
  };
  let state = createInitialWorkspaceState();
  const dispatch = (a: import('./state').WorkspaceAction) => {
    state = workspaceReducer(state, a);
  };
  state = workspaceReducer(state, {
    type: 'conversations/upsert',
    conversation: {
      id: 'conv-1',
      mode: 'agent',
      title: 'p',
      providerId: 'openai',
      modelId: 'gpt-4o',
      projectId: '/tmp/project',
      permissionProfileId: 'readonly',
      createdAt: 't',
      updatedAt: 't',
    },
  });

  await sendOrQueue(adapter, dispatch, state, {
    conversationId: 'conv-1',
    content: 'hi',
    providerId: 'openai',
    modelId: 'gpt-4o',
    projectPath: '/tmp/project',
  });

  assert.equal(seen[0]?.permission_profile, 'readonly');
});

test('respondPermission binds run_id and request_id; cancelRun issues run.cancel', async () => {
  const adapter = new FixtureAssistantAdapter(goldenPermission);
  await adapter.connect();
  let state = createInitialWorkspaceState();
  const dispatch = (a: import('./state').WorkspaceAction) => {
    state = workspaceReducer(state, a);
  };
  state = workspaceReducer(state, {
    type: 'conversations/upsert',
    conversation: {
      id: 'conv-1',
      mode: 'agent',
      title: 'p',
      providerId: 'openai',
      modelId: 'gpt-4o',
      projectId: '/tmp/project',
      createdAt: 't',
      updatedAt: 't',
    },
  });
  const started = await sendOrQueue(adapter, dispatch, state, {
    conversationId: 'conv-1',
    content: 'write',
    providerId: 'openai',
    modelId: 'gpt-4o',
    projectPath: '/tmp/project',
  });
  assert.ok(started.runId);
  const runId = started.runId!;

  // Drain permission_requested via subscribe (goldenPermission ends at permission, not terminal)
  const drain = subscribeRun(adapter, dispatch, () => state, runId, 0);
  // Give fixture a tick to emit permission; then cancel subscribe wait via short timeout race
  await Promise.race([
    drain.catch(() => undefined),
    new Promise((r) => setTimeout(r, 50)),
  ]);
  // Apply scripted events manually if subscribe is still waiting for terminal
  const scripted = await adapter.request<
    Array<{ sequence: number; type: string; payload?: Record<string, unknown>; runId?: string; timestamp?: string }>
  >('run.getEvents', { run_id: runId, after_sequence: 0 });
  for (const e of scripted) {
    state = workspaceReducer(state, {
      type: 'event/apply',
      event: {
        runId,
        sequence: e.sequence,
        timestamp: e.timestamp ?? new Date().toISOString(),
        type: e.type,
        payload: e.payload ?? {},
      },
    });
  }
  assert.ok(
    state.interactions['perm-1'] ||
      state.runs[runId]?.status === 'waiting_permission' ||
      state.interactionOrder.includes('perm-1'),
    `expected permission interaction, status=${state.runs[runId]?.status} order=${JSON.stringify(state.interactionOrder)}`,
  );

  await respondPermission(adapter, dispatch, 'perm-1', true, 'once', runId);
  const eventsAfterAllow = await adapter.request<Array<{ type: string; payload?: Record<string, unknown> }>>(
    'run.getEvents',
    { run_id: runId, after_sequence: 0 },
  );
  assert.ok(
    eventsAfterAllow.some(
      (e) =>
        e.type === 'permission_responded' &&
        (e.payload?.permission_id === 'perm-1' || e.payload?.approved === true),
    ),
    `expected permission_responded, got ${JSON.stringify(eventsAfterAllow.map((e) => e.type))}`,
  );

  // Deny path on a second permission id
  await respondPermission(adapter, dispatch, 'perm-deny', false, 'once', runId);

  await cancelRun(adapter, dispatch, runId);
  const eventsAfterCancel = await adapter.request<Array<{ type: string }>>('run.getEvents', {
    run_id: runId,
    after_sequence: 0,
  });
  assert.ok(
    eventsAfterCancel.some((e) => e.type === 'interrupted'),
    'cancel must emit interrupted',
  );
  const runs = await adapter.request<Array<{ id: string; status: string }>>('run.list', {
    conversation_id: 'conv-1',
  });
  const cancelled = runs.find((r) => r.id === runId);
  assert.equal(cancelled?.status, 'interrupted');
});

test('duplicate sendOrQueue while busy enqueues prompt instead of second run', async () => {
  const adapter = new FixtureAssistantAdapter({
    ...goldenPermission,
    // leave run active (no completed)
  });
  await adapter.connect();
  let state = createInitialWorkspaceState();
  const dispatch = (a: import('./state').WorkspaceAction) => {
    state = workspaceReducer(state, a);
  };
  state = workspaceReducer(state, {
    type: 'conversations/upsert',
    conversation: {
      id: 'conv-1',
      mode: 'agent',
      title: 'busy',
      providerId: 'openai',
      modelId: 'gpt-4o',
      projectId: '/tmp/project',
      createdAt: 't',
      updatedAt: 't',
    },
  });
  const first = await sendOrQueue(adapter, dispatch, state, {
    conversationId: 'conv-1',
    content: 'first',
    providerId: 'openai',
    modelId: 'gpt-4o',
    projectPath: '/tmp/project',
  });
  assert.equal(first.queued, false);
  assert.ok(first.runId);
  // Mark active so second send queues
  state = workspaceReducer(state, {
    type: 'run/upsert',
    run: {
      id: first.runId!,
      conversationId: 'conv-1',
      status: 'running',
      providerId: 'openai',
      modelId: 'gpt-4o',
      permissionProfile: 'ask',
      startedAt: new Date().toISOString(),
    },
  });
  const second = await sendOrQueue(adapter, dispatch, state, {
    conversationId: 'conv-1',
    content: 'second',
    providerId: 'openai',
    modelId: 'gpt-4o',
    projectPath: '/tmp/project',
  });
  assert.equal(second.queued, true);
  assert.ok(second.promptQueueItemId);
});

test('respondPermission deny records approved=false bound to run_id', async () => {
  const adapter = new FixtureAssistantAdapter(goldenPermission);
  await adapter.connect();
  let state = createInitialWorkspaceState();
  const dispatch = (a: import('./state').WorkspaceAction) => {
    state = workspaceReducer(state, a);
  };
  state = workspaceReducer(state, {
    type: 'conversations/upsert',
    conversation: {
      id: 'conv-1',
      mode: 'agent',
      title: 'deny',
      providerId: 'openai',
      modelId: 'gpt-4o',
      projectId: '/tmp/project',
      createdAt: 't',
      updatedAt: 't',
    },
  });
  const started = await sendOrQueue(adapter, dispatch, state, {
    conversationId: 'conv-1',
    content: 'write',
    providerId: 'openai',
    modelId: 'gpt-4o',
    projectPath: '/tmp/project',
  });
  const runId = started.runId!;
  const scripted = await adapter.request<
    Array<{ sequence: number; type: string; payload?: Record<string, unknown>; timestamp?: string }>
  >('run.getEvents', { run_id: runId, after_sequence: 0 });
  for (const e of scripted) {
    state = workspaceReducer(state, {
      type: 'event/apply',
      event: {
        runId,
        sequence: e.sequence,
        timestamp: e.timestamp ?? new Date().toISOString(),
        type: e.type,
        payload: e.payload ?? {},
      },
    });
  }
  await respondPermission(adapter, dispatch, 'perm-1', false, 'once', runId);
  const events = await adapter.request<
    Array<{ type: string; payload?: Record<string, unknown> }>
  >('run.getEvents', { run_id: runId, after_sequence: 0 });
  const denied = events.find((e) => e.type === 'permission_responded');
  assert.ok(denied, 'expected permission_responded');
  assert.equal(denied!.payload?.approved, false);
  assert.equal(denied!.payload?.permission_id, 'perm-1');
});

test('respondPermission forwards this_run scope without remapping to run', async () => {
  const seen: Array<{ method: string; params?: unknown }> = [];
  const adapter: AssistantGateway = {
    async connect() {},
    async disconnect() {},
    async request(method: AssistantMethod, params?: unknown) {
      seen.push({ method, params });
      return { ok: true } as never;
    },
    async *subscribe() {},
    async getSnapshot() {
      return {
        conversation: {
          id: 'c',
          mode: 'agent',
          title: 't',
          providerId: 'p',
          modelId: 'm',
          createdAt: 't',
          updatedAt: 't',
        },
        messages: [],
        runs: [],
      };
    },
  };
  let state = createInitialWorkspaceState();
  const dispatch = (a: import('./state').WorkspaceAction) => {
    state = workspaceReducer(state, a);
  };
  await respondPermission(adapter, dispatch, 'perm-scope', true, 'this_run', 'run-9');
  const call = seen.find((s) => s.method === 'permission.respond');
  assert.ok(call, 'permission.respond must be called');
  const params = call!.params as Record<string, unknown>;
  assert.equal(params.scope, 'this_run');
  assert.equal(params.request_id, 'perm-scope');
  assert.equal(params.run_id, 'run-9');
  assert.equal(params.approved, true);
});

test('retryRun after cancel returns new or same run id via run.retry', async () => {
  const adapter = new FixtureAssistantAdapter(goldenTextStream);
  await adapter.connect();
  let state = createInitialWorkspaceState();
  const dispatch = (a: import('./state').WorkspaceAction) => {
    state = workspaceReducer(state, a);
  };
  state = workspaceReducer(state, {
    type: 'conversations/upsert',
    conversation: {
      id: 'conv-1',
      mode: 'agent',
      title: 'retry',
      providerId: 'openai',
      modelId: 'gpt-4o',
      projectId: '/tmp/project',
      createdAt: 't',
      updatedAt: 't',
    },
  });
  const started = await sendOrQueue(adapter, dispatch, state, {
    conversationId: 'conv-1',
    content: 'hi',
    providerId: 'openai',
    modelId: 'gpt-4o',
    projectPath: '/tmp/project',
  });
  const runId = started.runId!;
  await cancelRun(adapter, dispatch, runId);
  const retriedId = await retryRun(adapter, dispatch, runId);
  assert.ok(retriedId);
  // Fixture creates a new run on retry
  assert.notEqual(retriedId, '');
  const list = await adapter.request<Array<{ id: string }>>('run.list', {
    conversation_id: 'conv-1',
  });
  assert.ok(list.some((r) => r.id === retriedId));
});

test('project path missing and symlink-escape style path still require explicit path (no cwd invent)', async () => {
  const adapter = new FixtureAssistantAdapter(goldenTextStream);
  await adapter.connect();
  let state = createInitialWorkspaceState();
  const dispatch = (a: import('./state').WorkspaceAction) => {
    state = workspaceReducer(state, a);
  };
  state = workspaceReducer(state, {
    type: 'conversations/upsert',
    conversation: {
      id: 'conv-1',
      mode: 'agent',
      title: 'escape',
      providerId: 'openai',
      modelId: 'gpt-4o',
      projectId: null,
      createdAt: 't',
      updatedAt: 't',
    },
  });
  await assert.rejects(
    () =>
      sendOrQueue(adapter, dispatch, state, {
        conversationId: 'conv-1',
        content: 'x',
        providerId: 'openai',
        modelId: 'gpt-4o',
        projectPath: '   ',
      }),
    /project_path is required/,
  );
  // Explicit path is forwarded even if it looks like a traversal attempt —
  // sandbox reject is daemon PathScope responsibility, not silent cwd fallback.
  const started = await sendOrQueue(adapter, dispatch, state, {
    conversationId: 'conv-1',
    content: 'x',
    providerId: 'openai',
    modelId: 'gpt-4o',
    projectPath: '/tmp/project/../project',
  });
  assert.ok(started.runId);
});

test('run.getEvents replay after sequence gap returns later events only', async () => {
  const adapter = new FixtureAssistantAdapter(goldenTextStream);
  await adapter.connect();
  let state = createInitialWorkspaceState();
  const dispatch = (a: import('./state').WorkspaceAction) => {
    state = workspaceReducer(state, a);
  };
  state = workspaceReducer(state, {
    type: 'conversations/upsert',
    conversation: {
      id: 'conv-1',
      mode: 'agent',
      title: 'replay',
      providerId: 'openai',
      modelId: 'gpt-4o',
      projectId: '/tmp/project',
      createdAt: 't',
      updatedAt: 't',
    },
  });
  const started = await sendOrQueue(adapter, dispatch, state, {
    conversationId: 'conv-1',
    content: 'hi',
    providerId: 'openai',
    modelId: 'gpt-4o',
    projectPath: '/tmp/project',
  });
  const runId = started.runId!;
  const all = await adapter.request<Array<{ sequence: number; type: string }>>(
    'run.getEvents',
    { run_id: runId, after_sequence: 0 },
  );
  assert.ok(all.length >= 2);
  const mid = all[0]!.sequence;
  const tail = await adapter.request<Array<{ sequence: number }>>('run.getEvents', {
    run_id: runId,
    after_sequence: mid,
  });
  assert.ok(tail.every((e) => e.sequence > mid));
  assert.equal(tail.length, all.length - 1);
});
