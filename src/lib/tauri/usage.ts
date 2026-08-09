/**
 * tauri/usage — 用量域 facade（ARCH-002）
 *
 * 业务组件只允许经本 facade 访问 usage 能力；唯一 raw invoke 在 ./core.ts。
 */

import { cmd } from './core';
import type { NativesAPI } from './types';

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

