import assert from 'node:assert/strict';
import test from 'node:test';
import {
  createInitialWorkspaceState,
  workspaceReducer,
  type AssistantWorkspaceState,
} from './index';
import type { RunEvent } from '@/lib/assistant-protocol';
import {
  selectConversationMessages,
  selectPendingInteractions,
  selectPromptQueue,
} from './selectors';

function ev(
  runId: string,
  sequence: number,
  type: string,
  payload: Record<string, unknown> = {},
): RunEvent {
  return {
    runId,
    sequence,
    timestamp: `2026-07-17T12:00:0${sequence}.000Z`,
    type,
    payload,
  };
}

function withRun(state: AssistantWorkspaceState, runId = 'r1', conversationId = 'c1') {
  return workspaceReducer(state, {
    type: 'run/upsert',
    run: {
      id: runId,
      conversationId,
      status: 'running',
      providerId: 'openai',
      modelId: 'gpt-4o',
      permissionProfile: 'ask',
      startedAt: '2026-07-17T12:00:00.000Z',
    },
  });
}

test('applies events in order and reaches terminal only from completed event', () => {
  let state = withRun(createInitialWorkspaceState());
  state = workspaceReducer(state, { type: 'event/apply', event: ev('r1', 1, 'started') });
  assert.equal(state.runs.r1!.status, 'running');
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 2, 'text_delta', { text: 'Hi' }),
  });
  assert.equal(state.liveByRun.r1!.blocks[0]!.text, 'Hi');
  // GUI must not invent completed
  assert.notEqual(state.runs.r1!.status, 'completed');
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 3, 'completed', { reason: 'ok' }),
  });
  assert.equal(state.runs.r1!.status, 'completed');
  assert.ok(!state.liveByRun.r1, 'live bubble promoted on terminal');
});

test('ignores duplicate and out-of-order sequences (idempotent)', () => {
  let state = withRun(createInitialWorkspaceState());
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 1, 'text_delta', { text: 'A' }),
  });
  const afterFirst = state;
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 1, 'text_delta', { text: 'DUP' }),
  });
  assert.equal(state.liveByRun.r1!.blocks[0]!.text, 'A');
  assert.equal(state.eventsByRun.r1!.length, 1);
  // out of order older
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 0, 'text_delta', { text: 'X' }),
  });
  assert.equal(state.lastSequenceByRun.r1, 1);
  assert.equal(afterFirst.liveByRun.r1!.blocks[0]!.text, 'A');
});

test('sequence gap enters recovering and replay fills without duplicates', () => {
  let state = withRun(createInitialWorkspaceState());
  state = workspaceReducer(state, { type: 'connection/set', connection: 'connected', error: null });
  state = workspaceReducer(state, { type: 'event/apply', event: ev('r1', 1, 'started') });
  // gap: jump to 3
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 3, 'text_delta', { text: 'C' }),
  });
  assert.equal(state.recoveringRuns.r1, true);
  // Global connection must stay connected (run-level recovering only).
  assert.equal(state.connection, 'connected');
  // live last still 1
  assert.equal(state.lastSequenceByRun.r1, 1);

  state = workspaceReducer(state, {
    type: 'event/replay',
    runId: 'r1',
    events: [
      ev('r1', 2, 'text_delta', { text: 'B' }),
      ev('r1', 3, 'text_delta', { text: 'C' }),
      ev('r1', 4, 'completed', { reason: 'ok' }),
    ],
  });
  assert.ok(!state.recoveringRuns.r1);
  assert.equal(state.connection, 'connected');
  assert.equal(state.lastSequenceByRun.r1, 4);
  assert.equal(state.runs.r1!.status, 'completed');
  // no duplicate seq 3
  const seqs = state.eventsByRun.r1!.map((e) => e.sequence);
  assert.deepEqual(seqs, [1, 2, 3, 4]);
});

test('retains a bounded event window for long streams', () => {
  let state = withRun(createInitialWorkspaceState());
  for (let sequence = 1; sequence <= 10_000; sequence += 1) {
    state = workspaceReducer(state, {
      type: 'event/apply',
      event: ev('r1', sequence, 'text_delta', { text: 'x' }),
    });
  }
  assert.equal(state.eventsByRun.r1!.length, 2_000);
  assert.equal(state.eventsByRun.r1![0]!.sequence, 8_001);
  assert.equal(state.lastSequenceByRun.r1, 10_000);
});

test('tool call updates in place (no duplicate tool blocks)', () => {
  let state = withRun(createInitialWorkspaceState());
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 1, 'tool_call_requested', {
      id: 't1',
      name: 'read_file',
      input: { path: 'a' },
    }),
  });
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 2, 'tool_call_started', { id: 't1', name: 'read_file' }),
  });
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 3, 'tool_call_completed', {
      id: 't1',
      name: 'read_file',
      output: 'ok',
      is_error: false,
      duration_ms: 5,
    }),
  });
  const tools = state.liveByRun.r1!.blocks.filter((b) => b.type === 'tool_call');
  assert.equal(tools.length, 1);
  assert.equal(tools[0]!.toolStatus, 'completed');
  assert.equal(tools[0]!.toolOutput, 'ok');
});

test('tool_call_delta streams args into one card by id', () => {
  let state = withRun(createInitialWorkspaceState());
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 1, 'tool_call_delta', {
      id: 't1',
      name: 'list_dir',
      index: 0,
      arguments_delta: '{"path":',
    }),
  });
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 2, 'tool_call_delta', {
      id: 't1',
      index: 0,
      arguments_delta: '"."}',
    }),
  });
  const tools = state.liveByRun.r1!.blocks.filter((b) => b.type === 'tool_call');
  assert.equal(tools.length, 1);
  assert.equal(tools[0]!.toolStatus, 'running');
  assert.equal(tools[0]!.toolPartialArgs, '{"path":"."}');
  assert.deepEqual(tools[0]!.toolInput, { path: '.' });
});

test('permission queue binds to run and survives conversation switch', () => {
  let state = withRun(createInitialWorkspaceState(), 'r1', 'c1');
  state = workspaceReducer(state, {
    type: 'conversations/upsert',
    conversation: {
      id: 'c1',
      mode: 'agent',
      title: 'One',
      providerId: 'openai',
      modelId: 'gpt-4o',
      createdAt: '',
      updatedAt: '',
    },
  });
  state = workspaceReducer(state, {
    type: 'conversations/upsert',
    conversation: {
      id: 'c2',
      mode: 'agent',
      title: 'Two',
      providerId: 'openai',
      modelId: 'gpt-4o',
      createdAt: '',
      updatedAt: '',
    },
  });
  state = workspaceReducer(state, { type: 'conversations/setActive', id: 'c1' });
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 1, 'permission_requested', {
      permission_id: 'p1',
      tool_call_id: 'tc',
      tool_name: 'Write',
      reason: 'need',
      input: {},
    }),
  });
  assert.equal(state.runs.r1!.status, 'waiting_permission');
  assert.equal(selectPendingInteractions(state, 'c1').length, 1);

  // switch away — permission still queued
  state = workspaceReducer(state, { type: 'conversations/setActive', id: 'c2' });
  assert.equal(selectPendingInteractions(state, 'c1').length, 1);
  assert.equal(selectPendingInteractions(state).length, 1);
  assert.ok(state.runs.r1);
});

test('child subagent run tracked under parent', () => {
  let state = withRun(createInitialWorkspaceState());
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 1, 'subagent_created', {
      sub_run_id: 'sub1',
      task: 'Explore',
      agent_profile_id: 'explore',
    }),
  });
  assert.deepEqual(state.childRunsByParent.r1, ['sub1']);
  assert.equal(state.childSummaries.sub1!.task, 'Explore');
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 2, 'subagent_completed', { sub_run_id: 'sub1', result: 'ok' }),
  });
  assert.equal(state.childSummaries.sub1!.status, 'completed');
});

test('prompt queue optimistic enqueue and reassociate server id', () => {
  let state = createInitialWorkspaceState();
  state = workspaceReducer(state, {
    type: 'promptQueue/optimisticEnqueue',
    item: {
      id: 'temp-1',
      conversationId: 'c1',
      content: 'next prompt',
      source: 'user',
      createdAt: 't',
      order: 0,
      clientTempId: 'temp-1',
    },
  });
  assert.equal(selectPromptQueue(state, 'c1').length, 1);
  // double same temp id ignored
  state = workspaceReducer(state, {
    type: 'promptQueue/optimisticEnqueue',
    item: {
      id: 'temp-1',
      conversationId: 'c1',
      content: 'next prompt',
      source: 'user',
      createdAt: 't',
      order: 0,
      clientTempId: 'temp-1',
    },
  });
  assert.equal(selectPromptQueue(state, 'c1').length, 1);

  state = workspaceReducer(state, {
    type: 'promptQueue/reassociate',
    conversationId: 'c1',
    clientTempId: 'temp-1',
    serverItem: {
      id: 'server-9',
      conversationId: 'c1',
      content: 'next prompt',
      source: 'user',
      createdAt: 't',
      order: 0,
    },
  });
  const q = selectPromptQueue(state, 'c1');
  assert.equal(q.length, 1);
  assert.equal(q[0]!.id, 'server-9');
  assert.equal(q[0]!.clientTempId, undefined);
});

test('disconnect soft keeps messages and drafts', () => {
  let state = withRun(createInitialWorkspaceState());
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 1, 'text_delta', { text: 'keep me' }),
  });
  state = workspaceReducer(state, {
    type: 'composer/set',
    conversationId: 'c1',
    draft: { text: 'draft text' },
  });
  state = workspaceReducer(state, { type: 'disconnect/soft' });
  assert.equal(state.connection, 'offline');
  assert.equal(state.liveByRun.r1!.blocks[0]!.text, 'keep me');
  assert.equal(state.composerByConversation.c1!.text, 'draft text');
});

test('generation attempt discard rolls back partial live text', () => {
  let state = withRun(createInitialWorkspaceState());
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 1, 'generation_attempt_started', { attempt: 1 }),
  });
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 2, 'text_delta', { text: 'bad partial' }),
  });
  assert.equal(state.liveByRun.r1!.blocks[0]!.text, 'bad partial');
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 3, 'generation_attempt_discarded', { attempt: 1, reason: 'http_503' }),
  });
  assert.equal(state.liveByRun.r1!.blocks.length, 0);
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 4, 'generation_attempt_started', { attempt: 2 }),
  });
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 5, 'text_delta', { text: 'good' }),
  });
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 6, 'generation_attempt_committed', { attempt: 2 }),
  });
  assert.equal(state.liveByRun.r1!.blocks[0]!.text, 'good');
});

test('generation attempt discard preserves reasoning context', () => {
  let state = withRun(createInitialWorkspaceState());
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 1, 'generation_attempt_started', { attempt: 1 }),
  });
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 2, 'reasoning_delta', { text: 'diagnose first path' }),
  });
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 3, 'text_delta', { text: 'partial' }),
  });
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 4, 'generation_attempt_discarded', { attempt: 1, reason: 'http_503' }),
  });
  const reasoning = state.liveByRun.r1!.blocks.find((b) => b.type === 'reasoning');
  assert.equal(reasoning?.reasoning, 'diagnose first path');
  assert.equal(reasoning?.live, false);

  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 5, 'generation_attempt_started', { attempt: 2 }),
  });
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 6, 'reasoning_delta', { text: ' retry path' }),
  });
  const retriedReasoning = state.liveByRun.r1!.blocks.find((b) => b.type === 'reasoning');
  assert.equal(retriedReasoning?.reasoning, 'diagnose first path retry path');
  assert.equal(retriedReasoning?.live, true);
});

test('draft isolation per conversation', () => {
  let state = createInitialWorkspaceState();
  state = workspaceReducer(state, {
    type: 'composer/set',
    conversationId: 'a',
    draft: { text: 'AAA' },
  });
  state = workspaceReducer(state, {
    type: 'composer/set',
    conversationId: 'b',
    draft: { text: 'BBB' },
  });
  assert.equal(state.composerByConversation.a!.text, 'AAA');
  assert.equal(state.composerByConversation.b!.text, 'BBB');
  state = workspaceReducer(state, { type: 'conversations/setActive', id: 'b' });
  assert.equal(state.composerByConversation.a!.text, 'AAA');
});

test('reasoning then text freezes reasoning and tool+terminal', () => {
  let state = withRun(createInitialWorkspaceState());
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 1, 'reasoning_delta', { text: 'plan' }),
  });
  assert.equal(state.runs.r1!.status, 'reasoning');
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 2, 'text_delta', { text: 'answer' }),
  });
  assert.ok(state.liveByRun.r1!.reasoningFinishedAt);
  assert.equal(state.runs.r1!.status, 'running');
  const msgs = selectConversationMessages(state, 'c1');
  assert.ok(msgs.some((m) => m.role === 'assistant'));
});

test('queued event does not regress preparing/running status', () => {
  let state = withRun(createInitialWorkspaceState());
  state = workspaceReducer(state, {
    type: 'run/upsert',
    run: {
      ...state.runs.r1!,
      status: 'preparing',
    },
  });
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 1, 'queued', {}),
  });
  assert.equal(state.runs.r1!.status, 'preparing');
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 2, 'started', {}),
  });
  assert.equal(state.runs.r1!.status, 'running');
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 3, 'queued', {}),
  });
  assert.equal(state.runs.r1!.status, 'running');
});

test('failed terminal from event only', () => {
  let state = withRun(createInitialWorkspaceState());
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 1, 'failed', { error: 'boom', code: 'E1' }),
  });
  assert.equal(state.runs.r1!.status, 'failed');
  assert.equal(state.runs.r1!.errorCode, 'E1');
});

test('snapshot apply rebuilds messages without wiping other conversations', () => {
  let state = createInitialWorkspaceState();
  state = workspaceReducer(state, {
    type: 'messages/appendOptimistic',
    message: {
      id: 'other-msg',
      conversationId: 'other',
      role: 'user',
      status: 'complete',
      createdAt: 't',
      contentBlocks: [{ type: 'text', text: 'other' }],
    },
  });
  state = workspaceReducer(state, {
    type: 'snapshot/apply',
    snapshot: {
      conversation: {
        id: 'c1',
        mode: 'agent',
        title: 'Snap',
        providerId: 'openai',
        modelId: 'gpt-4o',
        createdAt: 't',
        updatedAt: 't',
      },
      messages: [
        {
          id: 'm1',
          conversationId: 'c1',
          role: 'user',
          status: 'complete',
          createdAt: 't',
          contentBlocks: [{ type: 'text', text: 'hello' }],
        },
      ],
      runs: [],
    },
  });
  assert.equal(state.messages['other-msg']!.contentBlocks[0]!.text, 'other');
  assert.equal(state.messages.m1!.contentBlocks[0]!.text, 'hello');
});

// ─── F1 regression: G1/G2/G3/G6/G9/P0-5/P0-13/G10 ────────

test('interaction_responded removes pending interaction (engine wire name)', () => {
  let state = withRun(createInitialWorkspaceState());
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 1, 'interaction_requested', {
      interaction_id: 'i1',
      id: 'i1',
      kind: 'ask_user',
      prompt: 'Pick one',
    }),
  });
  assert.equal(state.runs.r1!.status, 'waiting_user');
  assert.equal(selectPendingInteractions(state, 'c1').length, 1);

  // Engine emits interaction_responded (RunEventKind::InteractionResponded),
  // not interaction_resolved — the card must still disappear.
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 2, 'interaction_responded', {
      interaction_id: 'i1',
      response: { option: 'a' },
    }),
  });
  assert.equal(selectPendingInteractions(state, 'c1').length, 0);
  assert.deepEqual(state.interactionOrder, []);
  assert.equal(state.runs.r1!.status, 'running');
});

test('interaction_resolved still removes pending interaction (legacy name)', () => {
  let state = withRun(createInitialWorkspaceState());
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 1, 'interaction_requested', { id: 'i1', kind: 'ask_user', prompt: 'q' }),
  });
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 2, 'interaction_resolved', { id: 'i1' }),
  });
  assert.equal(selectPendingInteractions(state, 'c1').length, 0);
});

test('terminal run event clears its pending interactions (no zombie cards)', () => {
  let state = withRun(createInitialWorkspaceState());
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 1, 'permission_requested', {
      permission_id: 'p1',
      tool_call_id: 'tc',
      tool_name: 'Write',
      reason: 'need',
      input: {},
    }),
  });
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 2, 'interaction_requested', { id: 'i1', kind: 'ask_user', prompt: 'q' }),
  });
  assert.equal(state.interactionOrder.length, 2);

  // Another run's interaction must survive r1's terminal event.
  state = withRun(state, 'r2', 'c2');
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r2', 1, 'interaction_requested', { id: 'i-other', kind: 'ask_user', prompt: 'x' }),
  });

  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 3, 'cancelled', { reason: 'user' }),
  });
  assert.equal(state.runs.r1!.status, 'cancelled');
  assert.deepEqual(state.interactionOrder, ['i-other']);
  assert.equal(state.interactions.p1, undefined);
  assert.equal(state.interactions.i1, undefined);
  assert.ok(state.interactions['i-other']);
});

test('context_compressed produces a structured compaction block that persists', () => {
  let state = withRun(createInitialWorkspaceState());
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 1, 'text_delta', { text: 'before' }),
  });
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 2, 'context_compressed', {
      before_tokens: 12000,
      after_tokens: 4000,
      summary: 'dropped old tool outputs',
    }),
  });
  const live = state.liveByRun.r1!.blocks.find((b) => b.type === 'compaction');
  assert.ok(live, 'compaction block appended to live bubble');
  assert.equal(live!.beforeTokens, 12000);
  assert.equal(live!.afterTokens, 4000);
  assert.equal(live!.summary, 'dropped old tool outputs');

  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 3, 'completed', { reason: 'ok' }),
  });
  const msg = Object.values(state.messages).find((m) => m.runId === 'r1');
  assert.ok(msg);
  assert.ok(
    msg!.contentBlocks.some((b) => b.type === 'compaction' && b.beforeTokens === 12000),
    'compaction survives promotion to message',
  );
});

test('generation_attempt_failed(retrying) surfaces a retry notice block', () => {
  let state = withRun(createInitialWorkspaceState());
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 1, 'generation_attempt_failed', {
      attempt: 1,
      code: 'HTTP_503',
      retryable: true,
      retrying: true,
    }),
  });
  const notice = state.liveByRun.r1!.blocks.find(
    (b) => b.type === 'system_notice' && b.noticeKind === 'generation_retry',
  );
  assert.ok(notice, 'retry notice block appended');
  assert.equal(notice!.noticeData!.attempt, 1);
  assert.equal(notice!.noticeData!.code, 'HTTP_503');
  assert.equal(notice!.noticeData!.retrying, true);
});

test('generation_attempt_failed(final) does not add a notice (failed event owns it)', () => {
  let state = withRun(createInitialWorkspaceState());
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 1, 'generation_attempt_failed', {
      attempt: 3,
      code: 'HTTP_503',
      retryable: true,
      retrying: false,
    }),
  });
  const notices = state.liveByRun.r1?.blocks.filter((b) => b.type === 'system_notice') ?? [];
  assert.equal(notices.length, 0);
});

test('text -> tool -> text keeps two text blocks in stream order (no gluing)', () => {
  let state = withRun(createInitialWorkspaceState());
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 1, 'text_delta', { text: 'explain A' }),
  });
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 2, 'tool_call_started', { id: 't1', name: 'read_file' }),
  });
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 3, 'tool_call_completed', { id: 't1', output: 'ok' }),
  });
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 4, 'text_delta', { text: 'explain B' }),
  });
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 5, 'text_delta', { text: ' more' }),
  });
  const types = state.liveByRun.r1!.blocks.map((b) => b.type);
  assert.deepEqual(types, ['text', 'tool_call', 'text']);
  assert.equal(state.liveByRun.r1!.blocks[0]!.text, 'explain A');
  assert.equal(state.liveByRun.r1!.blocks[2]!.text, 'explain B more');
});

test('checkpoint / subagent events produce visible notice blocks', () => {
  let state = withRun(createInitialWorkspaceState());
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 1, 'checkpoint_created', { checkpoint_id: 'cp-1', label: 'run start' }),
  });
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 2, 'subagent_created', { sub_run_id: 'sub1', task: 'Explore' }),
  });
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 3, 'checkpoint_rewound', { checkpoint_id: 'cp-1', paths: ['a.ts', 'b.ts'] }),
  });
  const blocks = state.liveByRun.r1!.blocks;
  const created = blocks.find((b) => b.noticeKind === 'checkpoint_created');
  assert.ok(created);
  assert.equal(created!.noticeData!.checkpointId, 'cp-1');
  const sub = blocks.find((b) => b.type === 'subagent');
  assert.ok(sub);
  assert.equal(sub!.subRunId, 'sub1');
  assert.equal(sub!.noticeData!.task, 'Explore');
  const rewound = blocks.find((b) => b.noticeKind === 'checkpoint_rewound');
  assert.ok(rewound);
  assert.equal(rewound!.noticeData!.count, 2);
  // state-level child tracking still intact
  assert.deepEqual(state.childRunsByParent.r1, ['sub1']);
});

test('eventsByRun LRU: old terminal run buffers evicted, active runs kept', () => {
  let state = createInitialWorkspaceState();
  const evAt = (runId: string, sequence: number, type: string, iso: string): RunEvent => ({
    runId,
    sequence,
    timestamp: iso,
    type,
    payload: {},
  });
  // 10 terminal runs with strictly increasing finish times
  for (let i = 1; i <= 10; i += 1) {
    const runId = `run-${String(i).padStart(2, '0')}`;
    state = withRun(state, runId, 'c1');
    const iso = `2026-07-17T12:${String(i).padStart(2, '0')}:00.000Z`;
    state = workspaceReducer(state, {
      type: 'event/apply',
      event: evAt(runId, 1, 'started', iso),
    });
    state = workspaceReducer(state, {
      type: 'event/apply',
      event: evAt(runId, 2, 'completed', iso),
    });
  }
  // one active run with events
  state = withRun(state, 'run-live', 'c1');
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: evAt('run-live', 1, 'text_delta', '2026-07-17T13:00:00.000Z'),
  });

  const terminalKeys = Object.keys(state.eventsByRun).filter((id) => id !== 'run-live');
  assert.equal(terminalKeys.length, 8, 'keeps only the 8 most recent terminal runs');
  assert.ok(!state.eventsByRun['run-01'], 'oldest evicted');
  assert.ok(!state.eventsByRun['run-02'], 'second oldest evicted');
  assert.ok(state.eventsByRun['run-10'], 'newest terminal kept');
  assert.ok(state.eventsByRun['run-live'], 'active run never evicted');
  // watermark survives eviction so late duplicates still dedupe
  assert.equal(state.lastSequenceByRun['run-01'], 2);
  // promoted messages are untouched by event eviction
  assert.ok(Object.values(state.messages).some((m) => m.runId === 'run-01'));
});
