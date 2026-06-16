'use client';

import { useEffect, useRef, useState, useCallback } from 'react';
import { t, type Locale } from '@/i18n';
import { Grid3x3, List, ArrowUp, ArrowDown, Terminal, EyeOff, Eye, Menu } from 'lucide-react';
import FileBreadcrumb from '@/components/files/FileBreadcrumb';

const VIEW_LABELS: Record<string, string> = {
  dashboard: 'header.personal',
  files: '',
  ai: 'header.aiWorkbench',
  workshop: 'nav.workshop',
  settings: 'nav.settings',
  tools: 'nav.tools',
};

const DRAG = { WebkitAppRegion: 'drag' as const };
const NO_DRAG = { WebkitAppRegion: 'no-drag' as const };

interface FileState {
  viewMode: 'grid' | 'list';
  sortBy: 'name' | 'mtime' | 'size';
  sortDir: 'asc' | 'desc';
  showHidden: boolean;
  segments: string[];
  isFavorite: boolean;
  breadcrumbPath: string;
  projectBadge?: string | null;
}

export default function Header({
  activeView, onToggleSidebar, onToggleTerminal,
}: {
  activeView: string;
  onToggleSidebar: () => void;
  onToggleTerminal: () => void;
}) {
  const [locale, setLocale] = useState<Locale>('zh');
  const [tbClass, setTbClass] = useState('');
  const headerRef = useRef<HTMLElement>(null);

  // File-browser state (updated via event bridge)
  const [fileState, setFileState] = useState<FileState | null>(null);

  useEffect(() => {
    window.nativesAPI?.getLocale?.().then((l) => { if (l === 'en') setLocale('en'); }).catch(() => {});
  }, []);

  // Listen for file-browser state broadcast
  useEffect(() => {
    const handler = (e: Event) => {
      setFileState((e as CustomEvent).detail as FileState);
    };
    window.addEventListener('header-file-state', handler);
    return () => window.removeEventListener('header-file-state', handler);
  }, []);

  // Dispatch actions back to FileBrowser
  const dispatchAction = useCallback((type: string, value?: unknown) => {
    window.dispatchEvent(new CustomEvent('header-file-action', { detail: { type, value } }));
  }, []);

  // Responsive shrinking
  useEffect(() => {
    const el = headerRef.current;
    if (!el) return;
    const ro = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const w = entry.contentRect.width;
        if (w < 540) setTbClass('tb-min');
        else if (w < 660) setTbClass('tb-xxs');
        else if (w < 790) setTbClass('tb-xs');
        else if (w < 880) setTbClass('tb-sm');
        else setTbClass('');
      }
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const isFileView = activeView === 'files';
  const fs = fileState; // shorthand

  return (
    <header
      ref={headerRef}
      className={tbClass}
      style={{
        display: 'flex', alignItems: 'center', gap: 12,
        padding: '10px 16px',
        borderBottom: '1px solid var(--border, #262920)',
        background: 'var(--bg, #0b0c0a)',
        ...DRAG,
        userSelect: 'none',
        minHeight: 48,
      }}
    >
      {/* Left: sidebar toggle */}
      <button
        className="btn btn-ghost"
        onClick={onToggleSidebar}
        title={t(locale, 'sidebar.ariaToggle')}
        style={{ ...NO_DRAG, padding: '2px 6px', flexShrink: 0 }}
      >
        <Menu size={16} />
      </button>

      {/* Center: breadcrumb (file view) or view title (other views) */}
      <div style={{
        flex: '1 1 auto', minWidth: 130, overflow: 'auto',
        scrollbarWidth: 'none', msOverflowStyle: 'none',
        ...NO_DRAG,
      }}>
        {isFileView && fs ? (
          <FileBreadcrumb
            segments={fs.segments}
            onNavigate={(path) => {
              window.dispatchEvent(new CustomEvent('navigate-files', { detail: path }));
            }}
            isFavorite={fs.isFavorite}
            onToggleFavorite={() => {}}
            projectBadge={fs.projectBadge as any}
          />
        ) : (
          <span style={{ fontSize: 13, color: 'var(--text, #f2f2ea)', fontWeight: 500, whiteSpace: 'nowrap' }}>
            {VIEW_LABELS[activeView] ? t(locale, VIEW_LABELS[activeView]) : activeView}
          </span>
        )}
      </div>

      {/* Right: file-browser actions */}
      {isFileView && (
        <div style={{
          display: 'flex', alignItems: 'center', gap: 8, flexShrink: 0,
          ...NO_DRAG,
        }}>
          {/* Hidden toggle */}
          <label className={`switch ${tbClass.includes('tb-xs') || tbClass.includes('tb-sm') ? 'tb-hide' : ''}`} style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
            <input
              type="checkbox"
              checked={fs?.showHidden ?? false}
              onChange={() => dispatchAction('showHidden')}
              style={{ display: 'none' }}
            />
            <span className={`switch-track ${fs?.showHidden ? 'active' : ''}`}>
              <span className="switch-knob" />
            </span>
            <span style={{ fontSize: 11, color: 'var(--text-dim, #9b9d8c)', display: tbClass.includes('tb-xxs') ? 'none' : 'inline' }}>
              {fs?.showHidden ? <EyeOff size={12} /> : <Eye size={12} />}
            </span>
          </label>

          {/* Sort-by segmented control (name / time / size) */}
          <div className={`segmented-control ${tbClass.includes('tb-xxs') ? 'tb-hide' : ''}`} style={{ display: 'flex' }}>
            {(['name', 'mtime', 'size'] as const).map((key) => (
              <button
                key={key}
                className={`seg-item ${(fs?.sortBy ?? 'name') === key ? 'active' : ''}`}
                onClick={() => dispatchAction('sortBy', key)}
                style={{ fontSize: 10, padding: '2px 6px', lineHeight: '16px' }}
              >
                {key === 'name' ? 'A-Z' : key === 'mtime' ? t(locale, 'fileBrowser.modified') : t(locale, 'fileBrowser.size')}
              </button>
            ))}
          </div>

          {/* Sort direction */}
          <button
            className={`btn btn-ghost ${tbClass.includes('tb-min') ? 'tb-hide' : ''}`}
            onClick={() => dispatchAction('sortDir')}
            style={{ fontSize: 14, padding: '2px 6px' }}
            title={t(locale, (fs?.sortDir ?? 'asc') === 'asc' ? 'fileBrowser.ascending' : 'fileBrowser.descending')}
          >
            {(fs?.sortDir ?? 'asc') === 'asc' ? <ArrowUp size={12} /> : <ArrowDown size={12} />}
          </button>

          {/* View mode segmented control */}
          <div className={`segmented-control ${tbClass.includes('tb-xs') ? 'tb-hide' : ''}`} style={{ display: 'flex' }}>
            <button
              className={`seg-item ${(fs?.viewMode ?? 'grid') === 'grid' ? 'active' : ''}`}
              onClick={() => dispatchAction('viewMode', 'grid')}
              title={t(locale, 'fileBrowser.gridView')}
            >
              <Grid3x3 size={14} />
            </button>
            <button
              className={`seg-item ${(fs?.viewMode ?? 'grid') === 'list' ? 'active' : ''}`}
              onClick={() => dispatchAction('viewMode', 'list')}
              title={t(locale, 'fileBrowser.listView')}
            >
              <List size={14} />
            </button>
          </div>

          {/* Terminal toggle */}
          <button
            id="btn-terminal"
            className="btn btn-ghost"
            onClick={onToggleTerminal}
            style={{
              color: 'var(--accent, #cdf24b)',
              background: 'var(--accent-soft, #cdf24b15)',
              fontWeight: 700, padding: '2px 8px',
            }}
            title={t(locale, 'nav.terminalToggle')}
          >
            <Terminal size={14} />
          </button>
        </div>
      )}
    </header>
  );
}
