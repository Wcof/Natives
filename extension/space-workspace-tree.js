
export function createSpaceWorkspaceTree({ $, t, activateWorkspace, renameWorkspace, deleteWorkspace }) {
  const menu = $('workspace-menu');
  let menuWorkspace = null;
  let menuTrigger = null;

  function formatWorkspaceDisplayName(name) {
    if (!name || name === 'Personal Space' || name === 'Default' || name === 'Workspace' || name === 'Default Workspace') {
      return t('personalSpace', '个人空间');
    }
    const match = name.match(/^Workspace\s+(\d+)$/i);
    if (match) {
      return `${t('personalSpace', '个人空间')} ${match[1]}`;
    }
    return name;
  }

  function closeMenu({ restoreFocus = false } = {}) {
    if (menu.hidden) return;
    menu.hidden = true;
    menuTrigger?.setAttribute('aria-expanded', 'false');
    menuTrigger?.closest('.ws-item')?.classList.remove('menu-open');
    if (restoreFocus) menuTrigger?.focus();
    menuWorkspace = null;
    menuTrigger = null;
  }

  function openMenu(workspace, trigger, point) {
    closeMenu();
    menuWorkspace = workspace;
    menuTrigger = trigger;
    trigger?.setAttribute('aria-expanded', 'true');
    trigger?.closest('.ws-item')?.classList.add('menu-open');
    menu.hidden = false;
    const anchor = trigger?.getBoundingClientRect();
    const left = point?.x ?? anchor?.right ?? 0;
    const top = point?.y ?? anchor?.bottom ?? 0;
    const bounds = menu.getBoundingClientRect();
    menu.style.left = `${Math.max(8, Math.min(left, innerWidth - bounds.width - 8))}px`;
    menu.style.top = `${Math.max(8, Math.min(top, innerHeight - bounds.height - 8))}px`;
    menu.querySelector('[role="menuitem"]')?.focus();
  }

  menu.addEventListener('click', (event) => {
    const action = event.target.closest('[data-action]')?.dataset.action;
    const workspace = menuWorkspace;
    closeMenu();
    if (!workspace) return;
    if (action === 'rename') renameWorkspace(workspace);
    if (action === 'delete') deleteWorkspace(workspace);
  });
  document.addEventListener('pointerdown', (event) => {
    if (!menu.hidden && !menu.contains(event.target) && event.target !== menuTrigger) closeMenu();
  });
  document.addEventListener('keydown', (event) => {
    if (event.key === 'Escape' && !menu.hidden) {
      closeMenu({ restoreFocus: true });
      event.preventDefault();
    }
  });

  function render(session, activeWorkspaceId) {
    closeMenu();
    const tree = $('workspace-tree');
    tree.replaceChildren();
    for (const workspace of session?.workspaces || []) {
      const item = document.createElement('div');
      item.className = `ws-item${workspace.id === activeWorkspaceId ? ' active' : ''}`;

      const displayName = formatWorkspaceDisplayName(workspace.name);
      const select = document.createElement('button');
      select.type = 'button';
      select.className = 'ws-select';
      select.textContent = displayName;
      select.title = displayName;
      select.setAttribute('aria-current', workspace.id === activeWorkspaceId ? 'page' : 'false');
      select.onclick = () => activateWorkspace(workspace.id);
      select.oncontextmenu = (event) => {
        event.preventDefault();
        openMenu(workspace, select, { x: event.clientX, y: event.clientY });
      };

      const more = document.createElement('button');
      more.type = 'button';
      more.className = 'ws-more-btn';
      more.title = t('moreActions', '更多');
      more.setAttribute('aria-label', `${displayName} · ${more.title}`);
      more.setAttribute('aria-haspopup', 'menu');
      more.setAttribute('aria-expanded', 'false');
      more.innerHTML = '<svg class="icon"><use href="#i-more"/></svg>';
      more.onclick = (event) => {
        event.stopPropagation();
        openMenu(workspace, more);
      };

      item.append(select, more);
      tree.append(item);
    }
  }

  return { render };
}
