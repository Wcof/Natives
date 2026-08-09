/**
 * Wire mapper tests against real Rust serde shapes (flattened RunEventV2)
 * and host-wrapped payload envelopes.
 */
import assert from 'node:assert/strict';
import test from 'node:test';
import { mapWireMessage, mapWireRunEvent } from './wire';
import { workspaceReducer, createInitialWorkspaceState } from '../assistant-workspace/reducer';
import type { Run } from './types';

function withRun(state = createInitialWorkspaceState(), runId = 'r1', conversationId = 'c1') {
  const run: Run = {
    id: runId,
    conversationId,
    status: 'preparing',
    providerId: 'p',
    modelId: 'm',
    permissionProfile: 'ask',
    startedAt: new Date().toISOString(),
    retryCount: 0,
    lastEventSequence: 0,
  };
  return workspaceReducer(state, { type: 'run/upsert', run });
}

test('mapWireRunEvent: flattened Rust text_delta', () => {
  const ev = mapWireRunEvent({
    run_id: 'r1',
    run_sequence: 2,
    timestamp: '2026-07-22T00:00:00Z',
    type: 'text_delta',
    text: 'Hello ',
  });
  assert.equal(ev.runId, 'r1');
  assert.equal(ev.sequence, 2);
  assert.equal(ev.type, 'text_delta');
  assert.equal(ev.payload.text, 'Hello ');
});

test('mapWireRunEvent: flattened Rust tool_call_requested/started/completed', () => {
  const requested = mapWireRunEvent({
    run_id: 'r1',
    run_sequence: 1,
    timestamp: 't',
    type: 'tool_call_requested',
    id: 'tc1',
    name: 'list_dir',
    input: { path: '.' },
  });
  assert.equal(requested.type, 'tool_call_requested');
  assert.equal(requested.payload.id, 'tc1');
  assert.equal(requested.payload.name, 'list_dir');
  assert.deepEqual(requested.payload.input, { path: '.' });

  const completed = mapWireRunEvent({
    run_id: 'r1',
    run_sequence: 3,
    type: 'tool_call_completed',
    id: 'tc1',
    name: 'list_dir',
    output: 'ok',
    is_error: false,
    duration_ms: 12,
  });
  assert.equal(completed.payload.is_error, false);
  assert.equal(completed.payload.duration_ms, 12);
});

test('mapWireRunEvent: flattened Rust tool_call_delta', () => {
  const delta = mapWireRunEvent({
    run_id: 'r1',
    run_sequence: 2,
    type: 'tool_call_delta',
    index: 0,
    id: 'tc1',
    name: 'list_dir',
    arguments_delta: '{"path":',
  });
  assert.equal(delta.type, 'tool_call_delta');
  assert.equal(delta.payload.index, 0);
  assert.equal(delta.payload.arguments_delta, '{"path":');
  assert.equal(delta.payload.id, 'tc1');
});

test('mapWireRunEvent: host-wrapped payload envelope', () => {
  const ev = mapWireRunEvent({
    runId: 'r1',
    run_sequence: 4,
    type: 'failed',
    payload: {
      code: 'NO_CREDENTIALS',
      error: 'No credentials for provider',
    },
  });
  assert.equal(ev.type, 'failed');
  assert.equal(ev.payload.code, 'NO_CREDENTIALS');
  assert.equal(ev.payload.error, 'No credentials for provider');
});

test('mapWireRunEvent + reducer: tool_call_delta merges into one card then completes', () => {
  let state = withRun();
  const events = [
    mapWireRunEvent({
      run_id: 'r1',
      run_sequence: 1,
      type: 'tool_call_delta',
      index: 0,
      id: 'tc1',
      name: 'list_dir',
      arguments_delta: '{"path":',
    }),
    mapWireRunEvent({
      run_id: 'r1',
      run_sequence: 2,
      type: 'tool_call_delta',
      index: 0,
      id: 'tc1',
      arguments_delta: '"."}',
    }),
    mapWireRunEvent({
      run_id: 'r1',
      run_sequence: 3,
      type: 'tool_call_requested',
      id: 'tc1',
      name: 'list_dir',
      input: { path: '.' },
    }),
    mapWireRunEvent({
      run_id: 'r1',
      run_sequence: 4,
      type: 'tool_call_started',
      id: 'tc1',
      name: 'list_dir',
    }),
    mapWireRunEvent({
      run_id: 'r1',
      run_sequence: 5,
      type: 'tool_call_completed',
      id: 'tc1',
      name: 'list_dir',
      output: '["a"]',
      is_error: false,
      duration_ms: 3,
    }),
    mapWireRunEvent({
      run_id: 'r1',
      run_sequence: 6,
      type: 'text_delta',
      text: 'done',
    }),
    mapWireRunEvent({
      run_id: 'r1',
      run_sequence: 7,
      type: 'completed',
      reason: 'stop',
    }),
  ];
  for (const event of events) {
    state = workspaceReducer(state, { type: 'event/apply', event });
  }
  // Tools remain on the live bubble until terminal; after completed they stay in
  // eventsByRun (activity panel) and are filtered out of the promoted message body.
  // Assert via intermediate non-terminal state instead of the promoted message.
  let mid = withRun();
  for (const event of events.slice(0, 5)) {
    mid = workspaceReducer(mid, { type: 'event/apply', event });
  }
  const toolsLive = mid.liveByRun.r1?.blocks.filter((b) => b.type === 'tool_call') ?? [];
  assert.equal(toolsLive.length, 1);
  assert.equal(toolsLive[0]!.toolStatus, 'completed');
  assert.equal(toolsLive[0]!.toolName, 'list_dir');
  assert.equal(state.runs.r1?.status, 'completed');
  // Promoted message must not re-embed tool cards.
  const msgId = Object.keys(state.messages).find((id) => state.messages[id]?.runId === 'r1');
  if (msgId) {
    const toolsMsg =
      state.messages[msgId]?.contentBlocks.filter((b) => b.type === 'tool_call') ?? [];
    assert.equal(toolsMsg.length, 0);
  }
});

test('mapWireRunEvent + reducer: failed event surfaces error (no permanent thinking)', () => {
  let state = withRun();
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: mapWireRunEvent({
      run_id: 'r1',
      run_sequence: 1,
      type: 'failed',
      code: 'NO_CREDENTIALS',
      error: 'No credentials for provider p',
    }),
  });
  assert.equal(state.runs.r1?.status, 'failed');
  assert.equal(state.runs.r1?.errorCode, 'NO_CREDENTIALS');
  assert.match(String(state.runs.r1?.errorMessage ?? ''), /No credentials/);
});

test('mapWireMessage: daemon run_reference bookkeeping block is filtered out', () => {
  // conversation_store.rs prepends { type: "run_reference", run_id } to every
  // assistant message; it must never surface as a "Legacy block" in the UI.
  const message = mapWireMessage({
    id: 'm1',
    conversation_id: 'c1',
    role: 'assistant',
    status: 'complete',
    created_at: 't',
    content_blocks: [
      { type: 'run_reference', run_id: 'r1' },
      { type: 'text', text: 'answer' },
      { type: 'unknown_future_type', foo: 1 },
    ],
  });
  assert.equal(message.contentBlocks.length, 2);
  assert.equal(message.contentBlocks[0]!.type, 'text');
  assert.equal(message.contentBlocks[0]!.text, 'answer');
  // Unknown types still degrade to legacy (timeline must not crash) …
  assert.equal(message.contentBlocks[1]!.type, 'legacy');
  assert.equal(message.contentBlocks[1]!.originalType, 'unknown_future_type');
  // … and no legacy shell remains for run_reference.
  assert.ok(!message.contentBlocks.some((b) => b.originalType === 'run_reference'));
});
