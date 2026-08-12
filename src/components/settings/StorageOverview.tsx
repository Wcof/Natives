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

/** 审计收口 #7：百分比文案——percent 参数不含 `%`（模板自带 {percent}%）。 */
export function storageUsedLabel(locale: ReturnType<typeof useLocale>, percent: number): string {
  return t(locale, 'settings.overviewStorageUsed', { percent: String(Math.round(percent)) });
}

/** 展示组件：同一数据流的纯投影（loading/error/ready/unavailable）。 */
export function StorageOverviewContent({
  locale,
  loading,
  error,
  info,
  onRetry,
}: {
  locale: ReturnType<typeof useLocale>;
  loading: boolean;
  error: string | null;
  info: StorageInfo | null;
  onRetry: () => void;
}) {
  if (loading) {
    return (
      <div className="flex items-center gap-2 text-xs text-[var(--text-secondary)]">
        <RefreshCw size={13} className="animate-spin" />
        {t(locale, 'common.loading')}
      </div>
    );
  }
  if (error) {
    return (
      <EmptyState
        title={t(locale, 'settings.overviewStorageUnavailable')}
        description={error}
        action={{ label: t(locale, 'common.retry'), onClick: onRetry }}
      />
    );
  }
  if (!info) {
    return (
      <EmptyState
        title={t(locale, 'settings.overviewStorageUnavailable')}
        description={t(locale, 'settings.overviewStorageUnknown')}
        action={{ label: t(locale, 'common.retry'), onClick: onRetry }}
      />
    );
  }
  const percent = storagePercent(info);
  if (percent === null) {
    // totalBytes<=0 / 未知容量：绝不伪装成真实 0%——渲染 unavailable + retry。
    return (
      <EmptyState
        title={t(locale, 'settings.overviewStorageUnavailable')}
        description={t(locale, 'settings.overviewStorageUnknown')}
        action={{ label: t(locale, 'common.retry'), onClick: onRetry }}
      />
    );
  }
  return (
    <div className="space-y-2 text-xs text-[var(--text-secondary)]">
      <div className="flex items-center justify-between gap-3">
        {/* 审计收口 #7：percent 参数不含 %（文案模板自带 {percent}%），
            避免渲染成 50%%；且恰好一个 %。 */}
        <span>{storageUsedLabel(locale, percent)}</span>
        <span className="font-mono text-[var(--text)]">
          {formatBytes(info.usedBytes)} / {formatBytes(info.totalBytes)}
        </span>
      </div>
      <div
        className="h-2 w-full overflow-hidden rounded-full bg-[var(--surface-hover)]"
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(percent)}
        aria-label={t(locale, 'settings.overviewStorage')}
      >
        <div
          className="h-full rounded-full bg-[var(--primary)]"
          style={{ width: `${percent}%` }}
        />
      </div>
      <div className="flex items-center justify-between gap-3">
        <span>{t(locale, 'settings.overviewStorageAvailable', { size: formatBytes(info.availableBytes) })}</span>
        <span className="inline-flex items-center gap-1 text-[var(--text-disabled)]">
          <TriangleAlert size={11} />
          {t(locale, 'settings.overviewStorageSource')}
        </span>
      </div>
    </div>
  );
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
      <StorageOverviewContent
        locale={locale}
        loading={loading}
        error={error}
        info={info}
        onRetry={() => void load()}
      />
    </section>
  );
}
