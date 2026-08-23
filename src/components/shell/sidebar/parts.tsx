'use client';

import type { ComponentType, DragEvent, MouseEvent as ReactMouseEvent, ReactNode } from 'react';
import {
  ArrowLeft,
  BarChart3,
  Bell,
  ChevronDown,
  ChevronRight,
  File,
  Folder,
  FolderOpen,
  Globe,
  Laptop,
  Layers,
  LayoutDashboard,
  Maximize2,
  Minus,
  PanelLeft,
  PanelLeftClose,
  Search,
  Settings,
  Sparkles,
  Square,
  Star,
  X,
} from 'lucide-react';
import * as LucideIcons from 'lucide-react';
import { t } from '@/i18n';
import { BUILTIN_TOOLS } from '@/lib/builtin-tools';
import { favoritesNavTarget, type FavoriteItem } from '@/lib/favorites-client';
import SidebarDirTree from '../SidebarDirTree';
import { QUICK_ACCESS_ITEMS, FILE_MANAGER_DIRS, SETTINGS_NAV_ITEMS, type ModuleItem } from './model';
import type { SidebarController } from './useSidebar';

export function SidebarNavItem({
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
          ? `flex h-9 w-9 items-center justify-center rounded-lg transition-[color,background-color,border-color,opacity,transform] ${
              isActive
                ? 'sidebar-nav-active'
                : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)]'
            }`
          : `flex w-full items-center gap-2.5 rounded-lg px-3 py-1.5 text-left text-sm transition-[color,background-color,border-color,opacity,transform] ${
              isActive
                ? 'sidebar-nav-active'
                : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)]'
            }`
      }
    >
      <span className="shrink-0">{icon}</span>
      {!collapsed && <span className="truncate">{label}</span>}
    </button>
  );
}

export function FavoriteIcon({ item }: { item: FavoriteItem }) {
  if (item.kind === 'file') {
    return item.isDir === false
      ? <File size={15} className="shrink-0" />
      : <Folder size={15} className="shrink-0" />;
  }
  if (item.kind === 'module') return <Square size={15} className="shrink-0" />;
  return <Star size={15} className="shrink-0" />;
}

function Titlebar({ c }: { c: SidebarController }) {
  return (
    <div className="shrink-0 relative z-[60] pointer-events-auto">
      <div
        className="titlebar-row"
        data-collapsed={c.isCollapsed ? 'true' : 'false'}
        data-native-traffic={c.usesNativeTrafficLights ? 'true' : 'false'}
      >
        {c.usesNativeTrafficLights ? (
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
            aria-label={t(c.locale, 'header.windowControls')}
          >
            <button
              type="button"
              onClick={() => void c.handleWindowAction('minimize')}
              className="window-ctrl-btn"
              aria-label={t(c.locale, 'header.minimize')}
              title={t(c.locale, 'header.minimize')}
            >
              <Minus size={12} strokeWidth={2.25} />
            </button>
            <button
              type="button"
              onClick={() => void c.handleWindowAction('maximize')}
              className="window-ctrl-btn"
              aria-label={t(c.locale, 'header.maximize')}
              title={t(c.locale, 'header.maximize')}
            >
              <Maximize2 size={11} strokeWidth={2.25} />
            </button>
            <button
              type="button"
              onClick={() => void c.handleWindowAction('close')}
              className="window-ctrl-btn window-ctrl-close"
              aria-label={t(c.locale, 'header.close')}
              title={t(c.locale, 'header.close')}
            >
              <X size={12} strokeWidth={2.25} />
            </button>
          </div>
        )}

        {/* 中间弹性拖拽条：不覆盖两侧交互控件 */}
        {!c.isCollapsed && (
          <div className="titlebar-drag-fill" data-tauri-drag-region />
        )}

        <button
          type="button"
          onClick={(e) => {
            e.preventDefault();
            e.stopPropagation();
            c.onToggle();
          }}
          onMouseDown={(e) => {
            // 阻止父级/系统 drag-region 抢走 mousedown
            e.stopPropagation();
          }}
          className={
            c.isCollapsed
              ? 'titlebar-collapse-btn flex h-7 w-7 items-center justify-center rounded-md text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)]'
              : 'titlebar-collapse-btn flex h-7 w-7 items-center justify-center rounded-md text-[var(--text-disabled)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)]'
          }
          aria-label={t(c.locale, c.isCollapsed ? 'sidebar.expand' : 'sidebar.collapse')}
          title={t(c.locale, c.isCollapsed ? 'sidebar.expand' : 'sidebar.collapse')}
        >
          {c.isCollapsed ? <PanelLeft size={15} /> : <PanelLeftClose size={15} />}
        </button>
      </div>
    </div>
  );
}

function SettingsNav({ c }: { c: SidebarController }) {
  return (
    <div className="flex-1 flex flex-col min-h-0 pt-1">
      {/* Settings items — flat nav with five sections */}
      <div className={`flex flex-col gap-0.5 flex-1 overflow-y-auto ${c.isCollapsed ? 'items-center px-2' : 'px-3'}`}>
        {/* 返回主页：置于《通用》正上方，严格左对齐 */}
        <button
          type="button"
          onClick={() => c.selectNavigation('dashboard', '__dashboard__')}
          title={t(c.locale, 'settings.backHome')}
          aria-label={t(c.locale, 'settings.backHome')}
          className={
            c.isCollapsed
              ? 'flex h-9 w-9 items-center justify-center rounded-lg text-[var(--text-secondary)] hover:text-[var(--primary)] hover:bg-[var(--border-subtle)] transition-[color,background-color,border-color,opacity,transform] mb-0.5 shrink-0'
              : 'flex w-full items-center gap-2.5 rounded-lg px-3 py-2 text-left text-[var(--text-secondary)] hover:text-[var(--primary)] hover:bg-[var(--border-subtle)] transition-[color,background-color,border-color,opacity,transform] font-medium mb-0.5 shrink-0'
          }
        >
          <ArrowLeft size={15} className="shrink-0" />
          {!c.isCollapsed && <span className="truncate text-sm">{t(c.locale, 'settings.backHome')}</span>}
        </button>

        <div className="w-full my-1 border-t border-[var(--border-subtle)] opacity-50" />

        {SETTINGS_NAV_ITEMS.map((item) => {
          const Icon = item.icon;
          const isActive = c.activeSettingsSection === item.id;
          const label = t(c.locale, item.labelKey);
          return (
            <button
              key={item.id}
              type="button"
              aria-current={isActive ? 'page' : undefined}
              aria-label={label}
              onClick={() => c.selectNavigation('__settings__', `settings:${item.id}`)}
              title={label}
              className={
                c.isCollapsed
                  ? `flex h-9 w-9 items-center justify-center rounded-lg transition-[color,background-color,border-color,opacity,transform] ${
                      isActive
                        ? 'sidebar-nav-active'
                        : 'text-[var(--text-secondary)] hover:text-[var(--primary)] hover:bg-[var(--border-subtle)]'
                    }`
                  : `flex w-full items-center gap-2.5 rounded-lg px-3 py-2 text-left transition-[color,background-color,border-color,opacity,transform] ${
                      isActive
                        ? 'sidebar-nav-active'
                        : 'text-[var(--text-secondary)] hover:text-[var(--primary)] hover:bg-[var(--border-subtle)]'
                    }`
              }
            >
              <Icon size={15} className="shrink-0" />
              {!c.isCollapsed && <span className="truncate text-sm">{label}</span>}
            </button>
          );
        })}
      </div>
    </div>
  );
}

function QuickAccessSection({ c }: { c: SidebarController }) {
  const { locale } = c;
  return (
    <>
      {/* Quick Access List — fixed home shortcut */}
      <div className="px-3 pb-1 pt-0 text-[0.6875rem] font-semibold uppercase tracking-[0.08em] text-[var(--text-disabled)]">
        {t(locale, 'sidebar.quickAccess')}
      </div>
      <div className="mb-3 flex flex-col gap-0.5 px-3">
        {QUICK_ACCESS_ITEMS.map((item) => {
          const Icon = item.icon;
          const isActive = c.activeNavigationId === item.target;
          const label = t(locale, 'sidebar.quickAccessDirs.' + item.id);
          return (
            <button
              key={item.id}
              type="button"
              onClick={() => c.selectNavigation(item.target, item.target)}
              className={`flex w-full items-center gap-2.5 rounded-lg px-3 py-1.5 text-left transition-[color,background-color,border-color,opacity,transform] ${
                isActive
                  ? 'sidebar-nav-active'
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
    </>
  );
}

/**
 * Collapsible "文件管理器" section: Desktop / Documents / Downloads are flat
 * navigation items (no expandable tree, no arrows). Clicking one opens the
 * Files view at that directory via `__files__:<path>`.
 */
function FileManagerSection({ c }: { c: SidebarController }) {
  const { locale, fileManagerExpanded } = c;
  return (
    <div className="mb-1">
      <div className="flex items-center px-3 pb-1 pt-2 text-[0.6875rem] font-semibold uppercase tracking-[0.08em] text-[var(--text-disabled)]">
        <span className="min-w-0 flex-1 truncate">{t(locale, 'sidebar.fileManager')}</span>
        <button
          type="button"
          onClick={() => c.setFileManagerExpanded((value: boolean) => !value)}
          className="rounded p-1 text-[var(--text-disabled)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)]"
          title={fileManagerExpanded ? t(locale, 'common.collapse') : t(locale, 'common.expand')}
          aria-label={fileManagerExpanded ? t(locale, 'common.collapse') : t(locale, 'common.expand')}
          aria-expanded={fileManagerExpanded}
        >
          {fileManagerExpanded ? <ChevronDown size={13} /> : <ChevronRight size={13} />}
        </button>
      </div>
      {fileManagerExpanded && (
        <div className="mb-3 flex flex-col gap-0.5 px-3">
          {FILE_MANAGER_DIRS.map((item) => {
            const Icon = item.icon;
            const isActive = c.activeNavigationId === item.target;
            const label = t(locale, 'sidebar.quickAccessDirs.' + item.id);
            return (
              <button
                key={item.id}
                type="button"
                onClick={() => c.selectNavigation(item.target, item.target)}
                className={`flex w-full items-center gap-2.5 rounded-lg px-3 py-1.5 text-left transition-[color,background-color,border-color,opacity,transform] ${
                  isActive
                    ? 'sidebar-nav-active'
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
      )}
    </div>
  );
}

function AppsSection({ c }: { c: SidebarController }) {
  const { locale, sidebarApps, appsExpanded, setAppsExpanded } = c;
  const isAppsActive = c.activeNavigationId === 'apps';

  const getKindIcon = (kind: string) => {
    switch (kind) {
      case 'local_project':
        return <Folder size={14} className="text-[var(--success)] shrink-0" />;
      case 'system_application':
        return <Laptop size={14} className="text-[var(--interactive-accent)] shrink-0" />;
      case 'web_application':
        return <Globe size={14} className="text-[var(--primary)] shrink-0" />;
      default:
        return <Layers size={14} className="text-[var(--interactive-accent)] shrink-0" />;
    }
  };

  return (
    <div className="mb-3">
      <div className="flex items-center justify-between px-3 pb-1 pt-0 text-[0.6875rem] font-semibold uppercase tracking-[0.08em] text-[var(--text-disabled)]">
        <button
          type="button"
          onClick={() => setAppsExpanded((prev) => !prev)}
          className="flex items-center gap-1 hover:text-[var(--text-secondary)] transition-colors"
        >
          {appsExpanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
          <span>{t(locale, 'nav.apps')}</span>
        </button>
        {sidebarApps.length > 0 && (
          <span className="text-[10px] text-[var(--text-disabled)] font-normal">
            {sidebarApps.length}
          </span>
        )}
      </div>

      {/* Main Apps View Button */}
      <div className="flex flex-col gap-0.5 px-3">
        <button
          type="button"
          onClick={() => c.selectNavigation('apps', 'apps')}
          className={`flex w-full items-center gap-2.5 rounded-lg px-3 py-1.5 text-left transition-[color,background-color,border-color,opacity,transform] ${
            isAppsActive
              ? 'sidebar-nav-active'
              : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)]'
          }`}
        >
          <Layers size={15} className="shrink-0" />
          <span className="truncate text-sm">{t(locale, 'appsPage.title')}</span>
        </button>

        {/* Projected Sidebar Apps */}
        {appsExpanded && sidebarApps.length > 0 && (
          <div className="flex flex-col gap-0.5 pl-3 pt-0.5 border-l border-[var(--border-subtle)] ml-4">
            {sidebarApps.map((app) => {
              const itemTarget = `apps:item:${app.appId}`;
              const isItemActive = c.activeNavigationId === itemTarget;
              return (
                <button
                  key={app.appId}
                  type="button"
                  onClick={() => c.handleAppClick(app)}
                  className={`flex w-full items-center gap-2 rounded-lg px-2.5 py-1 text-left text-xs transition-[color,background-color,border-color,opacity,transform] ${
                    isItemActive
                      ? 'sidebar-nav-active font-medium'
                      : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)]'
                  }`}
                  title={app.title}
                >
                  {getKindIcon(app.kind)}
                  <span className="truncate flex-1">{app.title}</span>
                  {app.runtimeState === 'running' && (
                    <span className="h-1.5 w-1.5 rounded-full bg-[var(--success)] shrink-0" />
                  )}
                </button>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}

function FavoritesSection({ c }: { c: SidebarController }) {
  const { locale, favorites, visibleFavorites, hiddenFavoriteCount } = c;
  return (
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
            const isActive = c.activeNavigationId === navTarget;
            const title = item.kind === 'file' ? item.target : item.label;
            // 行渲染保持原样；arrow 为目录树注入的行首 ▸/▾（非目录/不可用为 null）
            const row = (arrow: ReactNode) => (
              <div
                role="listitem"
                className={`group flex w-full items-center gap-1 rounded-lg transition-[color,background-color,border-color,opacity,transform] ${
                  isActive
                    ? 'sidebar-nav-active'
                    : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)]'
                }`}
              >
                <button
                  type="button"
                  onClick={() => c.handleFavoriteClick(item)}
                  className="flex min-w-0 flex-1 items-center gap-2.5 rounded-lg px-3 py-1.5 text-left"
                  title={title}
                >
                  {arrow}
                  <FavoriteIcon item={item} />
                  <span className="truncate text-sm">{item.label}</span>
                </button>
                <button
                  type="button"
                  onClick={(e: ReactMouseEvent) => c.handleFavoriteRemove(item, e)}
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
            // 目录收藏（isDir 缺省按目录处理，与 favoriteIcon 一致）挂懒加载目录树
            const isDirFavorite = item.kind === 'file' && item.isDir !== false;
            if (!isDirFavorite) return <div key={item.id}>{row(null)}</div>;
            return (
              <SidebarDirTree
                key={item.id}
                path={item.target}
                locale={locale}
                activeNavigationId={c.activeNavigationId}
                onNavigate={c.handleDirTreeNavigate}
                renderRow={row}
              />
            );
          })}
          {hiddenFavoriteCount > 0 && (
            <button
              type="button"
              onClick={() => c.setFavoritesExpanded((v) => !v)}
              className="rounded-lg px-3 py-1 text-left text-xs text-[var(--text-disabled)] hover:bg-[var(--surface-hover)] hover:text-[var(--text-secondary)]"
            >
              {c.favoritesExpanded
                ? t(locale, 'sidebar.showLess')
                : t(locale, 'sidebar.showMore', { count: hiddenFavoriteCount })}
            </button>
          )}
        </div>
      )}
    </div>
  );
}

function ModulesSection({ c }: { c: SidebarController }) {
  const { locale, modules } = c;
  return (
    <>
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
          onDragEnd={() => void c.handleDragEnd()}
        >
          {modules.map((module: ModuleItem, index: number) => {
            const moduleId = module.manifest?.id ?? module.moduleId;
            const moduleName =
              module.manifest?.i18n?.name?.[c.normalizedLocale] ??
              module.manifest?.name ??
              module.moduleId;
            const moduleIcon = module.manifest?.icon;
            const navigationId = `module:${moduleId}`;
            return (
              <SidebarNavItem
                key={`${moduleId}-${index}`}
                isActive={c.activeNavigationId === navigationId}
                icon={
                  moduleIcon ? (
                    <img src={moduleIcon} alt="" draggable={false} className="h-[18px] w-[18px] object-contain" />
                  ) : (
                    <Square size={18} />
                  )
                }
                label={moduleName}
                role="option"
                aria-selected={c.activeNavigationId === navigationId}
                draggable
                onDragStart={() => c.setDragIndex(index)}
                onDragOver={(event) => c.handleDragOver(event, index)}
                onClick={() => c.selectNavigation(navigationId, moduleId)}
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
    </>
  );
}

function BuiltinToolsSection({ c }: { c: SidebarController }) {
  const { locale, enabledTools } = c;
  if (enabledTools.length === 0) return null;
  return (
    <>
      <div className="px-3 pb-1 pt-2 text-[0.6875rem] font-semibold uppercase tracking-[0.08em] text-[var(--text-disabled)]">
        {t(locale, 'nav.builtinTools')}
      </div>
      <div className="mb-3 flex flex-col gap-0.5 px-3">
        {enabledTools.map((et) => {
          const toolDef = BUILTIN_TOOLS.find((tool) => tool.id === et.id);
          if (!toolDef) return null;
          const navigationId = `builtin:${et.id}`;
          const toolLabel = t(locale, toolDef.labelKey);
          // Dynamic icon lookup from lucide
          const IconComp = (LucideIcons as unknown as Record<string, ComponentType<{ size?: number; className?: string }>>)[toolDef.icon];
          return (
            <SidebarNavItem
              key={et.id}
              isActive={c.activeNavigationId === navigationId}
              icon={IconComp ? <IconComp size={15} /> : <Square size={15} />}
              label={toolLabel}
              onClick={() => c.selectNavigation(navigationId, navigationId)}
              title={toolLabel}
            />
          );
        })}
      </div>
    </>
  );
}

/** Normal-mode scrollable body: search, Quick Access, Favorites, Assistant, Modules, Builtin Tools. */
export function SidebarBody({ c }: { c: SidebarController }) {
  return (
    <div className="flex-1 overflow-y-auto min-h-0">
      {/* Search Capsule — 打开命令面板的入口。此前是无任何 handler 的
          装饰 input（能打字但什么都不发生），现改为真实按钮 */}
      <div className="px-4 pb-3">
        <button
          type="button"
          onClick={() => window.dispatchEvent(new CustomEvent('open-cmdk'))}
          className="flex h-10 w-full items-center gap-2 rounded-xl border border-[var(--border)] bg-[var(--surface)] px-3 text-left hover:border-[var(--primary)]"
          aria-label={t(c.locale, 'sidebar.searchPlaceholder')}
        >
          <Search size={14} className="text-[var(--text-disabled)] shrink-0" />
          <span className="min-w-0 flex-1 truncate text-sm text-[var(--text-disabled)]">
            {t(c.locale, 'sidebar.searchPlaceholder')}
          </span>
          <span className="shrink-0 rounded-md bg-[var(--surface)] px-1.5 py-0.5 text-[0.6875rem] font-medium text-[var(--text-disabled)]">
            ⌘K
          </span>
        </button>
      </div>

      {/* Quick Access / File Manager / Apps / Favorites / Modules / Builtin Tools */}
      <QuickAccessSection c={c} />
      <FileManagerSection c={c} />
      <AppsSection c={c} />
      <FavoritesSection c={c} />
      <ModulesSection c={c} />
      <BuiltinToolsSection c={c} />
    </div>
  );
}

function BottomNav({ c }: { c: SidebarController }) {
  const { locale } = c;
  return (
    <div className="shrink-0 px-3 py-2 border-t border-[var(--border-subtle)]">
      <button
        type="button"
        onClick={c.onNotificationClick}
        className="flex w-full items-center gap-2.5 rounded-lg px-3 py-1.5 text-sm text-[var(--text-secondary)] transition-[color,background-color,border-color,opacity,transform] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)]"
      >
        <Bell size={16} />
        <span>{t(locale, 'notifications.title')}</span>
      </button>
      <button
        type="button"
        onClick={() => c.selectNavigation('__settings__', 'settings:personal')}
        aria-current={c.activeNavigationId === '__settings__' ? 'page' : undefined}
        className={`flex w-full items-center gap-2.5 rounded-lg px-3 py-1.5 text-sm transition-[color,background-color,border-color,opacity,transform] ${
          c.activeNavigationId === '__settings__'
            ? 'sidebar-nav-active'
            : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)]'
        }`}
      >
        <Settings size={16} />
        <span>{t(locale, 'nav.settings')}</span>
      </button>
      <button
        type="button"
        onClick={() => c.selectNavigation('__workshop__', 'modules')}
        aria-current={c.activeNavigationId === '__workshop__' ? 'page' : undefined}
        className={`flex w-full items-center gap-2.5 rounded-lg px-3 py-1.5 text-sm transition-[color,background-color,border-color,opacity,transform] ${
          c.activeNavigationId === '__workshop__'
            ? 'sidebar-nav-active'
            : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)]'
        }`}
      >
        <Layers size={16} />
        <span>{t(locale, 'nav.modules')}</span>
      </button>
    </div>
  );
}

/** 折叠态 64px Icon Rail：一级导航图标 + 左下角头像，均保持可交互（决策 9）。 */
function IconRail({ c }: { c: SidebarController }) {
  const { locale } = c;
  const railItems = [
    {
      id: '__dashboard__',
      label: t(locale, 'nav.dashboard'),
      icon: <LayoutDashboard size={16} />,
      target: '__dashboard__',
    },
    {
      id: 'files',
      label: t(locale, 'nav.fileBrowser'),
      icon: <FolderOpen size={16} />,
      target: 'files',
    },
    {
      id: 'apps',
      label: t(locale, 'nav.apps'),
      icon: <Layers size={16} />,
      target: 'apps',
    },
    {
      id: 'ai',
      label: t(locale, 'nav.aiWorkbench'),
      icon: <Sparkles size={16} />,
      target: 'ai',
    },
    {
      id: 'usage',
      label: t(locale, 'nav.usage'),
      icon: <BarChart3 size={16} />,
      target: 'usage',
    },
  ];
  return (
    <>
      <nav className="flex-1 flex flex-col items-center gap-1 overflow-y-auto py-2 px-1" aria-label="Primary navigation">
        {railItems.map((item) => {
          const isActive =
            c.activeNavigationId === item.id ||
            (item.id === 'files' && c.activeNavigationId?.startsWith('__files__:')) ||
            (item.id === '__dashboard__' && c.activeNavigationId === null && !c.isSettingsMode);
          return (
            <button
              key={item.id}
              type="button"
              aria-current={isActive ? 'page' : undefined}
              aria-label={item.label}
              title={item.label}
              onClick={() => c.selectNavigation(item.id, item.target)}
              className={`flex h-9 w-9 items-center justify-center rounded-lg transition-[color,background-color,border-color,opacity,transform] ${
                isActive
                  ? 'sidebar-nav-active'
                  : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)]'
              }`}
            >
              {item.icon}
            </button>
          );
        })}
      </nav>
      {/* 左下角头像/缩写：折叠态只显示缩写（决策 9） */}
      <div className="shrink-0 flex justify-center px-1 pb-2">
        <span
          className="flex h-8 w-8 items-center justify-center rounded-full bg-[var(--primary)] text-[0.8125rem] font-semibold text-[var(--primary-foreground)]"
          title={c.username}
          aria-label={c.username}
        >
          {c.username}
        </span>
      </div>
    </>
  );
}

export function SidebarChrome({ c }: { c: SidebarController }) {
  return (
    <>
      {/* ── 标题栏：macOS 用系统 traffic lights；其它平台自绘按钮 ──
          注意：不要把折叠按钮放在 data-tauri-drag-region 内部，
          否则在 macOS/WKWebView 上点击会被当成拖窗口，无法折叠。 */}
      <Titlebar c={c} />

      {c.isSettingsMode ? (
        /* ── Settings Sidebar Layout ── */
        <SettingsNav c={c} />
      ) : c.isCollapsed ? (
        /* ── Collapsed 64px Icon Rail：图标导航 + 左下头像，全部可交互 ── */
        <IconRail c={c} />
      ) : (
        /* ── Normal Sidebar Layout ── */
        <>
          <SidebarBody c={c} />
          <BottomNav c={c} />
        </>
      )}
    </>
  );
}
