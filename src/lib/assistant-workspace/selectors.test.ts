import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { createInitialWorkspaceState } from './state';
import {
  selectArtifacts,
  selectChildRuns,
  selectEventsForRunTree,
  selectFileChanges,
  selectFileChangesForRunTree,
  selectRunEvents,
} from './selectors';

describe('assistant workspace selectors', () => {
  it('keeps empty effect dependencies referentially stable', () => {
    const state = createInitialWorkspaceState();
    assert.strictEqual(selectRunEvents(state, null), selectRunEvents(state, null));
    assert.strictEqual(selectFileChanges(state, null), selectFileChanges(state, null));
    assert.strictEqual(selectArtifacts(state, null), selectArtifacts(state, null));
    assert.strictEqual(selectChildRuns(state, null), selectChildRuns(state, null));
    assert.strictEqual(selectEventsForRunTree(state, null), selectEventsForRunTree(state, null));
    assert.strictEqual(
      selectFileChangesForRunTree(state, null),
      selectFileChangesForRunTree(state, null),
    );
  });
});
