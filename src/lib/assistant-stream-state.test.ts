import assert from 'node:assert/strict';
import test from 'node:test';
import { createAssistantStreamState, reduceAssistantStreamEvent } from './assistant-stream-state';

test('reduces text, tools, usage and completion in sequence', () => {
  let state = createAssistantStreamState('run');
  state = reduceAssistantStreamEvent(state, { runId: 'run', sequence: 1, type: 'assistant_delta', payload: { text: 'Hi' } });
  state = reduceAssistantStreamEvent(state, { runId: 'run', sequence: 2, type: 'tool_started', payload: { tool_call_id: 't', tool_name: 'Read', args: { path: 'a' } } });
  state = reduceAssistantStreamEvent(state, { runId: 'run', sequence: 3, type: 'usage_updated', payload: { input_tokens: 3, output_tokens: 4 } });
  state = reduceAssistantStreamEvent(state, { runId: 'run', sequence: 4, type: 'completed', payload: {} });
  assert.equal(state.status, 'completed');
  assert.equal(state.blocks[0]!.text, 'Hi');
  assert.equal(state.blocks[1]!.toolStatus, 'running');
  assert.equal(state.usage.outputTokens, 4);
});

test('ignores another run and duplicate sequence', () => {
  const state = reduceAssistantStreamEvent(createAssistantStreamState('a'), { runId: 'b', sequence: 1, type: 'completed', payload: {} });
  assert.equal(state.status, 'idle');
  assert.equal(reduceAssistantStreamEvent(state, { runId: 'a', sequence: 0, type: 'completed', payload: {} }), state);
});

test('restores a pending permission request from persisted events', () => {
  const state = reduceAssistantStreamEvent(createAssistantStreamState('run'), {
    runId: 'run', sequence: 1, type: 'permission_requested',
    payload: { tool_call_id: 'permission', tool_name: 'Write', reason: 'needs approval', args: { path: 'a' } },
  });
  assert.equal(state.status, 'waiting_permission');
  assert.equal(state.permissionRequest?.id, 'permission');
});

test('freezes reasoning time when assistant output begins', () => {
  let state = createAssistantStreamState('run');
  state = reduceAssistantStreamEvent(state, { runId: 'run', sequence: 1, timestamp: '2026-07-16T00:00:01Z', type: 'reasoning_delta', payload: { text: 'plan' } });
  state = reduceAssistantStreamEvent(state, { runId: 'run', sequence: 2, timestamp: '2026-07-16T00:00:03Z', type: 'assistant_delta', payload: { text: 'answer' } });
  assert.equal(state.reasoningStartedAt, '2026-07-16T00:00:01Z');
  assert.equal(state.reasoningFinishedAt, '2026-07-16T00:00:03Z');
  assert.equal(state.blocks[0]!.type, 'reasoning');
});
