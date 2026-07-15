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
