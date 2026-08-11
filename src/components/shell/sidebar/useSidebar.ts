'use client';

import { startTransition, useCallback, useEffect, useRef, useState } from 'react';
import type { Dispatch, MouseEvent as ReactMouseEvent, DragEvent, SetStateAction } from 'react';
import type { Locale } from '@/i18n';
import { seedAllBuiltinTools } from '@/lib/builtin-tools';
import {
  useFavorites,
  favoritesNavTarget,
  removeAndPersistFavorite,
  FAVORITES_SIDEBAR_PREVIEW,
  type FavoriteItem,
} from '@/lib/favorites-client';
import { useAssistantActions } from '@/components/assistant/AssistantWorkspaceContext';
import { getSettingsSection, isSettingsView } from '../settings-navigation';
import {
  clampSidebarWidth,
  detectNativeTrafficLights,
  getModuleId,
  getNavigationId,
  SIDEBAR_COLLAPSED_WIDTH,
  SIDEBAR_DEFAULT_WIDTH,
  type ModuleItem,
} from './model';

export interface SidebarProps {
  isCollapsed: boolean;
  onToggle: () => void;
  width: number;
  onResize: (width: number) => void;
  activeModuleId?: string;
  onModuleSelect: (target: string) => void;
  onNotificationClick: () => void;
  locale?: Locale;
}

export interface SidebarController {
  locale: Locale;
  isCollapsed: boolean;
  onToggle: () => void;
  onNotificationClick: () => void;
  onModuleSelect: (target: string) => void;
  width: number;
  onResize: (width: number) => void;
  modules: ModuleItem[];
  setModules: (modules: ModuleItem[]) => void;
  dragIndex: number | null;
  setDragIndex: (index: number | null) => void;
  favorites: FavoriteItem[];
  favoritesExpanded: boolean;
  setFavoritesExpanded: Dispatch<SetStateAction<boolean>>;
  visibleFavorites: FavoriteItem[];
  hiddenFavoriteCount: number;
  /** Collapsible "文件管理器" section (fixed system directories, no arrows). */
  fileManagerExpanded: boolean;
  setFileManagerExpanded: Dispatch<SetStateAction<boolean>>;
  assistantExpanded: boolean;
  setAssistantExpanded: Dispatch<SetStateAction<boolean>>;
  activeNavigationId: string | null;
  enabledTools: Array<{ id: string; driver: string }>;
  usesNativeTrafficLights: boolean;
  isResizing: boolean;
  draftWidth: number;
  sidebarWidth: number;
  normalizedLocale: 'zh' | 'en';
  isSettingsMode: boolean;
  activeSettingsSection: string | null;
  assistantActions: ReturnType<typeof useAssistantActions>['actions'];
  selectNavigation: (navigationId: string, target: string) => void;
  handleFavoriteClick: (item: FavoriteItem) => void;
  handleDirTreeNavigate: (path: string) => void;
  handleFavoriteRemove: (item: FavoriteItem, e: ReactMouseEvent) => void;
  handleWindowAction: (action: 'minimize' | 'maximize' | 'close') => Promise<void>;
  handleSidebarDragStart: (e: ReactMouseEvent) => void;
  handleSidebarDragDoubleClick: () => void;
  handleDragOver: (event: DragEvent<HTMLButtonElement>, index: number) => void;
  handleDragEnd: () => Promise<void>;
}

export function useSidebar({
  isCollapsed,
  onToggle,
  width,
  onResize,
  activeModuleId,
  onModuleSelect,
  onNotificationClick,
  locale = 'zh',
}: SidebarProps): SidebarController {
  const [modules, setModules] = useState<ModuleItem[]>([]);
  const [dragIndex, setDragIndex] = useState<number | null>(null);
  const { favorites } = useFavorites();
  const [favoritesExpanded, setFavoritesExpanded] = useState(false);
  const [fileManagerExpanded, setFileManagerExpanded] = useState(true);
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

  // 目录树子行跳转：与 Quick Access/收藏一致走 `__files__:<path>`
  // （ShellLayout 侧统一 setActiveView('files') + navigateToFiles）
  const handleDirTreeNavigate = useCallback(
    (path: string) => {
      const target = `__files__:${path}`;
      selectNavigation(target, target);
    },
    [selectNavigation],
  );

  const handleFavoriteRemove = useCallback((item: FavoriteItem, e: ReactMouseEvent) => {
    e.stopPropagation();
    e.preventDefault();
    void removeAndPersistFavorite({ id: item.id });
  }, []);

  // macOS 使用系统原生 traffic lights（tauri.macos.conf.json Overlay）；
  // Windows/Linux 仍为无边框窗口，需要自绘最小化/最大化/关闭。
  //
  // 必须用「挂载后才切换」的模式：SSR 与客户端首帧都渲染 fallback
  // window-controls，等 useEffect 再切到 native spacer。若在 render 里直接
  // 读 detectNativeTrafficLights()，macOS 上会 SSR=false / CSR=true 导致
  // hydration mismatch。
  const [usesNativeTrafficLights, setUsesNativeTrafficLights] = useState(false);
  useEffect(() => {
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
  const [draftWidth, setDraftWidth] = useState(width);
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
    let latest = startW;
    setDraftWidth(startW);

    const handleMove = (ev: MouseEvent) => {
      // Handle sits on the right edge: moving right grows the sidebar.
      const delta = ev.clientX - startX;
      latest = clampSidebarWidth(startW + delta);
      setDraftWidth(latest);
    };
    const handleUp = () => {
      onResize(latest);
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
  const sidebarWidth = isCollapsed ? SIDEBAR_COLLAPSED_WIDTH : (isResizing ? draftWidth : width);

  return {
    locale,
    isCollapsed,
    onToggle,
    onNotificationClick,
    onModuleSelect,
    width,
    onResize,
    modules,
    setModules,
    dragIndex,
    setDragIndex,
    favorites,
    favoritesExpanded,
    setFavoritesExpanded,
    visibleFavorites,
    hiddenFavoriteCount,
    fileManagerExpanded,
    setFileManagerExpanded,
    assistantExpanded,
    setAssistantExpanded,
    activeNavigationId,
    enabledTools,
    usesNativeTrafficLights,
    isResizing,
    draftWidth,
    sidebarWidth,
    normalizedLocale,
    isSettingsMode,
    activeSettingsSection,
    assistantActions,
    selectNavigation,
    handleFavoriteClick,
    handleDirTreeNavigate,
    handleFavoriteRemove,
    handleWindowAction,
    handleSidebarDragStart,
    handleSidebarDragDoubleClick,
    handleDragOver,
    handleDragEnd,
  };
}
