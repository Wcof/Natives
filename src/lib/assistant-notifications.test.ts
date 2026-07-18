import assert from 'node:assert/strict';
import test from 'node:test';
import {
  locateAssistantTarget,
  shouldSuppressDesktopNotification,
  notificationTitle,
  ASSISTANT_LOCATE_EVENT,
} from './assistant-notifications';

test('suppresses desktop notification for the conversation currently open', () => {
  assert.equal(shouldSuppressDesktopNotification('c1', 'c1'), true);
  assert.equal(shouldSuppressDesktopNotification('c1', 'c2'), false);
  assert.equal(shouldSuppressDesktopNotification(null, 'c1'), false);
});

test('notification titles are bilingual', () => {
  assert.match(notificationTitle('waiting_permission', true), /权限/);
  assert.match(notificationTitle('run_failed', false), /failed/i);
});

test('locateAssistantTarget dispatches project→conversation→run path', () => {
  const events: unknown[] = [];
  // @ts-expect-error polyfill
  globalThis.window = {
    dispatchEvent: (e: Event) => {
      events.push(e);
      return true;
    },
  };
  locateAssistantTarget({
    projectPath: '/proj',
    conversationId: 'c9',
    runId: 'r9',
    interactionId: 'i9',
  });
  assert.equal(events.length, 1);
  const ce = events[0] as CustomEvent;
  assert.equal(ce.type, ASSISTANT_LOCATE_EVENT);
  assert.equal(ce.detail.conversationId, 'c9');
  assert.equal(ce.detail.runId, 'r9');
  // @ts-expect-error cleanup
  delete globalThis.window;
});
