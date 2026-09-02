/**
 * Files keyboard shortcuts and focus controller.
 * Centralizes all keydown handlers for navigation, clipboard, deletion, and search triggers.
 */

export function bindFilesShortcuts({
  $,
  session,
  ops,
  searchController,
  hideContextMenu,
  toggleSidebar,
  syncGridControls,
  storageSet,
  renderEntries,
  renderSelection,
  openItemFromDoubleClick,
  selectEntry,
  renameSelected,
  trashSelected,
  openQuickFilter,
  closeQuickFilter,
  applyQuickFilter,
  openSearchDialog,
  setStatus,
  t,
  call,
  openLanguageSettings,
  openEditorSelected,
}) {
  function handleKeydown(event) {
    if (event.key === 'Escape') {
      if (ops.isBusy()) {
        ops.cancelActive();
        const uploadId = ops.activeUploadId();
        if (uploadId) call('import_cancel', { uploadId }).catch(() => {});
      }
      searchController.cancelActiveSearches();
      hideContextMenu();
      if ($('modal')?.open) $('modal').close('cancel');
      if ($('dirty-modal')?.open) $('dirty-modal').close('cancel');
      if ($('conflict-modal')?.open) $('conflict-modal').close('cancel');
      return;
    }

    const modifier = event.metaKey || event.ctrlKey;
    const shortcut = event.key.toLowerCase();

    if (event.altKey && !event.metaKey && !event.ctrlKey) {
      if (event.key.toLowerCase() !== 'l') return;
      event.preventDefault();
      openLanguageSettings?.();
      return;
    }

    if (modifier && ['[', ']'].includes(event.key)) {
      event.preventDefault();
      if (event.key === '[') $('back')?.click();
      else $('forward')?.click();
      return;
    }
    if (!['[', ']'].includes(event.key) && modifier && shortcut === 'b') {
      event.preventDefault();
      toggleSidebar();
      return;
    }

    if (modifier && event.shiftKey && shortcut === 'g') { event.preventDefault(); let viewMode = session.viewMode; viewMode = viewMode === 'grid' ? 'list' : 'grid'; session.viewMode = viewMode; $('list-view')?.setAttribute('aria-pressed', String(session.viewMode === 'list')); $('grid-view')?.setAttribute('aria-pressed', String(session.viewMode === 'grid')); renderEntries(); syncGridControls(); storageSet('natives-view-mode', session.viewMode).catch(() => {}); return; }

    const hasTextSelection = Boolean(window.getSelection() && !window.getSelection().isCollapsed && window.getSelection().toString().length > 0);
    if (event.target.matches('input,textarea,select,[contenteditable="true"]') || (modifier && shortcut === 'c' && hasTextSelection) || (hasTextSelection && ['c', 'x', 'a'].includes(shortcut))) return;

    if (modifier && shortcut === 'a') {
      event.preventDefault();
      session.selectedPaths = new Set(session.entries.map((item) => item.path));
      renderSelection();
      return;
    }

    if (modifier && shortcut === 'c') {
      event.preventDefault();
      ops.setClipboard('copy');
      return;
    }

    if (modifier && shortcut === 'x') {
      event.preventDefault();
      ops.setClipboard('move');
      return;
    }

    if (modifier && shortcut === 'v') {
      event.preventDefault();
      ops.pasteClipboard();
      return;
    }

    if (modifier && shortcut === 'd') {
      event.preventDefault();
      ops.duplicateSelected();
      return;
    }

    const row = event.target.closest('.entry');
    if (!row) return;
    const index = Number(row.dataset.index);

    if (event.key === 'ContextMenu' || (event.key === 'F10' && event.shiftKey)) {
      event.preventDefault();
      row.dispatchEvent(new MouseEvent('contextmenu', {
        bubbles: true,
        clientX: row.getBoundingClientRect().left + 12,
        clientY: row.getBoundingClientRect().bottom,
      }));
    } else if (event.key === 'Enter') {
      if (event.key !== 'Enter' || event.shiftKey || modifier) {
        if (event.key !== 'Enter' || session.selectedPaths.size !== 1) return;
        event.preventDefault();
        openEditorSelected?.();
        return;
      }
      event.preventDefault();
      openItemFromDoubleClick(session.entries[index]);
    } else if (event.key === ' ') {
      event.preventDefault();
      selectEntry(index, event);
    } else if (event.key === 'F2') {
      event.preventDefault();
      renameSelected();
    } else if (event.key === 'Delete' || event.key === 'Backspace') {
      event.preventDefault();
      trashSelected();
    } else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      const next = Math.max(0, Math.min(session.entries.length - 1, index + (event.key === 'ArrowDown' ? 1 : -1)));
      selectEntry(next, event);
      document.querySelector(`[data-index="${next}"]`)?.focus();
    }
  }

  document.addEventListener('keydown', handleKeydown);

  document.addEventListener('keydown', (event) => {
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'f') {
      if (document.activeElement?.classList.contains('pdf-preview')) return;
      event.preventDefault();
      openQuickFilter();
    }
  });

  document.addEventListener('keydown', (event) => {
    if (event.key === '/' && !event.metaKey && !event.ctrlKey && !event.altKey && !event.target.matches('input,textarea,select,[contenteditable="true"]')) {
      event.preventDefault();
      openQuickFilter();
    }
  });

  document.addEventListener('keydown', (event) => {
    if (event.key !== 'Escape' || event.target !== $('quick-filter')) return;
    if ($('quick-filter')?.value) {
      $('quick-filter').value = '';
      applyQuickFilter();
      return;
    }
    closeQuickFilter();
  });

  document.addEventListener('keydown', (event) => {
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
      event.preventDefault();
      openSearchDialog();
    }
  });

  document.addEventListener('keydown', (event) => {
    if (event.key === 'Tab' && !event.shiftKey && event.target === $('search')) {
      event.preventDefault();
      searchController.toggleSearchScope();
    }
  });
}
