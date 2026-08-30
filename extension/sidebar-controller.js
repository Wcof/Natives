/** Shared product-sidebar resize/collapse behavior for every extension surface. */
export function createSidebarController({
  resizer,
  toggleButton,
  initialWidth = 248,
  initialCollapsed = false,
  onWidthChange,
  onCollapsedChange,
  t = (key, fallback) => fallback || key,
}) {
  let width = 248;
  let collapsed = false;
  let resizing = false;

  function setWidth(nextWidth, notify = true) {
    width = Math.min(420, Math.max(190, Number(nextWidth) || 248));
    document.documentElement.style.setProperty('--sidebar-width', `${width}px`);
    resizer?.setAttribute('aria-valuenow', String(width));
    if (notify) onWidthChange?.(width);
  }

  function setCollapsed(nextCollapsed, notify = true) {
    collapsed = Boolean(nextCollapsed);
    document.body.classList.toggle('sidebar-collapsed', collapsed);
    if (toggleButton) {
      toggleButton.setAttribute('aria-pressed', String(collapsed));
      const title = t(collapsed ? 'expandSidebar' : 'collapseSidebar', collapsed ? '展开侧栏' : '折叠侧栏');
      toggleButton.title = title;
      toggleButton.setAttribute('aria-label', title);
    }
    if (notify) onCollapsedChange?.(collapsed);
  }

  toggleButton?.addEventListener('click', () => setCollapsed(!collapsed));
  resizer?.addEventListener('mousedown', (event) => {
    resizing = true;
    document.body.style.cursor = 'col-resize';
    document.body.style.userSelect = 'none';
    event.preventDefault();
  });
  resizer?.addEventListener('keydown', (event) => {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
    setWidth(event.key === 'Home' ? 190 : event.key === 'End' ? 420 : width + (event.key === 'ArrowRight' ? 20 : -20));
    event.preventDefault();
  });
  document.addEventListener('mousemove', (event) => {
    if (resizing) setWidth(event.clientX, false);
  });
  document.addEventListener('mouseup', () => {
    if (!resizing) return;
    resizing = false;
    document.body.style.cursor = '';
    document.body.style.userSelect = '';
    onWidthChange?.(width);
  });

  setWidth(initialWidth, false);
  setCollapsed(initialCollapsed, false);
  return {
    setWidth,
    setCollapsed,
    toggle: () => setCollapsed(!collapsed),
    refreshLabel: () => setCollapsed(collapsed, false),
    get width() { return width; },
    get collapsed() { return collapsed; },
  };
}
