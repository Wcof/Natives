'use client';

import { useSyncExternalStore } from 'react';

/**
 * useHydrated — SSR-safe client hydration guard.
 *
 * Returns `true` on client after hydration, `false` during SSR.
 * Replaces the common `useState(false) + useEffect(() => setMounted(true), [])` pattern
 * which triggers React compiler's set-state-in-effect lint error.
 *
 * Uses useSyncExternalStore with no real subscription (client-only snapshot).
 */
const emptySubscribe = () => () => {};

export function useHydrated(): boolean {
  return useSyncExternalStore(
    emptySubscribe,
    () => true,   // client snapshot
    () => false,  // SSR snapshot
  );
}
