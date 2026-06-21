'use client';

import { startTransition, Fragment, useEffect, useRef, useState, useCallback, useMemo } from 'react';
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
  HardDrive,
  MoreHorizontal,
  LayoutGrid,
} from 'lucide-react';

// Tauri v2: drag region via data-tauri-drag-region attribute; no-drag is automatic
// for interactive elements (buttons, inputs, etc.)

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
  gridSize: 'sm' | 'md' | 'lg';
  segments: string[];
  isFavorite: boolean;
  breadcrumbPath: string;
  projectBadge?: string | null;
}

// Elastic breadcrumb. Layout rule (a segment = one clickable dir):
//   • Always show:  head anchor  →  … →  last two segments
//   • “~”      : /Users/<name> or /home/<name> collapsed to a single home crumb
//   • “/”      : pure filesystem root, rendered as a hard-drive icon (non-home paths only)
//   • Collapsing kicks in only when there are >= 4 displayed items, and only
//     the *middle* is hidden — anchors on both ends stay, so deep paths never
//     lose context. Click … to expand inline.
const COLLAPSE_THRESHOLD = 4; // collapse when total displayed items >= this

interface CrumbItem {
  /** Display label, e.g. "~" or "Documents" */
  label: string;
  /** Absolute path this crumb navigates to */
  path: string;
  /** Optional icon node rendered before the label */
  icon?: React.ReactNode;
}

/** Build the crumb list from the raw segments coming from FileBrowser. */
function buildCrumbs(segments: string[]): CrumbItem[] {
  // FileBrowser yields ['/'] when at the filesystem root
  if (segments.length === 0 || (segments.length === 1 && segments[0] === '/')) {
    return [{ label: '/', path: '/', icon: <HardDrive size={14} /> }];
  }

  // Detect a home-rooted path so /Users/<name> or /home/<name> collapses to ~.
  // segments[0] is “Users” / “home”, segments[1] is the user dir.
  const isHomePath =
    (segments[0] === 'Users' || segments[0] === 'home') && segments.length >= 2;

  const items: CrumbItem[] = [];

  if (isHomePath) {
    // Anchor: ~ → /Users/<name> (the home directory itself)
    items.push({
      label: '~',
      path: '/' + segments.slice(0, 2).join('/'),
      icon: <Home size={14} />,
    });
    // Everything deeper than the home dir becomes normal segments.
    for (let i = 2; i < segments.length; i++) {
      const seg = segments[i]!;
      items.push({
        label: seg,
        path: '/' + segments.slice(0, i + 1).join('/'),
      });
    }
  } else {
    // Non-home absolute path: prepend a filesystem-root entry so users can
    // still jump back to “/”, then one crumb per real segment.
    items.push({ label: '/', path: '/', icon: <HardDrive size={14} /> });
    for (let i = 0; i < segments.length; i++) {
      const seg = segments[i]!;
      items.push({
        label: seg,
        path: '/' + segments.slice(0, i + 1).join('/'),
      });
    }
  }

  return items;
}

function BreadcrumbPath({
  segments,
  onNavigate,
  locale,
}: {
  segments: string[];
  onNavigate: (path: string) => void;
  locale: Locale;
}) {
  const crumbs = useMemo(() => buildCrumbs(segments), [segments]);
  const [expanded, setExpanded] = useState(false);
  const overflowRef = useRef<HTMLDivElement>(null);
  const activeRef = useRef<HTMLDivElement>(null);

  // Reset collapse whenever path changes
  useEffect(() => {
    startTransition(() => { setExpanded(false); });
  }, [crumbs]);

  // When expanded, auto-scroll the active (last) crumb into view so the user
  // lands on their current location instead of the path’s start.
  useEffect(() => {
    if (expanded && overflowRef.current && activeRef.current) {
      overflowRef.current.scrollLeft = overflowRef.current.scrollWidth;
    }
  }, [expanded, crumbs]);

  // Same for a plain path change while already expanded.
  useEffect(() => {
    if (expanded && overflowRef.current) {
      overflowRef.current.scrollLeft = overflowRef.current.scrollWidth;
    }
  });

  const total = crumbs.length;
  const showCompact = !expanded && total >= COLLAPSE_THRESHOLD;

  // In compact mode we render: [head …] [ … ] [… last-1, last]
  const headCount = 1; // always anchor the first item
  const tailCount = 2; // keep the last two for context

  const renderCrumb = (crumb: CrumbItem, isLast: boolean, keyPrefix: string) => (
    <button
      key={`${keyPrefix}-${crumb.path}`}
      className={`group inline-flex h-8 max-w-[180px] items-center gap-1 rounded-md px-2 text-xs transition-colors shrink min-w-0 ${
        isLast
          ? 'bg-[var(--vibe-active-bg)] text-[var(--vibe-active-color)] font-medium'
          : 'text-[var(--vibe-btn-text)] hover:bg-[var(--vibe-btn-bg)] hover:text-[var(--vibe-btn-hover-color)]'
      }`}
      onClick={() => onNavigate(crumb.path)}
      title={crumb.path}
    >
      {crumb.icon && <span className="shrink-0">{crumb.icon}</span>}
      <span className="truncate">{crumb.label}</span>
    </button>
  );

  const separator = (
    <ChevronRight size={12} className="mx-0.5 shrink-0 text-[var(--text-faint)]" aria-hidden />
  );

  // Single root crumb — no separators, no scroll container needed.
  if (total === 1) {
    return (
      <div ref={activeRef}>
        {renderCrumb(crumbs[0]!, true, 'solo')}
      </div>
    );
  }

  // Build the inline (expanded) row.
  const expandedRow = (
    <div
      ref={overflowRef}
      className="flex items-center gap-0.5 overflow-x-auto min-w-0 scrollbar-none"
    >
      {crumbs.map((crumb, i) => (
        <Fragment key={`crumb-${i}`}>
          {i > 0 && separator}
          <span ref={i === total - 1 ? activeRef : undefined} className="inline-flex min-w-0">
            {renderCrumb(crumb, i === total - 1, 'exp')}
          </span>
        </Fragment>
      ))}
    </div>
  );

  if (!showCompact) {
    return expanded ? expandedRow : (
      <div className="flex items-center gap-0.5 overflow-hidden min-w-0">
        {crumbs.map((crumb, i) => (
          <Fragment key={`crumb-${i}`}>
            {i > 0 && separator}
            {renderCrumb(crumb, i === total - 1, 'row')}
          </Fragment>
        ))}
      </div>
    );
  }

  // Compact row: head › … › tail(-2) › tail(-1)
  const head = crumbs.slice(0, headCount);
  const tail = crumbs.slice(total - tailCount);

  return (
    <div className="flex items-center gap-0.5 overflow-hidden min-w-0">
      {head.map((c, i) => (
        <Fragment key="head">
          {renderCrumb(c, false, `h-${i}`)}
        </Fragment>
      ))}
      {separator}
      <button
        className="inline-flex h-8 shrink-0 items-center rounded-md px-1.5 text-xs text-[var(--vibe-btn-text)] transition-colors hover:bg-[var(--vibe-btn-bg)] hover:text-[var(--vibe-btn-hover-color)]"
        onClick={() => setExpanded(true)}
        title={t(locale, 'header.showFull')}
        aria-label={t(locale, 'header.showFull')}
      >
        <MoreHorizontal size={14} />
      </button>
      {separator}
      {tail.map((crumb, i) => (
        <Fragment key={`tail-${i}`}>
          {renderCrumb(crumb, i === tail.length - 1, `t-${i}`)}
        </Fragment>
      ))}
    </div>
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
      data-tauri-drag-region
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
        >
          <PanelLeft size={15} />
        </button>
      )}

      {/* 根据 activeView 渲染不同导航 */}
      {isFileView ? (
        /* ── 文件浏览器：动态面包屑 + 控件 ── */
        <>
          {/* Dynamic breadcrumbs — smart truncation handled inside BreadcrumbPath */}
          <nav
            aria-label={t(locale, 'fileBrowser.breadcrumbLabel')}
            className="flex min-w-0 flex-1 items-center"
          >
            <BreadcrumbPath
              segments={fs?.segments ?? []}
              onNavigate={(path) => window.dispatchEvent(new CustomEvent('navigate-files', { detail: path }))}
              locale={locale}
            />
          </nav>

          {/* New Folder */}
          <div className="flex items-center">
            <button className="vibe-btn !h-8" onClick={() => dispatchAction('newFolder', fs?.breadcrumbPath ?? '/')} title={t(locale, 'fileBrowser.newFolder')}>
              <FolderPlus size={14} />
              <span className="text-xs">{t(locale, 'fileBrowser.newFolder')}</span>
            </button>
          </div>

          {/* View mode toggle + Sort + Filter + Search */}
          <div className="flex items-center gap-1.5">
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

            {/* Grid size toggle — only visible in grid mode */}
            {(fs?.viewMode ?? 'grid') === 'grid' && (
              <div className="flex items-center gap-0.5 rounded-lg bg-[var(--vibe-btn-bg)] p-0.5 border border-[var(--vibe-btn-border)]">
                {([
                  { key: 'sm' as const, label: 'S' },
                  { key: 'md' as const, label: 'M' },
                  { key: 'lg' as const, label: 'L' },
                ]).map((opt) => (
                  <button
                    key={opt.key}
                    className={`flex h-7 w-7 items-center justify-center rounded-md text-[10px] font-bold transition-all ${
                      (fs?.gridSize ?? 'md') === opt.key
                        ? 'bg-[var(--vibe-active-bg)] text-[var(--vibe-active-color)] shadow-sm'
                        : 'text-[var(--vibe-btn-text)] hover:text-[var(--vibe-btn-hover-color)]'
                    }`}
                    onClick={() => dispatchAction('gridSize', opt.key)}
                    title={t(locale, `fileBrowser.gridSize${opt.key.toUpperCase()}` as any)}
                  >
                    {opt.label}
                  </button>
                ))}
              </div>
            )}

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
            <div className="relative flex items-center" >
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
        <div className="flex items-center gap-2 flex-1 min-w-0" >
          <span className="text-sm font-semibold text-[#2f3136]">
            {VIEW_LABELS[activeView] ? t(locale, VIEW_LABELS[activeView]) : activeView}
          </span>
        </div>
      )}
    </header>
  );
}
