'use client';

import { ReactNode, useState, useCallback, useEffect } from 'react';
import { FileText, Bell, Info, GitBranch } from 'lucide-react';
import { t, type Locale } from '@/i18n';

export type RightPanelMode = 'file-preview' | 'notifications' | 'module-details' | 'closed';
export type PreviewSubMode = 'preview' | 'info' | 'git';

interface RightPanelProps {
  mode: RightPanelMode;
  onModeChange: (mode: RightPanelMode) => void;
  previewSubMode?: PreviewSubMode;
  onPreviewSubModeChange?: (mode: PreviewSubMode) => void;
  width: number;
  onResize: (width: number) => void;
  title?: string;
  children?: ReactNode;
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
}: RightPanelProps) {
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
      className={`vibe-right-panel ${!isOpen ? 'collapsed' : ''}`}
      role="region"
      aria-label={getTitle()}
      style={{ width: isOpen ? width : 0, position: 'relative' }}
    >

      {/* Header with glass effect */}
      <div className="right-panel-header" style={{
        background: 'var(--vibe-toolbar-bg)',
        borderBottom: '1px solid var(--vibe-sidebar-border)',
      }}>
        <div className="flex items-center gap-1">
          {mode === 'file-preview' ? (
            /* ── file-preview mode: sub-mode selector icons + Bell ── */
            <>
              <button
                className={`flex items-center justify-center p-1.5 rounded-lg transition-all ${
                  previewSubMode === 'preview'
                    ? 'bg-[var(--vibe-active-bg)] text-[var(--vibe-active-color)]'
                    : 'text-[var(--text-faint)] hover:bg-[var(--vibe-btn-hover-bg)] hover:text-[var(--vibe-btn-hover-color)]'
                }`}
                onClick={() => onPreviewSubModeChange?.('preview')}
                title={t(locale, 'filePreview.tabPreview')}
              >
                <FileText size={15} />
              </button>
              <button
                className={`flex items-center justify-center p-1.5 rounded-lg transition-all ${
                  previewSubMode === 'info'
                    ? 'bg-[var(--vibe-active-bg)] text-[var(--vibe-active-color)]'
                    : 'text-[var(--text-faint)] hover:bg-[var(--vibe-btn-hover-bg)] hover:text-[var(--vibe-btn-hover-color)]'
                }`}
                onClick={() => onPreviewSubModeChange?.('info')}
                title={t(locale, 'filePreview.tabInfo')}
              >
                <Info size={15} />
              </button>
              <button
                className={`flex items-center justify-center p-1.5 rounded-lg transition-all ${
                  previewSubMode === 'git'
                    ? 'bg-[var(--vibe-active-bg)] text-[var(--vibe-active-color)]'
                    : 'text-[var(--text-faint)] hover:bg-[var(--vibe-btn-hover-bg)] hover:text-[var(--vibe-btn-hover-color)]'
                }`}
                onClick={() => onPreviewSubModeChange?.('git')}
                title={t(locale, 'filePreview.tabGit')}
              >
                <GitBranch size={15} />
              </button>
              <div className="w-px h-4 mx-0.5 bg-[var(--vibe-sidebar-border)]" />
              <button
                className="flex items-center justify-center p-1.5 rounded-lg transition-all text-[var(--text-faint)] hover:bg-[var(--vibe-btn-hover-bg)] hover:text-[var(--vibe-btn-hover-color)]"
                onClick={() => onModeChange('notifications')}
                title={t(locale, 'rightPanel.title.notifications')}
              >
                <Bell size={15} />
              </button>
            </>
          ) : (
            /* ── Other modes: mode-switch icons ── */
            <>
              <button
                className="flex items-center justify-center p-1.5 rounded-lg transition-all text-[var(--text-faint)] hover:bg-[var(--vibe-btn-hover-bg)] hover:text-[var(--vibe-btn-hover-color)]"
                onClick={() => onModeChange('file-preview')}
                title={t(locale, 'rightPanel.filePreview')}
              >
                <FileText size={15} />
              </button>
              <button
                className={`flex items-center justify-center p-1.5 rounded-lg transition-all ${
                  mode === 'notifications'
                    ? 'bg-[var(--vibe-active-bg)] text-[var(--vibe-active-color)]'
                    : 'text-[var(--text-faint)] hover:bg-[var(--vibe-btn-hover-bg)] hover:text-[var(--vibe-btn-hover-color)]'
                }`}
                onClick={() => onModeChange('notifications')}
                title={t(locale, 'rightPanel.title.notifications')}
              >
                <Bell size={15} />
              </button>
              <button
                className={`flex items-center justify-center p-1.5 rounded-lg transition-all ${
                  mode === 'module-details'
                    ? 'bg-[var(--vibe-active-bg)] text-[var(--vibe-active-color)]'
                    : 'text-[var(--text-faint)] hover:bg-[var(--vibe-btn-hover-bg)] hover:text-[var(--vibe-btn-hover-color)]'
                }`}
                onClick={() => onModeChange('module-details')}
                title={t(locale, 'rightPanel.title.moduleDetails')}
              >
                <Info size={15} />
              </button>
            </>
          )}
        </div>
        <button
          className="flex items-center justify-center p-1.5 rounded-lg text-[var(--text-faint)] hover:bg-[var(--vibe-btn-hover-bg)] hover:text-[var(--vibe-btn-hover-color)] transition-all"
          onClick={handleClose}
          title={t(locale, 'rightPanel.closePanel')}
          aria-label="Close panel"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M18 6L6 18M6 6l12 12" />
          </svg>
        </button>
      </div>

      {/* Content area */}
      <div className="right-panel-content">
        {children || (
          <div className="flex flex-col items-center justify-center h-[200px] text-[var(--text-faint)] text-[13px]">
            <div className="mb-3 flex justify-center">
              <div className="w-10 h-10 rounded-full bg-[var(--vibe-btn-bg)] flex items-center justify-center">
                {mode === 'notifications' ? <Bell size={18} /> : <FileText size={18} />}
              </div>
            </div>
            <div className="text-center">
              {mode === 'notifications' ? t(locale, 'rightPanel.empty.notifications') : t(locale, 'rightPanel.empty.selectFile')}
            </div>
          </div>
        )}
      </div>
    </aside>
  );
}
