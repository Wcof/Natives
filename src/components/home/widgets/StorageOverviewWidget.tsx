'use client';

/**
 * 存储概览 Widget。
 * 展示磁盘根卷 used/total/available，复用 diskApi，
 * 不碰内部私有状态，不造假数据。
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { HardDrive, RefreshCw } from 'lucide-react';
import { t, useLocale } from '@/i18n';
import { diskApi } from '@/lib/files-api';
import { classifyError } from '@/lib/error-classifier';

interface StorageInfo {
  totalBytes: number;
  usedBytes: number;
  availableBytes: number;
}

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

export function StorageOverviewWidget() {
  const locale = useLocale();
  const [info, setInfo] = useState<StorageInfo | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const mountedRef = useRef(true);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const data = (await diskApi().systemInfo()) as StorageInfo;
      if (!mountedRef.current) return;
      if (data && typeof data.totalBytes === 'number' && typeof data.usedBytes === 'number') {
        setInfo(data);
      } else {
        setInfo(null);
      }
    } catch (err) {
      if (!mountedRef.current) return;
      setError(classifyError(err).userMessage);
    } finally {
      if (mountedRef.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    void load();
    return () => {
      mountedRef.current = false;
    };
  }, [load]);

  const percent = info && info.totalBytes > 0 ? Math.min(100, (info.usedBytes / info.totalBytes) * 100) : null;

  return (
    <div className="flex h-full flex-col justify-between gap-2 p-1">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-1.5 text-xs font-semibold text-[var(--text)]">
          <HardDrive size={14} className="text-[var(--primary)]" />
          <span>{t(locale, 'settings.overviewStorage')}</span>
        </div>
        {loading && <RefreshCw size={11} className="animate-spin text-[var(--text-disabled)]" />}
      </div>

      {info && percent !== null ? (
        <div className="space-y-1.5 text-xs text-[var(--text-secondary)]">
          <div className="flex items-center justify-between text-[0.6875rem]">
            <span>{Math.round(percent)}%</span>
            <span className="font-mono text-[var(--text)]">
              {formatBytes(info.usedBytes)} / {formatBytes(info.totalBytes)}
            </span>
          </div>
          <div className="h-1.5 w-full overflow-hidden rounded-full bg-[var(--surface-hover)]">
            <div className="h-full rounded-full bg-[var(--primary)]" style={{ width: `${percent}%` }} />
          </div>
          <div className="text-[0.6875rem] text-[var(--text-disabled)]">
            {t(locale, 'settings.overviewStorageAvailable', { size: formatBytes(info.availableBytes) })}
          </div>
        </div>
      ) : (
        <div className="text-xs text-[var(--text-disabled)]">
          {error ?? (loading ? t(locale, 'common.loading') : t(locale, 'settings.overviewStorageUnknown'))}
        </div>
      )}
    </div>
  );
}
