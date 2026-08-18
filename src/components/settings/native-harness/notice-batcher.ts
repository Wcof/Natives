export interface HarnessNoticeLike {
  kind: string;
}

type RefreshKind = 'none' | 'notice' | 'runs' | 'workspace';

export interface HarnessNoticeBatcherOptions {
  detailVisible: boolean;
  dirty: boolean;
  refreshWorkspace: () => void;
  refreshRuns: () => void;
  showRemoteChange: () => void;
  schedule?: (callback: () => void, delayMs: number) => ReturnType<typeof setTimeout>;
  cancel?: (timer: ReturnType<typeof setTimeout>) => void;
}

/**
 * Coalesce a replay page from `harness.subscribe` into one refresh.
 *
 * A new panel subscribes from cursor 0, so a historical page can contain up
 * to 100 notices. Refreshing the complete workspace per notice exhausts the
 * Renderer and Host RPC queue before the page can paint.
 */
export function createHarnessNoticeBatcher(options: HarnessNoticeBatcherOptions) {
  const schedule = options.schedule ?? ((callback, delayMs) => setTimeout(callback, delayMs));
  const cancel = options.cancel ?? ((timer) => clearTimeout(timer));
  let timer: ReturnType<typeof setTimeout> | null = null;
  let pending: RefreshKind = 'none';

  const priority: Record<RefreshKind, number> = {
    none: 0,
    notice: 1,
    runs: 2,
    workspace: 3,
  };

  const flush = () => {
    timer = null;
    const next = pending;
    pending = 'none';
    if (next === 'workspace') options.refreshWorkspace();
    else if (next === 'runs') options.refreshRuns();
    else if (next === 'notice') options.showRemoteChange();
  };

  return {
    notify(event: HarnessNoticeLike) {
      const next: RefreshKind =
        event.kind === 'trace_updated' && options.detailVisible
          ? 'runs'
          : options.dirty
            ? 'notice'
            : 'workspace';
      if (priority[next] > priority[pending]) pending = next;
      if (timer === null) timer = schedule(flush, 50);
    },
    dispose() {
      if (timer !== null) cancel(timer);
      timer = null;
      pending = 'none';
    },
  };
}
