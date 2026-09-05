/**
 * Space bootstrap: restore per-tab UI state before the module paints so a
 * refresh stays on the current view. A fresh new tab keeps the initial
 * collapsed layout (the per-tab state is empty on its first load).
 */
try {
  const navType = performance.getEntriesByType('navigation')[0]?.type;
  const state = JSON.parse(sessionStorage.getItem('natives-space-ui') || 'null');
  if ((navType === 'reload' || navType === 'back_forward') && state?.sidebarCollapsed === false) {
    document.body.classList.remove('sidebar-collapsed');
  }
} catch {}
