/**
 * Source-level guards for connection recovery UX.
 * Ensures quiet long-poll resubscribe never promotes global "reconnecting".
 */
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const workbench = readFileSync(new URL('./AssistantWorkbench.tsx', import.meta.url), 'utf8');
const controller = readFileSync(
  new URL('../../lib/assistant-workspace/controller.ts', import.meta.url),
  'utf8',
);
const sidebar = readFileSync(new URL('./AssistantSidebarSection.tsx', import.meta.url), 'utf8');
// The subscribe loop moved out of the Workbench into a hook both it and the
// creator workbench consume; the invariant is asserted where it now lives.
const runHook = readFileSync(
  new URL('../../lib/assistant-workspace/use-assistant-run.ts', import.meta.url),
  'utf8',
);

test('quiet soft-resub is never promoted to global reconnecting', () => {
  assert.equal(runHook.includes('Still waiting for engine terminal event'), false);
  assert.equal(runHook.includes('n > 40'), false);
  // Soft resub must still exist with capped backoff.
  assert.match(runHook, /quiet|Quiet/);
  assert.match(runHook, /RESUB_STEP_MS|Math\.min\(/);
  // The subscribe loop must not touch global connection state on quiet polls.
  assert.equal(runHook.includes("connection: 'reconnecting'"), false);
  assert.equal(runHook.includes('connection/set'), false);
  // And the Workbench must consume that loop rather than keep its own copy.
  assert.match(workbench, /useAssistantRunSubscription/);
  assert.equal(workbench.includes('const startSubscription = useCallback'), false);
});

test('controller quiet iterator end is a no-op on connection; transport errors reconnect', () => {
  assert.match(controller, /intentionally no-op on connection state/);
  assert.match(controller, /iterator_ended_without_terminal|subscribe quiet end/);
  // Transport path still surfaces reconnecting.
  assert.match(
    controller,
    /connection:\s*'reconnecting'[\s\S]{0,80}error:\s*message/,
  );
  // Sequence gap uses recoveringRuns (run-level), and clears global recovering.
  assert.match(controller, /sequence gap recovery|sequence_gap/);
  assert.match(controller, /recovering\/set/);
  assert.match(controller, /connection === 'recovering'/);
});

test('successful live event clears reconnecting and reconnectAttempts', () => {
  assert.match(
    controller,
    /reconnectAttempts:\s*0/,
  );
  assert.match(
    controller,
    /conn === 'reconnecting'[\s\S]{0,120}connection:\s*'connected'/,
  );
});

test('conversation menu exposes copy conversation ID via clipboard helper', () => {
  assert.match(sidebar, /copyToClipboard/);
  assert.match(sidebar, /from '@\/lib\/clipboard'/);
  assert.match(sidebar, /assistant\.copyConversationId|Copy conversation ID|复制会话 ID/);
  assert.match(sidebar, /assistant\.conversationIdCopied|Conversation ID copied|会话 ID 已复制/);
  assert.match(
    sidebar,
    /assistant\.copyConversationIdFailed|Failed to copy conversation ID|复制会话 ID 失败/,
  );
  // Copies the raw conversation.id, not a display title.
  assert.match(sidebar, /copyToClipboard\(id\)/);
});
