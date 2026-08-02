import type { RunEvent } from './types';

/** Renderer-only recovery state. It never invents a daemon event/sequence. */
export type ProjectionRecovery =
  | { kind: 'complete'; lastSequence: number }
  | { kind: 'incomplete'; lastSequence: number; reason: 'gap' | 'authoritative_event_missing' };

export interface ProjectionState {
  runId: string;
  lastSequence: number;
  terminal: boolean;
  seen: Set<string>;
  recovery: ProjectionRecovery;
}

export class AuthoritativeEventMissing extends Error {
  readonly runId: string;
  constructor(runId: string) {
    super(`authoritative terminal event missing for run ${runId}`);
    this.name = 'AuthoritativeEventMissing';
    this.runId = runId;
  }
}

export function createProjectionState(runId: string, afterSequence = 0): ProjectionState {
  return {
    runId,
    lastSequence: afterSequence,
    terminal: false,
    seen: new Set(),
    recovery: { kind: 'complete', lastSequence: afterSequence },
  };
}

export function applyProjectionEvent(state: ProjectionState, event: RunEvent): ProjectionState {
  if (event.runId !== state.runId) return state;
  const key = `${event.runId}:${event.sequence}`;
  if (state.seen.has(key) || event.sequence <= state.lastSequence) return state;
  const gap = event.sequence > state.lastSequence + 1;
  const terminal = ['completed', 'failed', 'cancelled', 'interrupted'].includes(event.type);
  const seen = new Set(state.seen);
  seen.add(key);
  return {
    ...state,
    lastSequence: event.sequence,
    terminal: state.terminal || terminal,
    seen,
    recovery: gap
      ? { kind: 'incomplete', lastSequence: event.sequence, reason: 'gap' }
      : { kind: 'complete', lastSequence: event.sequence },
  };
}
