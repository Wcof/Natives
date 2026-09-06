
try {
  const navType = performance.getEntriesByType('navigation')[0]?.type;
  const state = JSON.parse(sessionStorage.getItem('natives-space-ui') || 'null');
  if ((navType === 'reload' || navType === 'back_forward') && state?.sidebarCollapsed === false) {
    document.body.classList.remove('sidebar-collapsed');
  }
} catch {}
