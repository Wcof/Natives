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
  selectPendingInteractions,
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

  it('selectConversationMessages stays referentially stable across composer ticks', () => {
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
      messagesByConversation: { c1: ['m1'] },
      messages: {
        m1: {
          id: 'm1',
          conversationId: 'c1',
          role: 'user',
          status: 'completed',
          createdAt: '2026-07-23T00:00:00.000Z',
          contentBlocks: [{ type: 'text', text: 'hi' }],
        },
      },
    };
    const before = selectConversationMessages(state, 'c1');
    // Unrelated composer tick — new state object, same message/run/live tables.
    const afterComposer = workspaceReducer(state, {
      type: 'composer/set',
      conversationId: 'c1',
      draft: { text: 'drafting…' },
    });
    const after = selectConversationMessages(afterComposer, 'c1');
    assert.strictEqual(
      after,
      before,
      'composer/set must not force a new messages[] identity for the timeline',
    );
  });

  it('selectConversationMessages returns shared EMPTY when conversation has no rows', () => {
    const state = createInitialWorkspaceState();
    const a = selectConversationMessages(state, 'c-empty');
    const b = selectConversationMessages(state, 'c-empty');
    assert.strictEqual(a, b);
    assert.equal(a.length, 0);
  });

  it('selectPendingInteractions is stable when interactions maps are unchanged', () => {
    let state = createInitialWorkspaceState();
    state = {
      ...state,
      interactionOrder: ['i1'],
      interactions: {
        i1: {
          id: 'i1',
          kind: 'permission',
          runId: 'r1',
          conversationId: 'c1',
          createdAt: '2026-07-23T00:00:00.000Z',
          toolCallId: 'tc1',
          toolName: 'bash',
          reason: 'run',
          input: {},
        },
      },
    };
    const a = selectPendingInteractions(state, 'c1');
    // Pure state clone with same interaction tables (composer-style identity churn).
    const next = { ...state, composerByConversation: { c1: { text: 'x', attachments: [], updatedAt: 't' } } };
    const b = selectPendingInteractions(next, 'c1');
    assert.strictEqual(a, b);
    assert.equal(a.length, 1);
  });

  it('selectChildRuns stays stable when childSummaries map identity is unchanged', () => {
    let state = createInitialWorkspaceState();
    state = {
      ...state,
      childRunsByParent: { r1: ['c-run'] },
      childSummaries: {
        'c-run': {
          id: 'c-run',
          parentRunId: 'r1',
          status: 'running',
          task: 't',
        },
      },
    };
    const a = selectChildRuns(state, 'r1');
    const next = { ...state, composerByConversation: {} };
    const b = selectChildRuns(next, 'r1');
    assert.strictEqual(a, b);
    assert.equal(a.length, 1);
  });

  it('selectEventsForRunTree multi-child merge stays stable without event array changes', () => {
    let state = createInitialWorkspaceState();
    const rootEvents = [
      {
        runId: 'r1',
        sequence: 1,
        timestamp: '2026-07-23T00:00:01.000Z',
        type: 'message_delta',
        payload: {},
      },
    ] as never[];
    const childEvents = [
      {
        runId: 'c-run',
        sequence: 1,
        timestamp: '2026-07-23T00:00:02.000Z',
        type: 'message_delta',
        payload: {},
      },
    ] as never[];
    state = {
      ...state,
      childRunsByParent: { r1: ['c-run'] },
      eventsByRun: {
        r1: rootEvents,
        'c-run': childEvents,
      },
    };
    const a = selectEventsForRunTree(state, 'r1');
    const next = {
      ...state,
      // Unrelated map replacement; same eventsByRun + childRunsByParent identities.
      composerByConversation: { c1: { text: 'z', attachments: [], updatedAt: 't' } },
    };
    const b = selectEventsForRunTree(next, 'r1');
    assert.strictEqual(a, b);
    assert.equal(a.length, 2);
  });
});
