/**
 * Creative Draft state machine pure helpers tests.
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  MAX_DRAFT_REVISIONS,
  canTransition,
  clampRevision,
  draftActions,
  isDraftBusy,
  revisionLabel,
  revisionsToPrune,
  shouldPruneRevisions,
} from './creative-draft';
import type { CreativeDraftState } from './creative-draft';

const ALL_STATES: CreativeDraftState[] = [
  'drafting',
  'generating',
  'ready',
  'publishing',
  'published',
  'archived',
];

/** §3.3 的合法迁移全集，其余组合一律非法。 */
const LEGAL: Array<[CreativeDraftState, CreativeDraftState]> = [
  ['drafting', 'generating'],
  ['generating', 'ready'],
  ['ready', 'generating'],
  ['ready', 'publishing'],
  ['publishing', 'published'],
  ['publishing', 'ready'],
  ['published', 'archived'],
];

describe('creative-draft transitions', () => {
  it('accepts every legal edge', () => {
    for (const [from, to] of LEGAL) {
      assert.equal(canTransition(from, to), true, `${from} -> ${to}`);
    }
  });

  it('lint fail still lands on ready', () => {
    // 成功与失败共用 generating -> ready 出口，差异只在修订指针。
    assert.equal(canTransition('generating', 'ready'), true);
    assert.equal(canTransition('generating', 'publishing'), false);
    assert.equal(canTransition('generating', 'drafting'), false);
  });

  it('publish failure falls back to ready', () => {
    assert.equal(canTransition('publishing', 'ready'), true);
    assert.equal(canTransition('publishing', 'archived'), false);
  });

  it('rejects every other pair including self-loops', () => {
    const legal = new Set(LEGAL.map(([f, t]) => `${f}>${t}`));
    for (const from of ALL_STATES) {
      for (const to of ALL_STATES) {
        if (legal.has(`${from}>${to}`)) continue;
        assert.equal(canTransition(from, to), false, `${from} -> ${to}`);
      }
    }
  });

  it('archived is terminal', () => {
    for (const to of ALL_STATES) {
      assert.equal(canTransition('archived', to), false, to);
    }
  });
});

describe('creative-draft actions', () => {
  it('drafting can only generate', () => {
    assert.deepEqual(draftActions('drafting', 0), {
      canGenerate: true,
      canUndo: false,
      canPublish: false,
      canDelete: true,
    });
  });

  it('ready unlocks generate/publish, undo needs a second revision', () => {
    assert.deepEqual(draftActions('ready', 1), {
      canGenerate: true,
      canUndo: false,
      canPublish: true,
      canDelete: true,
    });
    assert.deepEqual(draftActions('ready', 2), {
      canGenerate: true,
      canUndo: true,
      canPublish: true,
      canDelete: true,
    });
    // 无修订时不可发布。
    assert.equal(draftActions('ready', 0).canPublish, false);
  });

  it('in-flight states disable everything', () => {
    for (const state of ['generating', 'publishing'] as CreativeDraftState[]) {
      assert.equal(isDraftBusy(state), true, state);
      assert.deepEqual(draftActions(state, 3), {
        canGenerate: false,
        canUndo: false,
        canPublish: false,
        canDelete: false,
      }, state);
    }
  });

  it('published and archived stay read-only but deletable', () => {
    for (const state of ['published', 'archived'] as CreativeDraftState[]) {
      const a = draftActions(state, 5);
      assert.equal(a.canGenerate, false, state);
      assert.equal(a.canUndo, false, state);
      assert.equal(a.canPublish, false, state);
      assert.equal(a.canDelete, true, state);
    }
  });

  it('non-busy states are never busy', () => {
    for (const state of ['drafting', 'ready', 'published', 'archived'] as CreativeDraftState[]) {
      assert.equal(isDraftBusy(state), false, state);
    }
  });
});

describe('creative-draft revision pointer', () => {
  it('moves within bounds', () => {
    assert.equal(clampRevision(3, -1, 10), 2);
    assert.equal(clampRevision(3, 1, 10), 4);
    assert.equal(clampRevision(3, 0, 10), 3);
  });

  it('clamps to rev-1 at the lower bound', () => {
    assert.equal(clampRevision(1, -1, 10), 1);
    assert.equal(clampRevision(1, -99, 10), 1);
    assert.equal(clampRevision(0, -1, 10), 1);
  });

  it('clamps to max at the upper bound', () => {
    assert.equal(clampRevision(10, 1, 10), 10);
    assert.equal(clampRevision(9, 99, 10), 10);
    assert.equal(clampRevision(5, 0, 3), 3);
  });

  it('degenerate max and non-finite input collapse to 1', () => {
    assert.equal(clampRevision(5, 1, 0), 1);
    assert.equal(clampRevision(5, 1, Number.NaN), 1);
    assert.equal(clampRevision(Number.NaN, 1, 10), 1);
  });

  it('labels the current revision', () => {
    assert.equal(revisionLabel(3, 3), 'rev-3 •');
    assert.equal(revisionLabel(2, 3), 'rev-2');
    assert.equal(revisionLabel(1, 1), 'rev-1 •');
  });
});

describe('creative-draft revision pruning', () => {
  const range = (n: number): number[] => Array.from({ length: n }, (_, i) => i + 1);

  it('prunes only above the cap', () => {
    assert.equal(MAX_DRAFT_REVISIONS, 50);
    assert.equal(shouldPruneRevisions(MAX_DRAFT_REVISIONS - 1), false);
    assert.equal(shouldPruneRevisions(MAX_DRAFT_REVISIONS), false);
    assert.equal(shouldPruneRevisions(MAX_DRAFT_REVISIONS + 1), true);
    assert.deepEqual(revisionsToPrune(range(MAX_DRAFT_REVISIONS)), []);
  });

  it('always keeps rev-1 and the newest revision', () => {
    const dropped = revisionsToPrune(range(MAX_DRAFT_REVISIONS + 3));
    assert.equal(dropped.length, 3);
    assert.equal(dropped.includes(1), false);
    assert.equal(dropped.includes(MAX_DRAFT_REVISIONS + 3), false);
    // 从第二旧开始丢弃。
    assert.deepEqual(dropped, [2, 3, 4]);
  });

  it('is order independent', () => {
    const shuffled = range(MAX_DRAFT_REVISIONS + 2).reverse();
    assert.deepEqual(revisionsToPrune(shuffled), [2, 3]);
  });

  it('never drops rev-1 even with sparse revision numbers', () => {
    // 已裁剪过的草稿：rev-1 保留，中段是空洞。
    const sparse = [1, ...range(MAX_DRAFT_REVISIONS + 1).map((n) => n + 100)];
    const dropped = revisionsToPrune(sparse);
    assert.equal(dropped.includes(1), false);
    assert.equal(dropped.length, sparse.length - MAX_DRAFT_REVISIONS);
    assert.deepEqual(dropped, [101, 102]);
  });
});
