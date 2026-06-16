'use client';

import { useState, useEffect, useCallback } from 'react';
import { t, type Locale } from '@/i18n';
import { Folder, FileText } from 'lucide-react';
import { webFsClient } from '@/lib/web-fs-client';

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
  }, [locale]);

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

  return (
    <div
      style={{
        position: 'fixed', inset: 0, zIndex: 60,
        background: 'rgba(0,0,0,0.42)',
        display: 'flex', alignItems: 'flex-start', justifyContent: 'center',
        paddingTop: '18vh',
      }}
      onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}
    >
      <div style={{
        width: 540, maxWidth: '92vw', borderRadius: 14, padding: 18,
        background: 'color-mix(in srgb, var(--bg-2,#131410) 85%, transparent)',
        backdropFilter: 'blur(20px) saturate(1.4)',
        boxShadow: '0 0 0 0.5px rgba(0,0,0,0.12), 0 12px 40px rgba(0,0,0,0.22)',
        maxHeight: '70vh', display: 'flex', flexDirection: 'column',
      }}>
        {/* Title */}
        <div style={{
          fontSize: 13, fontWeight: 600, color: 'var(--text,#f2f2ea)',
          marginBottom: 12, paddingBottom: 10,
          borderBottom: '1px solid var(--border,#262920)',
        }}>
          {t(locale, 'fileBrowser.diskUsage')} · {formatPath(currentPath)}
        </div>

        {/* Loading */}
        {loading && (
          <div style={{ padding: 20, textAlign: 'center', color: 'var(--text-faint,#62655a)', fontSize: 12 }}>
            {t(locale, 'fileBrowser.diskUsageLoading')}
          </div>
        )}

        {/* Error */}
        {!loading && error && (
          <div style={{ padding: 20, textAlign: 'center', color: 'var(--danger)', fontSize: 12 }}>
            {error}
          </div>
        )}

        {/* Content */}
        {!loading && !error && (
          <div style={{ flex: 1, overflow: 'auto' }}>
            {/* Total + hint */}
            {totalSize > 0 && (
              <div style={{ fontSize: 11, color: 'var(--text-dim,#9b9d8c)', marginBottom: 8 }}>
                {t(locale, 'fileBrowser.diskUsageTotal')}: {fmtBytes(totalSize)}
                {items.length > DISPLAY_COUNT && (
                  <span> · {t(locale, 'fileBrowser.diskUsageShowFirst')} {DISPLAY_COUNT}</span>
                )}
              </div>
            )}

            {/* Parent directory up button */}
            {currentPath !== '/' && (
              <div
                className="disk-up"
                onClick={handleUp}
                style={{
                  display: 'flex', alignItems: 'center', gap: 8,
                  padding: '6px 8px', borderRadius: 6, cursor: 'pointer',
                  fontSize: 12, color: 'var(--accent,#FFF5E6)',
                  marginBottom: 4,
                }}
                onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.background = 'var(--bg-3,#1c1e17)'; }}
                onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.background = 'transparent'; }}
              >
                ↑ {t(locale, 'fileBrowser.diskUsageUp')}
              </div>
            )}

            {/* Items */}
            {displayItems.map((item, idx) => {
              const barWidth = Math.max(1, Math.round((item.size / maxSize) * 100));
              return (
                <div
                  key={item.path}
                  onClick={() => item.isDir ? handleDirClick(item.path) : undefined}
                  className="disk-row"
                  style={{
                    display: 'flex', alignItems: 'center', gap: 8,
                    padding: '4px 8px', borderRadius: 4,
                    cursor: item.isDir ? 'pointer' : 'default',
                    position: 'relative', overflow: 'hidden',
                  }}
                  onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.background = 'var(--bg-3,#1c1e17)'; }}
                  onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.background = 'transparent'; }}
                >
                  {/* Background bar */}
                  <div style={{
                    position: 'absolute', left: 0, top: 0, bottom: 0,
                    width: `${barWidth}%`, borderRadius: 4,
                    background: 'var(--accent-soft, #FFF5E618)',
                    transition: 'width 0.3s ease',
                  }} />

                  {/* Icon */}
                  <span style={{
                    position: 'relative', zIndex: 1, display: 'inline-flex',
                    color: item.isDir ? 'var(--accent,#FFF5E6)' : 'var(--text-dim,#9b9d8c)',
                    flexShrink: 0,
                  }}>
                    {item.isDir ? <Folder size={14} /> : <FileText size={14} />}
                  </span>

                  {/* Name */}
                  <div style={{
                    position: 'relative', zIndex: 1,
                    flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
                    fontSize: 12, color: 'var(--text,#f2f2ea)',
                  }}>
                    {item.name}
                  </div>

                  {/* Size */}
                  <div style={{
                    position: 'relative', zIndex: 1,
                    fontSize: 11, fontFamily: 'var(--font-mono)',
                    color: 'var(--text-dim,#9b9d8c)', flexShrink: 0,
                  }}>
                    {item.sizeFormatted || fmtBytes(item.size)}
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
