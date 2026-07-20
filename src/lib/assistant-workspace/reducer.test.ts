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
  state = workspaceReducer(state, { type: 'event/apply', event: ev('r1', 1, 'started') });
  // gap: jump to 3
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: ev('r1', 3, 'text_delta', { text: 'C' }),
  });
  assert.equal(state.recoveringRuns.r1, true);
  assert.equal(state.connection, 'recovering');
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
  assert.equal(state.lastSequenceByRun.r1, 4);
  assert.equal(state.runs.r1!.status, 'completed');
  // no duplicate seq 3
  const seqs = state.eventsByRun.r1!.map((e) => e.sequence);
  assert.deepEqual(seqs, [1, 2, 3, 4]);
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
