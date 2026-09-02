export function bindFilesToolbar({
  $, session, nativeClient, disconnectNative, init, searchController, loadDirectory,
  navigate, navigateFromInput, normalizePathInput, parentAndName, setPendingSelectionPath,
  pageSize, syncSortTabs, updateSortDirection, storageSet, ops, preview, selectedItems,
  renderEntries, applySidebarCollapsed, showDiskUsage, call, toast, setStatus, t,
  onFollowChange,
}) {
  const refreshCurrentView = () => {
    const query = searchController.getSearchQuery();
    return query ? searchController.search(query) : loadDirectory(session.currentPath);
  };

  function toggleSidebar() {
    session.sidebarCollapsed = !session.sidebarCollapsed;
    applySidebarCollapsed();
    storageSet('natives-sidebar-collapsed', session.sidebarCollapsed).catch(() => {});
  }

  function syncGridControls() {
    document.querySelectorAll('.grid-size-button').forEach((button) => {
      button.hidden = session.viewMode !== 'grid';
    });
  }

  function setGridSize(size) {
    session.gridSize = ['small', 'medium', 'large'].includes(size) ? size : 'medium';
    session.viewMode = 'grid';
    $('list-view')?.setAttribute('aria-pressed', 'false');
    $('grid-view')?.setAttribute('aria-pressed', 'true');
    document.documentElement.dataset.gridSize = session.gridSize;
    for (const value of ['small', 'medium', 'large']) $('grid-' + value)?.setAttribute('aria-pressed', String(value === session.gridSize));
    storageSet('natives-grid-size', session.gridSize).catch(() => {});
    storageSet('natives-view-mode', 'grid').catch(() => {});
    renderEntries();
    $('entries')?.classList.toggle('grid-small', session.gridSize === 'small');
    $('entries')?.classList.toggle('grid-large', session.gridSize === 'large');
    syncGridControls();
  }

  $('back').onclick = () => {
    if (session.historyIndex <= 0) return;
    session.historyIndex--;
    navigate(session.history[session.historyIndex], false);
  };
  $('forward').onclick = () => {
    if (session.historyIndex >= session.history.length - 1) return;
    session.historyIndex++;
    navigate(session.history[session.historyIndex], false);
  };
  $('up').onclick = () => session.currentPath && navigate(session.currentPath.split('/').slice(0, -1).join('/') || '/');
  $('refresh').onclick = () => {
    if (!nativeClient.connected) {
      disconnectNative();
      init();
    } else if (session.currentPath) refreshCurrentView();
    else init();
  };
  if ($('retry')) $('retry').onclick = () => { disconnectNative(); init(); };
  $('previous-page').onclick = () => {
    if (session.pageOffset < pageSize) return;
    session.pageOffset -= pageSize;
    refreshCurrentView();
  };
  $('next-page').onclick = () => {
    if (!session.pageHasMore) return;
    session.pageOffset += pageSize;
    refreshCurrentView();
  };
  if ($('path-form')) $('path-form').onsubmit = (event) => {
    event.preventDefault();
    navigateFromInput($('path-input')?.value.trim());
  };
  $('search').oninput = (event) => searchController.debounceSearch(event.target.value);

  document.querySelectorAll('.sort-tab').forEach((tab) => {
    tab.onclick = () => {
      const sortBy = tab.dataset.sort;
      if (session.sortBy === sortBy) {
        session.sortDirection = session.sortDirection === 'asc' ? 'desc' : 'asc';
        updateSortDirection();
        storageSet('natives-sort-direction', session.sortDirection).catch(() => {});
      } else {
        session.sortBy = sortBy;
        syncSortTabs();
        storageSet('natives-sort-by', session.sortBy).catch(() => {});
      }
      session.pageOffset = 0;
      refreshCurrentView();
    };
  });

  $('new-menu').onclick = () => {
    const popover = $('new-popover');
    popover.hidden = !popover.hidden;
    if (popover.hidden) return;
    const anchor = $('new-menu').getBoundingClientRect();
    const width = popover.offsetWidth || 180;
    const height = popover.offsetHeight || 180;
    popover.style.left = `${Math.max(8, Math.min(anchor.right - width, window.innerWidth - width - 8))}px`;
    popover.style.top = `${anchor.bottom + height + 6 <= window.innerHeight ? anchor.bottom + 6 : Math.max(8, anchor.top - height - 6)}px`;
  };
  $('new-folder').onclick = () => { $('new-popover').hidden = true; ops.createEntry('directory'); };
  $('new-file').onclick = () => { $('new-popover').hidden = true; ops.createEntry('file'); };
  $('import-files').onclick = () => {
    $('new-popover').hidden = true;
    const input = document.createElement('input');
    input.type = 'file';
    input.multiple = true;
    input.onchange = () => ops.importFileList(input.files);
    input.click();
  };
  $('import-folder').onclick = () => {
    $('new-popover').hidden = true;
    const input = document.createElement('input');
    input.type = 'file';
    input.multiple = true;
    input.webkitdirectory = true;
    input.onchange = () => ops.importFileList(input.files);
    input.click();
  };

  if ($('open')) $('open').onclick = () => selectedItems().length === 1 && ops.openItem(selectedItems()[0]);
  if ($('editor')) $('editor').onclick = ops.openEditorSelected;
  if ($('reveal')) $('reveal').onclick = ops.revealSelected;
  if ($('rename')) $('rename').onclick = ops.renameSelected;
  if ($('copy')) $('copy').onclick = () => ops.transfer('copy');
  if ($('move')) $('move').onclick = () => ops.transfer('move');
  if ($('trash')) $('trash').onclick = ops.trashSelected;
  $('list-view').onclick = () => {
    session.viewMode = 'list';
    renderEntries();
    $('list-view').setAttribute('aria-pressed', 'true');
    $('grid-view').setAttribute('aria-pressed', 'false');
  };
  $('grid-view').onclick = () => {
    session.viewMode = 'grid';
    renderEntries();
    $('list-view').setAttribute('aria-pressed', 'false');
    $('grid-view').setAttribute('aria-pressed', 'true');
  };
  $('close-preview').onclick = preview.resetPreview;
  $('sort-direction').onclick = () => {
    session.sortDirection = session.sortDirection === 'asc' ? 'desc' : 'asc';
    updateSortDirection();
    session.pageOffset = 0;
    refreshCurrentView();
  };
  if ($('create-archive')) $('create-archive').onclick = ops.createZip;
  $('clear-file-clipboard').onclick = ops.clearFileClipboard;
  $('cancel-operation').onclick = () => {
    if (!ops.isBusy()) return;
    ops.cancelActive();
    const uploadId = ops.activeUploadId();
    if (uploadId) call('import_cancel', { uploadId }).catch(() => {});
    const batch = ops.getBatch?.();
    if (batch?.requestId) call('batch_cancel', { requestId: batch.requestId }).catch(() => {});
  };
  $('retry-operation').onclick = ops.retryFailedOperation;
  $('show-hidden').onchange = (event) => {
    session.showHidden = event.target.checked;
    session.pageOffset = 0;
    if (session.currentPath) refreshCurrentView();
  };
  $('recursive-search').onchange = (event) => {
    searchController.setRecursive(event.target.checked);
    session.pageOffset = 0;
    if (session.currentPath) refreshCurrentView();
  };
  $('follow-changes').onchange = (event) => {
    session.followChanges = event.target.checked;
    onFollowChange?.(session.followChanges);
  };

  $('sort-direction').addEventListener('click', () => storageSet('natives-sort-direction', session.sortDirection));
  $('show-hidden').addEventListener('change', () => storageSet('natives-show-hidden', session.showHidden));
  $('follow-changes').addEventListener('change', () => storageSet('natives-follow-changes', session.followChanges));
  $('list-view').addEventListener('click', () => { storageSet('natives-view-mode', 'list'); syncGridControls(); });
  $('grid-view').addEventListener('click', () => { storageSet('natives-view-mode', 'grid'); syncGridControls(); });
  for (const size of ['small', 'medium', 'large']) $('grid-' + size).onclick = () => setGridSize(size);
  $('disk-usage').onclick = showDiskUsage;
  $('open-trash').onclick = async () => {
    try {
      await call('open_trash');
      toast(t('trashOpened', '已打开系统废纸篓'));
    } catch (error) {
      setStatus(error.message, 'error');
    }
  };

  if ($('path-form')) $('path-form').addEventListener('submit', async (event) => {
    const raw = normalizePathInput($('path-input')?.value);
    if (!raw || raw.includes('/') || raw.includes('\\')) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    try {
      const result = await call('locate', { query: raw }, crypto.randomUUID());
      const match = result?.entries?.[0];
      if (match?.path) {
        setPendingSelectionPath(match.path);
        navigate(parentAndName(match.path).parent);
      } else navigate(raw);
    } catch {
      navigate(raw);
    }
  }, true);

  return { toggleSidebar, syncGridControls, setGridSize };
}
