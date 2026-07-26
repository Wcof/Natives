/**
 * Regression: shell fallback actions must not be silent no-ops and a host
 * refresh failure must not masquerade as an empty workspace.
 *
 * P0-3 (2026-07-26): rename/archive/pin from the sidebar were
 * `workbenchActions?.…` optional chains — on the files/settings page (no
 * Workbench mounted) they silently did nothing.
 * P0-10: refreshNavigationFromHost swallowed request failures as `[]` and
 * fully replaced navigation → users read it as "history deleted".
 */
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { describe, it } from 'node:test';
import { fileURLToPath } from 'node:url';

const contextSrc = readFileSync(
  fileURLToPath(new URL('./AssistantWorkspaceContext.tsx', import.meta.url)),
  'utf8',
);

describe('assistant workspace shell fallbacks (workbench unmounted)', () => {
  it('rename/archive have host direct branches instead of optional-chain no-ops', () => {
    assert.equal(contextSrc.includes('workbenchActions?.renameConversation('), false);
    assert.equal(contextSrc.includes('workbenchActions?.archiveConversation('), false);
    assert.match(contextSrc, /request\('conversation\.rename'/);
    assert.match(contextSrc, /request\('conversation\.archive'/);
  });

  it('pin fallback writes the same db key/shape as the Workbench', () => {
    assert.match(
      contextSrc,
      /PINNED_CONVERSATIONS_KEY = 'assistant:pinnedConversations'/,
    );
    // Fallback must persist through nativesAPI.db and refresh navigation after.
    assert.match(contextSrc, /db\.set\(PINNED_CONVERSATIONS_KEY/);
    // Refresh applies pins so the sidebar reflects them without a Workbench mount.
    assert.match(contextSrc, /readPinnedConversationIds\(\)/);
    assert.match(contextSrc, /pinnedIds\.has\(c\.id\)/);
  });

  it('fallback failures surface via classifyError -> toast (no alert/confirm)', () => {
    assert.match(contextSrc, /classifyError\(/);
    assert.match(contextSrc, /toast\(classifyError\(/);
    assert.equal(/\balert\(/.test(contextSrc), false);
    assert.equal(/\bconfirm\(/.test(contextSrc), false);
  });
});

describe('navigation refresh failure honesty (loadError)', () => {
  it('snapshot carries loadError and empty state is distinguished from failure', () => {
    assert.match(contextSrc, /loadError\?: string \| null/);
    // Failed fetches yield null (sentinel), not [] — so real empties stay honest.
    assert.match(contextSrc, /registeredProjects === null \|\| conversations === null/);
    // Failure branch keeps prev.groups: it publishes loading/loadError and returns
    // before the groups-replacing publish.
    assert.match(
      contextSrc,
      /publishNavigation\(\(prev\) => \(\{ \.\.\.prev, loading: false, loadError: message \}\)\);\s*\n\s*return;/,
    );
    // Success clears the error.
    assert.match(contextSrc, /loadError: null,/);
  });

  it('publishNavigation structural bailout compares loadError', () => {
    assert.match(contextSrc, /prev\.loadError === next\.loadError &&/);
  });
});
