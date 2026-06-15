/**
 * safe-stream.ts — Defensive wrapper for ReadableStreamDefaultController.
 *
 * Adapted from CodePilot's safe-stream.ts.
 *
 * Silently swallows "already closed" errors from late async callbacks.
 * With multiple call sites that call controller.enqueue() from async callbacks
 * (keep-alive timers, late tool-result handlers, post-abort processing),
 * a single unprotected call crashes the stream.
 *
 * This wrapper eliminates that entire class of race-condition crashes.
 */

export interface SafeStreamController<T> {
  enqueue(chunk: T): void;
  close(): void;
  error(err: unknown): void;
  /** True after the controller has transitioned to closed. */
  readonly closed: boolean;
}

/**
 * Wrap a ReadableStream controller with one that silently ignores
 * "already closed" errors on enqueue/close, and tracks a `closed` flag.
 *
 * Use this at the top of every `new ReadableStream({ start(controller) { ... } })`:
 *
 *   start(controllerRaw) {
 *     const controller = wrapController(controllerRaw);
 *     // ... rest of the code unchanged ...
 *   }
 */
export function wrapController<T>(
  raw: ReadableStreamDefaultController<T>,
  onClosedWrite?: (kind: 'enqueue' | 'close') => void,
): SafeStreamController<T> {
  let closed = false;
  let warned = false;

  const isClosedError = (e: unknown): boolean => {
    if (!(e instanceof Error)) return false;
    return /already closed|stream is closed|controller has been (released|closed)|invalid state/i.test(e.message);
  };

  const noteClosed = (kind: 'enqueue' | 'close') => {
    closed = true;
    if (!warned && onClosedWrite) {
      warned = true;
      onClosedWrite(kind);
    }
  };

  return {
    enqueue(chunk: T): void {
      if (closed) return;
      try {
        raw.enqueue(chunk);
      } catch (e) {
        if (isClosedError(e)) {
          noteClosed('enqueue');
          return;
        }
        throw e;
      }
    },
    close(): void {
      if (closed) return;
      closed = true;
      try {
        raw.close();
      } catch (e) {
        if (!isClosedError(e)) throw e;
      }
    },
    error(err: unknown): void {
      if (closed) return;
      closed = true;
      try {
        raw.error(err);
      } catch {
        /* ignore — the consumer already gave up */
      }
    },
    get closed(): boolean {
      return closed;
    },
  };
}
