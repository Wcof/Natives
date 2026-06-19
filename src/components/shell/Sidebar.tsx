'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import type {
  ButtonHTMLAttributes,
  DragEvent,
  MouseEvent,
  ReactNode,
} from 'react';
import type { LucideIcon } from 'lucide-react';
import {
  Bell,
  Download,
  FileText,
  Layers,
  LayoutDashboard,
  Minus,
  Monitor,
  Search,
  Settings,
  Square,
  Star,
  X,
  Zap,
} from 'lucide-react';
import { t, type Locale } from '@/i18n';

interface ModuleManifest {
  id: string;
  name: string;
  icon?: string;
  i18n?: {
    name?: Record<string, string>;
  };
}

interface ModuleItem {
  moduleId: string;
  manifest: ModuleManifest | null;
  error?: string;
}

interface QuickAccessItem {
  id: 'home' | 'desktop' | 'documents' | 'downloads';
  target: string;
  path?: string;
  icon: LucideIcon;
}

const QUICK_ACCESS_ITEMS: readonly QuickAccessItem[] = [
  {
    id: 'home',
    target: '__dashboard__',
    icon: LayoutDashboard,
  },
  {
    id: 'desktop',
    target: '__files__:~/Desktop',
    path: '~/Desktop',
    icon: Monitor,
  },
  {
    id: 'documents',
    target: '__files__:~/.natives',
    path: '~/.natives',
    icon: FileText,
  },
  {
    id: 'downloads',
    target: '__files__:~/Downloads',
    path: '~/Downloads',
    icon: Download,
  },
];

interface SidebarProps {
  isCollapsed: boolean;
  onToggle: () => void;
  width: number;
  onResize: (width: number) => void;
  activeModuleId?: string;
  onModuleSelect: (target: string) => void;
  onNotificationClick: () => void;
  locale?: Locale;
}

function getModuleId(module: ModuleItem): string {
  return module.manifest?.id ?? module.moduleId;
}

function getNavigationId(activeModuleId?: string): string | null {
  if (!activeModuleId) return null;
  if (activeModuleId === 'dashboard' || activeModuleId === '__dashboard__') return '__dashboard__';
  if (activeModuleId === 'settings' || activeModuleId === '__settings__') return '__settings__';
  if (activeModuleId === 'workshop' || activeModuleId === '__workshop__') return '__workshop__';
  if (activeModuleId.startsWith('module:')) return activeModuleId;
  if (activeModuleId.startsWith('__files__:')) return activeModuleId;
  // Other views (files/ai/tools/modules/store) are not sidebar entries —
  // returning null keeps the previous highlight from sticking after navigation.
  return null;
}

function SidebarNavItem({
  isActive,
  icon,
  label,
  title,
  onClick,
  role,
  'aria-selected': ariaSelected,
  draggable,
  onDragStart,
  onDragOver,
}: {
  isActive: boolean;
  icon: ReactNode;
  label: string;
  title?: string;
  onClick: () => void;
  role?: string;
  'aria-selected'?: boolean;
  draggable?: boolean;
  onDragStart?: (e: DragEvent<HTMLButtonElement>) => void;
  onDragOver?: (e: DragEvent<HTMLButtonElement>) => void;
}) {
  return (
    <button
      type="button"
      role={role}
      aria-selected={ariaSelected}
      draggable={draggable}
      onDragStart={onDragStart}
      onDragOver={onDragOver}
      onClick={onClick}
      title={title}
      className={`flex w-full items-center gap-2.5 rounded-lg px-3 py-1.5 text-left text-sm transition-all ${
        isActive
          ? 'bg-[var(--vibe-active-bg)] text-[var(--vibe-active-color)] font-medium'
          : 'text-[var(--text-dim)] hover:bg-[var(--vibe-btn-hover-bg)] hover:text-[var(--vibe-btn-hover-color)]'
      }`}
    >
      <span className="shrink-0">{icon}</span>
      <span className="truncate">{label}</span>
    </button>
  );
}

export default function Sidebar({
  isCollapsed,
  onToggle,
  width,
  onResize,
  activeModuleId,
  onModuleSelect,
  onNotificationClick,
  locale = 'zh',
}: SidebarProps) {
  const [modules, setModules] = useState<ModuleItem[]>([]);
  const [dragIndex, setDragIndex] = useState<number | null>(null);
  const [favorites, setFavorites] = useState<string[]>([]);
  const [activeNavigationId, setActiveNavigationId] = useState<string | null>(
    () => getNavigationId(activeModuleId),
  );

  useEffect(() => {
    setActiveNavigationId(getNavigationId(activeModuleId));
  }, [activeModuleId]);

  const selectNavigation = useCallback(
    (navigationId: string, target: string) => {
      setActiveNavigationId(navigationId);
      onModuleSelect(target);
    },
    [onModuleSelect],
  );

  const loadFavorites = useCallback(async () => {
    try {
      const stored = await window.nativesAPI?.db?.get('settings:favorites');
      setFavorites(stored ? JSON.parse(stored as string) : []);
    } catch {
      setFavorites([]);
    }
  }, []);

  useEffect(() => {
    void loadFavorites();
    const handleFavoritesChanged = () => void loadFavorites();
    window.addEventListener('favorites-changed', handleFavoritesChanged);
    return () => window.removeEventListener('favorites-changed', handleFavoritesChanged);
  }, [loadFavorites]);

  // ── 窗口控制（关闭 / 最小化 / 最大化）──
  const [isMaximized, setIsMaximized] = useState(false);

  useEffect(() => {
    let unlistenResize: (() => void) | undefined;
    let cancelled = false;

    const setupListener = async () => {
      try {
        const { getCurrentWindow } = await import('@tauri-apps/api/window');
        const activeWin = getCurrentWindow();

        // Initial state
        if (!cancelled) {
          setIsMaximized(await activeWin.isMaximized());
        }

        // Listen to resize events instead of polling every 300ms
        const unsub = await activeWin.onResized(async () => {
          if (!cancelled) {
            setIsMaximized(await activeWin.isMaximized());
          }
        });
        unlistenResize = unsub;
      } catch {
        // Fallback: polling only when Tauri API is unavailable (browser dev)
        const poll = setInterval(async () => {
          try {
            const m = await window.nativesAPI?.windowControls?.isMaximized?.();
            if (m !== undefined && !cancelled) setIsMaximized(m);
          } catch { /* ignore */ }
        }, 2000);
        // Store cleanup ref
        const origCleanup = cleanup;
        cleanup = () => {
          clearInterval(poll);
          origCleanup?.();
        };
      }
    };

    let cleanup: (() => void) | undefined;
    setupListener();

    return () => {
      cancelled = true;
      if (unlistenResize) unlistenResize();
      if (cleanup) cleanup();
    };
  }, []);

  const handleWindowAction = useCallback(async (action: 'minimize' | 'maximize' | 'close') => {
    const ctrl = window.nativesAPI?.windowControls;
    if (!ctrl) return;
    if (action === 'minimize') await ctrl.minimize();
    else if (action === 'close') await ctrl.close();
    else {
      await ctrl.maximize();
      try { setIsMaximized(await ctrl.isMaximized()); } catch { /* ignore */ }
    }
  }, []);

  // ── 长按 Zoom 弹出菜单（macOS 原生行为）──
  const [zoomMenuOpen, setZoomMenuOpen] = useState(false);
  const [zoomMenuPos, setZoomMenuPos] = useState<{ x: number; y: number } | null>(null);
  const zoomTimerRef = useRef<number | null>(null);
  const zoomPopupRef = useRef<HTMLDivElement>(null);
  const zoomBtnRef = useRef<HTMLButtonElement>(null);

  const handleTileWindow = useCallback(async (action: string) => {
    const ctrl = window.nativesAPI?.windowControls as Record<string, unknown> | undefined;
    if (!ctrl) return;
    try {
      const fn = ctrl.tileWindow as ((a: string) => Promise<void>) | undefined;
      await fn?.(action);
    } catch { /* fallback */ }
  }, []);

  // 当菜单打开时，document mouseup 检测鼠标下方元素
  useEffect(() => {
    if (!zoomMenuOpen) return;
    const handler = (e: globalThis.MouseEvent) => {
      const target = document.elementFromPoint(e.clientX, e.clientY);
      // 向上查找最近的 data-tile-action 元素
      let el: Element | null = target;
      while (el && el !== document.body) {
        if (el instanceof HTMLElement && el.dataset.tileAction) {
          handleTileWindow(el.dataset.tileAction);
          break;
        }
        el = el.parentElement;
      }
      setZoomMenuOpen(false);
    };
    document.addEventListener('mouseup', handler);
    return () => document.removeEventListener('mouseup', handler);
  }, [zoomMenuOpen, handleTileWindow]);

  // ── 弹窗内 SVG 图标组件 ──
  const iconWrap = (svg: React.ReactNode) => <svg width="22" height="14" viewBox="0 0 22 14" fill="none" className="text-[var(--text-dim)]">{svg}</svg>;

  const leftHalfIcon = iconWrap(
    <><rect x="0.5" y="0.5" width="21" height="13" rx="1.5" stroke="currentColor" strokeOpacity="0.3"/><rect x="0.5" y="0.5" width="10" height="13" rx="1.5" fill="currentColor" fillOpacity="0.15" stroke="currentColor"/></>
  );
  const rightHalfIcon = iconWrap(
    <><rect x="0.5" y="0.5" width="21" height="13" rx="1.5" stroke="currentColor" strokeOpacity="0.3"/><rect x="11.5" y="0.5" width="10" height="13" rx="1.5" fill="currentColor" fillOpacity="0.15" stroke="currentColor"/></>
  );
  const topHalfIcon = iconWrap(
    <><rect x="0.5" y="0.5" width="21" height="13" rx="1.5" stroke="currentColor" strokeOpacity="0.3"/><rect x="0.5" y="0.5" width="21" height="6" rx="1.5" fill="currentColor" fillOpacity="0.15" stroke="currentColor"/></>
  );
  const bottomHalfIcon = iconWrap(
    <><rect x="0.5" y="0.5" width="21" height="13" rx="1.5" stroke="currentColor" strokeOpacity="0.3"/><rect x="0.5" y="7.5" width="21" height="6" rx="1.5" fill="currentColor" fillOpacity="0.15" stroke="currentColor"/></>
  );
  const fillIcon = iconWrap(
    <rect x="0.5" y="0.5" width="21" height="13" rx="1.5" fill="currentColor" fillOpacity="0.15" stroke="currentColor"/>
  );
  const leftFillIcon = iconWrap(
    <><rect x="0.5" y="0.5" width="21" height="13" rx="1.5" stroke="currentColor" strokeOpacity="0.3"/><rect x="0.5" y="0.5" width="7" height="13" rx="1.5" fill="currentColor" fillOpacity="0.15" stroke="currentColor"/></>
  );
  const rightFillIcon = iconWrap(
    <><rect x="0.5" y="0.5" width="21" height="13" rx="1.5" stroke="currentColor" strokeOpacity="0.3"/><rect x="14.5" y="0.5" width="7" height="13" rx="1.5" fill="currentColor" fillOpacity="0.15" stroke="currentColor"/></>
  );
  const tileIcon = iconWrap(
    <><rect x="0.5" y="0.5" width="9.5" height="5.5" rx="1" stroke="currentColor" strokeOpacity="0.3"/><rect x="12" y="0.5" width="9.5" height="5.5" rx="1" stroke="currentColor" strokeOpacity="0.3"/><rect x="0.5" y="8" width="9.5" height="5.5" rx="1" fill="currentColor" fillOpacity="0.15" stroke="currentColor"/><rect x="12" y="8" width="9.5" height="5.5" rx="1" fill="currentColor" fillOpacity="0.15" stroke="currentColor"/></>
  );

  useEffect(() => {
    let cancelled = false;
    const loadModules = async () => {
      try {
        const api = window.nativesAPI;
        if (!api?.module?.scan) return;
        const rawOrder = await api.db?.get('settings:module_order');
        const savedOrder = rawOrder ? JSON.parse(rawOrder as string) : [];
        const scannedModules = (await api.module.scan()) as ModuleItem[];
        const orderedModules = [...scannedModules];
        if (savedOrder.length > 0) {
          const orderIndex = new Map(savedOrder.map((id: string, index: number) => [id, index]));
          orderedModules.sort(
            (left, right) =>
              ((orderIndex.get(getModuleId(left)) ?? Infinity) as number) -
              ((orderIndex.get(getModuleId(right)) ?? Infinity) as number),
          );
        }
        if (!cancelled) setModules(orderedModules);
      } catch {
        if (!cancelled) setModules([]);
      }
    };
    void loadModules();
    return () => { cancelled = true; };
  }, []);

  const handleDragOver = (event: DragEvent<HTMLButtonElement>, index: number) => {
    event.preventDefault();
    if (dragIndex === null || dragIndex === index) return;
    setModules((currentModules) => {
      const nextModules = [...currentModules];
      const [movedModule] = nextModules.splice(dragIndex, 1);
      if (!movedModule) return currentModules;
      nextModules.splice(index, 0, movedModule);
      return nextModules;
    });
    setDragIndex(index);
  };

  const handleDragEnd = async () => {
    setDragIndex(null);
    try {
      await window.nativesAPI?.db?.set('settings:module_order', modules.map(getModuleId));
    } catch { /* session-only fallback */ }
  };

  const normalizedLocale = locale.startsWith('zh') ? 'zh' : 'en';

  if (isCollapsed) {
    return (
      <aside style={{ width: 0, overflow: 'hidden' }} role="navigation" aria-label={t(locale, 'nav.modules')}>
      </aside>
    );
  }

  return (
    <aside
      className="vibe-sidebar flex flex-col h-full overflow-hidden"
      style={{ width }}
      role="navigation"
      aria-label={t(locale, 'nav.modules')}
      data-sidebar
    >
      {/* ── 窗口控制 + Brand Header ── */}
      <div className="shrink-0" data-tauri-drag-region style={{ WebkitAppRegion: 'drag' } as React.CSSProperties}>
        {/* 窗口控制按钮行 */}
      <div className="flex items-center justify-start gap-[6px] px-3 pt-2.5 pb-1">
        {/* 关闭 — macOS 红圆 */}
        <button
          onClick={() => handleWindowAction('close')}
          className="group flex h-3.5 w-3.5 items-center justify-center rounded-full bg-[var(--mac-red)] hover:bg-[var(--mac-red-hover)] hover:shadow-[0_0_6px_var(--mac-red-glow)] transition-all"
          style={{ WebkitAppRegion: 'no-drag' } as React.CSSProperties}
          aria-label="关闭"
          title="关闭"
        >
          <X size={9} className="text-[#1a1a1e] opacity-0 group-hover:opacity-100 transition-opacity" strokeWidth={2.5} />
        </button>
        {/* 最小化 — macOS 黄圆 */}
        <button
          onClick={() => handleWindowAction('minimize')}
          className="group flex h-3.5 w-3.5 items-center justify-center rounded-full bg-[var(--mac-yellow)] hover:bg-[var(--mac-yellow-hover)] hover:shadow-[0_0_6px_var(--mac-yellow-glow)] transition-all"
          style={{ WebkitAppRegion: 'no-drag' } as React.CSSProperties}
          aria-label="最小化"
          title="最小化"
        >
          <Minus size={9} className="text-[#1a1a1e] opacity-0 group-hover:opacity-100 transition-opacity" strokeWidth={2.5} />
        </button>
        {/* 最大化 / 全屏 — macOS 绿圆 */}
        <div className="relative" ref={zoomPopupRef}>
          <button
            ref={zoomBtnRef}
            onClick={() => handleWindowAction('maximize')}
            onMouseDown={(e) => {
              const cx = e.clientX;
              const cy = e.clientY;
              zoomTimerRef.current = window.setTimeout(() => {
                setZoomMenuPos({ x: cx, y: cy });
                setZoomMenuOpen(true);
              }, 500);
            }}
            onMouseUp={() => {
              if (zoomTimerRef.current) {
                clearTimeout(zoomTimerRef.current);
                zoomTimerRef.current = null;
              }
            }}
            onMouseLeave={() => {
              if (zoomTimerRef.current) {
                clearTimeout(zoomTimerRef.current);
                zoomTimerRef.current = null;
              }
            }}
            className="group flex h-3.5 w-3.5 items-center justify-center rounded-full bg-[var(--mac-green)] hover:bg-[var(--mac-green-hover)] hover:shadow-[0_0_6px_var(--mac-green-glow)] transition-all"
            style={{ WebkitAppRegion: 'no-drag' } as React.CSSProperties}
            aria-label={isMaximized ? '还原' : '最大化'}
            title={isMaximized ? '还原' : '最大化'}
          >
            <svg width="8" height="8" viewBox="0 0 10 10" fill="none" className="text-[#1a1a1e] opacity-0 group-hover:opacity-100 transition-opacity">
              <path d="M7 1h2v2" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"/>
              <path d="M9 1l-4 4" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"/>
              <path d="M3 9H1V7" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"/>
              <path d="M1 9l4-4" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"/>
            </svg>
            </button>

            {/* 长按弹出菜单 — macOS 窗口管理，跟随鼠标位置 */}
            {zoomMenuOpen && zoomMenuPos && (
              <div
                className="fixed z-50 min-w-[180px] rounded-xl border border-[var(--vibe-btn-border)] bg-[var(--vibe-toolbar-bg)] backdrop-blur-2xl p-1.5 shadow-2xl"
                style={{ left: zoomMenuPos.x, top: zoomMenuPos.y, WebkitAppRegion: 'no-drag' } as React.CSSProperties}
              >
                <p className="px-2.5 pb-1 pt-0.5 text-[0.625rem] font-medium uppercase tracking-[0.06em] text-[var(--text-faint)]">
                  移动与调整大小
                </p>
                <div className="grid grid-cols-4 gap-1 px-1 pb-2">
                  {[
                    { id: 'left', label: '左', icon: leftHalfIcon },
                    { id: 'right', label: '右', icon: rightHalfIcon },
                    { id: 'top', label: '上', icon: topHalfIcon },
                    { id: 'bottom', label: '下', icon: bottomHalfIcon },
                  ].map((opt) => (
                    <button
                      key={opt.id}
                      data-tile-action={opt.id}
                      className="flex flex-col items-center gap-1 rounded-lg px-2 py-2 text-[0.625rem] text-[var(--text-dim)] hover:bg-[var(--vibe-btn-bg)] hover:text-[var(--text)] transition-all"
                      style={{ WebkitAppRegion: 'no-drag' } as React.CSSProperties}
                      title={opt.label}
                    >
                      {opt.icon}
                      <span>{opt.label}</span>
                    </button>
                  ))}
                </div>
                <div className="mx-2 my-1 border-t border-[var(--vibe-btn-border)]" />
                <p className="px-2.5 pb-1 pt-1.5 text-[0.625rem] font-medium uppercase tracking-[0.06em] text-[var(--text-faint)]">
                  填充与排列
                </p>
                <div className="grid grid-cols-4 gap-1 px-1 pb-1">
                  {[
                    { id: 'fullscreen', label: '填充', icon: fillIcon },
                    { id: 'left-half', label: '居左', icon: leftFillIcon },
                    { id: 'right-half', label: '居右', icon: rightFillIcon },
                    { id: 'tile', label: '平铺', icon: tileIcon },
                  ].map((opt) => (
                    <button
                      key={opt.id}
                      data-tile-action={opt.id}
                      className="flex flex-col items-center gap-1 rounded-lg px-2 py-2 text-[0.625rem] text-[var(--text-dim)] hover:bg-[var(--vibe-btn-bg)] hover:text-[var(--text)] transition-all"
                      style={{ WebkitAppRegion: 'no-drag' } as React.CSSProperties}
                      title={opt.label}
                    >
                      {opt.icon}
                      <span>{opt.label}</span>
                    </button>
                  ))}
                </div>
              </div>
            )}
          </div>
        </div>

        {/* Brand Header */}
        <div className="flex items-center gap-3 px-4 pb-3" style={{ WebkitAppRegion: 'no-drag' } as React.CSSProperties}>
          <div className="flex h-8 w-8 items-center justify-center rounded-lg border border-[var(--vibe-search-border)] bg-[var(--vibe-search-bg)]">
            <Zap size={18} className="text-[var(--vibe-nav-icon)]" />
          </div>
          <div>
            <h1 className="text-lg font-bold text-[var(--vibe-brand-text)] leading-tight">Natives</h1>
            <p className="text-[0.6875rem] text-[var(--vibe-brand-sub)]">personal desktop</p>
          </div>
        </div>
      </div>

      {/* 中间可滚动区域 */}
      <div className="flex-1 overflow-y-auto min-h-0">
        {/* Search Capsule */}
        <div className="px-4 pb-3" style={{ WebkitAppRegion: 'no-drag' } as React.CSSProperties}>
          <div className="flex h-10 items-center gap-2 rounded-xl border border-[var(--vibe-search-border)] bg-[var(--vibe-search-bg)] px-3">
            <Search size={14} className="text-[var(--vibe-search-placeholder)] shrink-0" />
            <input
              type="text"
              placeholder={t(locale, 'sidebar.searchPlaceholder')}
              className="min-w-0 flex-1 bg-transparent text-sm text-[var(--vibe-brand-text)] outline-none focus-visible:outline-none placeholder:text-[var(--vibe-search-placeholder)]"
              style={{ WebkitAppRegion: 'no-drag' } as React.CSSProperties}
            />
            <span className="shrink-0 rounded-md bg-[var(--vibe-btn-bg)] px-1.5 py-0.5 text-[0.6875rem] font-medium text-[var(--vibe-search-placeholder)]">
              ⌘K
            </span>
          </div>
        </div>

        {/* Quick Access List */}
        <div className="px-3 pb-1 pt-0 text-[0.6875rem] font-semibold uppercase tracking-[0.08em] text-[var(--vibe-section-header)]">
          {t(locale, 'sidebar.quickAccess')}
        </div>
        <div className="mb-3 flex flex-col gap-0.5 px-3">
          {QUICK_ACCESS_ITEMS.map((item) => {
            const Icon = item.icon;
            const isActive = activeNavigationId === item.target;
            return (
              <button
                key={item.id}
                type="button"
                onClick={() => selectNavigation(item.target, item.target)}
                className={`flex w-full items-center gap-2.5 rounded-lg px-3 py-1.5 text-left transition-all ${
                  isActive
                    ? 'bg-[var(--vibe-active-bg)] text-[var(--vibe-active-color)] font-medium'
                    : 'text-[var(--text-dim)] hover:bg-[var(--vibe-btn-hover-bg)] hover:text-[var(--vibe-btn-hover-color)]'
                }`}
                title={item.id}
              >
                <Icon size={15} className="shrink-0" />
                <span className="truncate text-sm">
                  {t(locale, 'sidebar.quickAccessDirs.' + item.id)}
                </span>
              </button>
            );
          })}
        </div>

        {/* Modules section (existing modules from scanning) */}
        {modules.length > 0 && (
          <>
            <div className="px-4 pb-1 pt-2 text-[0.6875rem] font-semibold uppercase tracking-[0.08em] text-[var(--vibe-section-header)]">
              {t(locale, 'nav.modules')}
            </div>
            <div
              className="flex flex-col gap-0.5 px-3"
              role="listbox"
              aria-label={t(locale, 'nav.modules')}
              aria-live="polite"
              onDragEnd={() => void handleDragEnd()}
            >
              {modules.map((module, index) => {
                const moduleId = getModuleId(module);
                const moduleName =
                  module.manifest?.i18n?.name?.[normalizedLocale] ??
                  module.manifest?.name ??
                  module.moduleId;
                const moduleIcon = module.manifest?.icon;
                const navigationId = `module:${moduleId}`;
                return (
                  <SidebarNavItem
                    key={moduleId}
                    isActive={activeNavigationId === navigationId}
                    icon={
                      moduleIcon ? (
                        <img src={moduleIcon} alt="" draggable={false} className="h-[18px] w-[18px] object-contain" />
                      ) : (
                        <Square size={18} />
                      )
                    }
                    label={moduleName}
                    role="option"
                    aria-selected={activeNavigationId === navigationId}
                    draggable
                    onDragStart={() => setDragIndex(index)}
                    onDragOver={(event) => handleDragOver(event, index)}
                    onClick={() => selectNavigation(navigationId, moduleId)}
                    title={moduleName}
                  />
                );
              })}
            </div>
          </>
        )}

        {/* Favorites */}
        {favorites.length > 0 && (
          <>
            <div className="px-4 pb-1 pt-3 text-[0.6875rem] font-semibold uppercase tracking-[0.08em] text-[var(--vibe-section-header)]">
              {t(locale, 'sidebar.favorites')}
            </div>
            <div className="flex flex-col gap-0.5 px-3">
              {favorites.map((favoritePath) => {
                const navigationId = `__files__:${favoritePath}`;
                const label = favoritePath.split('/').pop() || favoritePath;
                return (
                  <SidebarNavItem
                    key={favoritePath}
                    isActive={activeNavigationId === navigationId}
                    icon={<Star size={15} />}
                    label={label}
                    onClick={() => selectNavigation(navigationId, navigationId)}
                    title={favoritePath}
                  />
                );
              })}
            </div>
          </>
        )}
      </div>

      {/* 底部固定区域：通知、设置、创意工坊 */}
      <div className="shrink-0 px-3 py-2 border-t border-[var(--vibe-border-subtle)]">
        <button
          type="button"
          onClick={onNotificationClick}
          className="flex w-full items-center gap-2.5 rounded-lg px-3 py-1.5 text-sm text-[var(--text-dim)] transition-all hover:bg-[var(--vibe-btn-hover-bg)] hover:text-[var(--vibe-btn-hover-color)]"
        >
          <Bell size={16} />
          <span>{t(locale, 'notifications.title')}</span>
        </button>
        <button
          type="button"
          onClick={() => selectNavigation('__settings__', '__settings__')}
          className={`flex w-full items-center gap-2.5 rounded-lg px-3 py-1.5 text-sm transition-all ${
            activeNavigationId === '__settings__'
              ? 'bg-[var(--vibe-active-bg)] text-[var(--vibe-active-color)] font-medium'
              : 'text-[var(--text-dim)] hover:bg-[var(--vibe-btn-hover-bg)] hover:text-[var(--vibe-btn-hover-color)]'
          }`}
        >
          <Settings size={16} />
          <span>{t(locale, 'nav.settings')}</span>
        </button>
        <button
          type="button"
          onClick={() => selectNavigation('__workshop__', '__workshop__')}
          className={`flex w-full items-center gap-2.5 rounded-lg px-3 py-1.5 text-sm transition-all ${
            activeNavigationId === '__workshop__'
              ? 'bg-[var(--vibe-active-bg)] text-[var(--vibe-active-color)] font-medium'
              : 'text-[var(--text-dim)] hover:bg-[var(--vibe-btn-hover-bg)] hover:text-[var(--vibe-btn-hover-color)]'
          }`}
        >
          <Layers size={16} />
          <span>{t(locale, 'nav.workshop')}</span>
        </button>
      </div>

    </aside>
  );
}
