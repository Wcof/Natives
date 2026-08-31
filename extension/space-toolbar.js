/**
 * Dashboard Toolbar & Empty State Controller (<120 lines).
 * Handles floating settings/widget/fullscreen controls and empty state CTA.
 */

export function createSpaceToolbar({
  $,
  t,
  onToggleSettings,
  onToggleWidgets,
  onToggleSidebar,
  onOpenCatalog,
}) {
  const toolbar = $('dashboard-toolbar');
  const settingsBtn = $('space-settings-btn');
  const toggleSidebarBtn = $('space-toggle-sidebar-btn');
  const toggleWidgetsBtn = $('space-toggle-widgets-btn');
  const fullscreenBtn = $('space-fullscreen-btn');
  const emptyState = $('space-empty-state');
  const emptyAddBtn = $('space-empty-add-btn');

  let widgetsHidden = false;

  function isEditableFocused() {
    const el = document.activeElement;
    if (!el) return false;
    const tag = el.tagName.toLowerCase();
    return tag === 'input' || tag === 'textarea' || tag === 'select' || el.isContentEditable;
  }

  function applyWidgetsHidden(hidden) {
    widgetsHidden = Boolean(hidden);
    if (typeof document !== 'undefined' && document.body?.classList) {
      document.body.classList.toggle('space-widgets-hidden', widgetsHidden);
    }
    const host = $('dashboard-host');
    if (host) {
      host.classList?.toggle?.('widgets-hidden', widgetsHidden);
      if (host.dataset) host.dataset.widgetsHidden = String(widgetsHidden);
    }
    if (toggleWidgetsBtn && typeof toggleWidgetsBtn.setAttribute === 'function') {
      toggleWidgetsBtn.setAttribute('aria-pressed', String(widgetsHidden));
    }
    onToggleWidgets?.(widgetsHidden);
  }

  // Setup buttons
  if (settingsBtn) {
    settingsBtn.onclick = () => onToggleSettings();
  }

  if (toggleSidebarBtn) {
    toggleSidebarBtn.onclick = () => onToggleSidebar?.();
  }

  if (toggleWidgetsBtn) {
    toggleWidgetsBtn.onclick = () => {
      applyWidgetsHidden(!widgetsHidden);
    };
  }

  if (fullscreenBtn) {
    if (document.fullscreenEnabled) {
      fullscreenBtn.onclick = () => {
        if (!document.fullscreenElement) {
          document.documentElement.requestFullscreen().catch(() => {});
        } else {
          document.exitFullscreen().catch(() => {});
        }
      };
    } else {
      fullscreenBtn.style.display = 'none';
    }
  }

  if (emptyAddBtn) {
    emptyAddBtn.onclick = () => onOpenCatalog();
  }

  // Global Keyboard Shortcuts: S (settings), W (toggle widgets), F (fullscreen)
  function handleKeyDown(event) {
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    if (isEditableFocused()) return;

    const key = event.key.toLowerCase();
    if (key === 's') {
      event.preventDefault();
      onToggleSettings();
    } else if (key === 'w') {
      event.preventDefault();
      applyWidgetsHidden(!widgetsHidden);
    } else if (key === 'f' && document.fullscreenEnabled) {
      event.preventDefault();
      if (!document.fullscreenElement) {
        document.documentElement.requestFullscreen().catch(() => {});
      } else {
        document.exitFullscreen().catch(() => {});
      }
    }
  }

  if (typeof document !== 'undefined' && typeof document.addEventListener === 'function') {
    document.addEventListener('keydown', handleKeyDown);
  }

  function sync(snapshot) {
    const activeWidgets = (snapshot?.widgets || []).filter((w) => w.enabled);
    const isEmpty = !snapshot?.widgets || snapshot.widgets.length === 0;
    if (emptyState) {
      emptyState.hidden = !isEmpty;
    }
  }

  return {
    sync,
    settingsButton: settingsBtn,
    destroy() {
      document.removeEventListener('keydown', handleKeyDown);
    },
  };
}
