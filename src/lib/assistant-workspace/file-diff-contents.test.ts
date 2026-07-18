import assert from 'node:assert/strict';
import test from 'node:test';
import {
  extractDiffContentsFromEvents,
  hydrateFileDiffContents,
} from './file-diff-contents';
import type { RunEvent } from '@/lib/assistant-protocol';

function ev(
  sequence: number,
  payload: Record<string, unknown>,
): RunEvent {
  return {
    runId: 'r1',
    sequence,
    timestamp: 't',
    type: 'file_changed',
    payload,
  };
}

test('extractDiffContentsFromEvents reads before/after from payloads', () => {
  const map = extractDiffContentsFromEvents([
    ev(1, { path: 'a.ts', before: 'old\n', after: 'old\nnew\n', change_type: 'modified' }),
    ev(2, { path: 'b.ts', content: 'only-after' }),
  ]);
  assert.equal(map['a.ts']!.before, 'old\n');
  assert.equal(map['a.ts']!.after, 'old\nnew\n');
  assert.equal(map['b.ts']!.after, 'only-after');
});

test('hydrateFileDiffContents fills after via readFile when missing', async () => {
  const result = await hydrateFileDiffContents({
    fileChanges: [{ path: 'src/x.ts', changeType: 'modified' }],
    events: [ev(1, { path: 'src/x.ts', change_type: 'modified' })],
    readFile: async (path) => (path === 'src/x.ts' ? 'live-content' : null),
  });
  assert.equal(result['src/x.ts']!.after, 'live-content');
  assert.equal(result['src/x.ts']!.before, '');
});

test('event after takes precedence over previous empty', async () => {
  const result = await hydrateFileDiffContents({
    fileChanges: [{ path: 'a.ts', changeType: 'modified' }],
    events: [
      ev(1, { path: 'a.ts', before: 'a', after: 'b' }),
    ],
    previous: { 'a.ts': { before: '', after: '' } },
  });
  assert.equal(result['a.ts']!.before, 'a');
  assert.equal(result['a.ts']!.after, 'b');
});
