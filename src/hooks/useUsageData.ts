'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { classifyError } from '@/lib/error-classifier';
import type {
  DashboardState,
  UsageCacheMetadata,
  UsageCacheReadResult,
  UsageDashboardResponse,
  UsageViewRequest,
} from '@/types/usage';

/**
 * Shared usage data hook (ARCH-001 · F3-01).
 *
 * Single source of truth for the usage snapshot data flow: reading the
 * host's snapshot cache and running a user-initiated scan+snapshot sync.
 * Both the dashboard Feature (`dashboard/UsageDashboard`) and the Settings
 * Personal Overview consume this hook, so no Feature owns a private second
 * copy of the cache/sync state machine.
 *
 * The hook only touches the cache/sync data path; filters, derived metrics,
 * warning banners and export state stay in the composing Feature UI.
 */

export interface UsageSyncOutcome {
  ok: boolean;
  /** Classified user-facing message when the sync failed; null on success. */
  message: string | null;
}

export interface UseUsageDataResult {
  /** Snapshot state machine shared by every usage surface. */
  state: DashboardState;
  /** Classified error message from the last cache read / sync failure. */
  errorMsg: string | null;
  isSyncing: boolean;
  lastSyncTime: number | null;
  /** Dismiss the persistent error banner. */
  clearError: () => void;
  /** Re-read the snapshot cache. Never starts a scan. */
  loadCached: () => Promise<void>;
  /** User-initiated scan + snapshot. Callers decide their own success toast. */
  sync: () => Promise<UsageSyncOutcome>;
}

export function useUsageData(
  buildViewRequest: () => UsageViewRequest,
): UseUsageDataResult {
  const requestIdRef = useRef(0);
  const [state, setState] = useState<DashboardState>({ kind: 'reading-cache' });
  const [isSyncing, setIsSyncing] = useState(false);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [lastSyncTime, setLastSyncTime] = useState<number | null>(null);

  const clearError = useCallback(() => setErrorMsg(null), []);

  const loadCached = useCallback(async () => {
    const rid = ++requestIdRef.current;
    setErrorMsg(null);
    try {
      const api = window.nativesAPI;
      if (!api?.usage?.getCached) throw new Error('usage API not available');
      const result = (await api.usage.getCached(buildViewRequest())) as UsageCacheReadResult;
      if (rid !== requestIdRef.current) return;
      if (result.state === 'ready') {
        setState({ kind: 'ready', data: result.response, metadata: result.metadata });
        setLastSyncTime(result.metadata.generatedAtMs);
      } else {
        setState({ kind: 'missing-cache' });
      }
    } catch (err) {
      if (rid !== requestIdRef.current) return;
      setErrorMsg(classifyError(err).userMessage);
      setState({ kind: 'missing-cache' });
    }
  }, [buildViewRequest]);

  const sync = useCallback(async (): Promise<UsageSyncOutcome> => {
    const rid = ++requestIdRef.current;
    setIsSyncing(true);
    setErrorMsg(null);
    try {
      const api = window.nativesAPI;
      if (!api?.usage?.sync) throw new Error('usage API not available');
      const request = buildViewRequest();
      const result = (await api.usage.sync({
        timeZone: request.timeZone,
        currentView: request,
      })) as { metadata: UsageCacheMetadata; response: UsageDashboardResponse };
      if (rid !== requestIdRef.current) return { ok: false, message: null };
      setState({ kind: 'ready', data: result.response, metadata: result.metadata });
      setLastSyncTime(result.metadata.generatedAtMs);
      return { ok: true, message: null };
    } catch (err) {
      const message = classifyError(err).userMessage;
      if (rid !== requestIdRef.current) return { ok: false, message: null };
      setErrorMsg(message);
      return { ok: false, message };
    } finally {
      setIsSyncing(false);
    }
  }, [buildViewRequest]);

  // Read the cache on mount and whenever the view request changes
  // (preset / custom range / project filter).
  useEffect(() => {
    setState({ kind: 'reading-cache' });
    void loadCached();
  }, [loadCached]);

  return { state, errorMsg, isSyncing, lastSyncTime, clearError, loadCached, sync };
}
