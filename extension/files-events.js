/**
 * Event binding and hotkeys for files.html (<200 lines).
 */

export function bindFilesEvents({
  $,
  session,
  t,
  storageSet,
  toggleSidebar,
  setSidebarWidth,
  beginPreviewResize,
  filesSidebar,
  filesPreviewPanel,
  showDiskUsage,
  navigate,
  loadDirectory,
  search,
  openItem,
  openEditorSelected,
  revealSelected,
  renameSelected,
  transfer,
  duplicateSelected,
  pasteClipboard,
  setClipboard,
  clearFileClipboard,
  trashSelected,
  createZip,
  extractArchive,
  selectedItems,
  renderEntries,
  renderSelection,
  ops,
  preview,
}) {
  let resizingSidebar = false;

  // Hotkeys & Navigation
  window.addEventListener('keydown', (event) => {
    if (['INPUT', 'TEXTAREA'].includes(document.activeElement?.tagName)) return;
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
      event.preventDefault();
      $('search')?.focus();
    }
  });

  // Resizers
  const sidebarResizer = $('sidebar-resizer');
  if (sidebarResizer) {
    sidebarResizer.addEventListener('mousedown', (event) => {
      resizingSidebar = true;
      document.body.style.cursor = 'col-resize';
      document.body.style.userSelect = 'none';
      event.preventDefault();
    });
    sidebarResizer.addEventListener('keydown', (event) => {
      if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
      setSidebarWidth(event.key === 'Home' ? 190 : event.key === 'End' ? 420 : session.sidebarWidth + (event.key === 'ArrowRight' ? 20 : -20));
    });
  }

  document.addEventListener('mousemove', (event) => {
    if (resizingSidebar) {
      setSidebarWidth(event.clientX);
    }
  });

  document.addEventListener('mouseup', () => {
    if (resizingSidebar) {
      resizingSidebar = false;
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
      storageSet('natives-sidebar-width', session.sidebarWidth).catch(() => {});
    }
  });
}
