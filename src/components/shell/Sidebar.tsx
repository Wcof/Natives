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
  Star,
  Folder,
  File,
  X,
  Minus,
  Maximize2,
} from 'lucide-react';
import * as LucideIcons from 'lucide-react';
import { t, type Locale } from '@/i18n';
import { BUILTIN_TOOLS, seedAllBuiltinTools } from '@/lib/builtin-tools';
import {
  useFavorites,
  favoritesNavTarget,
  removeAndPersistFavorite,
  FAVORITES_SIDEBAR_PREVIEW,
  type FavoriteItem,
} from '@/lib/favorites-client';
import AssistantSidebarSection from '@/components/assistant/AssistantSidebarSection';
import { useAssistantActions } from '@/components/assistant/AssistantWorkspaceContext';
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
export const SIDEBAR_COLLAPSED_WIDTH = 0;
/** Expanded sidebar floor — labels still readable. */
export const SIDEBAR_MIN_WIDTH = 200;
/** Hard cap; further limited by viewport so main content keeps a floor. */
export const SIDEBAR_MAX_WIDTH = 420;
/** Keep at least this much room for workspace + right panel when open. */
const SIDEBAR_MAIN_FLOOR = 480;
/** Default expanded width (also double-click reset). */
export const SIDEBAR_DEFAULT_WIDTH = 248;

export function clampSidebarWidth(
  width: number,
  viewportWidth = typeof window !== 'undefined' ? window.innerWidth : 1280,
): number {
  const maxByViewport = Math.max(SIDEBAR_MIN_WIDTH, viewportWidth - SIDEBAR_MAIN_FLOOR);
  const max = Math.min(SIDEBAR_MAX_WIDTH, maxByViewport);
  return Math.max(SIDEBAR_MIN_WIDTH, Math.min(max, Math.round(width)));
}

/** macOS builds overlay native traffic lights; other platforms paint fallback controls. */
function detectNativeTrafficLights(): boolean {
  if (typeof window === 'undefined') return false;
  const nav = window.navigator;
  const platform = (nav as Navigator & { userAgentData?: { platform?: string } }).userAgentData?.platform
    ?? nav.platform
    ?? nav.userAgent
    ?? '';
  return /Mac|iPhone|iPad|iPod/i.test(platform);
}

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
  const { favorites } = useFavorites();
  const [favoritesExpanded, setFavoritesExpanded] = useState(false);
  const [assistantExpanded, setAssistantExpanded] = useState(true);
  const [activeNavigationId, setActiveNavigationId] = useState<string | null>(
    () => getNavigationId(activeModuleId),
  );
  // Actions only — stream runtime ticks must not re-render the whole shell rail.
  const { actions: assistantActions } = useAssistantActions();

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

  const visibleFavorites = favoritesExpanded
    ? favorites
    : favorites.slice(0, FAVORITES_SIDEBAR_PREVIEW);
  const hiddenFavoriteCount = Math.max(0, favorites.length - FAVORITES_SIDEBAR_PREVIEW);

  const handleFavoriteClick = useCallback(
    (item: FavoriteItem) => {
      const target = favoritesNavTarget(item);
      selectNavigation(target, target);
    },
    [selectNavigation],
  );

  const handleFavoriteRemove = useCallback((item: FavoriteItem, e: ReactMouseEvent) => {
    e.stopPropagation();
    e.preventDefault();
    void removeAndPersistFavorite({ id: item.id });
  }, []);

  const favoriteIcon = (item: FavoriteItem) => {
    if (item.kind === 'file') {
      return item.isDir === false
        ? <File size={15} className="shrink-0" />
        : <Folder size={15} className="shrink-0" />;
    }
    if (item.kind === 'module') return <Square size={15} className="shrink-0" />;
    return <Star size={15} className="shrink-0" />;
  };

  // macOS 使用系统原生 traffic lights（tauri.macos.conf.json Overlay）；
  // Windows/Linux 仍为无边框窗口，需要自绘最小化/最大化/关闭。
  //
  // 必须用「挂载后才切换」的模式：SSR 与客户端首帧都渲染 fallback
  // window-controls，等 useEffect 再切到 native spacer。若在 render 里直接
  // 读 detectNativeTrafficLights()，macOS 上会 SSR=false / CSR=true 导致
  // hydration mismatch。
  const [usesNativeTrafficLights, setUsesNativeTrafficLights] = useState(false);
  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect -- platform chrome only known after mount
    setUsesNativeTrafficLights(detectNativeTrafficLights());
  }, []);

  const handleWindowAction = useCallback(async (
    action: 'minimize' | 'maximize' | 'close',
  ) => {
    if (usesNativeTrafficLights) return;
    const ctrl = window.nativesAPI?.windowControls;
    if (!ctrl) return;
    try {
      if (action === 'minimize') await ctrl.minimize();
      else if (action === 'close') await ctrl.close();
      else await ctrl.maximize();
    } catch { /* ignore */ }
  }, [usesNativeTrafficLights]);

  // ── Width resize (right-edge drag handle) ──
  const [isResizing, setIsResizing] = useState(false);
  const widthRef = useRef(width);
  widthRef.current = width;

  useEffect(() => {
    if (isCollapsed) return;
    const onWinResize = () => {
      const next = clampSidebarWidth(widthRef.current);
      if (next !== widthRef.current) onResize(next);
    };
    window.addEventListener('resize', onWinResize);
    return () => window.removeEventListener('resize', onWinResize);
  }, [isCollapsed, onResize]);

  const handleSidebarDragStart = useCallback((e: ReactMouseEvent) => {
    if (isCollapsed) return;
    e.preventDefault();
    e.stopPropagation();
    setIsResizing(true);
    const startX = e.clientX;
    const startW = widthRef.current;

    const handleMove = (ev: MouseEvent) => {
      // Handle sits on the right edge: moving right grows the sidebar.
      const delta = ev.clientX - startX;
      onResize(clampSidebarWidth(startW + delta));
    };
    const handleUp = () => {
      setIsResizing(false);
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
      document.removeEventListener('mousemove', handleMove);
      document.removeEventListener('mouseup', handleUp);
    };

    document.body.style.cursor = 'col-resize';
    document.body.style.userSelect = 'none';
    document.addEventListener('mousemove', handleMove);
    document.addEventListener('mouseup', handleUp);
  }, [isCollapsed, onResize]);

  const handleSidebarDragDoubleClick = useCallback(() => {
    onResize(clampSidebarWidth(SIDEBAR_DEFAULT_WIDTH));
  }, [onResize]);

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
    <div className={`doppelrand-outer h-full ${isCollapsed ? 'w-0 !border-0 !border-none overflow-visible' : 'relative'}`} style={isCollapsed ? { border: 'none' } : undefined}>
    <div className={`doppelrand-inner h-full ${isCollapsed ? 'w-0 !border-0 !border-none overflow-visible' : ''}`} style={isCollapsed ? { border: 'none' } : undefined}>
    <aside
      className={`flex flex-col h-full ${isCollapsed ? 'overflow-visible' : 'overflow-hidden'}${isResizing ? ' is-resizing' : ''}`}
      style={{
        width: sidebarWidth,
        position: isCollapsed ? 'fixed' : 'relative',
        top: isCollapsed ? 0 : undefined,
        left: isCollapsed ? 0 : undefined,
        zIndex: isCollapsed ? 60 : undefined,
        background: isCollapsed ? 'transparent' : 'var(--sidebar)',
        border: 'none',
        borderRight: isCollapsed ? 'none' : '1px solid var(--border)',
        transition: isResizing ? 'none' : undefined,
        pointerEvents: isCollapsed ? 'none' : 'auto',
      }}
      role="navigation"
      aria-label={t(locale, 'nav.modules')}
      data-sidebar
      data-collapsed={isCollapsed ? 'true' : 'false'}
      data-resizing={isResizing ? 'true' : 'false'}
    >
      {/* ── 标题栏：macOS 用系统 traffic lights；其它平台自绘按钮 ──
          注意：不要把折叠按钮放在 data-tauri-drag-region 内部，
          否则在 macOS/WKWebView 上点击会被当成拖窗口，无法折叠。 */}
      <div className="shrink-0 relative z-[60] pointer-events-auto">
        <div
          className="titlebar-row"
          data-collapsed={isCollapsed ? 'true' : 'false'}
          data-native-traffic={usesNativeTrafficLights ? 'true' : 'false'}
        >
          {usesNativeTrafficLights ? (
            /* 为系统红黄绿按钮留位；可拖区域单独挂在 spacer 上 */
            <div
              className="native-traffic-spacer"
              data-tauri-drag-region
              aria-hidden="true"
            />
          ) : (
            <div
              className="window-controls"
              role="toolbar"
              aria-label={t(locale, 'header.windowControls')}
            >
              <button
                type="button"
                onClick={() => void handleWindowAction('minimize')}
                className="window-ctrl-btn"
                aria-label={t(locale, 'header.minimize')}
                title={t(locale, 'header.minimize')}
              >
                <Minus size={12} strokeWidth={2.25} />
              </button>
              <button
                type="button"
                onClick={() => void handleWindowAction('maximize')}
                className="window-ctrl-btn"
                aria-label={t(locale, 'header.maximize')}
                title={t(locale, 'header.maximize')}
              >
                <Maximize2 size={11} strokeWidth={2.25} />
              </button>
              <button
                type="button"
                onClick={() => void handleWindowAction('close')}
                className="window-ctrl-btn window-ctrl-close"
                aria-label={t(locale, 'header.close')}
                title={t(locale, 'header.close')}
              >
                <X size={12} strokeWidth={2.25} />
              </button>
            </div>
          )}

          {/* 设置模式下：直接在标题栏加入“返回首页”交互按钮 */}
          {isSettingsMode && (
            <button
              type="button"
              onClick={() => selectNavigation('dashboard', '__dashboard__')}
              className="flex items-center gap-1.5 rounded-lg px-2 py-1 text-xs text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)] transition-all font-medium shrink-0 ml-1"
              title={t(locale, 'settings.backHome')}
              aria-label={t(locale, 'settings.backHome')}
            >
              <ArrowLeft size={13} />
              {!isCollapsed && <span>{t(locale, 'settings.backHome')}</span>}
            </button>
          )}

          {/* 中间弹性拖拽条：不覆盖两侧交互控件 */}
          {!isCollapsed && (
            <div className="titlebar-drag-fill" data-tauri-drag-region />
          )}

          <button
            type="button"
            onClick={(e) => {
              e.preventDefault();
              e.stopPropagation();
              onToggle();
            }}
            onMouseDown={(e) => {
              // 阻止父级/系统 drag-region 抢走 mousedown
              e.stopPropagation();
            }}
            className={
              isCollapsed
                ? 'titlebar-collapse-btn flex h-7 w-7 items-center justify-center rounded-md text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)]'
                : 'titlebar-collapse-btn flex h-7 w-7 items-center justify-center rounded-md text-[var(--text-disabled)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)]'
            }
            aria-label={t(locale, isCollapsed ? 'sidebar.expand' : 'sidebar.collapse')}
            title={t(locale, isCollapsed ? 'sidebar.expand' : 'sidebar.collapse')}
          >
            {isCollapsed ? <PanelLeft size={15} /> : <PanelLeftClose size={15} />}
          </button>
        </div>
      </div>

      {isSettingsMode ? (
        /* ── Settings Sidebar Layout ── */
        <div className="flex-1 flex flex-col min-h-0 pt-1">
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
      ) : isCollapsed ? null : (
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

            {/* Quick Access List — fixed system shortcuts */}
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

            {/* Favorites — first-level shortcut rail (always visible; empty shows placeholder) */}
            <div className="mb-3" data-sidebar-favorites>
              <div className="px-3 pb-1 pt-0 text-[0.6875rem] font-semibold uppercase tracking-[0.08em] text-[var(--text-disabled)]">
                {t(locale, 'sidebar.favorites')}
              </div>
              {favorites.length === 0 ? (
                <div className="px-3 py-1.5 text-xs italic text-[var(--text-disabled)]">
                  {t(locale, 'sidebar.noFavorites')}
                </div>
              ) : (
                <div className="flex flex-col gap-0.5 px-3" role="list" aria-label={t(locale, 'sidebar.favorites')}>
                  {visibleFavorites.map((item) => {
                    const navTarget = favoritesNavTarget(item);
                    const isActive = activeNavigationId === navTarget;
                    const title = item.kind === 'file' ? item.target : item.label;
                    return (
                      <div
                        key={item.id}
                        role="listitem"
                        className={`group flex w-full items-center gap-1 rounded-lg transition-all ${
                          isActive
                            ? 'bg-[var(--accent)] text-[var(--accent-ink)] font-medium'
                            : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)]'
                        }`}
                      >
                        <button
                          type="button"
                          onClick={() => handleFavoriteClick(item)}
                          className="flex min-w-0 flex-1 items-center gap-2.5 rounded-lg px-3 py-1.5 text-left"
                          title={title}
                        >
                          {favoriteIcon(item)}
                          <span className="truncate text-sm">{item.label}</span>
                        </button>
                        <button
                          type="button"
                          onClick={(e) => handleFavoriteRemove(item, e)}
                          className={`mr-1.5 shrink-0 rounded p-1 opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100 ${
                            isActive
                              ? 'text-[var(--accent-ink)]/70 hover:bg-[var(--accent-ink)]/10 hover:text-[var(--accent-ink)]'
                              : 'text-[var(--text-disabled)] hover:bg-[var(--surface)] hover:text-[var(--text)]'
                          }`}
                          title={t(locale, 'fileBrowser.removeFromFavorites')}
                          aria-label={t(locale, 'fileBrowser.removeFromFavorites')}
                        >
                          <X size={12} />
                        </button>
                      </div>
                    );
                  })}
                  {hiddenFavoriteCount > 0 && (
                    <button
                      type="button"
                      onClick={() => setFavoritesExpanded((v) => !v)}
                      className="rounded-lg px-3 py-1 text-left text-xs text-[var(--text-disabled)] hover:bg-[var(--surface-hover)] hover:text-[var(--text-secondary)]"
                    >
                      {favoritesExpanded
                        ? t(locale, 'sidebar.showLess')
                        : t(locale, 'sidebar.showMore', { count: hiddenFavoriteCount })}
                    </button>
                  )}
                </div>
              )}
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
    {/* Right-edge resize handle — expanded only */}
    {!isCollapsed && (
      <div
        className={`sidebar-drag-handle ${isResizing ? 'active' : ''}`}
        onMouseDown={handleSidebarDragStart}
        onDoubleClick={handleSidebarDragDoubleClick}
        role="separator"
        aria-orientation="vertical"
        aria-valuenow={width}
        aria-valuemin={SIDEBAR_MIN_WIDTH}
        aria-valuemax={SIDEBAR_MAX_WIDTH}
        aria-label={t(locale, 'sidebar.ariaResize')}
        title={t(locale, 'sidebar.dragToResize')}
      />
    )}
    </div>
  );
}
