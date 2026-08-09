/**
 * Regression: MessageRow memo must not be busted by per-render `?? []`.
 *
 * H_tl (2026-07-23): ConversationTimeline passed `eventsByRun[id] ?? []`, which
 * allocated a fresh array every parent render → MessageRow custom memo always
 * saw runEvents identity change even when the message was idle.
 */
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { describe, it } from 'node:test';
import { fileURLToPath } from 'node:url';

const timelineSrc = readFileSync(
  fileURLToPath(new URL('../ui/conversation/ConversationTimeline.tsx', import.meta.url)),
  'utf8',
);
const workbenchSrc = readFileSync(
  fileURLToPath(new URL('./AssistantWorkbench.tsx', import.meta.url)),
  'utf8',
);
const activitySrc = readFileSync(
  fileURLToPath(new URL('../../lib/assistant-activity-view.ts', import.meta.url)),
  'utf8',
);

describe('timeline / activity empty-array identity', () => {
  it('ConversationTimeline uses shared empty constants for runEvents + idle tools', () => {
    assert.match(timelineSrc, /const EMPTY_RUN_EVENTS:\s*RunEvent\[\]\s*=\s*\[\]/);
    assert.match(timelineSrc, /EMPTY_RUN_EVENTS/);
    assert.match(timelineSrc, /const EMPTY_TOOL_ACTIVITY/);
    assert.match(timelineSrc, /if \(!messageLive\) return EMPTY_TOOL_ACTIVITY/);
    // Must not allocate `?? []` for runEvents on each row (ignore comments).
    const withoutComments = timelineSrc.replace(/\/\*[\s\S]*?\*\//g, '').replace(/\/\/.*$/gm, '');
    assert.equal(/\?\?\s*\[\]/.test(withoutComments), false);
  });

  it('Workbench child-event path uses shared EMPTY_CHILD_EVENTS', () => {
    assert.match(workbenchSrc, /const EMPTY_CHILD_EVENTS:\s*RunEvent\[\]\s*=\s*\[\]/);
    assert.match(workbenchSrc, /EMPTY_CHILD_EVENTS/);
  });

  it('extractTodosFromEvents empty path is a shared constant', () => {
    assert.match(activitySrc, /const EMPTY_TODOS:\s*ActivityTodo\[\]\s*=\s*\[\]/);
    assert.match(activitySrc, /return EMPTY_TODOS/);
  });
});
