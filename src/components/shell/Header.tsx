'use client';

import { useEffect, useRef, useState } from 'react';
import { t, type Locale } from '@/i18n';
import { Grid3x3, List, ArrowUp, ArrowDown, Terminal, EyeOff, Eye, Menu } from 'lucide-react';
import FileBreadcrumb from '@/components/files/FileBreadcrumb';

interface HeaderProps {
  activeView: string;
  onToggleSidebar: () => void;
  onToggleTerminal: () => void;
  // File-browser props (only relevant when activeView === 'files')
  viewMode?: 'grid' | 'list';
  sortBy?: 'name' | 'mtime' | 'size';
  sortDir?: 'asc' | 'desc';
  showHidden?: boolean;
  breadcrumbSegments?: string[];
  onBreadcrumbNavigate?: (path: string) => void;
  breadcrumbIsFavorite?: boolean;
  onBreadcrumbToggleFavorite?: () => void;
  onViewModeChange?: (mode: 'grid' | 'list') => void;
  onSortChange?: (sortBy: 'name' | 'mtime' | 'size') => void;
  onSortDirChange?: (dir: 'asc' | 'desc') => void;
  onShowHiddenChange?: (show: boolean) => void;
}

const VIEW_LABELS: Record<string, string> = {
  dashboard: 'dashboard.title',
  files: '',
  ai: 'nav.aiWorkbench',
  workshop: 'nav.workshop',
  settings: 'nav.settings',
  tools: 'nav.tools',
};

// Helper to create WebkitAppRegion style values
const DRAG = { WebkitAppRegion: 'drag' as const };
const NO_DRAG = { WebkitAppRegion: 'no-drag' as const };

export default function Header({
  activeView, onToggleSidebar, onToggleTerminal,
  viewMode, sortBy, sortDir, showHidden,
  breadcrumbSegments, onBreadcrumbNavigate,
  breadcrumbIsFavorite, onBreadcrumbToggleFavorite,
  onViewModeChange, onSortDirChange, onShowHiddenChange,
}: HeaderProps) {
  const [locale, setLocale] = useState<Locale>('zh');
  const [tbClass, setTbClass] = useState('');
  const headerRef = useRef<HTMLElement>(null);

  useEffect(() => {
    window.nativesAPI?.getLocale?.().then((l) => { if (l === 'en') setLocale('en'); }).catch(() => {});
  }, []);

  // Responsive shrinking (fanbox-style: ResizeObserver)
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
  const showBreadcrumb = isFileView && breadcrumbSegments;

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

      {/* Center: breadcrumb or view title */}
      <div style={{
        flex: '1 1 auto', minWidth: 130, overflow: 'auto',
        scrollbarWidth: 'none', msOverflowStyle: 'none',
        ...NO_DRAG,
      }}>
        {showBreadcrumb ? (
          <FileBreadcrumb
            segments={breadcrumbSegments}
            onNavigate={onBreadcrumbNavigate!}
            isFavorite={breadcrumbIsFavorite}
            onToggleFavorite={onBreadcrumbToggleFavorite}
          />
        ) : (
          <span style={{ fontSize: 13, color: 'var(--text, #f2f2ea)', fontWeight: 500, whiteSpace: 'nowrap' }}>
            {VIEW_LABELS[activeView] ? t(locale, VIEW_LABELS[activeView]) : activeView}
          </span>
        )}
      </div>

      {/* Right: file-browser actions (only in file view) */}
      {isFileView && (
        <div style={{
          display: 'flex', alignItems: 'center', gap: 8, flexShrink: 0,
          ...NO_DRAG,
        }}>
          {/* Hidden toggle */}
          <label
            className={`switch ${tbClass.includes('tb-xs') || tbClass.includes('tb-sm') ? 'tb-hide' : ''}`}
            style={{ display: 'flex', alignItems: 'center', gap: 4 }}
          >
            <input
              type="checkbox"
              checked={showHidden}
              onChange={(e) => onShowHiddenChange?.(e.target.checked)}
              style={{ display: 'none' }}
            />
            <span className={`switch-track ${showHidden ? 'active' : ''}`}>
              <span className="switch-knob" />
            </span>
            <span style={{ fontSize: 11, color: 'var(--text-dim, #9b9d8c)', display: tbClass.includes('tb-xxs') ? 'none' : 'inline' }}>
              {showHidden ? <EyeOff size={12} /> : <Eye size={12} />}
            </span>
          </label>

          {/* Sort direction */}
          <button
            className={`btn btn-ghost ${tbClass.includes('tb-min') ? 'tb-hide' : ''}`}
            onClick={() => onSortDirChange?.(sortDir === 'asc' ? 'desc' : 'asc')}
            style={{ fontSize: 14, padding: '2px 6px' }}
            title={t(locale, sortDir === 'asc' ? 'fileBrowser.ascending' : 'fileBrowser.descending')}
          >
            {sortDir === 'asc' ? <ArrowUp size={12} /> : <ArrowDown size={12} />}
          </button>

          {/* View mode segmented control */}
          <div className={`segmented-control ${tbClass.includes('tb-xs') ? 'tb-hide' : ''}`} style={{ display: 'flex' }}>
            <button
              className={`seg-item ${viewMode === 'grid' ? 'active' : ''}`}
              onClick={() => onViewModeChange?.('grid')}
              title={t(locale, 'fileBrowser.gridView')}
            >
              <Grid3x3 size={14} />
            </button>
            <button
              className={`seg-item ${viewMode === 'list' ? 'active' : ''}`}
              onClick={() => onViewModeChange?.('list')}
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
