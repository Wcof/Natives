'use client';

import { useState, useEffect } from 'react';
import { t, useLocale } from '@/i18n';
import { diskApi } from '@/lib/files-api';
import { Activity, HardDrive, WifiOff } from 'lucide-react';
import { FONT_SIZE } from '@/lib/design-tokens';

/**
 * 控制中枢小组件（widget 模式，ShellLayout 直渲染）。
 * 只呈现真实指标：CPU / 内存（system_metrics）+ 磁盘（disk_system_info）。
 * 旧版包含大量假控件（Wi-Fi/蓝牙/隔空投送恒显「已连接」、亮度/音量假滑杆、
 * 假沙箱开关、demo 计数按钮），且在 var(--surface) 背景上硬编码白字——均已移除。
 */

interface Metrics {
  cpuUsage: number;
  memoryUsedBytes: number;
  memoryTotalBytes: number;
}

interface DiskInfo {
  totalBytes: number;
  usedBytes: number;
  availableBytes: number;
}

const POLL_INTERVAL_MS = 5000;

function formatGB(bytes: number): string {
  return `${(bytes / 1024 / 1024 / 1024).toFixed(1)} GB`;
}

export default function ControlHubWidget() {
  const locale = useLocale();
  const [metrics, setMetrics] = useState<Metrics | null>(null);
  const [disk, setDisk] = useState<DiskInfo | null>(null);
  const [pollFailed, setPollFailed] = useState(false);

  useEffect(() => {
    let cancelled = false;
    // files-api 契约：非 Tauri 环境无 disk 能力时静默不轮询
    let api: ReturnType<typeof diskApi>;
    try {
      api = diskApi();
    } catch {
      setPollFailed(true);
      return;
    }

    const poll = async () => {
      try {
        const [m, d] = await Promise.all([
          api.systemMetrics() as Promise<Metrics>,
          api.systemInfo() as Promise<DiskInfo>,
        ]);
        if (cancelled) return;
        setMetrics(m);
        setDisk(d);
        setPollFailed(false);
      } catch {
        if (!cancelled) setPollFailed(true);
      }
    };

    const onVisibility = () => {
      if (document.visibilityState === 'visible') void poll();
    };
    onVisibility();
    document.addEventListener('visibilitychange', onVisibility);
    const timer = setInterval(() => {
      if (document.visibilityState === 'visible') void poll();
    }, POLL_INTERVAL_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }, []);

  const cpuPercent = metrics ? Math.round(metrics.cpuUsage) : null;
  const memPercent = metrics && metrics.memoryTotalBytes > 0
    ? (metrics.memoryUsedBytes / metrics.memoryTotalBytes) * 100
    : null;
  const diskPercent = disk && disk.totalBytes > 0
    ? (disk.usedBytes / disk.totalBytes) * 100
    : null;

  return (
    <div
      className="select-none"
      style={{
        display: 'flex',
        justifyContent: 'center',
        alignItems: 'center',
        width: '100%',
        height: '100%',
        minHeight: '100%',
        position: 'relative',
        overflow: 'hidden',
        fontFamily: 'var(--font-ui), system-ui, sans-serif',
        background: 'var(--background)',
      }}
    >
      <div
        data-tauri-drag-region
        style={{
          width: 390,
          position: 'relative',
          zIndex: 10,
          background: 'var(--surface)',
          border: '1px solid var(--border)',
          borderRadius: 'var(--radius-xl)',
          boxShadow: 'var(--shadow-modal)',
          padding: 24,
          boxSizing: 'border-box',
          color: 'var(--text)',
        }}
      >
        {/* Header */}
        <div
          className="mb-5 flex flex-col items-center justify-center pb-3 text-center"
          style={{ borderBottom: '1px solid var(--border)' }}
        >
          <h1
            className="font-bold tracking-tight"
            style={{ fontSize: FONT_SIZE.lg + 2, fontFamily: 'var(--font-display)' }}
          >
            {t(locale, 'controlHub.title')}
          </h1>
          <p className="mt-1" style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-secondary)' }}>
            {t(locale, 'controlHub.subtitle')}
          </p>
        </div>

        {/* System Monitor Panel */}
        <div
          className="flex flex-col gap-3 rounded-2xl p-4"
          style={{ background: 'var(--background)', border: '1px solid var(--border)' }}
        >
          <div
            className="flex items-center justify-between pb-1.5 text-[11px] font-bold"
            style={{ borderBottom: '1px solid var(--border)', color: 'var(--text)' }}
          >
            <span className="flex items-center gap-1.5">
              <Activity size={13} style={{ color: 'var(--diff-add)' }} />
              {t(locale, 'controlHub.systemMonitor')}
            </span>
            {pollFailed ? (
              <span
                className="flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[9px] font-semibold"
                style={{ color: 'var(--danger)', background: 'var(--danger-soft)' }}
              >
                <WifiOff size={10} /> {t(locale, 'controlHub.metricsUnavailable')}
              </span>
            ) : (
              <span
                className="rounded-md px-1.5 py-0.5 text-[9px] font-semibold"
                style={{ color: 'var(--diff-add)', background: 'var(--surface-hover)' }}
              >
                Live
              </span>
            )}
          </div>

          <MeterRow
            label={t(locale, 'controlHub.cpuActivity')}
            valueText={cpuPercent !== null ? `${cpuPercent}%` : '—'}
            percent={cpuPercent}
          />
          <MeterRow
            label={t(locale, 'controlHub.memoryFootprint')}
            valueText={
              metrics
                ? `${Math.round(metrics.memoryUsedBytes / 1024 / 1024)} MB / ${Math.round(metrics.memoryTotalBytes / 1024 / 1024)} MB`
                : '—'
            }
            percent={memPercent}
          />
          <div className="flex items-center gap-2 pt-1" style={{ borderTop: '1px solid var(--border)' }}>
            <HardDrive size={12} style={{ color: 'var(--text-secondary)', flexShrink: 0 }} />
            <div className="min-w-0 flex-1">
              <MeterRow
                label={t(locale, 'controlHub.diskUsage')}
                valueText={disk ? `${formatGB(disk.usedBytes)} / ${formatGB(disk.totalBytes)}` : '—'}
                percent={diskPercent}
              />
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

function MeterRow({ label, valueText, percent }: { label: string; valueText: string; percent: number | null }) {
  return (
    <div>
      <div className="mb-1 flex justify-between text-[10px]" style={{ color: 'var(--text-secondary)' }}>
        <span>{label}</span>
        <span className="font-mono">{valueText}</span>
      </div>
      <div className="h-1.5 w-full overflow-hidden rounded-full" style={{ background: 'var(--border)' }}>
        <div
          className="h-full rounded-full transition-all duration-300"
          style={{ width: `${percent ?? 0}%`, background: 'var(--accent)' }}
        />
      </div>
    </div>
  );
}
