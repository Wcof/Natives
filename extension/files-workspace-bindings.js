export function bindFilesWorkspaceInteractions({
  $, session, ops, searchController, contextMenu, guardDirty, hasDirtyEditor, renderSelection,
  setStatus, t, selectEntry, openQuickFilter, closeQuickFilter, applyQuickFilter,
  openSearchDialog, loadDirectory,
}) {
  const entries = $('entries');
  let dragDepth = 0;

  entries?.addEventListener('click', (event) => {
    if (event.target !== entries) return;
    const clear = () => { session.selectedPaths.clear(); renderSelection(); };
    if (hasDirtyEditor()) guardDirty(clear); else clear();
  });
  entries?.addEventListener('dragenter', (event) => {
    event.preventDefault();
    dragDepth++;
    entries.classList.add('drop-target');
    setStatus(t('dropHint', '释放以导入或移动文件'));
  });
  entries?.addEventListener('dragover', (event) => {
    event.preventDefault();
    entries.classList.add('drop-target');
    event.dataTransfer.dropEffect = event.dataTransfer.types.includes('text/plain') ? 'move' : 'copy';
  });
  entries?.addEventListener('dragleave', () => {
    dragDepth = Math.max(0, dragDepth - 1);
    if (!dragDepth) entries.classList.remove('drop-target');
  });
  for (const type of ['dragend', 'drop']) entries?.addEventListener(type, () => {
    dragDepth = 0;
    entries.classList.remove('drop-target');
  });
  entries?.addEventListener('drop', async (event) => {
    event.preventDefault();
    if (!session.currentPath) return;
    const raw = event.dataTransfer.getData('text/plain');
    if (event.dataTransfer.files.length) await ops.importFileList(event.dataTransfer.files);
    else if (raw.startsWith('[')) await ops.moveDroppedPaths(raw, session.currentPath);
  });

  document.addEventListener('drop', (event) => {
    const row = event.target.closest('.entry');
    const uris = event.dataTransfer?.getData('text/uri-list');
    const item = session.entries.find((entry) => entry.path === row?.dataset.path);
    if (!row || !uris || !item?.isDir) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    ops.copyDroppedUris(uris, row.dataset.path).catch((error) => setStatus(error.message, 'error'));
  }, true);
  document.addEventListener('drop', (event) => {
    const uris = event.dataTransfer?.getData('text/uri-list');
    if (!uris || event.target.closest('.entry') || !event.target.closest('#entries') || !session.currentPath) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    ops.copyDroppedUris(uris, session.currentPath).catch((error) => setStatus(error.message, 'error'));
  }, true);

  document.addEventListener('click', (event) => {
    if (!event.target.closest('.context-menu') && !event.target.closest('#new-menu')) contextMenu.hideContextMenu();
    if (!event.target.closest('#new-popover') && !event.target.closest('#new-menu')) $('new-popover').hidden = true;
  });
  document.addEventListener('contextmenu', (event) => {
    if (event.target.closest('.entry')) return;
    event.preventDefault();
    if (session.currentPath) contextMenu.showContextMenu(event.clientX, event.clientY);
  });

  document.addEventListener('keydown', (event) => {
    if (event.target.matches('.preview-image') && ['+', '=', '-', '_', '0'].includes(event.key)) {
      event.preventDefault();
      const image = event.target;
      const current = Number(image.dataset.zoom || 1);
      const next = event.key === '0' ? 1 : Math.min(4, Math.max(0.5, current + (event.key === '-' || event.key === '_' ? -0.1 : 0.1)));
      image.dataset.zoom = String(next);
      image.style.transform = `scale(${next})`;
      image.style.cursor = next === 1 ? 'zoom-in' : 'zoom-out';
      return;
    }
    if (!['Home', 'End', 'PageDown', 'PageUp'].includes(event.key) || event.target.matches('input,textarea,select')) return;
    const row = event.target.closest('.entry');
    if (!row) return;
    event.preventDefault();
    const index = Number(row.dataset.index);
    const pageStep = Math.max(1, Math.floor((entries.clientHeight || 400) / 36));
    const delta = event.key === 'End' ? session.entries.length : event.key === 'Home' ? -session.entries.length : event.key === 'PageDown' ? pageStep : -pageStep;
    const next = Math.max(0, Math.min(session.entries.length - 1, index + delta));
    selectEntry(next, event);
    document.querySelector(`[data-index="${next}"]`)?.focus();
  });
  document.addEventListener('keydown', (event) => {
    if (!event.target.matches('.preview-image') || !['Enter', ' '].includes(event.key)) return;
    event.preventDefault();
    event.target.click();
  });

  $('scope-toggle')?.addEventListener('click', () => searchController.toggleSearchScope());
  $('command-search-trigger')?.addEventListener('click', openSearchDialog);
  $('quick-filter-toggle')?.addEventListener('click', openQuickFilter);
  $('quick-filter')?.addEventListener('input', applyQuickFilter);
  $('search')?.addEventListener('keydown', (event) => {
    if (event.key !== 'Enter') return;
    event.preventDefault();
    $('search-dialog')?.close();
  });
  searchController.updateSearchScopeButton();

  document.addEventListener('keydown', (event) => {
    const query = searchController.getSearchQuery();
    if (event.key !== 'Escape' || !query) return;
    event.preventDefault();
    searchController.cancelActiveSearches();
    if ($('search')) $('search').value = '';
    searchController.setSearchQuery('');
    searchController.setGlobalMode(false);
    session.pageOffset = 0;
    if (session.currentPath) loadDirectory(session.currentPath);
  });

  new MutationObserver(() => {
    const query = searchController.getSearchQuery();
    if (!query || entries.childElementCount === 0) return;
    const count = entries.childElementCount;
    $('status').textContent = `${count}${session.pageHasMore ? '+' : ''} ${t('searchResults', '个搜索结果')}${session.pageHasMore ? ` · ${t('moreAvailable', '可继续加载')}` : ''}`;
  }).observe(entries, { childList: true });
}
