'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { HardDrive, RefreshCw, TriangleAlert } from 'lucide-react';
import { useLocale, t } from '@/i18n';
import { classifyError } from '@/lib/error-classifier';
import { diskApi } from '@/lib/files-api';
import { EmptyState } from '@/components/ui/EmptyState';

/** Real root-volume storage info (camelCase wire from commands/disk.rs). */
export interface StorageInfo {
  totalBytes: number;
  usedBytes: number;
  availableBytes: number;
}

export function isStorageInfo(value: unknown): value is StorageInfo {
  if (!value || typeof value !== 'object') return false;
  const candidate = value as Partial<StorageInfo>;
  return [candidate.totalBytes, candidate.usedBytes, candidate.availableBytes]
    .every((item) => typeof item === 'number' && Number.isFinite(item) && item >= 0);
}

export function formatBytes(bytes: number): string {
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

export function storagePercent(info: StorageInfo): number | null {
  if (info.totalBytes <= 0) return null;
  return Math.min(100, (info.usedBytes / info.totalBytes) * 100);
}

/**
 * 问题7：设置页「个人概览」的存储卡。只展示真实根卷 used/total/available，
 * 复用 `diskApi().systemInfo`（commands/disk.rs 根卷权威），失败不显示 0。
 * 组合发生在 Shell 层 SettingsPage：本组件不 import UsageDashboard。
 */
export default function StorageOverview() {
  const locale = useLocale();
  const [info, setInfo] = useState<StorageInfo | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const requestIdRef = useRef(0);

  const load = useCallback(async () => {
    const requestId = ++requestIdRef.current;
    setLoading(true);
    setError(null);
    try {
      const value = await diskApi().systemInfo();
      if (requestId !== requestIdRef.current) return;
      if (!isStorageInfo(value)) {
        setError(t(locale, 'settings.overviewStorageUnavailable'));
        return;
      }
      setInfo(value);
    } catch (err) {
      if (requestId !== requestIdRef.current) return;
      setError(classifyError(err).userMessage);
    } finally {
      if (requestId === requestIdRef.current) setLoading(false);
    }
  }, [locale]);

  useEffect(() => {
    void load();
    return () => {
      requestIdRef.current += 1;
    };
  }, [load]);

  return (
    <section
      className="rounded-2xl border border-[var(--border-subtle)] bg-[var(--surface)] p-5"
      aria-busy={loading}
      data-testid="storage-overview"
    >
      <div className="mb-4 flex items-center gap-2 text-sm font-semibold text-[var(--text)]">
        <HardDrive size={16} className="text-[var(--primary)]" />
        {t(locale, 'settings.overviewStorage')}
      </div>

      {loading && (
        <div className="flex items-center gap-2 text-xs text-[var(--text-secondary)]">
          <RefreshCw size={13} className="animate-spin" />
          {t(locale, 'common.loading')}
        </div>
      )}

      {!loading && error && (
        <EmptyState
          title={t(locale, 'settings.overviewStorageUnavailable')}
          description={error}
          action={{
            label: t(locale, 'common.retry'),
            onClick: () => void load(),
          }}
        />
      )}

      {!loading && !error && info && (
        <div className="space-y-2 text-xs text-[var(--text-secondary)]">
          {(() => {
            const percent = storagePercent(info);
            return (
              <>
                <div className="flex items-center justify-between gap-3">
                  <span>{t(locale, 'settings.overviewStorageUsed', { percent: `${Math.round(percent ?? 0)}%` })}</span>
                  <span className="font-mono text-[var(--text)]">
                    {formatBytes(info.usedBytes)} / {formatBytes(info.totalBytes)}
                  </span>
                </div>
                <div
                  className="h-2 w-full overflow-hidden rounded-full bg-[var(--surface-hover)]"
                  role="progressbar"
                  aria-valuemin={0}
                  aria-valuemax={100}
                  aria-valuenow={Math.round(percent ?? 0)}
                  aria-label={t(locale, 'settings.overviewStorage')}
                >
                  <div
                    className="h-full rounded-full bg-[var(--primary)]"
                    style={{ width: `${percent ?? 0}%` }}
                  />
                </div>
                <div className="flex items-center justify-between gap-3">
                  <span>{t(locale, 'settings.overviewStorageAvailable', { size: formatBytes(info.availableBytes) })}</span>
                  <span className="inline-flex items-center gap-1 text-[var(--text-disabled)]">
                    <TriangleAlert size={11} />
                    {t(locale, 'settings.overviewStorageSource')}
                  </span>
                </div>
              </>
            );
          })()}
        </div>
      )}
    </section>
  );
}
