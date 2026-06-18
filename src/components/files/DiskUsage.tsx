'use client';

import { useState, useEffect } from 'react';
import { FbFolder, FbText } from '@/lib/file-icons';
import { t, type Locale } from '@/i18n';
import { type DiskUsageItem } from '@/types/file';
import { SPACING, FONT_SIZE, BORDER_RADIUS, TRANSITION } from '@/lib/design-tokens';

interface DiskUsageProps {
  dirPath: string;
  onNavigate: (path: string) => void;
}

export default function DiskUsage({ dirPath, onNavigate }: DiskUsageProps) {
  const [items, setItems] = useState<DiskUsageItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [locale, setLocale] = useState<Locale>('zh');

  useEffect(() => {
    async function loadLocale() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved === 'en') setLocale('en'); else setLocale('zh');
      } catch { /* ignore */ }
    }
    loadLocale();
  }, []);

  useEffect(() => {
    async function load() {
      setLoading(true);
      try {
        const res = await fetch(`/api/fs/du?path=${encodeURIComponent(dirPath)}`);
        if (res.ok) {
          const data = await res.json();
          setItems(Array.isArray(data) ? data : []);
        }
      } catch { /* ignore */ }
      setLoading(false);
    }
    load();
  }, [dirPath]);

  if (loading) return <div style={{ padding: 12, color: 'var(--text-faint)' }}>Analyzing...</div>;

  const maxSize = items.length > 0 ? items[0]!.size : 1;
  const MAX_BAR_WIDTH = 300;

  return (
    <div style={{ padding: 8 }}>
      {items.length === 0 ? (
        <div style={{ padding: SPACING.xl, textAlign: 'center', color: 'var(--vibe-btn-text)', fontSize: FONT_SIZE.md }}>
          {t(locale, 'fileBrowser.empty')}
        </div>
      ) : (
        items.slice(0, 30).map((item) => {
          const barWidth = Math.max(4, (item.size / maxSize) * MAX_BAR_WIDTH);
          return (
            <div
              key={item.path}
              onClick={() => item.isDir ? onNavigate(item.path) : undefined}
              style={{
                display: 'flex', alignItems: 'center', gap: 8,
                padding: '4px 8px', cursor: item.isDir ? 'pointer' : 'default',
                borderBottom: '1px solid var(--vibe-btn-border)',
                fontSize: FONT_SIZE.md,
                transition: 'background 0.08s',
              }}
              onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.background = 'var(--vibe-toolbar-bg)'; }}
              onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.background = 'transparent'; }}
            >
              {/* Icon */}
              <span style={{ fontSize: FONT_SIZE.xl, display: 'inline-flex', alignItems: 'center', justifyContent: 'center', color: item.isDir ? 'var(--accent)' : 'var(--vibe-btn-text)' }}>
                {item.isDir ? <FbFolder size={18} /> : <FbText size={18} />}
              </span>

              {/* Name */}
              <div style={{
                flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
                color: 'var(--vibe-brand-text)',
              }}>
                {item.name}
              </div>

              {/* Size */}
              <div style={{
                width: 70, textAlign: 'right',
                color: 'var(--vibe-btn-text)', fontSize: FONT_SIZE.sm,
              }}>
                {item.sizeFormatted}
              </div>

              {/* Bar */}
              <div style={{
                width: MAX_BAR_WIDTH, height: 10,
                background: 'var(--vibe-btn-bg)',
                borderRadius: 5, overflow: 'hidden', flexShrink: 0,
              }}>
                <div style={{
                  width: barWidth, height: '100%',
                  background: item.isDir
                    ? 'var(--accent)'
                    : 'var(--vibe-btn-text)',
                  borderRadius: 5,
                  transition: 'width 0.3s ease',
                  opacity: item.isDir ? 0.8 : 0.4,
                }} />
              </div>
            </div>
          );
        })
      )}
    </div>
  );
}
