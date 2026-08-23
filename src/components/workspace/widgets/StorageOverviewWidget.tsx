'use client';

/**
 * Storage Overview Widget（B-027）—— 迁移到 V2 Definition。
 * 复用 disk domain（diskApi().systemInfo()），不造假数据。
 */

import { z } from 'zod';
import { t, useLocale } from '@/i18n';
import { FONT_SIZE, SPACING } from '@/lib/design-tokens';
import type { WidgetDefinition, WidgetProps } from '@/lib/workspace/widgets';
import {
  loadStorageOverview,
  storageAdapterKey,
} from '@/lib/workspace/widgets/adapters/storage';
import type { StorageOverviewData } from '@/lib/workspace/widgets/adapters/storage';

type StorageOverviewSettings = Record<string, unknown>;

function formatBytes(bytes: number): string {
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let value = bytes;
  let index = 0;
  while (value >= 1024 && index < units.length - 1) {
    value /= 1024;
    index += 1;
  }
  const display = Number.isInteger(value) ? String(Math.round(value)) : value.toFixed(1);
  return `${display} ${units[index]}`;
}

function StorageOverviewView({ data }: WidgetProps<StorageOverviewData, StorageOverviewSettings>) {
  const locale = useLocale();
  const info = data?.info ?? null;
  const percent = info && info.totalBytes > 0 ? Math.min(100, (info.usedBytes / info.totalBytes) * 100) : null;

  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        justifyContent: 'center',
        gap: SPACING.xs,
        height: '100%',
        padding: '0 4px',
        minWidth: 0,
      }}
    >
      {info && percent !== null ? (
        <div
          style={{
            display: 'flex',
            flexDirection: 'column',
            gap: 6,
            fontSize: FONT_SIZE.xs,
            color: 'var(--text-secondary)',
            minWidth: 0,
          }}
        >
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              fontSize: 11,
            }}
          >
            <span>{Math.round(percent)}%</span>
            <span style={{ fontFamily: 'var(--font-mono)', color: 'var(--text)' }}>
              {formatBytes(info.usedBytes)} / {formatBytes(info.totalBytes)}
            </span>
          </div>
          <div
            style={{
              height: 6,
              width: '100%',
              overflow: 'hidden',
              borderRadius: 999,
              background: 'var(--surface-hover)',
            }}
          >
            <div
              style={{
                height: '100%',
                borderRadius: 999,
                background: 'var(--primary)',
                width: `${percent}%`,
              }}
            />
          </div>
          <div style={{ fontSize: 11, color: 'var(--text-tertiary)' }}>
            {t(locale, 'settings.overviewStorageAvailable', { size: formatBytes(info.availableBytes) })}
          </div>
        </div>
      ) : (
        <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-tertiary)' }}>
          {t(locale, 'settings.overviewStorageUnknown')}
        </div>
      )}
    </div>
  );
}

export const storageOverviewWidgetDefinition: WidgetDefinition<StorageOverviewData, StorageOverviewSettings> = {
  type: 'storage_overview',
  titleKey: 'home.widgetStorageOverview',
  descriptionKey: 'home.widgetStorageOverview',
  configVersion: 1,
  defaultConfig: {},
  configSchema: z.record(z.string(), z.unknown()),
  size: 'medium',
  surfacePolicy: { surfaces: ['crystal', 'material', 'plain'], allowBlur: true, allowGlow: false },
  adapterKeyBuilder: storageAdapterKey,
  load: loadStorageOverview,
  Component: StorageOverviewView,
};

export default storageOverviewWidgetDefinition;
