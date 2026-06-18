'use client';

import { useState, useEffect, useCallback } from 'react';
import { createPortal } from 'react-dom';
import { t, type Locale } from '@/i18n';
import { X } from 'lucide-react';
import { FbFolder, FbText } from '@/lib/file-icons';
import { webFsClient } from '@/lib/web-fs-client';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';

interface DiskUsageItem {
  name: string;
  path: string;
  size: number;
  sizeFormatted: string;
  isDir: boolean;
}

interface DiskUsagePanelProps {
  dirPath: string;
  onClose: () => void;
  onNavigate: (path: string) => void;
}

export default function DiskUsagePanel({ dirPath, onClose, onNavigate }: DiskUsagePanelProps) {
  const [items, setItems] = useState<DiskUsageItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [currentPath, setCurrentPath] = useState(dirPath);
  const [locale, setLocale] = useState<Locale>('zh');
  const [error, setError] = useState<string | null>(null);
  const [mounted, setMounted] = useState(false);
  const [elapsed, setElapsed] = useState(0);

  useEffect(() => {
    setMounted(true);
  }, []);

  useEffect(() => {
    window.nativesAPI?.getLocale?.().then((l) => { if (l === 'en') setLocale('en'); }).catch(() => {});
  }, []);

  // Close on Escape
  useEffect(() => {
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', handleKey);
    return () => window.removeEventListener('keydown', handleKey);
  }, [onClose]);

  // Elapsed timer while loading
  useEffect(() => {
    if (!loading) return;
    setElapsed(0);
    const id = setInterval(() => setElapsed((s) => s + 1), 1000);
    return () => clearInterval(id);
  }, [loading]);

  const load = useCallback(async (path: string) => {
    setLoading(true);
    setError(null);
    try {
      const native = (window as any).nativesAPI?.disk?.usage;
      let data: DiskUsageItem[] | null = null;
      if (native) {
        data = await native(path);
      } else {
        data = await webFsClient.diskUsage(path);
      }
      if (Array.isArray(data)) {
        setItems(data.sort((a, b) => b.size - a.size));
      } else {
        setItems([]);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load');
      setItems([]);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    load(currentPath);
  }, [currentPath, load]);

  const handleDirClick = useCallback((subPath: string) => {
    setCurrentPath(subPath);
  }, []);

  const handleUp = useCallback(() => {
    const parent = currentPath.substring(0, currentPath.lastIndexOf('/')) || '/';
    setCurrentPath(parent);
  }, [currentPath]);

  const formatPath = (p: string) => {
    const home = typeof window !== 'undefined' ? (window as any).__homeDir || '~' : '~';
    return p.startsWith('/Users/') ? '~' + p.slice(6) : p;
  };

  const totalSize = items.reduce((sum, i) => sum + i.size, 0);
  const maxSize = items.length > 0 ? (items[0]?.size ?? 1) : 1;
  const DISPLAY_COUNT = 30;
  const displayItems = items.slice(0, DISPLAY_COUNT);

  const fmtBytes = (bytes: number): string => {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`;
  };

  if (!mounted) return null;

  return createPortal(
    <div
      style={{
        position: 'fixed', inset: 0, zIndex: 9999,
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        background: 'rgba(0,0,0,0.45)',
        backdropFilter: 'blur(var(--glass-overlay-blur, 24px)) saturate(var(--glass-overlay-saturation, 150%))',
        WebkitBackdropFilter: 'blur(var(--glass-overlay-blur, 24px)) saturate(var(--glass-overlay-saturation, 150%))',
        animation: 'fadeIn 0.18s cubic-bezier(0.16, 1, 0.3, 1)',
      }}
      onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}
    >
      <div
        className="anim-dropIn"
        style={{
          width: 540, maxWidth: '92vw',
          background: 'var(--vibe-toolbar-bg)',
          backdropFilter: 'blur(var(--vibe-sidebar-blur, 28px)) saturate(var(--vibe-sidebar-saturation, 145%))',
          WebkitBackdropFilter: 'blur(var(--vibe-sidebar-blur, 28px)) saturate(var(--vibe-sidebar-saturation, 145%))',
          border: '0.0625rem solid var(--vibe-toolbar-border)',
          borderRadius: 'var(--radius)',
          boxShadow: 'var(--vibe-toolbar-shadow)',
          padding: SPACING.lg,
          maxHeight: '70vh', display: 'flex', flexDirection: 'column',
        }}
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header with close button */}
        <div style={{
          display: 'flex', alignItems: 'center', justifyContent: 'space-between',
          marginBottom: SPACING.md, paddingBottom: 10,
          borderBottom: '1px solid var(--vibe-btn-border)',
        }}>
          <span style={{ fontSize: FONT_SIZE.lg, fontWeight: 600, color: 'var(--vibe-brand-text)' }}>
            {t(locale, 'fileBrowser.diskUsage')} · {formatPath(currentPath)}
          </span>
          <button
            onClick={onClose}
            title={t(locale, 'common.close')}
            style={{
              display: 'flex', alignItems: 'center', justifyContent: 'center',
              width: 24, height: 24, borderRadius: BORDER_RADIUS.md, border: 'none',
              background: 'transparent', color: 'var(--vibe-btn-text)',
              cursor: 'pointer',
            }}
            onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.background = 'var(--vibe-btn-hover-bg)'; }}
            onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.background = 'transparent'; }}
          >
            <X size={14} />
          </button>
        </div>

        {/* Loading */}
        {loading && (
          <div style={{ padding: SPACING.xl, textAlign: 'center', color: 'var(--vibe-btn-text)', fontSize: FONT_SIZE.md }}>
            {t(locale, 'fileBrowser.diskUsageLoading')}（{elapsed}s）
          </div>
        )}

        {/* Error */}
        {!loading && error && (
          <div style={{ padding: SPACING.xl, textAlign: 'center', color: 'var(--danger)', fontSize: FONT_SIZE.md }}>
            {error}
          </div>
        )}

        {/* Content */}
        {!loading && !error && (
          <div style={{ flex: 1, overflow: 'auto' }}>
            {totalSize > 0 && (
              <div style={{ fontSize: FONT_SIZE.sm, color: 'var(--vibe-btn-text)', marginBottom: SPACING.sm }}>
                {t(locale, 'fileBrowser.diskUsageTotal')}: {fmtBytes(totalSize)}
                {items.length > DISPLAY_COUNT && (
                  <span> · {t(locale, 'fileBrowser.diskUsageShowFirst')} {DISPLAY_COUNT}</span>
                )}
              </div>
            )}

            {currentPath !== '/' && (
              <div
                className="disk-up"
                onClick={handleUp}
                style={{
                  display: 'flex', alignItems: 'center', gap: 8,
                  padding: '6px 8px', borderRadius: BORDER_RADIUS.md, cursor: 'pointer',
                  fontSize: FONT_SIZE.md, color: 'var(--accent)', marginBottom: SPACING.xs,
                }}
                onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.background = 'var(--vibe-btn-bg)'; }}
                onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.background = 'transparent'; }}
              >
                ↑ {t(locale, 'fileBrowser.diskUsageUp')}
              </div>
            )}

            {displayItems.map((item) => {
              const barWidth = maxSize > 0 ? (item.size / maxSize) * 100 : 0;
              return (
                <div
                  key={item.path}
                  onClick={() => { if (item.isDir) { setCurrentPath(item.path); onNavigate(item.path); } }}
                  style={{
                    display: 'flex', alignItems: 'center', gap: 10,
                    padding: '8px 12px', borderRadius: BORDER_RADIUS.md,
                    cursor: item.isDir ? 'pointer' : 'default',
                    position: 'relative', overflow: 'hidden',
                  }}
                  onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.background = 'var(--vibe-btn-bg)'; }}
                  onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.background = 'transparent'; }}
                >
                  <div style={{
                    position: 'absolute', left: 0, top: 0, bottom: 0,
                    width: `${barWidth}%`, borderRadius: BORDER_RADIUS.sm,
                    background: 'var(--accent-soft, #cdf24b18)',
                    transition: 'width 0.3s ease',
                  }} />
                  <span style={{
                    position: 'relative', zIndex: 1, display: 'inline-flex',
                    color: item.isDir ? 'var(--accent)' : 'var(--vibe-btn-text)',
                    flexShrink: 0,
                  }}>
                    {item.isDir ? <FbFolder size={18} /> : <FbText size={18} />}
                  </span>
                  <div style={{
                    position: 'relative', zIndex: 1,
                    flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
                    fontSize: FONT_SIZE.md, color: 'var(--vibe-brand-text)',
                  }}>
                    {item.name}
                  </div>
                  <div style={{
                    position: 'relative', zIndex: 1,
                    fontSize: FONT_SIZE.sm, fontFamily: 'var(--font-mono)',
                    color: 'var(--vibe-btn-text)', flexShrink: 0,
                  }}>
                    {item.sizeFormatted || fmtBytes(item.size)}
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>,
    document.body
  );
}
