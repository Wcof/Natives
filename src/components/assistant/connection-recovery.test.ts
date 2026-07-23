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

test('workbench no longer promotes quiet soft-resub to global reconnecting after n>40', () => {
  assert.equal(workbench.includes('Still waiting for engine terminal event'), false);
  assert.equal(workbench.includes('n > 40'), false);
  // Soft resub must still exist with backoff, without connection/set reconnecting.
  assert.match(workbench, /soft-resubscribe|Quiet soft resubscribe/);
  assert.match(workbench, /Math\.min\(250 \* n, 2000\)/);
  // startSubscription must not dispatch reconnecting for quiet polls.
  const startIdx = workbench.indexOf('const startSubscription = useCallback');
  assert.ok(startIdx >= 0);
  const startChunk = workbench.slice(startIdx, startIdx + 1800);
  assert.equal(startChunk.includes("connection: 'reconnecting'"), false);
  assert.equal(startChunk.includes('connection/set'), false);
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
