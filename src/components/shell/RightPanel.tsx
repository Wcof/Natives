'use client';

import { ReactNode, useState, useCallback, useEffect, useRef } from 'react';
import { FileText, Bell, Info, GitBranch } from 'lucide-react';
import { t, type Locale } from '@/i18n';

export type RightPanelMode = 'file-preview' | 'notifications' | 'module-details' | 'closed';
export type PreviewSubMode = 'preview' | 'info' | 'git';

/** Default open width — also used as double-click reset target. */
export const RIGHT_PANEL_DEFAULT_WIDTH = 320;
/** Narrowest usable width (header icons + close still fit). */
export const RIGHT_PANEL_MIN_WIDTH = 260;
/** Hard cap; further limited by viewport so main content keeps a floor. */
export const RIGHT_PANEL_MAX_WIDTH = 640;
/** Keep at least this much horizontal room for sidebar + main workspace. */
const MAIN_CONTENT_FLOOR = 420;

export function clampRightPanelWidth(width: number, viewportWidth = typeof window !== 'undefined' ? window.innerWidth : 1280): number {
  const maxByViewport = Math.max(RIGHT_PANEL_MIN_WIDTH, viewportWidth - MAIN_CONTENT_FLOOR);
  const max = Math.min(RIGHT_PANEL_MAX_WIDTH, maxByViewport);
  return Math.max(RIGHT_PANEL_MIN_WIDTH, Math.min(max, Math.round(width)));
}

interface RightPanelProps {
  mode: RightPanelMode;
  onModeChange: (mode: RightPanelMode) => void;
  previewSubMode?: PreviewSubMode;
  onPreviewSubModeChange?: (mode: PreviewSubMode) => void;
  width: number;
  onResize: (width: number) => void;
  title?: string;
  children?: ReactNode;
  extraHeaderContent?: ReactNode;
}

export default function RightPanel({
  mode,
  onModeChange,
  previewSubMode,
  onPreviewSubModeChange,
  width,
  onResize,
  title,
  children,
  extraHeaderContent,
}: RightPanelProps) {
  const [locale, setLocale] = useState<Locale>('en');
  const [isDragging, setIsDragging] = useState(false);
  const isOpen = mode !== 'closed';
  const widthRef = useRef(width);
  widthRef.current = width;

  useEffect(() => {
    async function loadLocale() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved === 'en') setLocale('en'); else setLocale('zh');
      } catch { /* ignore */ }
    }
    loadLocale();
  }, []);

  // Re-clamp when the window shrinks so the panel never starves the main column.
  useEffect(() => {
    if (!isOpen) return;
    const onWinResize = () => {
      const next = clampRightPanelWidth(widthRef.current);
      if (next !== widthRef.current) onResize(next);
    };
    window.addEventListener('resize', onWinResize);
    return () => window.removeEventListener('resize', onWinResize);
  }, [isOpen, onResize]);

  const handleClose = () => onModeChange('closed');

  const handleDragStart = useCallback((e: React.MouseEvent) => {
    if (!isOpen) return;
    e.preventDefault();
    e.stopPropagation();
    setIsDragging(true);
    const startX = e.clientX;
    const startW = widthRef.current;

    const handleMove = (ev: MouseEvent) => {
      // Drag handle is on the left edge: moving left grows the panel.
      const delta = startX - ev.clientX;
      onResize(clampRightPanelWidth(startW + delta));
    };
    const handleUp = () => {
      setIsDragging(false);
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
      document.removeEventListener('mousemove', handleMove);
      document.removeEventListener('mouseup', handleUp);
    };

    document.body.style.cursor = 'col-resize';
    document.body.style.userSelect = 'none';
    document.addEventListener('mousemove', handleMove);
    document.addEventListener('mouseup', handleUp);
  }, [isOpen, onResize]);

  const handleDragDoubleClick = useCallback(() => {
    onResize(clampRightPanelWidth(RIGHT_PANEL_DEFAULT_WIDTH));
  }, [onResize]);

  const getTitle = () => {
    if (title) return title;
    switch (mode) {
      case 'file-preview': return t(locale, 'rightPanel.title.preview');
      case 'notifications': return t(locale, 'rightPanel.title.notifications');
      case 'module-details': return t(locale, 'rightPanel.title.moduleDetails');
      default: return t(locale, 'rightPanel.title.panel');
    }
  };

  const density = width < 300 ? 'compact' : width >= 480 ? 'wide' : 'normal';

  return (
    <aside
      className={`right-panel ${!isOpen ? 'collapsed' : ''} ${isDragging ? 'is-resizing' : ''}`}
      role="region"
      aria-label={getTitle()}
      data-density={density}
      data-resizing={isDragging ? 'true' : 'false'}
      style={{
        width: isOpen ? width : 0,
        position: 'relative',
        // Snap during drag; keep CSS transition only when idle.
        transition: isDragging ? 'none' : undefined,
      }}
    >
      {/* Left-edge resize handle — only when open */}
      {isOpen && (
        <div
          className={`right-panel-drag-handle ${isDragging ? 'active' : ''}`}
          onMouseDown={handleDragStart}
          onDoubleClick={handleDragDoubleClick}
          role="separator"
          aria-orientation="vertical"
          aria-valuenow={width}
          aria-valuemin={RIGHT_PANEL_MIN_WIDTH}
          aria-valuemax={RIGHT_PANEL_MAX_WIDTH}
          aria-label={t(locale, 'rightPanel.resize')}
          title={t(locale, 'rightPanel.resizeHint')}
        />
      )}

      {/* Header with glass effect */}
      <div className="right-panel-header" style={{
        background: 'var(--surface)',
        borderBottom: '1px solid var(--border)',
      }}>
        <div className="right-panel-tabs flex items-center gap-1 min-w-0">
          {/* ── Always show 4 mode-tab icons: Preview / Info / Git / Notifications ── */}
          <button
            className={`flex items-center justify-center p-1.5 rounded-lg transition-all shrink-0 ${
              mode === 'file-preview' && previewSubMode === 'preview'
                ? 'bg-[var(--primary-soft)] text-[var(--primary)]'
                : 'text-[var(--text-disabled)] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)]'
            }`}
            onClick={() => {
              onModeChange('file-preview');
              onPreviewSubModeChange?.('preview');
            }}
            title={t(locale, 'rightPanel.filePreview')}
          >
            <FileText size={15} />
          </button>
          <button
            className={`flex items-center justify-center p-1.5 rounded-lg transition-all shrink-0 ${
              mode === 'file-preview' && previewSubMode === 'info'
                ? 'bg-[var(--primary-soft)] text-[var(--primary)]'
                : 'text-[var(--text-disabled)] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)]'
            }`}
            onClick={() => {
              onModeChange('file-preview');
              onPreviewSubModeChange?.('info');
            }}
            title={t(locale, 'rightPanel.title.moduleDetails')}
          >
            <Info size={15} />
          </button>
          <button
            className={`flex items-center justify-center p-1.5 rounded-lg transition-all shrink-0 ${
              mode === 'file-preview' && previewSubMode === 'git'
                ? 'bg-[var(--primary-soft)] text-[var(--primary)]'
                : 'text-[var(--text-disabled)] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)]'
            }`}
            onClick={() => {
              onModeChange('file-preview');
              onPreviewSubModeChange?.('git');
            }}
            title={t(locale, 'rightPanel.git')}
          >
            <GitBranch size={15} />
          </button>
          <button
            className={`flex items-center justify-center p-1.5 rounded-lg transition-all shrink-0 ${
              mode === 'notifications'
                ? 'bg-[var(--primary-soft)] text-[var(--primary)]'
                : 'text-[var(--text-disabled)] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)]'
            }`}
            onClick={() => onModeChange('notifications')}
            title={t(locale, 'rightPanel.title.notifications')}
          >
            <Bell size={15} />
          </button>
        </div>

        {/* Optional title — truncates when narrow so tabs/close stay reachable */}
        {title && (
          <div
            className="right-panel-title min-w-0 flex-1 px-2 text-xs text-[var(--text-secondary)] truncate"
            title={title}
          >
            {title}
          </div>
        )}

        {/* Spacer only when no title */}
        {!title && <div style={{ flex: 1, minWidth: 0 }} />}

        {/* Extra header content (e.g., edit toggle button) — adjacent to close button */}
        {extraHeaderContent && (
          <div className="right-panel-extra flex items-center gap-0.5 shrink-0">
            {extraHeaderContent}
          </div>
        )}

        <button
          className="flex items-center justify-center p-1.5 rounded-lg text-[var(--text-disabled)] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)] transition-all shrink-0"
          onClick={handleClose}
          title={t(locale, 'rightPanel.closePanel')}
          aria-label={t(locale, 'rightPanel.closePanel')}
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M18 6L6 18M6 6l12 12" />
          </svg>
        </button>
      </div>

      {/* Content area — min-width:0 so children flex/overflow instead of blowing out the panel */}
      <div className="right-panel-content">
        {children || (
          <div className="flex flex-col items-center justify-center h-[200px] text-[var(--text-disabled)] text-[13px]">
            <div className="mb-3 flex justify-center">
              <div className="w-10 h-10 rounded-full bg-[var(--surface)] flex items-center justify-center">
                {mode === 'notifications' ? <Bell size={18} /> : <FileText size={18} />}
              </div>
            </div>
            <div className="text-center px-2">
              {mode === 'notifications' ? t(locale, 'rightPanel.empty.notifications') : t(locale, 'rightPanel.empty.selectFile')}
            </div>
          </div>
        )}
      </div>
    </aside>
  );
}
