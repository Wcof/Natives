'use client';

import { startTransition, useCallback, useEffect, useRef, useState } from 'react';
import type {
  DragEvent,
  MouseEvent as ReactMouseEvent,
  ReactNode,
} from 'react';
import type { LucideIcon } from 'lucide-react';
import {
  Bell,
  Download,
  FileText,
  Layers,
  LayoutDashboard,
  MessageSquare,
  Monitor,
  Search,
  Settings,
  Square,
  ArrowLeft,
  ChevronDown,
  ChevronRight,
  FolderPlus,
  Palette,
  Plug,
  SlidersHorizontal,
  Cpu,
  Server,
  PanelLeft,
  PanelLeftClose,
} from 'lucide-react';
import * as LucideIcons from 'lucide-react';
import { t, type Locale } from '@/i18n';
import { BUILTIN_TOOLS, seedAllBuiltinTools } from '@/lib/builtin-tools';
import AssistantSidebarSection from '@/components/assistant/AssistantSidebarSection';
import { useAssistantWorkspace } from '@/components/assistant/AssistantWorkspaceContext';
import {
  isSettingsView,
  getSettingsSection,
  type SettingsSection,
} from './settings-navigation';

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

interface FavoriteItem {
  path: string;
  addedAt: number;
}

const SETTINGS_NAV_ITEMS = [
  { id: 'general', labelKey: 'settings.tabGeneral', icon: Settings },
  { id: 'appearance', labelKey: 'settings.tabAppearance', icon: Palette },
  { id: 'providers', labelKey: 'settings.tabProviders', icon: Cpu },
  { id: 'runtime', labelKey: 'settings.tabExecutor', icon: SlidersHorizontal },
  { id: 'engine', labelKey: 'settings.tabEngineCaps', icon: Server },
  { id: 'plugins', labelKey: 'settings.tabPlugins', icon: Plug },
] satisfies ReadonlyArray<{
  id: SettingsSection;
  labelKey: string;
  icon: LucideIcon;
}>;

/** Collapsed rail width — icon-only navigation, still interactive. */
export const SIDEBAR_COLLAPSED_WIDTH = 64;

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
  if (isSettingsView(activeModuleId)) return '__settings__';
  if (activeModuleId === 'workshop' || activeModuleId === '__workshop__' || activeModuleId === 'modules' || activeModuleId === 'store') {
    return '__workshop__';
  }
  if (activeModuleId === 'assistant' || activeModuleId === '__assistant__') return '__assistant__';
  if (activeModuleId.startsWith('module:')) return activeModuleId;
  if (activeModuleId.startsWith('__files__:')) return activeModuleId;
  if (activeModuleId.startsWith('builtin:')) return activeModuleId;
  // Other views (files/ai/tools) are not fixed sidebar entries —
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
  collapsed = false,
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
  collapsed?: boolean;
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
      title={title ?? label}
      aria-label={label}
      className={
        collapsed
          ? `flex h-9 w-9 items-center justify-center rounded-lg transition-all ${
              isActive
                ? 'bg-[var(--accent)] text-[var(--accent-ink)]'
                : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)]'
            }`
          : `flex w-full items-center gap-2.5 rounded-lg px-3 py-1.5 text-left text-sm transition-all ${
              isActive
                ? 'bg-[var(--accent)] text-[var(--accent-ink)] font-medium'
                : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)]'
            }`
      }
    >
      <span className="shrink-0">{icon}</span>
      {!collapsed && <span className="truncate">{label}</span>}
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
  const [favorites, setFavorites] = useState<FavoriteItem[]>([]);
  const [favoritesExpanded, setFavoritesExpanded] = useState(false);
  const [assistantExpanded, setAssistantExpanded] = useState(true);
  const [activeNavigationId, setActiveNavigationId] = useState<string | null>(
    () => getNavigationId(activeModuleId),
  );
  const { actions: assistantActions } = useAssistantWorkspace();

  // Builtin tool enabled state (from DB)
  const [enabledTools, setEnabledTools] = useState<Array<{ id: string; driver: string }>>([]);

  const loadEnabledTools = useCallback(async () => {
    try {
      const api = window.nativesAPI;
      if (!api?.builtinTool) return;
      // Seed all tools first
      await seedAllBuiltinTools();
      const list = await api.builtinTool.list();
      setEnabledTools(
        list
          .filter((t: { enabled: boolean }) => t.enabled)
          .map((t: { id: string; driver: string }) => ({ id: t.id, driver: t.driver })),
      );
    } catch { /* browser dev mode */ }
  }, []);

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    loadEnabledTools();
    let unsub: (() => void) | undefined;
    try {
      if (window.nativesAPI?.onDbStateChanged) {
        unsub = window.nativesAPI.onDbStateChanged(() => loadEnabledTools());
      }
    } catch { /* browser dev mode */ }
    return () => { unsub?.(); };
  }, [loadEnabledTools]);

  useEffect(() => {
    startTransition(() => { setActiveNavigationId(getNavigationId(activeModuleId)); });
  }, [activeModuleId]);

  const selectNavigation = useCallback(
    (navigationId: string, target: string) => {
      setActiveNavigationId(navigationId);
      onModuleSelect(target);
    },
    [onModuleSelect],
  );

  const isSettingsMode = isSettingsView(activeModuleId);
  const activeSettingsSection = getSettingsSection(activeModuleId);

  const loadFavorites = useCallback(async () => {
    try {
      const stored = await window.nativesAPI?.db?.get('settings:favorites');
      if (!stored) { setFavorites([]); return; }
      const parsed = JSON.parse(stored as string);
      // Migrate old format: string[] → FavoriteItem[]
      if (Array.isArray(parsed) && parsed.length > 0 && typeof parsed[0] === 'string') {
        const migrated: FavoriteItem[] = (parsed as string[]).map((p, i) => ({ path: p, addedAt: Date.now() + i }));
        setFavorites(migrated);
      } else {
        setFavorites(parsed as FavoriteItem[]);
      }
    } catch {
      setFavorites([]);
    }
  }, []);

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void loadFavorites();
    const handleFavoritesChanged = () => void loadFavorites();
    window.addEventListener('favorites-changed', handleFavoritesChanged);
    return () => window.removeEventListener('favorites-changed', handleFavoritesChanged);
  }, [loadFavorites]);

  // ── 窗口控制（关闭 / 最小化 / 最大化 / 全屏）──
  const [isMaximized, setIsMaximized] = useState(false);
  const [isFullscreen, setIsFullscreen] = useState(false);
  const [windowActive, setWindowActive] = useState(true);
  const longPressTriggeredRef = useRef(false);

  const refreshWindowState = useCallback(async () => {
    const ctrl = window.nativesAPI?.windowControls;
    try {
      if (ctrl?.isMaximized) setIsMaximized(await ctrl.isMaximized());
      if (ctrl?.isFullscreen) setIsFullscreen(await ctrl.isFullscreen());
    } catch { /* browser/dev fallback */ }
  }, []);

  useEffect(() => {
    let unlistenResize: (() => void) | undefined;
    let unlistenFocus: (() => void) | undefined;
    let unlistenBlur: (() => void) | undefined;
    let cancelled = false;

    const setupListener = async () => {
      try {
        const { getCurrentWindow } = await import('@tauri-apps/api/window');
        const activeWin = getCurrentWindow();

        if (!cancelled) {
          setIsMaximized(await activeWin.isMaximized());
          setIsFullscreen(await activeWin.isFullscreen());
          setWindowActive(await activeWin.isFocused());
        }

        const unsubResize = await activeWin.onResized(async () => {
          if (cancelled) return;
          setIsMaximized(await activeWin.isMaximized());
          setIsFullscreen(await activeWin.isFullscreen());
        });
        unlistenResize = unsubResize;

        const unsubFocus = await activeWin.onFocusChanged(({ payload: focused }) => {
          if (!cancelled) setWindowActive(focused);
        });
        unlistenFocus = unsubFocus;
      } catch {
        // Fallback: polling only when Tauri API is unavailable (browser dev)
        const poll = setInterval(async () => {
          try {
            const ctrl = window.nativesAPI?.windowControls;
            if (!ctrl || cancelled) return;
            if (ctrl.isMaximized) {
              const m = await ctrl.isMaximized();
              if (!cancelled) setIsMaximized(m);
            }
            if (ctrl.isFullscreen) {
              const f = await ctrl.isFullscreen();
              if (!cancelled) setIsFullscreen(f);
            }
          } catch { /* ignore */ }
        }, 2000);
        const onFocus = () => setWindowActive(true);
        const onBlur = () => setWindowActive(false);
        window.addEventListener('focus', onFocus);
        window.addEventListener('blur', onBlur);
        cleanup = () => {
          clearInterval(poll);
          window.removeEventListener('focus', onFocus);
          window.removeEventListener('blur', onBlur);
        };
      }
    };

    let cleanup: (() => void) | undefined;
    setupListener();

    return () => {
      cancelled = true;
      if (unlistenResize) unlistenResize();
      if (unlistenFocus) unlistenFocus();
      if (unlistenBlur) unlistenBlur();
      if (cleanup) cleanup();
    };
  }, []);

  const handleWindowAction = useCallback(async (
    action: 'minimize' | 'maximize' | 'close' | 'fullscreen',
  ) => {
    const ctrl = window.nativesAPI?.windowControls;
    if (!ctrl) return;
    try {
      if (action === 'minimize') await ctrl.minimize();
      else if (action === 'close') await ctrl.close();
      else if (action === 'fullscreen') {
        if (ctrl.toggleFullscreen) await ctrl.toggleFullscreen();
        else await ctrl.tileWindow?.('fullscreen');
      } else {
        // macOS zoom: if fullscreen, exit; otherwise toggle maximize
        await ctrl.maximize();
      }
      await refreshWindowState();
    } catch { /* ignore */ }
  }, [refreshWindowState]);

  // ── 长按 Zoom 弹出菜单（macOS 原生行为）──
  const [zoomMenuOpen, setZoomMenuOpen] = useState(false);
  const [zoomMenuPos, setZoomMenuPos] = useState<{ x: number; y: number } | null>(null);
  const zoomTimerRef = useRef<number | null>(null);
  const zoomPopupRef = useRef<HTMLDivElement>(null);
  const zoomBtnRef = useRef<HTMLButtonElement>(null);

  const handleTileWindow = useCallback(async (action: string) => {
    const ctrl = window.nativesAPI?.windowControls;
    if (!ctrl?.tileWindow) return;
    try {
      await ctrl.tileWindow(action);
      await refreshWindowState();
    } catch { /* fallback */ }
  }, [refreshWindowState]);

  // 当菜单打开时，document mouseup 检测鼠标下方元素
  useEffect(() => {
    if (!zoomMenuOpen) return;
    const handler = (e: globalThis.MouseEvent) => {
      const target = document.elementFromPoint(e.clientX, e.clientY);
      // 向上查找最近的 data-tile-action 元素
      let el: Element | null = target;
      while (el && el !== document.body) {
        if (el instanceof HTMLElement && el.dataset.tileAction) {
          void handleTileWindow(el.dataset.tileAction);
          break;
        }
        el = el.parentElement;
      }
      setZoomMenuOpen(false);
    };
    document.addEventListener('mouseup', handler);
    return () => document.removeEventListener('mouseup', handler);
  }, [zoomMenuOpen, handleTileWindow]);

  const clearZoomTimer = useCallback(() => {
    if (zoomTimerRef.current) {
      clearTimeout(zoomTimerRef.current);
      zoomTimerRef.current = null;
    }
  }, []);

  const handleZoomClick = useCallback((e: ReactMouseEvent<HTMLButtonElement>) => {
    // Long-press already handled the interaction
    if (longPressTriggeredRef.current) {
      longPressTriggeredRef.current = false;
      return;
    }
    // Option/Alt-click → toggle native fullscreen (macOS convention)
    if (e.altKey) {
      void handleWindowAction('fullscreen');
      return;
    }
    void handleWindowAction('maximize');
  }, [handleWindowAction]);

  const handleZoomMouseDown = useCallback((e: ReactMouseEvent<HTMLButtonElement>) => {
    if (e.button !== 0) return;
    longPressTriggeredRef.current = false;
    const cx = e.clientX;
    const cy = e.clientY;
    clearZoomTimer();
    zoomTimerRef.current = window.setTimeout(() => {
      longPressTriggeredRef.current = true;
      setZoomMenuPos({ x: cx, y: cy });
      setZoomMenuOpen(true);
    }, 420);
  }, [clearZoomTimer]);

  // ── 弹窗内 SVG 图标组件 ──
  const iconWrap = (svg: React.ReactNode) => <svg width="22" height="14" viewBox="0 0 22 14" fill="none" className="text-[var(--text-secondary)]">{svg}</svg>;

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

    // R-T6: refresh sidebar module list only on module-channel events
    let unsubModule: (() => void) | undefined;
    try {
      if (window.nativesAPI?.onDbStateChanged) {
        unsubModule = window.nativesAPI.onDbStateChanged((_event, channel) => {
          if (channel !== 'module') return;
          if (!cancelled) void loadModules();
        });
      }
    } catch { /* browser dev mode */ }

    return () => {
      cancelled = true;
      unsubModule?.();
    };
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
  const sidebarWidth = isCollapsed ? SIDEBAR_COLLAPSED_WIDTH : width;

  return (
    <div className="doppelrand-outer h-full">
    <div className="doppelrand-inner h-full">
    <aside
      className="flex flex-col h-full overflow-hidden"
      style={{
        width: sidebarWidth,
        background: 'var(--sidebar)',
        borderRight: '1px solid var(--border)'
      }}
      role="navigation"
      aria-label={t(locale, 'nav.modules')}
      data-sidebar
      data-collapsed={isCollapsed ? 'true' : 'false'}
    >
      {/* ── 窗口控制（macOS traffic lights） ── */}
      <div className="shrink-0 relative z-[60]">
        <div
          className="mac-traffic-row"
          data-collapsed={isCollapsed ? 'true' : 'false'}
          data-tauri-drag-region
        >
          <div
            className="mac-traffic-lights"
            data-collapsed={isCollapsed ? 'true' : 'false'}
            data-active={windowActive ? 'true' : 'false'}
            data-force-glyphs={zoomMenuOpen ? 'true' : undefined}
            role="toolbar"
            aria-label={t(locale, 'header.windowControls')}
          >
            {/* 关闭 — macOS 红圆 */}
            <button
              type="button"
              onClick={() => void handleWindowAction('close')}
              className="mac-traffic-btn close"
              aria-label={t(locale, 'header.close')}
              title={t(locale, 'header.close')}
            >
              <svg viewBox="0 0 10 10" fill="none" aria-hidden="true">
                <path d="M2.2 2.2l5.6 5.6M7.8 2.2L2.2 7.8" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
              </svg>
            </button>
            {/* 最小化 — macOS 黄圆 */}
            <button
              type="button"
              onClick={() => void handleWindowAction('minimize')}
              className="mac-traffic-btn minimize"
              aria-label={t(locale, 'header.minimize')}
              title={t(locale, 'header.minimize')}
            >
              <svg viewBox="0 0 10 10" fill="none" aria-hidden="true">
                <path d="M2.2 5h5.6" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
              </svg>
            </button>
            {/* Zoom — 单击最大化/还原；Option 全屏；长按分屏菜单 */}
            <div className="relative" ref={zoomPopupRef}>
              <button
                type="button"
                ref={zoomBtnRef}
                onClick={handleZoomClick}
                onMouseDown={handleZoomMouseDown}
                onMouseUp={clearZoomTimer}
                onMouseLeave={clearZoomTimer}
                className="mac-traffic-btn zoom"
                aria-label={
                  isFullscreen || isMaximized
                    ? t(locale, 'header.restore')
                    : t(locale, 'header.maximize')
                }
                title={
                  isFullscreen || isMaximized
                    ? t(locale, 'header.restore')
                    : t(locale, 'header.zoomHint')
                }
              >
                {isFullscreen || isMaximized ? (
                  <svg viewBox="0 0 10 10" fill="none" aria-hidden="true">
                    <path d="M2 6.2V8h1.8M8 3.8V2H6.2" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" />
                    <path d="M2 8l2.4-2.4M8 2L5.6 4.4" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" />
                  </svg>
                ) : (
                  <svg viewBox="0 0 10 10" fill="none" aria-hidden="true">
                    <path d="M3.5 1.8H1.8V3.5M6.5 1.8h1.7V3.5M3.5 8.2H1.8V6.5M6.5 8.2h1.7V6.5" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" />
                  </svg>
                )}
              </button>

              {/* 长按弹出菜单 — macOS 窗口管理，跟随鼠标位置 */}
              {zoomMenuOpen && zoomMenuPos && (
                <div
                  className="fixed z-50 min-w-[180px] rounded-xl border border-[var(--border)] bg-[var(--surface)] p-1.5 shadow-modal"
                  style={{ left: zoomMenuPos.x, top: zoomMenuPos.y }}
                >
                  <p className="px-2.5 pb-1 pt-0.5 text-[0.625rem] font-medium uppercase tracking-[0.06em] text-[var(--text-disabled)]">
                    {t(locale, 'header.tileMove')}
                  </p>
                  <div className="grid grid-cols-4 gap-1 px-1 pb-2">
                    {[
                      { id: 'left', label: t(locale, 'header.tileLeft'), icon: leftHalfIcon },
                      { id: 'right', label: t(locale, 'header.tileRight'), icon: rightHalfIcon },
                      { id: 'top', label: t(locale, 'header.tileTop'), icon: topHalfIcon },
                      { id: 'bottom', label: t(locale, 'header.tileBottom'), icon: bottomHalfIcon },
                    ].map((opt) => (
                      <button
                        key={opt.id}
                        type="button"
                        data-tile-action={opt.id}
                        className="flex flex-col items-center gap-1 rounded-lg px-2 py-2 text-[0.625rem] text-[var(--text-secondary)] hover:bg-[var(--surface)] hover:text-[var(--text)] transition-all"
                        title={opt.label}
                      >
                        {opt.icon}
                        <span>{opt.label}</span>
                      </button>
                    ))}
                  </div>
                  <div className="mx-2 my-1 border-t border-[var(--border)]" />
                  <p className="px-2.5 pb-1 pt-1.5 text-[0.625rem] font-medium uppercase tracking-[0.06em] text-[var(--text-disabled)]">
                    {t(locale, 'header.tileFill')}
                  </p>
                  <div className="grid grid-cols-4 gap-1 px-1 pb-1">
                    {[
                      { id: 'fullscreen', label: t(locale, 'header.tileFullscreen'), icon: fillIcon },
                      { id: 'left-half', label: t(locale, 'header.tileLeftHalf'), icon: leftFillIcon },
                      { id: 'right-half', label: t(locale, 'header.tileRightHalf'), icon: rightFillIcon },
                      { id: 'tile', label: t(locale, 'header.tileRestore'), icon: tileIcon },
                    ].map((opt) => (
                      <button
                        key={opt.id}
                        type="button"
                        data-tile-action={opt.id}
                        className="flex flex-col items-center gap-1 rounded-lg px-2 py-2 text-[0.625rem] text-[var(--text-secondary)] hover:bg-[var(--surface)] hover:text-[var(--text)] transition-all"
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

          {!isCollapsed && (
            <button
              type="button"
              onClick={onToggle}
              className="ml-auto flex h-7 w-7 items-center justify-center rounded-md text-[var(--text-disabled)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)]"
              aria-label={t(locale, 'sidebar.collapse')}
              title={t(locale, 'sidebar.collapse')}
            >
              <PanelLeftClose size={15} />
            </button>
          )}

          {isCollapsed && (
            <button
              type="button"
              onClick={onToggle}
              className="flex h-9 w-9 items-center justify-center rounded-lg text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)]"
              aria-label={t(locale, 'sidebar.expand')}
              title={t(locale, 'sidebar.expand')}
            >
              <PanelLeft size={15} />
            </button>
          )}
        </div>
      </div>

      {isSettingsMode ? (
        /* ── Settings Sidebar Layout ── */
        <div className="flex-1 flex flex-col min-h-0">
          {/* Back button */}
          <div className={`pt-2 ${isCollapsed ? 'px-2 pb-2' : 'px-4 pb-3'}`}>
            <button
              onClick={() => selectNavigation('dashboard', '__dashboard__')}
              className={
                isCollapsed
                  ? 'flex h-9 w-9 items-center justify-center rounded-lg text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)] transition-all'
                  : 'flex items-center gap-1.5 rounded-lg px-3 py-2 text-xs text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)] transition-all w-full font-medium'
              }
              title={t(locale, 'settings.backHome')}
              aria-label={t(locale, 'settings.backHome')}
            >
              <ArrowLeft size={13} />
              {!isCollapsed && <span>{t(locale, 'settings.backHome')}</span>}
            </button>
          </div>

          {/* Section title */}
          {!isCollapsed && (
            <div className="px-5 pb-2 pt-1 text-[0.6875rem] font-semibold uppercase tracking-[0.08em] text-[var(--text-disabled)]">
              {t(locale, 'settings.title')}
            </div>
          )}

          {/* Settings items — flat nav with five sections */}
          <div className={`flex flex-col gap-0.5 flex-1 overflow-y-auto ${isCollapsed ? 'items-center px-2' : 'px-3'}`}>
            {SETTINGS_NAV_ITEMS.map((item) => {
              const Icon = item.icon;
              const isActive = activeSettingsSection === item.id;
              const label = t(locale, item.labelKey);
              return (
                <button
                  key={item.id}
                  type="button"
                  aria-current={isActive ? 'page' : undefined}
                  aria-label={label}
                  onClick={() => selectNavigation('__settings__', `settings:${item.id}`)}
                  title={label}
                  className={
                    isCollapsed
                      ? `flex h-9 w-9 items-center justify-center rounded-lg transition-all ${
                          isActive
                            ? 'bg-[var(--accent)] text-[var(--accent-ink)]'
                            : 'text-[var(--text-secondary)] hover:text-[var(--primary)] hover:bg-[var(--border-subtle)]'
                        }`
                      : `flex w-full items-center gap-2.5 rounded-lg px-3 py-2 text-left transition-all ${
                          isActive
                            ? 'bg-[var(--accent)] text-[var(--accent-ink)] font-medium'
                            : 'text-[var(--text-secondary)] hover:text-[var(--primary)] hover:bg-[var(--border-subtle)]'
                        }`
                  }
                >
                  <Icon size={15} className="shrink-0" />
                  {!isCollapsed && <span className="truncate text-sm">{label}</span>}
                </button>
              );
            })}
          </div>
        </div>
      ) : isCollapsed ? (
        /* ── Collapsed icon rail ── */
        <>
          <div className="flex-1 overflow-y-auto min-h-0 flex flex-col items-center gap-1 px-2 pt-1">
            <button
              type="button"
              onClick={() => window.dispatchEvent(new CustomEvent('open-cmdk'))}
              className="flex h-9 w-9 items-center justify-center rounded-lg text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)]"
              title={t(locale, 'sidebar.searchPlaceholder')}
              aria-label={t(locale, 'sidebar.searchPlaceholder')}
            >
              <Search size={15} />
            </button>

            <div className="my-1 h-px w-6 bg-[var(--border-subtle)]" />

            {QUICK_ACCESS_ITEMS.map((item) => {
              const Icon = item.icon;
              const label = t(locale, 'sidebar.quickAccessDirs.' + item.id);
              return (
                <SidebarNavItem
                  key={item.id}
                  collapsed
                  isActive={activeNavigationId === item.target}
                  icon={<Icon size={15} />}
                  label={label}
                  onClick={() => selectNavigation(item.target, item.target)}
                />
              );
            })}

            <div className="my-1 h-px w-6 bg-[var(--border-subtle)]" />

            <SidebarNavItem
              collapsed
              isActive={activeNavigationId === '__assistant__'}
              icon={<MessageSquare size={15} />}
              label={t(locale, 'nav.assistant')}
              onClick={() => selectNavigation('__assistant__', '__assistant__')}
            />

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
                  key={`${moduleId}-${index}`}
                  collapsed
                  isActive={activeNavigationId === navigationId}
                  icon={
                    moduleIcon ? (
                      <img src={moduleIcon} alt="" draggable={false} className="h-[18px] w-[18px] object-contain" />
                    ) : (
                      <Square size={15} />
                    )
                  }
                  label={moduleName}
                  onClick={() => selectNavigation(navigationId, moduleId)}
                />
              );
            })}

            {enabledTools.map((et) => {
              const toolDef = BUILTIN_TOOLS.find((tool) => tool.id === et.id);
              if (!toolDef) return null;
              const navigationId = `builtin:${et.id}`;
              const toolLabel = locale.startsWith('zh') ? toolDef.label.zh : toolDef.label.en;
              const IconComp = (LucideIcons as unknown as Record<string, React.ComponentType<{ size?: number; className?: string }>>)[toolDef.icon];
              return (
                <SidebarNavItem
                  key={et.id}
                  collapsed
                  isActive={activeNavigationId === navigationId}
                  icon={IconComp ? <IconComp size={15} /> : <Square size={15} />}
                  label={toolLabel}
                  onClick={() => selectNavigation(navigationId, navigationId)}
                />
              );
            })}
          </div>

          <div className="shrink-0 flex flex-col items-center gap-1 px-2 py-2 border-t border-[var(--border-subtle)]">
            <SidebarNavItem
              collapsed
              isActive={false}
              icon={<Bell size={16} />}
              label={t(locale, 'notifications.title')}
              onClick={onNotificationClick}
            />
            <SidebarNavItem
              collapsed
              isActive={activeNavigationId === '__settings__'}
              icon={<Settings size={16} />}
              label={t(locale, 'nav.settings')}
              onClick={() => selectNavigation('__settings__', 'settings:general')}
            />
            <SidebarNavItem
              collapsed
              isActive={activeNavigationId === '__workshop__'}
              icon={<Layers size={16} />}
              label={t(locale, 'nav.modules')}
              onClick={() => selectNavigation('__workshop__', 'modules')}
            />
          </div>
        </>
      ) : (
        /* ── Normal Sidebar Layout ── */
        <>
          {/* 中间可滚动区域 */}
          <div className="flex-1 overflow-y-auto min-h-0">
            {/* Search Capsule */}
            <div className="px-4 pb-3">
              <div className="flex h-10 items-center gap-2 rounded-xl border border-[var(--border)] bg-[var(--surface)] px-3">
                <Search size={14} className="text-[var(--text-disabled)] shrink-0" />
                <input
                  type="text"
                  placeholder={t(locale, 'sidebar.searchPlaceholder')}
                  className="min-w-0 flex-1 bg-transparent text-sm text-[var(--text)] outline-none focus-visible:outline-none placeholder:text-[var(--text-disabled)]"
                />
                <span className="shrink-0 rounded-md bg-[var(--surface)] px-1.5 py-0.5 text-[0.6875rem] font-medium text-[var(--text-disabled)]">
                  ⌘K
                </span>
              </div>
            </div>

            {/* Quick Access List */}
            <div className="px-3 pb-1 pt-0 text-[0.6875rem] font-semibold uppercase tracking-[0.08em] text-[var(--text-disabled)]">
              {t(locale, 'sidebar.quickAccess')}
            </div>
            <div className="mb-3 flex flex-col gap-0.5 px-3">
              {QUICK_ACCESS_ITEMS.map((item) => {
                const Icon = item.icon;
                const isActive = activeNavigationId === item.target;
                const label = t(locale, 'sidebar.quickAccessDirs.' + item.id);
                return (
                  <button
                    key={item.id}
                    type="button"
                    onClick={() => selectNavigation(item.target, item.target)}
                    className={`flex w-full items-center gap-2.5 rounded-lg px-3 py-1.5 text-left transition-all ${
                      isActive
                        ? 'bg-[var(--accent)] text-[var(--accent-ink)] font-medium'
                        : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)]'
                    }`}
                    title={label}
                  >
                    <Icon size={15} className="shrink-0" />
                    <span className="truncate text-sm">{label}</span>
                  </button>
                );
              })}
            </div>

            {/* Assistant is a first-level directory, parallel to Quick Access. */}
            <div className="mb-1">
              <div className="flex items-center px-3 pb-1 pt-2 text-[0.6875rem] font-semibold uppercase tracking-[0.08em] text-[var(--text-disabled)]">
                <span className="min-w-0 flex-1 truncate">{t(locale, 'nav.assistant')}</span>
                <button
                  type="button"
                  onClick={() => setAssistantExpanded(value => !value)}
                  className="rounded p-1 text-[var(--text-disabled)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)]"
                  title={assistantExpanded ? t(locale, 'common.collapse') : t(locale, 'common.expand')}
                  aria-label={assistantExpanded ? t(locale, 'common.collapse') : t(locale, 'common.expand')}
                >
                  {assistantExpanded ? <ChevronDown size={13} /> : <ChevronRight size={13} />}
                </button>
                <button
                  type="button"
                  onClick={() => { selectNavigation('assistant', '__assistant__'); assistantActions?.addProjectFolder(); }}
                  className="rounded p-1 text-[var(--text-disabled)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)]"
                  title={t(locale, 'assistant.chooseProjectDirectory')}
                  aria-label={t(locale, 'assistant.chooseProjectDirectory')}
                >
                  <FolderPlus size={13} />
                </button>
              </div>
              {assistantExpanded && (
                <div className="px-3">
                   <AssistantSidebarSection
                    locale={locale}
                    activeNavigationId={activeNavigationId}
                    onNavigateAssistant={() => selectNavigation('__assistant__', '__assistant__')}
                  />
                </div>
              )}
            </div>

          {/* Modules section — same level as Quick Access */}
          <div className="px-3 pb-1 pt-2 text-[0.6875rem] font-semibold uppercase tracking-[0.08em] text-[var(--text-disabled)]">
            {t(locale, 'nav.modules')}
          </div>
          {modules.length > 0 ? (
            <div
              className="mb-3 flex flex-col gap-0.5 px-3"
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
                    key={`${moduleId}-${index}`}
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
          ) : (
            <div className="mb-3 px-3 py-2 text-xs text-[var(--text-disabled)] italic">
              {t(locale, 'sidebar.noModules')}
            </div>
          )}

            {/* Builtin Tools section */}
            {enabledTools.length > 0 && (
              <>
                <div className="px-3 pb-1 pt-2 text-[0.6875rem] font-semibold uppercase tracking-[0.08em] text-[var(--text-disabled)]">
                  {t(locale, 'nav.builtinTools')}
                </div>
                <div className="mb-3 flex flex-col gap-0.5 px-3">
                  {enabledTools.map((et) => {
                    const toolDef = BUILTIN_TOOLS.find((tool) => tool.id === et.id);
                    if (!toolDef) return null;
                    const navigationId = `builtin:${et.id}`;
                    const toolLabel = locale.startsWith('zh') ? toolDef.label.zh : toolDef.label.en;
                    // Dynamic icon lookup from lucide
                    const IconComp = (LucideIcons as unknown as Record<string, React.ComponentType<{ size?: number; className?: string }>>)[toolDef.icon];
                    return (
                      <SidebarNavItem
                        key={et.id}
                        isActive={activeNavigationId === navigationId}
                        icon={IconComp ? <IconComp size={15} /> : <Square size={15} />}
                        label={toolLabel}
                        onClick={() => selectNavigation(navigationId, navigationId)}
                        title={toolLabel}
                      />
                    );
                  })}
                </div>
              </>
            )}
          </div>

          {/* 底部固定区域：通知、设置、个人创意 */}
          <div className="shrink-0 px-3 py-2 border-t border-[var(--border-subtle)]">
            <button
              type="button"
              onClick={onNotificationClick}
              className="flex w-full items-center gap-2.5 rounded-lg px-3 py-1.5 text-sm text-[var(--text-secondary)] transition-all hover:bg-[var(--surface-hover)] hover:text-[var(--primary)]"
            >
              <Bell size={16} />
              <span>{t(locale, 'notifications.title')}</span>
            </button>
            <button
              type="button"
              onClick={() => selectNavigation('__settings__', 'settings:general')}
              className={`flex w-full items-center gap-2.5 rounded-lg px-3 py-1.5 text-sm transition-all ${
                activeNavigationId === '__settings__'
                  ? 'bg-[var(--accent)] text-[var(--accent-ink)] font-medium'
                  : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)]'
              }`}
            >
              <Settings size={16} />
              <span>{t(locale, 'nav.settings')}</span>
            </button>
            <button
              type="button"
              onClick={() => selectNavigation('__workshop__', 'modules')}
              className={`flex w-full items-center gap-2.5 rounded-lg px-3 py-1.5 text-sm transition-all ${
                activeNavigationId === '__workshop__'
                  ? 'bg-[var(--accent)] text-[var(--accent-ink)] font-medium'
                  : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)]'
              }`}
            >
              <Layers size={16} />
              <span>{t(locale, 'nav.modules')}</span>
            </button>
          </div>
        </>
      )}

    </aside>
    </div>
    </div>
  );
}
