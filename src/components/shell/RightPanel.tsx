'use client';

import { ReactNode, useState, useCallback, useEffect } from 'react';
import { t, type Locale } from '@/i18n';

export type RightPanelMode = 'file-preview' | 'notifications' | 'module-details' | 'closed';

interface RightPanelProps {
  mode: RightPanelMode;
  onModeChange: (mode: RightPanelMode) => void;
  width: number;
  onResize: (width: number) => void;
  title?: string;
  children?: ReactNode;
}

export default function RightPanel({
  mode,
  onModeChange,
  width,
  onResize,
  title,
  children,
}: RightPanelProps) {
  const [isDragging, setIsDragging] = useState(false);
  const [locale, setLocale] = useState<Locale>('en');
  const isOpen = mode !== 'closed';

  useEffect(() => {
    async function loadLocale() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved === 'en') setLocale('en'); else setLocale('zh');
      } catch { /* ignore */ }
    }
    loadLocale();
  }, []);

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    setIsDragging(true);
    const startX = e.clientX;
    const startW = width;

    const handleMove = (ev: MouseEvent) => {
      const delta = startX - ev.clientX;
      const newWidth = Math.max(200, Math.min(400, startW + delta));
      onResize(newWidth);
    };
    const handleUp = () => {
      setIsDragging(false);
      document.removeEventListener('mousemove', handleMove);
      document.removeEventListener('mouseup', handleUp);
    };
    document.addEventListener('mousemove', handleMove);
    document.addEventListener('mouseup', handleUp);
  }, [width, onResize]);

  const handleClose = () => onModeChange('closed');

  const getTitle = () => {
    if (title) return title;
    switch (mode) {
      case 'file-preview': return t(locale, 'rightPanel.title.preview');
      case 'notifications': return t(locale, 'rightPanel.title.notifications');
      case 'module-details': return t(locale, 'rightPanel.title.moduleDetails');
      default: return t(locale, 'rightPanel.title.panel');
    }
  };

  return (
    <aside
      className={`right-panel ${!isOpen ? 'collapsed' : ''}`}
      role="region"
      aria-label={getTitle()}
      style={{ width: isOpen ? width : 0, position: 'relative' }}
    >
      {/* Resize drag handle */}
      {isOpen && (
        <div
          className={`sidebar-drag-handle ${isDragging ? 'active' : ''}`}
          style={{ left: -3, right: 'auto' }}
          onMouseDown={handleMouseDown}
        />
      )}

      <div className="right-panel-header">
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          {/* Mode tabs */}
          <button
            className={`btn-ghost ${mode === 'file-preview' ? 'active' : ''}`}
            onClick={() => onModeChange('file-preview')}
            style={{
              fontSize: 11, padding: '3px 8px', borderRadius: 4,
              color: mode === 'file-preview' ? 'var(--accent)' : 'var(--text-faint)',
            }}
            title={t(locale, 'rightPanel.filePreview')}
          >
            📄
          </button>
          <button
            className={`btn-ghost ${mode === 'notifications' ? 'active' : ''}`}
            onClick={() => onModeChange('notifications')}
            style={{
              fontSize: 11, padding: '3px 8px', borderRadius: 4,
              color: mode === 'notifications' ? 'var(--accent)' : 'var(--text-faint)',
            }}
            title={t(locale, 'rightPanel.title.notifications')}
          >
            🔔
          </button>
          <button
            className={`btn-ghost ${mode === 'module-details' ? 'active' : ''}`}
            onClick={() => onModeChange('module-details')}
            style={{
              fontSize: 11, padding: '3px 8px', borderRadius: 4,
              color: mode === 'module-details' ? 'var(--accent)' : 'var(--text-faint)',
            }}
            title={t(locale, 'rightPanel.title.moduleDetails')}
          >
            ℹ️
          </button>
        </div>
        <button className="btn-ghost" onClick={handleClose} aria-label="Close panel">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M18 6L6 18M6 6l12 12" />
          </svg>
        </button>
      </div>

      <div className="right-panel-content">
        {children || (
          <div style={{
            display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center',
            height: 200, color: 'var(--text-faint)', fontSize: 13,
          }}>
            <div style={{ fontSize: 28, marginBottom: 8 }}>
              {mode === 'notifications' ? '🔔' : '📄'}
            </div>
            <div>{mode === 'notifications' ? t(locale, 'rightPanel.empty.notifications') : t(locale, 'rightPanel.empty.selectFile')}</div>
          </div>
        )}
      </div>
    </aside>
  );
}
