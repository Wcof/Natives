const TICK_INTERVAL_MS = 1_000;

export interface SavedStatusTickerEnvironment {
  isVisible: () => boolean;
  subscribeToVisibility: (listener: () => void) => () => void;
  schedule: (callback: () => void, delayMs: number) => number;
  cancel: (id: number) => void;
}

const browserEnvironment: SavedStatusTickerEnvironment = {
  isVisible: () => document.visibilityState === 'visible',
  subscribeToVisibility: (listener) => {
    document.addEventListener('visibilitychange', listener);
    return () => document.removeEventListener('visibilitychange', listener);
  },
  schedule: (callback, delayMs) => window.setTimeout(callback, delayMs),
  cancel: (id) => window.clearTimeout(id),
};

export function formatSavedStatusLabel(
  savedAt: number,
  now: number,
  labels: { justNow: string; secondsAgo: string },
): string {
  const seconds = Math.max(0, Math.round((now - savedAt) / 1_000));
  return seconds < 2
    ? labels.justNow
    : labels.secondsAgo.replace('{seconds}', String(seconds));
}

export function startSavedStatusTicker(
  savedAt: number | null,
  update: () => void,
  environment: SavedStatusTickerEnvironment = browserEnvironment,
): () => void {
  if (savedAt === null) return () => undefined;

  let stopped = false;
  let timer: number | null = null;

  const cancelTimer = () => {
    if (timer === null) return;
    environment.cancel(timer);
    timer = null;
  };

  const startVisibleCycle = () => {
    if (stopped || timer !== null || !environment.isVisible()) return;
    update();
    if (stopped || !environment.isVisible()) return;
    timer = environment.schedule(() => {
      timer = null;
      startVisibleCycle();
    }, TICK_INTERVAL_MS);
  };

  const onVisibilityChange = () => {
    if (!environment.isVisible()) {
      cancelTimer();
      return;
    }
    startVisibleCycle();
  };

  const unsubscribe = environment.subscribeToVisibility(onVisibilityChange);
  startVisibleCycle();

  return () => {
    stopped = true;
    cancelTimer();
    unsubscribe();
  };
}
