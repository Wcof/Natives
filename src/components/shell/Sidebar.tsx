'use client';

import { t } from '@/i18n';
import { useSidebar, type SidebarProps } from './sidebar/useSidebar';
import { SidebarChrome } from './sidebar/parts';
import { SIDEBAR_MAX_WIDTH, SIDEBAR_MIN_WIDTH } from './sidebar/model';

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
    <div className={`shell-sidebar-frame h-full ${c.isCollapsed ? 'w-0 overflow-visible' : 'relative'}`}>
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
        borderRight: 'none',
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
