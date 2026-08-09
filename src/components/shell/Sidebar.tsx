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
  Blocks,
  BookMarked,
  CalendarClock,
  Download,
  FileText,
  Layers,
  LayoutDashboard,
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
import SidebarDirTree from './SidebarDirTree';
import { useAssistantActions } from '@/components/assistant/AssistantWorkspaceContext';
import {
  isSettingsView,
  getSettingsSection,
  type SettingsSection,
} from './settings-navigation';

export {
  SIDEBAR_COLLAPSED_WIDTH,
  SIDEBAR_MIN_WIDTH,
  SIDEBAR_MAX_WIDTH,
  SIDEBAR_DEFAULT_WIDTH,
  clampSidebarWidth,
} from './sidebar/model';

export default function Sidebar(props: SidebarProps) {
  const c = useSidebar(props);
  const { locale } = c;

  return (
    <div className={`doppelrand-outer h-full ${c.isCollapsed ? 'w-0 !border-0 !border-none overflow-visible' : 'relative'}`} style={c.isCollapsed ? { border: 'none' } : undefined}>
    <div className={`doppelrand-inner h-full ${c.isCollapsed ? 'w-0 !border-0 !border-none overflow-visible' : ''}`} style={c.isCollapsed ? { border: 'none' } : undefined}>
    <aside
      className={`flex flex-col h-full ${c.isCollapsed ? 'overflow-visible' : 'overflow-hidden'}${c.isResizing ? ' is-resizing' : ''}`}
      style={{
        width: c.sidebarWidth,
        position: c.isCollapsed ? 'fixed' : 'relative',
        top: c.isCollapsed ? 0 : undefined,
        left: c.isCollapsed ? 0 : undefined,
        zIndex: c.isCollapsed ? 60 : undefined,
        background: c.isCollapsed ? 'transparent' : 'var(--sidebar)',
        border: 'none',
        borderRight: c.isCollapsed ? 'none' : '1px solid var(--border)',
        transition: c.isResizing ? 'none' : undefined,
        pointerEvents: c.isCollapsed ? 'none' : 'auto',
      }}
      role="navigation"
      aria-label={t(locale, 'nav.modules')}
      data-sidebar
      data-collapsed={c.isCollapsed ? 'true' : 'false'}
      data-resizing={c.isResizing ? 'true' : 'false'}
    >
      <SidebarChrome c={c} />
    </aside>
    </div>
    {/* Right-edge resize handle — expanded only */}
    {!c.isCollapsed && (
      <div
        className={`sidebar-drag-handle ${c.isResizing ? 'active' : ''}`}
        onMouseDown={c.handleSidebarDragStart}
        onDoubleClick={c.handleSidebarDragDoubleClick}
        role="separator"
        aria-orientation="vertical"
        aria-valuenow={c.width}
        aria-valuemin={SIDEBAR_MIN_WIDTH}
        aria-valuemax={SIDEBAR_MAX_WIDTH}
        aria-label={t(locale, 'sidebar.ariaResize')}
        title={t(locale, 'sidebar.dragToResize')}
      />
    )}
    </div>
  );
}
