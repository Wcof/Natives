import assert from 'node:assert/strict';
import test from 'node:test';
import { FixtureAssistantAdapter } from './fixture-adapter';
import {
  createInitialWorkspaceState,
  workspaceReducer,
  selectConversationMessages,
  selectPendingInteractions,
  selectPromptQueue,
  selectArtifacts,
  selectChildRuns,
  selectRunEvents,
} from '@/lib/assistant-workspace';
import {
  goldenTextStream,
  goldenTool,
  goldenPermission,
  goldenAskUser,
  goldenSubagent,
  goldenArtifact,
  goldenProviderError,
  goldenPromptQueue,
  goldenReasoning,
} from '@/lib/assistant-fixtures/golden';

async function drainSubscribe(
  adapter: FixtureAssistantAdapter,
  runId: string,
  after = 0,
): Promise<import('@/lib/assistant-protocol').RunEvent[]> {
  const out: import('@/lib/assistant-protocol').RunEvent[] = [];
  for await (const e of adapter.subscribe(runId, after)) {
    out.push(e);
  }
  return out;
}

test('fixture text stream drives reducer to completed without duplicate messages', async () => {
  const adapter = new FixtureAssistantAdapter(goldenTextStream);
  await adapter.connect();
  const run = await adapter.request<{ id: string; conversationId: string }>('run.start', {
    conversation_id: 'conv-1',
    content: 'hi',
    provider_id: 'openai',
    model_id: 'gpt-4o',
  });
  const events = await drainSubscribe(adapter, run.id, 0);
  let state = createInitialWorkspaceState();
  state = workspaceReducer(state, {
    type: 'run/upsert',
    run: {
      id: run.id,
      conversationId: 'conv-1',
      status: 'running',
      providerId: 'openai',
      modelId: 'gpt-4o',
      permissionProfile: 'ask',
    },
  });
  state = workspaceReducer(state, { type: 'event/applyBatch', events });
  assert.equal(state.runs[run.id]!.status, 'completed');
  const text = state.messages[`live-${run.id}`]?.contentBlocks.find((b) => b.type === 'text')?.text
    ?? selectConversationMessages(state, 'conv-1').flatMap((m) => m.contentBlocks).find((b) => b.type === 'text')?.text;
  assert.equal(text, 'Hello world');
  const assistantMsgs = selectConversationMessages(state, 'conv-1').filter((m) => m.role === 'assistant');
  assert.equal(assistantMsgs.length, 1);
});

test('fixture tool in-place update', async () => {
  const adapter = new FixtureAssistantAdapter(goldenTool);
  await adapter.connect();
  const run = await adapter.request<{ id: string }>('run.start', {
    conversation_id: 'conv-1',
    content: 'read',
  });
  const events = await drainSubscribe(adapter, run.id, 0);
  let state = withRun(run.id);
  // Step through the stream: requested/started/completed must patch one card
  // rather than append a new block per event.
  const statuses: Array<string | undefined> = [];
  for (const event of events) {
    state = workspaceReducer(state, { type: 'event/apply', event });
    const live = (state.liveByRun[run.id]?.blocks ?? []).filter((b) => b.type === 'tool_call');
    if (live.length > 0) {
      assert.equal(live.length, 1);
      statuses.push(live[0]!.toolStatus);
    }
  }
  assert.deepEqual([...new Set(statuses)], ['pending', 'running', 'completed']);

  // Terminal promotion keeps the answer body text-only; the finished tool card
  // is rendered from the retained run events (activity panel), not the message.
  const body = state.messages[`live-${run.id}`]?.contentBlocks ?? [];
  assert.equal(body.some((b) => b.type === 'tool_call'), false);
  assert.equal(body.find((b) => b.type === 'text')?.text, 'Done');
  const toolEvents = selectRunEvents(state, run.id).filter((e) =>
    e.type.startsWith('tool_call_'),
  );
  assert.deepEqual(toolEvents.map((e) => e.type), [
    'tool_call_requested',
    'tool_call_started',
    'tool_call_completed',
  ]);
});

function withRun(runId: string) {
  return workspaceReducer(createInitialWorkspaceState(), {
    type: 'run/upsert',
    run: {
      id: runId,
      conversationId: 'conv-1',
      status: 'running',
      providerId: 'openai',
      modelId: 'gpt-4o',
      permissionProfile: 'ask',
    },
  });
}

test('fixture permission allow/deny path', async () => {
  const adapter = new FixtureAssistantAdapter(goldenPermission);
  await adapter.connect();
  const run = await adapter.request<{ id: string }>('run.start', {
    conversation_id: 'conv-1',
    content: 'write',
  });
  const events = await drainSubscribe(adapter, run.id, 0);
  let state = withRun(run.id);
  state = workspaceReducer(state, { type: 'event/applyBatch', events });
  const pending = selectPendingInteractions(state, 'conv-1');
  assert.equal(pending.length, 1);
  assert.equal(pending[0]!.kind, 'permission');

  await adapter.request('permission.respond', {
    request_id: 'perm-1',
    approved: true,
    scope: 'once',
    run_id: run.id,
  });
  const more = await adapter.request<import('@/lib/assistant-protocol').RunEvent[]>('run.getEvents', {
    run_id: run.id,
    after_sequence: events[events.length - 1]!.sequence,
  });
  state = workspaceReducer(state, { type: 'event/applyBatch', events: more });
  assert.equal(selectPendingInteractions(state, 'conv-1').length, 0);
});

test('fixture ask user interaction', async () => {
  const adapter = new FixtureAssistantAdapter(goldenAskUser);
  await adapter.connect();
  const run = await adapter.request<{ id: string }>('run.start', {
    conversation_id: 'conv-1',
    content: 'q',
  });
  const events = await drainSubscribe(adapter, run.id, 0);
  let state = withRun(run.id);
  state = workspaceReducer(state, { type: 'event/applyBatch', events });
  assert.equal(selectPendingInteractions(state)[0]!.kind, 'ask_user');
});

test('fixture subagent + artifact', async () => {
  const adapter = new FixtureAssistantAdapter(goldenSubagent);
  await adapter.connect();
  const run = await adapter.request<{ id: string }>('run.start', {
    conversation_id: 'conv-1',
    content: 'sub',
  });
  let state = withRun(run.id);
  state = workspaceReducer(state, {
    type: 'event/applyBatch',
    events: await drainSubscribe(adapter, run.id, 0),
  });
  assert.equal(selectChildRuns(state, run.id).length, 1);
  assert.equal(selectChildRuns(state, run.id)[0]!.status, 'completed');

  const artAdapter = new FixtureAssistantAdapter(goldenArtifact);
  await artAdapter.connect();
  const run2 = await artAdapter.request<{ id: string }>('run.start', {
    conversation_id: 'conv-1',
    content: 'art',
  });
  let state2 = withRun(run2.id);
  state2 = workspaceReducer(state2, {
    type: 'event/applyBatch',
    events: await drainSubscribe(artAdapter, run2.id, 0),
  });
  assert.equal(selectArtifacts(state2, run2.id).length, 1);
  assert.equal(selectArtifacts(state2, run2.id)[0]!.path, '/tmp/out.md');
});

test('fixture prompt queue enqueue edit reorder sendNow reassociate', async () => {
  const adapter = new FixtureAssistantAdapter(goldenPromptQueue);
  await adapter.connect();
  const item = await adapter.request<{ id: string; content: string }>('promptQueue.enqueue', {
    conversation_id: 'conv-1',
    content: 'queued-1',
    client_temp_id: 'tmp-a',
  });
  assert.ok(item.id);
  await adapter.request('promptQueue.enqueue', {
    conversation_id: 'conv-1',
    content: 'queued-2',
  });
  let list = await adapter.request<Array<{ id: string; content: string }>>('promptQueue.list', {
    conversation_id: 'conv-1',
  });
  assert.equal(list.length, 2);
  await adapter.request('promptQueue.update', {
    id: item.id,
    conversation_id: 'conv-1',
    content: 'queued-1-edited',
  });
  await adapter.request('promptQueue.reorder', {
    conversation_id: 'conv-1',
    ids: [list[1]!.id, list[0]!.id],
  });
  await adapter.request('promptQueue.sendNow', { id: item.id });
  list = await adapter.request('promptQueue.list', { conversation_id: 'conv-1' });
  assert.ok(list.some((i) => i.content === 'queued-1-edited'));

  let state = createInitialWorkspaceState();
  state = workspaceReducer(state, {
    type: 'promptQueue/optimisticEnqueue',
    item: {
      id: 'tmp-a',
      clientTempId: 'tmp-a',
      conversationId: 'conv-1',
      content: 'queued-1',
      source: 'user',
      createdAt: 't',
      order: 0,
    },
  });
  state = workspaceReducer(state, {
    type: 'promptQueue/reassociate',
    conversationId: 'conv-1',
    clientTempId: 'tmp-a',
    serverItem: {
      id: item.id,
      conversationId: 'conv-1',
      content: 'queued-1-edited',
      source: 'user',
      createdAt: 't',
      order: 0,
    },
  });
  assert.equal(selectPromptQueue(state, 'conv-1')[0]!.id, item.id);
});

test('fixture provider error on start', async () => {
  const adapter = new FixtureAssistantAdapter(goldenProviderError);
  await adapter.connect();
  await assert.rejects(
    () => adapter.request('run.start', { conversation_id: 'conv-1', content: 'x' }),
    /Provider/,
  );
});

test('fixture reasoning then text', async () => {
  const adapter = new FixtureAssistantAdapter(goldenReasoning);
  await adapter.connect();
  const run = await adapter.request<{ id: string }>('run.start', {
    conversation_id: 'conv-1',
    content: 'think',
  });
  let state = withRun(run.id);
  state = workspaceReducer(state, {
    type: 'event/applyBatch',
    events: await drainSubscribe(adapter, run.id, 0),
  });
  const blocks =
    state.messages[`live-${run.id}`]?.contentBlocks ?? [];
  assert.ok(blocks.some((b) => b.type === 'reasoning'));
  assert.ok(blocks.some((b) => b.type === 'text' && b.text === 'Answer'));
});

test('reconnect: soft disconnect keeps content; replay continues', async () => {
  const adapter = new FixtureAssistantAdapter({
    id: 'reconnect-manual',
    conversations: goldenTextStream.conversations,
    eventsByRun: {
      __next__: [
        {
          runId: 'x',
          sequence: 1,
          timestamp: 't',
          type: 'started',
          payload: {},
        },
        {
          runId: 'x',
          sequence: 2,
          timestamp: 't',
          type: 'text_delta',
          payload: { text: 'part1' },
        },
        {
          runId: 'x',
          sequence: 3,
          timestamp: 't',
          type: 'text_delta',
          payload: { text: 'part2' },
        },
        {
          runId: 'x',
          sequence: 4,
          timestamp: 't',
          type: 'completed',
          payload: { reason: 'ok' },
        },
      ],
    },
    disconnectAfterEvents: 2,
  });
  await adapter.connect();
  const run = await adapter.request<{ id: string }>('run.start', {
    conversation_id: 'conv-1',
    content: 'go',
  });
  let state = withRun(run.id);
  const received: import('@/lib/assistant-protocol').RunEvent[] = [];
  try {
    for await (const e of adapter.subscribe(run.id, 0)) {
      received.push(e);
      state = workspaceReducer(state, { type: 'event/apply', event: e });
    }
  } catch {
    state = workspaceReducer(state, { type: 'disconnect/soft' });
  }
  assert.equal(state.connection, 'offline');
  assert.ok(state.liveByRun[run.id]?.blocks[0]?.text?.includes('part1'));

  await adapter.connect();
  const replay = await adapter.request<import('@/lib/assistant-protocol').RunEvent[]>('run.getEvents', {
    run_id: run.id,
    after_sequence: state.lastSequenceByRun[run.id] ?? 0,
  });
  state = workspaceReducer(state, {
    type: 'connection/set',
    connection: 'recovering',
  });
  state = workspaceReducer(state, { type: 'event/replay', runId: run.id, events: replay });
  assert.equal(state.runs[run.id]!.status, 'completed');
});

test('sequence gap via skipSequences then replay', async () => {
  // Build adapter where first subscribe skips seq 2
  const adapter = new FixtureAssistantAdapter({
    id: 'gap',
    conversations: goldenTextStream.conversations,
    eventsByRun: {
      'fixed-run': [
        { runId: 'fixed-run', sequence: 1, timestamp: 't', type: 'started', payload: {} },
        { runId: 'fixed-run', sequence: 2, timestamp: 't', type: 'text_delta', payload: { text: 'mid' } },
        { runId: 'fixed-run', sequence: 3, timestamp: 't', type: 'text_delta', payload: { text: 'end' } },
        { runId: 'fixed-run', sequence: 4, timestamp: 't', type: 'completed', payload: { reason: 'ok' } },
      ],
    },
    skipSequences: { 'fixed-run': [2] },
  });
  await adapter.connect();
  // manually put run
  (adapter as unknown as { runs: Record<string, unknown> }).runs['fixed-run'] = {
    id: 'fixed-run',
    conversationId: 'conv-1',
    status: 'running',
    providerId: 'openai',
    modelId: 'gpt-4o',
    permissionProfile: 'ask',
  };

  let state = withRun('fixed-run');
  const liveEvents: import('@/lib/assistant-protocol').RunEvent[] = [];
  for await (const e of adapter.subscribe('fixed-run', 0)) {
    liveEvents.push(e);
    state = workspaceReducer(state, { type: 'event/apply', event: e });
  }
  // After seq1, seq3 causes gap
  assert.ok(state.recoveringRuns['fixed-run'] || state.lastSequenceByRun['fixed-run'] === 1);

  const missing = await adapter.replay('fixed-run', state.lastSequenceByRun['fixed-run'] ?? 0);
  state = workspaceReducer(state, {
    type: 'event/replay',
    runId: 'fixed-run',
    events: missing,
  });
  assert.equal(state.runs['fixed-run']!.status, 'completed');
  assert.ok(!state.recoveringRuns['fixed-run']);
});
