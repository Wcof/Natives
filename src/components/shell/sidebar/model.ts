'use client';

import {
  Cpu,
  Download,
  FileText,
  LayoutDashboard,
  Monitor,
  Palette,
  Plug,
  Settings,
  SlidersHorizontal,
  type LucideIcon,
} from 'lucide-react';
import type { SettingsSection } from '../settings-navigation';
import { isSettingsView } from '../settings-navigation';

export interface ModuleManifest {
  id: string;
  name: string;
  icon?: string;
  i18n?: {
    name?: Record<string, string>;
  };
}

export interface ModuleItem {
  moduleId: string;
  manifest: ModuleManifest | null;
  error?: string;
}

export interface QuickAccessItem {
  id: 'home' | 'desktop' | 'documents' | 'downloads';
  target: string;
  path?: string;
  icon: LucideIcon;
}

/**
 * Fixed system directories shown inside the collapsible "文件管理器" section.
 * They are flat navigation items (no expandable tree / arrows): clicking one
 * opens the Files view at that directory. `documents` intentionally points at
 * the real `~/Documents` — the previous `~/.natives` mapping was wrong.
 */
export const FILE_MANAGER_DIRS: readonly QuickAccessItem[] = [
  {
    id: 'desktop',
    target: '__files__:~/Desktop',
    path: '~/Desktop',
    icon: Monitor,
  },
  {
    id: 'documents',
    target: '__files__:~/Documents',
    path: '~/Documents',
    icon: FileText,
  },
  {
    id: 'downloads',
    target: '__files__:~/Downloads',
    path: '~/Downloads',
    icon: Download,
  },
];

export const QUICK_ACCESS_ITEMS: readonly QuickAccessItem[] = [
  {
    id: 'home',
    target: '__dashboard__',
    icon: LayoutDashboard,
  },
];

export const SETTINGS_NAV_ITEMS = [
  { id: 'personal', labelKey: 'settings.tabPersonalOverview', icon: LayoutDashboard },
  { id: 'general', labelKey: 'settings.tabGeneral', icon: Settings },
  { id: 'appearance', labelKey: 'settings.tabAppearance', icon: Palette },
  { id: 'providers', labelKey: 'settings.tabProviders', icon: Cpu },
  { id: 'runtime', labelKey: 'settings.tabExecutor', icon: SlidersHorizontal },
  { id: 'plugins', labelKey: 'settings.tabPlugins', icon: Plug },
] satisfies ReadonlyArray<{
  id: SettingsSection;
  labelKey: string;
  icon: LucideIcon;
}>;

/** Collapsed rail width — icon-only navigation, still interactive. */
export const SIDEBAR_COLLAPSED_WIDTH = 64;
/** Expanded sidebar floor — labels still readable. */
export const SIDEBAR_MIN_WIDTH = 200;
/** Hard cap; further limited by viewport so main content keeps a floor. */
export const SIDEBAR_MAX_WIDTH = 420;
/** Keep at least this much room for workspace + right panel when open. */
export const SIDEBAR_MAIN_FLOOR = 480;
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
export function detectNativeTrafficLights(): boolean {
  if (typeof window === 'undefined') return false;
  const nav = window.navigator;
  const platform = (nav as Navigator & { userAgentData?: { platform?: string } }).userAgentData?.platform
    ?? nav.platform
    ?? nav.userAgent
    ?? '';
  return /Mac|iPhone|iPad|iPod/i.test(platform);
}

export function getModuleId(module: ModuleItem): string {
  return module.manifest?.id ?? module.moduleId;
}

export function getNavigationId(activeModuleId?: string): string | null {
  if (!activeModuleId) return null;
  if (activeModuleId === 'dashboard' || activeModuleId === '__dashboard__') return '__dashboard__';
  if (activeModuleId === 'usage' || activeModuleId === '__usage__') return 'usage';
  if (activeModuleId === 'apps' || activeModuleId === '__apps__') return 'apps';
  if (activeModuleId === 'ai' || activeModuleId === '__ai__') return 'ai';
  if (isSettingsView(activeModuleId)) return '__settings__';
  if (activeModuleId === 'workshop' || activeModuleId === '__workshop__' || activeModuleId === 'modules' || activeModuleId === 'store') {
    return '__workshop__';
  }
  if (activeModuleId === 'assistant' || activeModuleId === '__assistant__') return '__assistant__';
  if (activeModuleId === 'jobs' || activeModuleId === '__jobs__') return '__jobs__';
  if (activeModuleId === 'capabilities' || activeModuleId === '__capabilities__') return '__capabilities__';
  if (activeModuleId === 'library' || activeModuleId === '__library__') return '__library__';
  if (activeModuleId.startsWith('module:')) return activeModuleId;
  if (activeModuleId.startsWith('__files__:')) return activeModuleId;
  if (activeModuleId.startsWith('builtin:')) return activeModuleId;
  // Other views (files/ai/tools) are not fixed sidebar entries —
  // returning null keeps the previous highlight from sticking after navigation.
  return null;
}
