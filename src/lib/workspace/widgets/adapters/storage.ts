// ── Storage Overview Adapter（B-027） ──
// 复用现有 disk domain（@/lib/files-api → diskApi().systemInfo()），
// 不碰内部私有状态，不造假数据。

import { diskApi } from '@/lib/files-api';
import type { WidgetConfig, WidgetDataContext } from '../types';

export interface StorageOverviewData {
  info: {
    totalBytes: number;
    usedBytes: number;
    availableBytes: number;
  } | null;
  unavailable: boolean;
}

interface StorageInfoLike {
  totalBytes?: unknown;
  usedBytes?: unknown;
  availableBytes?: unknown;
}

export function storageAdapterKey(_config: WidgetConfig): string {
  return 'storage.overview';
}

export async function loadStorageOverview(ctx: WidgetDataContext): Promise<StorageOverviewData> {
  try {
    const data = (await diskApi().systemInfo()) as unknown as StorageInfoLike;
    if (ctx.signal.aborted) throw new DOMException('Aborted', 'AbortError');
    if (
      data &&
      typeof data.totalBytes === 'number' &&
      typeof data.usedBytes === 'number' &&
      typeof data.availableBytes === 'number'
    ) {
      return {
        info: {
          totalBytes: data.totalBytes,
          usedBytes: data.usedBytes,
          availableBytes: data.availableBytes,
        },
        unavailable: false,
      };
    }
    return { info: null, unavailable: true };
  } catch (err) {
    if (err instanceof DOMException && err.name === 'AbortError') throw err;
    console.warn('[widget] storage-overview loader unavailable:', err);
    return { info: null, unavailable: true };
  }
}
