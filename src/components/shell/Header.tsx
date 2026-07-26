'use client';

import { startTransition, Fragment, useEffect, useRef, useState, useCallback, useMemo } from 'react';
import { motion } from 'framer-motion';
import { t, type Locale } from '@/i18n';
import {
  FILE_EVENTS,
  dispatchFileEvent,
  onFileEvent,
  type HeaderFileAction,
  type HeaderFileState,
} from '@/lib/file-events';
import {
  Grid3x3,
  List,
  Home,
  ChevronRight,
  FolderPlus,
  Search,
  ArrowUpDown,
  X,
  ArrowUp,
  ArrowDown,
  ArrowLeft,
  ArrowRight,
  Eye,
  EyeOff,
  PanelLeft,
  HardDrive,
  MoreHorizontal,
  FolderSearch,
  RefreshCw,
} from 'lucide-react';

// Tauri v2: drag region via data-tauri-drag-region attribute; no-drag is automatic
// for interactive elements (buttons, inputs, etc.)

const VIEW_LABELS: Record<string, string> = {
  dashboard: 'nav.dashboard',
  files: '',
  ai: 'header.aiWorkbench',
  workshop: 'nav.modules',
  jobs: 'nav.jobs',
  capabilities: 'nav.capabilities',
  assistant: 'header.aiWorkbench',
  settings: 'nav.settings',
  tools: 'nav.tools',
  modules: 'nav.modules',
  store: 'nav.modules',
};

// 文件浏览器上行状态的类型契约统一由 file-events.ts 提供
type FileState = HeaderFileState;

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
  // Must declare deps — a bare useEffect re-runs after every render.
  useEffect(() => {
    if (expanded && overflowRef.current) {
      overflowRef.current.scrollLeft = overflowRef.current.scrollWidth;
    }
  }, [expanded, crumbs]);

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
          ? 'bg-[var(--primary-soft)] text-[var(--primary)] font-medium'
          : 'text-[var(--text-secondary)] hover:bg-[var(--surface)] hover:text-[var(--primary)]'
      }`}
      onClick={() => onNavigate(crumb.path)}
      title={crumb.path}
    >
      {crumb.icon && <span className="shrink-0">{crumb.icon}</span>}
      <span className="truncate">{crumb.label}</span>
    </button>
  );

  const separator = (
    <ChevronRight size={12} className="mx-0.5 shrink-0 text-[var(--text-disabled)]" aria-hidden />
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
        className="inline-flex h-8 shrink-0 items-center rounded-md px-1.5 text-xs text-[var(--text-secondary)] transition-colors hover:bg-[var(--surface)] hover:text-[var(--primary)]"
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

  // 监听文件浏览器状态上行广播（file-events 契约）
  useEffect(() => onFileEvent(FILE_EVENTS.headerFileState, setFileState), []);

  // 下行动作回传 FileBrowser（discriminated union，编译期对齐两端）
  const dispatchAction = useCallback((action: HeaderFileAction) => {
    dispatchFileEvent(FILE_EVENTS.headerFileAction, action);
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
      className="flex items-center gap-3 px-4 bg-[var(--surface)] border-b border-[var(--border)] min-h-[40px] tb-hide"
      style={{
        userSelect: 'none',
      }}
    >
      {/* Restore the sidebar from the workspace only while it is hidden. */}
      {sidebarCollapsed && onToggleSidebar && (
        <button
          className="btn-secondary-v1 !h-8 !w-8 !p-0 flex items-center justify-center shrink-0"
          onClick={onToggleSidebar}
          title={t(locale, 'sidebar.expand')}
        >
          <PanelLeft size={15} />
        </button>
      )}

      {/* 根据 activeView 渲染不同导航 */}
      {isFileView ? (
        /* ── 文件浏览器：导航 + 动态面包屑 + 控件 ── */
        <>
          {/* History / up / refresh — always visible in header chrome */}
          <div className="flex items-center gap-0.5 shrink-0">
            <button
              className="btn-secondary-v1 !h-8 !w-8 !p-0 flex items-center justify-center"
              disabled={!fs?.canGoBack}
              onClick={() => dispatchAction({ type: 'back' })}
              title={`${t(locale, 'fileBrowser.back')} ⌘[`}
              style={{ opacity: fs?.canGoBack ? 1 : 0.35 }}
            >
              <ArrowLeft size={14} />
            </button>
            <button
              className="btn-secondary-v1 !h-8 !w-8 !p-0 flex items-center justify-center"
              disabled={!fs?.canGoForward}
              onClick={() => dispatchAction({ type: 'forward' })}
              title={`${t(locale, 'fileBrowser.forward')} ⌘]`}
              style={{ opacity: fs?.canGoForward ? 1 : 0.35 }}
            >
              <ArrowRight size={14} />
            </button>
            <button
              className="btn-secondary-v1 !h-8 !w-8 !p-0 flex items-center justify-center"
              disabled={!fs?.canGoUp}
              onClick={() => dispatchAction({ type: 'up' })}
              title={t(locale, 'fileBrowser.goUp')}
              style={{ opacity: fs?.canGoUp ? 1 : 0.35 }}
            >
              <ArrowUp size={14} />
            </button>
            <button
              className="btn-secondary-v1 !h-8 !w-8 !p-0 flex items-center justify-center"
              onClick={() => dispatchAction({ type: 'refresh' })}
              title={t(locale, 'fileBrowser.refresh')}
            >
              <RefreshCw size={13} style={{ animation: fs?.loading ? 'spin 0.8s linear infinite' : undefined }} />
            </button>
          </div>

          {/* Dynamic breadcrumbs — smart truncation handled inside BreadcrumbPath */}
          <nav
            aria-label={t(locale, 'fileBrowser.breadcrumbLabel')}
            className="flex min-w-0 flex-1 items-center"
          >
            <BreadcrumbPath
              segments={fs?.segments ?? []}
              onNavigate={(path) => dispatchFileEvent(FILE_EVENTS.navigateFiles, path)}
              locale={locale}
            />
          </nav>

          {/* New Folder */}
          <div className="flex items-center">
            <button className="btn-secondary-v1 !h-8" onClick={() => dispatchAction({ type: 'newFolder', value: fs?.breadcrumbPath ?? '/' })} title={t(locale, 'fileBrowser.newFolder')}>
              <FolderPlus size={14} />
              <span className="text-xs">{t(locale, 'fileBrowser.newFolder')}</span>
            </button>
          </div>

          {/* View mode toggle + Sort + Filter + Search */}
          <div className="flex items-center gap-1.5">
            {/* View mode toggle */}
            <div className="flex items-center gap-0.5 rounded-lg bg-[var(--surface)] p-0.5 border border-[var(--border)]">
              <button
                className={`flex h-7 w-7 items-center justify-center rounded-md transition-all ${
                  (fs?.viewMode ?? 'grid') === 'grid'
                    ? 'bg-[var(--primary-soft)] text-[var(--primary)] shadow-sm'
                    : 'text-[var(--text-secondary)] hover:text-[var(--primary)]'
                }`}
                onClick={() => dispatchAction({ type: 'viewMode', value: 'grid' })}
                title={t(locale, 'fileBrowser.gridView')}
              >
                <Grid3x3 size={14} />
              </button>
              <button
                className={`flex h-7 w-7 items-center justify-center rounded-md transition-all ${
                  (fs?.viewMode ?? 'grid') === 'list'
                    ? 'bg-[var(--primary-soft)] text-[var(--primary)] shadow-sm'
                    : 'text-[var(--text-secondary)] hover:text-[var(--primary)]'
                }`}
                onClick={() => dispatchAction({ type: 'viewMode', value: 'list' })}
                title={t(locale, 'fileBrowser.listView')}
              >
                <List size={14} />
              </button>
            </div>

            {/* Grid size toggle — only visible in grid mode */}
            {(fs?.viewMode ?? 'grid') === 'grid' && (
              <div className="flex items-center gap-0.5 rounded-lg bg-[var(--surface)] p-0.5 border border-[var(--border)]">
                {([
                  { key: 'sm' as const, label: 'S' },
                  { key: 'md' as const, label: 'M' },
                  { key: 'lg' as const, label: 'L' },
                ]).map((opt) => (
                  <button
                    key={opt.key}
                    className={`flex h-7 w-7 items-center justify-center rounded-md text-[10px] font-bold transition-all ${
                      (fs?.gridSize ?? 'md') === opt.key
                        ? 'bg-[var(--primary-soft)] text-[var(--primary)] shadow-sm'
                        : 'text-[var(--text-secondary)] hover:text-[var(--primary)]'
                    }`}
                    onClick={() => dispatchAction({ type: 'gridSize', value: opt.key })}
                    title={t(locale, `fileBrowser.gridSize${opt.key.toUpperCase()}` as any)}
                  >
                    {opt.label}
                  </button>
                ))}
              </div>
            )}

            {/* Sort — field + explicit direction */}
            <div ref={sortRef} className="relative">
              <button
                className="btn-secondary-v1 !h-8 text-xs"
                onClick={() => setSortOpen((v) => !v)}
                title={t(locale, 'fileBrowser.sort')}
                aria-haspopup="menu"
                aria-expanded={sortOpen}
              >
                <ArrowUpDown size={12} />
                <span>
                  {fs?.sortBy === 'mtime'
                    ? t(locale, 'fileBrowser.modified')
                    : fs?.sortBy === 'size'
                      ? t(locale, 'fileBrowser.size')
                      : t(locale, 'fileBrowser.name')}
                  {fs?.sortDir === 'asc' ? ' ↑' : ' ↓'}
                </span>
              </button>
              {sortOpen && (
                <div
                  role="menu"
                  className="absolute right-0 top-full mt-1 z-50 min-w-[176px] rounded-lg border border-[var(--border)] bg-[var(--surface)] p-1 shadow-popup"
                >
                  <div className="px-2 py-1 text-[10px] font-semibold uppercase tracking-wide text-[var(--text-disabled)]">
                    {t(locale, 'fileBrowser.sort')}
                  </div>
                  {([
                    { key: 'name' as const, label: t(locale, 'fileBrowser.sortByName') },
                    { key: 'mtime' as const, label: t(locale, 'fileBrowser.sortByModified') },
                    { key: 'size' as const, label: t(locale, 'fileBrowser.sortBySize') },
                  ]).map((opt) => {
                    const active = fs?.sortBy === opt.key;
                    return (
                      <button
                        key={opt.key}
                        role="menuitemradio"
                        aria-checked={active}
                        className={`flex w-full items-center justify-between gap-3 rounded-md px-3 py-1.5 text-xs transition-all ${
                          active
                            ? 'bg-[var(--primary-soft)] text-[var(--primary)]'
                            : 'text-[var(--text-secondary)] hover:bg-[var(--bg-2)]'
                        }`}
                        onClick={() => {
                          // Same field toggles direction; new field switches with natural default.
                          dispatchAction({ type: 'sortBy', value: opt.key });
                          // Keep menu open when switching field so user can also pick direction.
                          if (active) setSortOpen(false);
                        }}
                      >
                        <span>{opt.label}</span>
                        {active && (
                          <span className="text-[10px] opacity-70">
                            {fs?.sortDir === 'asc' ? <ArrowUp size={12} /> : <ArrowDown size={12} />}
                          </span>
                        )}
                      </button>
                    );
                  })}
                  <div className="mx-2 my-1 border-t border-[var(--border)]" />
                  <div className="px-2 py-1 text-[10px] font-semibold uppercase tracking-wide text-[var(--text-disabled)]">
                    {t(locale, 'fileBrowser.sortDirection')}
                  </div>
                  {([
                    { key: 'asc' as const, label: t(locale, 'fileBrowser.ascending'), Icon: ArrowUp },
                    { key: 'desc' as const, label: t(locale, 'fileBrowser.descending'), Icon: ArrowDown },
                  ]).map((opt) => {
                    const active = (fs?.sortDir ?? 'asc') === opt.key;
                    return (
                      <button
                        key={opt.key}
                        role="menuitemradio"
                        aria-checked={active}
                        className={`flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-xs transition-all ${
                          active
                            ? 'bg-[var(--primary-soft)] text-[var(--primary)]'
                            : 'text-[var(--text-secondary)] hover:bg-[var(--bg-2)]'
                        }`}
                        onClick={() => {
                          dispatchAction({ type: 'sortDir', value: opt.key });
                          setSortOpen(false);
                        }}
                      >
                        <opt.Icon size={12} />
                        <span>{opt.label}</span>
                      </button>
                    );
                  })}
                </div>
              )}
            </div>

            {/* Filter — dropdown */}
            <div ref={filterRef} className="relative">
              <button
                className={`btn-secondary-v1 !h-8 text-xs ${fs?.showHidden ? 'active' : ''}`}
                onClick={() => setFilterOpen((v) => !v)}
                title={t(locale, 'fileBrowser.filter')}
              >
                {fs?.showHidden ? <EyeOff size={12} /> : <Eye size={12} />}
                <span>{t(locale, 'fileBrowser.filter')}</span>
              </button>
              {filterOpen && (
                <div
                  className="absolute right-0 top-full mt-1 z-50 min-w-[140px] rounded-lg border border-[var(--border)] bg-[var(--surface)] p-1 shadow-popup"

                >
                  <button
                    className={`flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-xs transition-all ${
                      !fs?.showHidden
                        ? 'bg-[var(--primary-soft)] text-[var(--primary)]'
                        : 'text-[var(--text-secondary)] hover:bg-[var(--surface)]'
                    }`}
                    onClick={() => {
                      if (fs?.showHidden) dispatchAction({ type: 'showHidden' });
                      setFilterOpen(false);
                    }}
                  >
                    <Eye size={12} />
                    <span>{t(locale, 'fileBrowser.hideHidden')}</span>
                  </button>
                  <button
                    className={`flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-xs transition-all ${
                      fs?.showHidden
                        ? 'bg-[var(--primary-soft)] text-[var(--primary)]'
                        : 'text-[var(--text-secondary)] hover:bg-[var(--surface)]'
                    }`}
                    onClick={() => {
                      if (!fs?.showHidden) dispatchAction({ type: 'showHidden' });
                      setFilterOpen(false);
                    }}
                  >
                    <EyeOff size={12} />
                    <span>{t(locale, 'fileBrowser.showHidden')}</span>
                  </button>
                </div>
              )}
            </div>

            {/* Local filter + global search */}
            <div className="relative flex items-center gap-1">
              {searchOpen ? (
                <div className="flex items-center gap-1 rounded-lg bg-[var(--surface)] border border-[var(--border)] px-2 py-1">
                  <Search size={12} className="text-[var(--text-disabled)] shrink-0" />
                  <input
                    ref={searchRef}
                    type="text"
                    className="bg-transparent border-none outline-none focus-visible:outline-none text-xs text-[var(--text)] w-[120px] placeholder:text-[var(--text-disabled)]"
                    placeholder={t(locale, 'fileBrowser.filterCurrent')}
                    defaultValue={fs?.searchQuery ?? ''}
                    onChange={(e) => dispatchAction({ type: 'search', value: e.target.value })}
                    onKeyDown={(e) => e.key === 'Escape' && setSearchOpen(false)}
                  />
                  <button
                    className="flex items-center justify-center text-[var(--text-disabled)] hover:text-[var(--text)] transition-colors"
                    onClick={() => { setSearchOpen(false); dispatchAction({ type: 'search', value: '' }); }}
                    title={t(locale, 'common.close')}
                  >
                    <X size={12} />
                  </button>
                </div>
              ) : (
                <button
                  className="btn-secondary-v1 !h-8 !w-8 !p-0 flex items-center justify-center"
                  onClick={() => setSearchOpen(true)}
                  title={t(locale, 'fileBrowser.filterCurrent')}
                >
                  <Search size={14} />
                </button>
              )}
              <button
                className="btn-secondary-v1 !h-8 !w-8 !p-0 flex items-center justify-center"
                onClick={() => dispatchAction({ type: 'globalSearch' })}
                title={`${t(locale, 'fileBrowser.globalSearch')} ⌘⇧F`}
              >
                <FolderSearch size={14} />
              </button>
            </div>
          </div>
        </>
      ) : (
        /* ── 其他视图：显示路由标题 ── */
        <div className="flex items-center gap-2 flex-1 min-w-0" >
          <motion.span
            key={activeView}
            initial={{ opacity: 0, y: -4 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.18, ease: [0.16, 1, 0.3, 1] }}
            className="text-sm font-semibold text-[var(--text)]"
          >
            {VIEW_LABELS[activeView] ? t(locale, VIEW_LABELS[activeView]) : activeView}
          </motion.span>
        </div>
      )}
    </header>
  );
}
