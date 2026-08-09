/**
 * tauri/usage — 用量域 facade（ARCH-002）
 *
 * 业务组件只允许经本 facade 访问 usage 能力；唯一 raw invoke 在 ./core.ts。
 */

import { cmd, subscribe } from './core';
import type { NativesAPI } from './types';
import type { UsageSnapshotChangedPayload } from '@/types/usage';
export { USAGE_SNAPSHOT_CHANGED_EVENT } from '@/types/usage';

  // Usage
export const usage: NativesAPI['usage'] = {
    getCached: (query: {
      preset: 'today' | '24h' | '7d' | '30d' | '90d' | 'custom';
      timeZone: string;
      projectPath: string | null;
      customStartMs?: number;
      customEndMs?: number;
    }) => cmd('usage_get_cached', { query }),
    sync: (request: {
      timeZone: string;
      currentView: {
        preset: 'today' | '24h' | '7d' | '30d' | '90d' | 'custom';
        timeZone: string;
        projectPath: string | null;
        customStartMs?: number;
        customEndMs?: number;
      };
    }) => cmd('usage_sync', { request }),
    getCcusageEnabled: () => cmd<boolean>('usage_get_ccusage_enabled'),
    setCcusageEnabled: (enabled: boolean) =>
      cmd<boolean>('usage_set_ccusage_enabled', { enabled }),
    detectCcusage: () => cmd<string | null>('usage_detect_ccusage'),
};

/**
 * Subscribe to the Host's `usage:snapshot-changed` bus event (R-T5). The Host
 * emits it after a successful usage_sync; consumers re-read the snapshot cache
 * (read-only, no scan). Returns an unsubscribe function.
 */
export function subscribeUsageSnapshotChanged(
  onChanged: (payload: UsageSnapshotChangedPayload) => void,
): () => void {
  return subscribe<UsageSnapshotChangedPayload>(
    'usage:snapshot-changed',
    (payload) => onChanged(payload),
  );
}
