/**
 * Probe: referential stability of selectors used as React effect deps.
 * Failing cases here are update-depth / thrash candidates in Workbench.
 */
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { createInitialWorkspaceState, workspaceReducer } from './index';
import {
  selectChildRuns,
  selectComposerDraft,
  selectConversationMessages,
  selectEventsForRunTree,
  selectFileChangesForRunTree,
  selectPromptQueue,
  selectRunEvents,
} from './selectors';

describe('selector identity (update-depth regression)', () => {
  it('empty run-scoped selectors stay referentially stable', () => {
    const state = createInitialWorkspaceState();
    assert.strictEqual(selectRunEvents(state, null), selectRunEvents(state, null));
    assert.strictEqual(selectRunEvents(state, 'missing'), selectRunEvents(state, 'missing'));
  });

  it('selectChildRuns empty results stay referentially stable', () => {
    const state = createInitialWorkspaceState();
    assert.strictEqual(selectChildRuns(state, null), selectChildRuns(state, null));
    assert.strictEqual(selectChildRuns(state, 'missing-parent'), selectChildRuns(state, 'missing-parent'));
  });

  it('selectConversationMessages / promptQueue empty stay stable', () => {
    const state = createInitialWorkspaceState();
    assert.strictEqual(
      selectConversationMessages(state, null),
      selectConversationMessages(state, null),
    );
    assert.strictEqual(selectPromptQueue(state, null), selectPromptQueue(state, null));
  });

  it('tree selectors with no root stay stable; empty root tree stays stable', () => {
    const state = createInitialWorkspaceState();
    assert.strictEqual(selectEventsForRunTree(state, null), selectEventsForRunTree(state, null));
    assert.strictEqual(
      selectFileChangesForRunTree(state, null),
      selectFileChangesForRunTree(state, null),
    );

    // Root exists but has zero events / zero children → must not allocate a fresh [] every call.
    let withRun = workspaceReducer(state, {
      type: 'run/upsert',
      run: {
        id: 'r1',
        conversationId: 'c1',
        status: 'running',
        providerId: 'p',
        modelId: 'm',
        permissionProfile: 'ask',
        startedAt: '2026-07-23T00:00:00.000Z',
      },
    });
    assert.strictEqual(selectEventsForRunTree(withRun, 'r1'), selectEventsForRunTree(withRun, 'r1'));
    assert.strictEqual(
      selectFileChangesForRunTree(withRun, 'r1'),
      selectFileChangesForRunTree(withRun, 'r1'),
    );
  });

  it('live bubble createdAt does not churn when run.startedAt is fixed', () => {
    let state = createInitialWorkspaceState();
    state = workspaceReducer(state, {
      type: 'run/upsert',
      run: {
        id: 'r1',
        conversationId: 'c1',
        status: 'running',
        providerId: 'p',
        modelId: 'm',
        permissionProfile: 'ask',
        startedAt: '2026-07-23T00:00:00.000Z',
      },
    });
    state = {
      ...state,
      activeRunByConversation: { c1: 'r1' },
    };
    const a = selectConversationMessages(state, 'c1');
    const b = selectConversationMessages(state, 'c1');
    assert.equal(a.length, 1);
    assert.equal(a[0]!.createdAt, b[0]!.createdAt);
    assert.equal(a[0]!.createdAt, '2026-07-23T00:00:00.000Z');
  });

  it('composer/set with identical text does not churn updatedAt / state identity', () => {
    let state = createInitialWorkspaceState();
    state = workspaceReducer(state, {
      type: 'composer/set',
      conversationId: 'c1',
      draft: { text: 'hello' },
    });
    const first = state.composerByConversation.c1!;
    const next = workspaceReducer(state, {
      type: 'composer/set',
      conversationId: 'c1',
      draft: { text: 'hello' },
    });
    assert.strictEqual(next, state);
    assert.equal(next.composerByConversation.c1!.updatedAt, first.updatedAt);
  });

  it('selectComposerDraft empty draft is referentially stable', () => {
    const state = createInitialWorkspaceState();
    assert.strictEqual(selectComposerDraft(state, null), selectComposerDraft(state, null));
    assert.strictEqual(selectComposerDraft(state, 'missing'), selectComposerDraft(state, 'missing'));
  });
});
