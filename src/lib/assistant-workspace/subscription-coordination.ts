/**
 * Pure subscription coordination for Assistant Workbench.
 *
 * Workbench used to bury wanted-set / cancel / single-sub-per-run rules inside
 * React effects that were only testable by reading source with regex. This
 * module exposes the same rules as plain functions.
 */

export interface SubscriptionSignal {
  aborted: boolean;
}

/**
 * Compute the set of run ids that should have an active soft-subscribe loop:
 * every non-terminal run the workspace currently tracks.
 */
export function computeWantedRunIds(
  runs: Record<string, { id: string; status: string }>,
  isActive: (status: string) => boolean,
): Set<string> {
  const wanted = new Set<string>();
  for (const run of Object.values(runs)) {
    if (isActive(run.status)) wanted.add(run.id);
  }
  return wanted;
}

/**
 * Diff wanted vs currently subscribed signals.
 * - cancel: in current but not wanted (or superseded)
 * - start: in wanted but no live (non-aborted) signal
 */
export function diffSubscriptionSets(
  wanted: Set<string>,
  current: Record<string, SubscriptionSignal | undefined>,
): { toStart: string[]; toCancel: string[] } {
  const toStart: string[] = [];
  const toCancel: string[] = [];
  for (const [runId, signal] of Object.entries(current)) {
    if (!signal) continue;
    if (!wanted.has(runId) && !signal.aborted) {
      toCancel.push(runId);
    }
  }
  for (const runId of wanted) {
    const signal = current[runId];
    if (!signal || signal.aborted) {
      toStart.push(runId);
    }
  }
  return { toStart, toCancel };
}

/**
 * Install a single-run subscription: abort any previous signal for the same
 * runId, register a fresh one, return it. Guarantees at most one live sub per run.
 */
export function replaceRunSubscription(
  signals: Record<string, SubscriptionSignal | undefined>,
  runId: string,
): SubscriptionSignal {
  const prev = signals[runId];
  if (prev) prev.aborted = true;
  const next: SubscriptionSignal = { aborted: false };
  signals[runId] = next;
  return next;
}

/**
 * Abort and drop signals that are no longer wanted.
 */
export function cancelUnwantedSubscriptions(
  signals: Record<string, SubscriptionSignal | undefined>,
  wanted: Set<string>,
): string[] {
  const cancelled: string[] = [];
  for (const [runId, signal] of Object.entries(signals)) {
    if (!signal) continue;
    if (!wanted.has(runId)) {
      signal.aborted = true;
      delete signals[runId];
      cancelled.push(runId);
    }
  }
  return cancelled;
}
