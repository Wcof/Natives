'use client';

/** Error thrown when the HTTP port cannot be obtained. */
export class HttpPortNotAvailableError extends Error {
  constructor() {
    super('Local HTTP service not ready');
    this.name = 'HttpPortNotAvailableError';
  }
}

/**
 * Unified HTTP port helper.
 *
 * Calls `window.nativesAPI.bridge.getHttpPort()` and caches the result so
 * all consumers share one round-trip.
 *
 * Never falls back to a hardcoded port — if the bridge is unavailable or
 * returns 0, an `HttpPortNotAvailableError` is thrown. Callers should handle
 * the error and show an appropriate "service not ready" state.
 *
 * Usage:
 *   import { getHttpPort, HttpPortNotAvailableError } from '@/lib/natives-http-port';
 *   const port = await getHttpPort();
 */

let cachedPort: number | null = null;
let pendingPromise: Promise<number> | null = null;

export async function getHttpPort(): Promise<number> {
  if (cachedPort !== null) return cachedPort;
  if (pendingPromise) return pendingPromise;

  pendingPromise = (async (): Promise<number> => {
    const api = (window as any).nativesAPI;
    if (!api?.bridge?.getHttpPort) {
      throw new HttpPortNotAvailableError();
    }
    const port = await api.bridge.getHttpPort();
    if (!port || port === 0) {
      throw new HttpPortNotAvailableError();
    }
    cachedPort = port;
    return cachedPort as number;
  })();

  try {
    return await pendingPromise;
  } finally {
    pendingPromise = null;
  }
}
