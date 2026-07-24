/**
 * Unit tests for timeline body filtering and tool activity derivation.
 */
import assert from 'node:assert/strict';
import test from 'node:test';
import type { ContentBlock } from '@/components/assistant/blocks';
import type { RunEvent } from '@/lib/assistant-protocol';
import {
  deriveToolActivityFromEvents,
  extractLiveThinking,
  filterTimelineBodyBlocks,
  selectActiveToolActivity,
  summarizeConversationChanges,
} from './assistant-timeline';

test('filterTimelineBodyBlocks drops tools and live reasoning, keeps text/plan', () => {
  const blocks: ContentBlock[] = [
    { type: 'reasoning', reasoning: 'live…', live: true },
    { type: 'tool_call', toolCallId: 't1', toolName: 'list_dir', toolStatus: 'running' },
    { type: 'text', text: '## Hello' },
    { type: 'tool_result', toolOutput: 'ok' },
    { type: 'reasoning', reasoning: 'done thought', live: false },
    { type: 'plan', planMarkdown: '- a' },
  ];
  const body = filterTimelineBodyBlocks(blocks);
  assert.deepEqual(
    body.map((b) => b.type),
    ['text', 'reasoning', 'plan'],
  );
  assert.equal(body.find((b) => b.type === 'reasoning')?.live, false);
});

test('extractLiveThinking only returns live reasoning blocks', () => {
  assert.equal(
    extractLiveThinking([{ type: 'reasoning', reasoning: 'x', live: false }]),
    null,
  );
  const live = extractLiveThinking([
    { type: 'text', text: 'a' },
    { type: 'reasoning', reasoning: ' thinking ', live: true },
  ]);
  assert.ok(live);
  assert.equal(live!.live, true);
  assert.equal(live!.text, 'thinking');
});

test('deriveToolActivityFromEvents merges lifecycle and nests by parent id', () => {
  const events: RunEvent[] = [
    {
      runId: 'r1',
      sequence: 1,
      timestamp: 't1',
      type: 'tool_call_requested',
      payload: { id: 'parent', name: 'task', input: {} },
    },
    {
      runId: 'r1',
      sequence: 2,
      timestamp: 't2',
      type: 'tool_call_started',
      payload: { id: 'child', name: 'list_dir', parent_tool_call_id: 'parent' },
    },
    {
      runId: 'r1',
      sequence: 3,
      timestamp: 't3',
      type: 'tool_call_completed',
      payload: { id: 'child', name: 'list_dir', output: 'ok', is_error: false },
    },
    {
      runId: 'r1',
      sequence: 4,
      timestamp: 't4',
      type: 'tool_call_completed',
      payload: { id: 'parent', name: 'task', output: 'done', is_error: false },
    },
  ];
  const tools = deriveToolActivityFromEvents(events);
  assert.equal(tools.length, 2);
  assert.equal(tools[0]!.toolCallId, 'parent');
  assert.equal(tools[0]!.status, 'completed');
  assert.equal(tools[1]!.toolCallId, 'child');
  assert.equal(tools[1]!.depth, 1);
  assert.equal(tools[1]!.status, 'completed');
  assert.deepEqual(selectActiveToolActivity(tools), []);
});

test('selectActiveToolActivity keeps only pending/running', () => {
  const active = selectActiveToolActivity([
    { toolCallId: 'a', toolName: 'a', status: 'running', depth: 0 },
    { toolCallId: 'b', toolName: 'b', status: 'completed', depth: 0 },
    { toolCallId: 'c', toolName: 'c', status: 'pending', depth: 1 },
    { toolCallId: 'd', toolName: 'd', status: 'failed', depth: 0 },
  ]);
  assert.deepEqual(
    active.map((t) => t.toolCallId),
    ['a', 'c'],
  );
});

test('tool detail keeps input, output, and the changed hunk until the answer completes', () => {
  const tools = deriveToolActivityFromEvents([
    { runId: 'r1', sequence: 1, timestamp: 't1', type: 'tool_call_requested', payload: { id: 'edit', name: 'write_file', input: { path: 'src/a.ts', content: 'new' } } },
    { runId: 'r1', sequence: 2, timestamp: 't2', type: 'tool_call_started', payload: { id: 'edit', name: 'write_file' } },
    { runId: 'r1', sequence: 3, timestamp: 't3', type: 'file_changed', payload: { path: 'src/a.ts', before: 'old', after: 'new' } },
    { runId: 'r1', sequence: 4, timestamp: 't4', type: 'tool_call_completed', payload: { id: 'edit', name: 'write_file', output: { ok: true } } },
  ]);
  assert.equal(tools.length, 1);
  assert.deepEqual(tools[0]!.input, { path: 'src/a.ts', content: 'new' });
  assert.deepEqual(tools[0]!.output, { ok: true });
  assert.deepEqual(tools[0]!.fileChanges, [{ path: 'src/a.ts', before: 'old', after: 'new' }]);
});

test('summarizeConversationChanges reports the net file count and line totals', () => {
  const summary = summarizeConversationChanges(
    [{ runId: 'r1', sequence: 1, timestamp: 't1', type: 'file_changed', payload: { path: 'src/a.ts', before: 'old\nkeep', after: 'new\nkeep\nadded' } }],
    [{ path: 'src/a.ts', changeType: 'modified', runId: 'r1' }, { path: 'README.md', changeType: 'created', runId: 'r1' }],
  );
  assert.equal(summary.files.length, 2);
  assert.equal(summary.additions, 2);
  assert.equal(summary.deletions, 1);
});
