/**
 * Regression: typing must not re-render Workbench / write localStorage every keystroke.
 *
 * H_type (2026-07-23): MessageInput.onDraftChange → composer/set + (temp)
 * publishNavigation + savePersistedDrafts on every key → 打字卡顿.
 *
 * Fix: debounce store writes in MessageInput; debounce localStorage in store
 * provider; ignore tempSession.draft-only changes for NavigationContext consumers.
 */
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { describe, it } from 'node:test';
import { fileURLToPath } from 'node:url';

const messageInputSrc = readFileSync(
  fileURLToPath(new URL('./MessageInput.tsx', import.meta.url)),
  'utf8',
);
const composerSrc = readFileSync(
  fileURLToPath(new URL('./workbench/WorkbenchComposer.tsx', import.meta.url)),
  'utf8',
);
const storeContextSrc = readFileSync(
  fileURLToPath(new URL('../../lib/assistant-workspace/context.tsx', import.meta.url)),
  'utf8',
);
const workspaceContextSrc = readFileSync(
  fileURLToPath(new URL('./AssistantWorkspaceContext.tsx', import.meta.url)),
  'utf8',
);

describe('composer draft debounce (typing thrash)', () => {
  it('MessageInput debounces draft persistence and flushes on send/unmount/switch', () => {
    assert.match(messageInputSrc, /DRAFT_PERSIST_DEBOUNCE_MS/);
    assert.match(messageInputSrc, /scheduleDraftToStore/);
    assert.match(messageInputSrc, /flushDraftToStore/);
    assert.match(messageInputSrc, /localDirtyRef/);
    // Typing path schedules; does not call onDraftChange synchronously.
    assert.match(messageInputSrc, /scheduleDraftToStore\(value\)/);
    // Send clears immediately.
    assert.match(messageInputSrc, /scheduleDraftToStore\('',\s*\{\s*immediate:\s*true\s*\}\)/);
    // draftKey identity for switch flush.
    assert.match(messageInputSrc, /draftKey/);
  });

  it('Composer passes draftKey and accepts conversationId on onDraftChange', () => {
    assert.match(composerSrc, /draftKey=\{activeId\}/);
    assert.match(composerSrc, /onDraftChange=\{\(text,\s*conversationId\)\s*=>/);
  });

  it('store provider debounces localStorage draft writes', () => {
    assert.match(storeContextSrc, /savePersistedDrafts\(draftsRef\.current\)/);
    assert.match(storeContextSrc, /setTimeout\(\(\)\s*=>\s*\{[\s\S]*?savePersistedDrafts/);
    // 400ms debounce (not every keystroke).
    assert.match(storeContextSrc, /,\s*400\)/);
  });

  it('publishNavigation ignores tempSession.draft-only changes for consumers', () => {
    assert.match(workspaceContextSrc, /function tempSessionEqual/);
    assert.match(workspaceContextSrc, /tempSessionEqual\(prev\.tempSession,\s*next\.tempSession\)/);
    // Draft text is explicitly not part of equality (workbench-local restore only).
    assert.match(
      workspaceContextSrc,
      /Draft text is workbench-local restore data/,
    );
  });
});
