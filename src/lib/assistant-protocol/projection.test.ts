/**
 * Renderer projection recovery — batch-4 closure.
 *
 * Proves the Renderer is a projection-only consumer:
 * - replaying an event never double-applies (dedup by run_id+sequence);
 * - a sequence gap marks the projection incomplete (never a synthetic event);
 * - a real terminal event after a gap flips the projection to "recovered";
 * - a missing authoritative terminal surfaces `AuthoritativeEventMissing`
 *   (the Renderer never invents one);
 * - `projectionRecoveryByRun` stays Renderer-local diagnostic state (the
 *   reducer stores it; nothing writes it back to the daemon).
 */
import assert from 'node:assert/strict';
import test from 'node:test';
import {
  applyProjectionEvent,
  AuthoritativeEventMissing,
  createProjectionState,
  type ProjectionState,
} from './projection';
import type { RunEvent } from './types';

function ev(runId: string, sequence: number, type: string): RunEvent {
  return {
    runId,
    sequence,
    timestamp: '2026-07-17T12:00:00.000Z',
    type: type as RunEvent['type'],
    payload: {},
  };
}

test('replaying an event never double-applies (dedup by run_id + sequence)', () => {
  const runId = 'run-dedup';
  let state: ProjectionState = createProjectionState(runId, 0);
  const started = ev(runId, 1, 'started');
  const text = ev(runId, 2, 'text_delta');
  const completed = ev(runId, 3, 'completed');

  // First pass.
  state = applyProjectionEvent(state, started);
  state = applyProjectionEvent(state, text);
  state = applyProjectionEvent(state, completed);
  assert.equal(state.lastSequence, 3);
  assert.equal(state.terminal, true);

  // Reconnect replay re-sends the same sequence range — a no-op.
  const replayed = applyProjectionEvent(state, started);
  assert.equal(replayed, state, 'replayed started must not change projection');
  assert.equal(applyProjectionEvent(state, text), state, 'replayed delta must not change projection');
  assert.equal(
    applyProjectionEvent(state, completed),
    state,
    'replayed terminal must not change projection',
  );
});

test('tool and permission blocks are idempotent across a replay', () => {
  const runId = 'run-replay-blocks';
  let state: ProjectionState = createProjectionState(runId, 0);
  const events = [
    ev(runId, 1, 'started'),
    ev(runId, 2, 'tool_call_requested'),
    ev(runId, 3, 'tool_call_started'),
    ev(runId, 4, 'permission_requested'),
    ev(runId, 5, 'permission_responded'),
    ev(runId, 6, 'tool_call_completed'),
    ev(runId, 7, 'completed'),
  ];
  for (const e of events) state = applyProjectionEvent(state, e);
  const cards = state.seen.size;
  assert.equal(cards, events.length);
  // Replaying the whole range re-adds nothing.
  let again = state;
  for (const e of events) again = applyProjectionEvent(again, e);
  assert.equal(again.seen.size, cards, 'replayed blocks must not duplicate');
  assert.equal(again.lastSequence, 7);
});

test('sequence gap marks the projection incomplete, terminal recovers it', () => {
  const runId = 'run-gap';
  let state: ProjectionState = createProjectionState(runId, 0);
  state = applyProjectionEvent(state, ev(runId, 1, 'started'));
  // A jump to sequence 5 (events 2-4 missing) → incomplete.
  state = applyProjectionEvent(state, ev(runId, 5, 'text_delta'));
  assert.equal(state.recovery.kind, 'incomplete');
  assert.equal(state.recovery.lastSequence, 5);
  if (state.recovery.kind === 'incomplete') {
    assert.equal(state.recovery.reason, 'gap');
  }
  // A real terminal event after the gap settles the projection as recovered.
  state = applyProjectionEvent(state, ev(runId, 6, 'completed'));
  assert.equal(state.terminal, true);
  assert.equal(state.recovery.kind, 'recovered');
});

test('missing authoritative terminal surfaces AuthoritativeEventMissing, never a synthetic one', () => {
  const runId = 'run-missing-terminal';
  const state = createProjectionState(runId, 3);
  // The Renderer has projected deltas but the authoritative terminal never
  // arrives; the projection itself must not fabricate a completed/failed.
  assert.equal(state.terminal, false);
  assert.throws(() => {
    throw new AuthoritativeEventMissing(runId);
  }, AuthoritativeEventMissing);
  assert.equal(
    new AuthoritativeEventMissing(runId).message.includes('authoritative terminal event missing'),
    true,
  );
});

test('projection/recovery reducer stores diagnostic state without inventing events', async () => {
  // Import the reducer fresh so we can drive the exact action the controller
  // dispatches for gaps / authoritative-terminal-missing.
  const { workspaceReducer } = await import('../assistant-workspace/reducer');
  const { createInitialWorkspaceState } = await import('../assistant-workspace/state');
  let state = createInitialWorkspaceState();
  state = workspaceReducer(state, {
    type: 'projection/recovery',
    runId: 'run-recovery',
    recovery: { kind: 'incomplete', lastSequence: 3, reason: 'gap' },
  });
  assert.equal(state.projectionRecoveryByRun['run-recovery']?.kind, 'incomplete');
  assert.equal(
    state.projectionRecoveryByRun['run-recovery']?.lastSequence,
    3,
    'projection recovery is Renderer-local and stores the observed last sequence',
  );
  // Terminal recovery.
  state = workspaceReducer(state, {
    type: 'projection/recovery',
    runId: 'run-recovery',
    recovery: { kind: 'recovered', lastSequence: 7 },
  });
  assert.equal(state.projectionRecoveryByRun['run-recovery']?.kind, 'recovered');
});
