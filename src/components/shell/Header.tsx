'use client';

import { useEffect, useRef, useState, useCallback } from 'react';
import { t, type Locale } from '@/i18n';
import {
  Grid3x3,
  List,
  Home,
  ChevronRight,
  FolderPlus,
  Search,
  ArrowUpDown,
  SlidersHorizontal,
  X,
  ArrowUp,
  ArrowDown,
  Eye,
  EyeOff,
  PanelLeft,
} from 'lucide-react';

const NO_DRAG = { WebkitAppRegion: 'no-drag' as const } as React.CSSProperties;

const VIEW_LABELS: Record<string, string> = {
  dashboard: 'nav.dashboard',
  files: '',
  ai: 'header.aiWorkbench',
  workshop: 'nav.workshop',
  settings: 'nav.settings',
  tools: 'nav.tools',
  modules: 'nav.modules',
  store: 'nav.store',
};

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

// Elastic breadcrumb: first + … + last when > 3 segments; click … to expand
const CRUMB_COLLAPSE_THRESHOLD = 3;

function BreadcrumbPath({
  segments,
  onNavigate,
  locale,
}: {
  segments: string[];
  onNavigate: (path: string) => void;
  locale: Locale;
}) {
  const [expanded, setExpanded] = useState(false);

  // Reset collapse whenever path changes
  useEffect(() => {
    setExpanded(false);
  }, [segments.join('/')]);

  // Root path — just a home icon
  if (segments.length === 0 || (segments.length === 1 && segments[0] === '/')) {
    return (
      <button
        className="vibe-btn !h-8 text-xs active"
        onClick={() => onNavigate('/')}
        title="/"
      >
        <Home size={14} className="inline" />
      </button>
    );
  }

  const total = segments.length;
  const showCompact = !expanded && total > CRUMB_COLLAPSE_THRESHOLD;

  const renderSegment = (segIndex: number) => {
    const seg = segments[segIndex];
    const isLast = segIndex === total - 1;
    const displayName = segIndex === 0 ? '~' : seg;
    const path = segIndex === 0 ? '/' : '/' + segments.slice(0, segIndex + 1).join('/');

    return (
      <button
        key={`seg-${segIndex}`}
        className={`vibe-btn !h-8 text-xs truncate ${isLast ? 'active max-w-[200px]' : 'max-w-[140px]'}`}
        onClick={() => onNavigate(path)}
        title={segIndex === 0 ? '/' : path}
      >
        {displayName}
      </button>
    );
  };

  return (
    <>
      {/* Root — always visible */}
      {renderSegment(0)}

      {showCompact ? (
        <>
          <ChevronRight size={12} className="text-[var(--text-faint)] shrink-0" />
          <button
            className="vibe-btn !h-8 text-xs px-1.5 shrink-0"
            onClick={() => setExpanded(true)}
            title={t(locale, 'header.showFull')}
          >
            …
          </button>
          <ChevronRight size={12} className="text-[var(--text-faint)] shrink-0" />
          {/* Last segment — always visible */}
          {renderSegment(total - 1)}
        </>
      ) : (
        /* Full view: root › seg1 › seg2 › … › last */
        segments.slice(1).map((_, offset) => {
          const segIndex = offset + 1;
          return (
            <span key={`wrap-${segIndex}`} className="flex items-center gap-1.5 min-w-0 overflow-hidden">
              <ChevronRight size={12} className="text-[var(--text-faint)] shrink-0" />
              {renderSegment(segIndex)}
            </span>
          );
        })
      )}
    </>
  );
}

export default function Header({
  activeView,
  sidebarCollapsed,
  onToggleSidebar,
}: {
  activeView: string;
  sidebarCollapsed?: boolean;
  onToggleSidebar?: () => void;
}) {
  const [locale, setLocale] = useState<Locale>('zh');
  const [tbClass, setTbClass] = useState('');
  const headerRef = useRef<HTMLElement>(null);

  // File-browser state (updated via event bridge)
  const [fileState, setFileState] = useState<FileState | null>(null);

  // Local UI state for sort dropdown, filter dropdown & search
  const [sortOpen, setSortOpen] = useState(false);
  const [filterOpen, setFilterOpen] = useState(false);
  const [searchOpen, setSearchOpen] = useState(false);
  const searchRef = useRef<HTMLInputElement>(null);
  const sortRef = useRef<HTMLDivElement>(null);
  const filterRef = useRef<HTMLDivElement>(null);

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

  // Click outside to close sort/filter/search popups
  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (sortRef.current && !sortRef.current.contains(e.target as Node)) {
        setSortOpen(false);
      }
      if (filterRef.current && !filterRef.current.contains(e.target as Node)) {
        setFilterOpen(false);
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, []);

  // Focus search input when opened
  useEffect(() => {
    if (searchOpen && searchRef.current) {
      searchRef.current.focus();
    }
  }, [searchOpen]);

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
  const fs = fileState;

  return (
    <header
      ref={headerRef}
      className={`vibe-toolbar flex items-center gap-3 px-4 ${tbClass}`}
      style={{
        userSelect: 'none',
      }}
    >
      {/* Sidebar toggle — always visible */}
      {onToggleSidebar && (
        <button
          className={`vibe-btn !h-8 !w-8 !p-0 flex items-center justify-center shrink-0 ${sidebarCollapsed ? '' : 'active'}`}
          onClick={onToggleSidebar}
          title={t(locale, sidebarCollapsed ? 'sidebar.expand' : 'sidebar.collapse')}
          style={NO_DRAG}
        >
          <PanelLeft size={15} />
        </button>
      )}

      {/* 根据 activeView 渲染不同导航 */}
      {isFileView ? (
        /* ── 文件浏览器：动态面包屑 + 控件 ── */
        <>
          {/* Dynamic breadcrumbs — smart truncation */}
          <div className="flex items-center gap-1.5 flex-1 min-w-0 overflow-x-auto scrollbar-none" style={NO_DRAG}>
            <BreadcrumbPath
              segments={fs?.segments ?? []}
              onNavigate={(path) => window.dispatchEvent(new CustomEvent('navigate-files', { detail: path }))}
              locale={locale}
            />
          </div>

          {/* New Folder */}
          <div className="flex items-center" style={NO_DRAG}>
            <button className="vibe-btn !h-8" onClick={() => dispatchAction('newFolder', fs?.breadcrumbPath ?? '/')} title={t(locale, 'fileBrowser.newFolder')}>
              <FolderPlus size={14} />
              <span className="text-xs">{t(locale, 'fileBrowser.newFolder')}</span>
            </button>
          </div>

          {/* View mode toggle + Sort + Filter + Search */}
          <div className="flex items-center gap-1.5" style={NO_DRAG}>
            {/* View mode toggle */}
            <div className="flex items-center gap-0.5 rounded-lg bg-[var(--vibe-btn-bg)] p-0.5 border border-[var(--vibe-btn-border)]">
              <button
                className={`flex h-7 w-7 items-center justify-center rounded-md transition-all ${
                  (fs?.viewMode ?? 'grid') === 'grid'
                    ? 'bg-[var(--vibe-active-bg)] text-[var(--vibe-active-color)] shadow-sm'
                    : 'text-[var(--vibe-btn-text)] hover:text-[var(--vibe-btn-hover-color)]'
                }`}
                onClick={() => dispatchAction('viewMode', 'grid')}
                title={t(locale, 'fileBrowser.gridView')}
              >
                <Grid3x3 size={14} />
              </button>
              <button
                className={`flex h-7 w-7 items-center justify-center rounded-md transition-all ${
                  (fs?.viewMode ?? 'grid') === 'list'
                    ? 'bg-[var(--vibe-active-bg)] text-[var(--vibe-active-color)] shadow-sm'
                    : 'text-[var(--vibe-btn-text)] hover:text-[var(--vibe-btn-hover-color)]'
                }`}
                onClick={() => dispatchAction('viewMode', 'list')}
                title={t(locale, 'fileBrowser.listView')}
              >
                <List size={14} />
              </button>
            </div>

            {/* Sort — dropdown */}
            <div ref={sortRef} className="relative">
              <button
                className="vibe-btn !h-8 text-xs"
                onClick={() => setSortOpen((v) => !v)}
                title={t(locale, 'fileBrowser.sort')}
              >
                <ArrowUpDown size={12} />
                <span>{t(locale, 'fileBrowser.sort')}</span>
              </button>
              {sortOpen && (
                <div
                  className="absolute right-0 top-full mt-1 z-50 min-w-[160px] rounded-lg border border-[var(--vibe-btn-border)] bg-[var(--vibe-toolbar-bg)] backdrop-blur-xl p-1 shadow-xl"
                  style={{ ...NO_DRAG }}
                >
                  {([
                    { key: 'name', label: t(locale, 'fileBrowser.sortByName') },
                    { key: 'mtime', label: t(locale, 'fileBrowser.sortByModified') },
                    { key: 'size', label: t(locale, 'fileBrowser.sortBySize') },
                  ] as const).map((opt) => (
                    <button
                      key={opt.key}
                      className={`flex w-full items-center justify-between gap-3 rounded-md px-3 py-1.5 text-xs transition-all ${
                        fs?.sortBy === opt.key
                          ? 'bg-[var(--vibe-active-bg)] text-[var(--vibe-active-color)]'
                          : 'text-[var(--vibe-btn-text)] hover:bg-[var(--vibe-btn-bg)]'
                      }`}
                      onClick={() => {
                        dispatchAction('sortBy', opt.key);
                        setSortOpen(false);
                      }}
                    >
                      <span>{opt.label}</span>
                      {fs?.sortBy === opt.key && (
                        <span className="text-[10px] opacity-60">
                          {fs?.sortDir === 'asc' ? <ArrowUp size={12} /> : <ArrowDown size={12} />}
                        </span>
                      )}
                    </button>
                  ))}
                  <div className="mx-2 my-1 border-t border-[var(--vibe-btn-border)]" />
                  <button
                    className="flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-xs text-[var(--vibe-btn-text)] hover:bg-[var(--vibe-btn-bg)] transition-all"
                    onClick={() => {
                      dispatchAction('sortDir');
                      setSortOpen(false);
                    }}
                  >
                    {fs?.sortDir === 'asc' ? (
                      <><ArrowUp size={12} /> {t(locale, 'fileBrowser.ascending')}</>
                    ) : (
                      <><ArrowDown size={12} /> {t(locale, 'fileBrowser.descending')}</>
                    )}
                  </button>
                </div>
              )}
            </div>

            {/* Filter — dropdown */}
            <div ref={filterRef} className="relative">
              <button
                className={`vibe-btn !h-8 text-xs ${fs?.showHidden ? 'text-[var(--vibe-active-color)]' : ''}`}
                onClick={() => setFilterOpen((v) => !v)}
                title={t(locale, 'fileBrowser.filter')}
              >
                {fs?.showHidden ? <EyeOff size={12} /> : <Eye size={12} />}
                <span>{t(locale, 'fileBrowser.filter')}</span>
              </button>
              {filterOpen && (
                <div
                  className="absolute right-0 top-full mt-1 z-50 min-w-[140px] rounded-lg border border-[var(--vibe-btn-border)] bg-[var(--vibe-toolbar-bg)] backdrop-blur-xl p-1 shadow-xl"
                  style={{ ...NO_DRAG }}
                >
                  <button
                    className={`flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-xs transition-all ${
                      !fs?.showHidden
                        ? 'bg-[var(--vibe-active-bg)] text-[var(--vibe-active-color)]'
                        : 'text-[var(--vibe-btn-text)] hover:bg-[var(--vibe-btn-bg)]'
                    }`}
                    onClick={() => {
                      if (fs?.showHidden) dispatchAction('showHidden');
                      setFilterOpen(false);
                    }}
                  >
                    <Eye size={12} />
                    <span>{t(locale, 'fileBrowser.hideHidden')}</span>
                  </button>
                  <button
                    className={`flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-xs transition-all ${
                      fs?.showHidden
                        ? 'bg-[var(--vibe-active-bg)] text-[var(--vibe-active-color)]'
                        : 'text-[var(--vibe-btn-text)] hover:bg-[var(--vibe-btn-bg)]'
                    }`}
                    onClick={() => {
                      if (!fs?.showHidden) dispatchAction('showHidden');
                      setFilterOpen(false);
                    }}
                  >
                    <EyeOff size={12} />
                    <span>{t(locale, 'fileBrowser.showHidden')}</span>
                  </button>
                </div>
              )}
            </div>

            {/* Search */}
            <div className="relative flex items-center" style={NO_DRAG}>
              {searchOpen ? (
                <div className="flex items-center gap-1 rounded-lg bg-[var(--vibe-btn-bg)] border border-[var(--vibe-btn-border)] px-2 py-1">
                  <Search size={12} className="text-[var(--text-faint)] shrink-0" />
                  <input
                    ref={searchRef}
                    type="text"
                    className="bg-transparent border-none outline-none focus-visible:outline-none text-xs text-[var(--text)] w-[120px] placeholder:text-[var(--text-faint)]"
                    placeholder={t(locale, 'fileBrowser.searchPlaceholder')}
                    onChange={(e) => dispatchAction('search', e.target.value)}
                    onKeyDown={(e) => e.key === 'Escape' && setSearchOpen(false)}
                  />
                  <button
                    className="flex items-center justify-center text-[var(--text-faint)] hover:text-[var(--text)] transition-colors"
                    onClick={() => { setSearchOpen(false); dispatchAction('search', ''); }}
                    title={t(locale, 'common.close')}
                  >
                    <X size={12} />
                  </button>
                </div>
              ) : (
                <button
                  className="vibe-btn !h-8 !w-8 !p-0 flex items-center justify-center"
                  onClick={() => setSearchOpen(true)}
                  title={t(locale, 'fileBrowser.searchPlaceholder')}
                >
                  <Search size={14} />
                </button>
              )}
            </div>
          </div>
        </>
      ) : (
        /* ── 其他视图：显示路由标题 ── */
        <div className="flex items-center gap-2 flex-1 min-w-0" style={NO_DRAG}>
          <span className="text-sm font-semibold text-[#2f3136]">
            {VIEW_LABELS[activeView] ? t(locale, VIEW_LABELS[activeView]) : activeView}
          </span>
        </div>
      )}
    </header>
  );
}
