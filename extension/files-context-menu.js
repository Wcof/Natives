

export function createFilesContextMenu({
  $,
  t,
  session,
  isTextFile,
  openItem,
  renderSelection,
  guardDirty,
  editorState,
  showDiskUsage,
  extractArchive,
  revealSelected,
  copyPathSelected,
  renameSelected,
  transfer,
  duplicateSelected,
  trashSelected,
  createEntry,
  ops,
  copyFileSelected,
  createZip,
  importFileList,
  searchController,
  loadDirectory,
  call,
  toast,
  setStatus,
}) {
  function openModal({ title, message = '', label = '', value = '', submit = () => {} }) {
    const dialog = $('modal');
    if (!dialog) return;
    $('modal-title').textContent = title;
    $('modal-message').textContent = message;
    $('modal-field-label').textContent = label;
    $('modal-input').value = value;
    $('modal-input').hidden = !label;
    $('modal-field-label').hidden = !label;
    dialog.returnValue = 'cancel';
    const form = $('modal-form');
    form.onsubmit = (event) => {
      event.preventDefault();
      const next = $('modal-input').value.trim();
      if (label && !next) return;
      dialog.close('default');
      submit(next);
    };
    $('modal-cancel').onclick = () => dialog.close('cancel');
    dialog.showModal();
    if (label) {
      $('modal-input').focus();
      $('modal-input').select();
    }
  }

  function showContextMenu(x, y, item) {
    const menu = $('context-menu');
    if (!menu) return;
    menu.replaceChildren();
    const isText = item && !item.isDir && (item.kind === 'text' || isTextFile(item));
    const actions = item ? [
      ['open', 'open', () => openItem(item)],
      ['previewAction', 'preview', () => {
        const show = () => {
          session.selectedPaths = new Set([item.path]);
          session.lastSelectedIndex = session.entries.findIndex((entry) => entry.path === item.path);
          renderSelection();
        };
        if (editorState()?.dirty) guardDirty(show);
        else show();
      }],
      ...(isText ? [['editAction', 'edit', () => {
        const showEdit = () => {
          session.selectedPaths = new Set([item.path]);
          session.lastSelectedIndex = session.entries.findIndex((entry) => entry.path === item.path);
          renderSelection({ editMode: true });
        };
        if (editorState()?.dirty) guardDirty(showEdit);
        else showEdit();
      }]] : []),
      ...(item.isDir ? [['diskUsage', 'diskUsage', () => showDiskUsage(item.path)]] : []),
      ...(item.kind === 'archive' && /\.(zip|jar|tar|tgz|tbz2?|txz|tar\.(gz|bz2|xz|zst))$/i.test(item.name) ? [['extractArchive', 'extractArchive', () => extractArchive(item)]] : []),
      ['reveal', 'reveal', () => revealSelected()],
      ['copyPath', 'copyPath', () => copyPathSelected()],
      ['rename', 'rename', () => renameSelected()],
      ['copy', 'copy', () => transfer('copy')],
      ['duplicate', 'duplicate', () => duplicateSelected()],
      ...(item.isDir && ops.getClipboard() ? [['paste', 'paste', () => ops.pasteClipboard(item.path)]] : []),
      ['move', 'move', () => transfer('move')],
      ['trash', 'trash', () => trashSelected()],
    ] : [
      ['newFolder', 'newFolder', () => createEntry('directory')],
      ['newFile', 'newFile', () => createEntry('file')],
      ...(ops.getClipboard() ? [['paste', 'paste', () => ops.pasteClipboard()]] : []),
    ];

    for (const [id, key, action] of actions) {
      const button = document.createElement('button');
      button.dataset.action = id;
      button.setAttribute('role', 'menuitem');
      button.textContent = t(key, key);
      button.onclick = () => {
        hideContextMenu();
        action();
      };
      menu.append(button);
    }

    if (!item && session.currentPath) {
      const refresh = document.createElement('button');
      refresh.dataset.action = 'refresh';
      refresh.setAttribute('role', 'menuitem');
      refresh.textContent = t('refresh', '刷新');
      refresh.onclick = () => {
        hideContextMenu();
        const sq = searchController.getSearchQuery();
        sq ? searchController.search(sq) : loadDirectory(session.currentPath);
      };
      const importButton = document.createElement('button');
      importButton.dataset.action = 'importFiles';
      importButton.setAttribute('role', 'menuitem');
      importButton.textContent = t('importFiles', '导入文件');
      importButton.onclick = () => {
        hideContextMenu();
        const input = document.createElement('input');
        input.type = 'file';
        input.multiple = true;
        input.onchange = () => importFileList(input.files);
        input.click();
      };
      menu.append(refresh, importButton);
    }

    if (item && session.selectedPaths.size && session.currentPath) {
      const archiveButton = document.createElement('button');
      archiveButton.dataset.action = 'createArchive';
      archiveButton.setAttribute('role', 'menuitem');
      archiveButton.textContent = t('createArchive', '创建 ZIP');
      archiveButton.onclick = () => {
        hideContextMenu();
        createZip();
      };
      menu.append(archiveButton);
    }

    if (item && !item.isDir && item.kind !== 'image') {
      const fileButton = document.createElement('button');
      fileButton.dataset.action = 'copyFile';
      fileButton.setAttribute('role', 'menuitem');
      fileButton.textContent = t('copyFile', '复制文件');
      fileButton.onclick = () => {
        hideContextMenu();
        copyFileSelected();
      };
      menu.append(fileButton);
    }

    if (item && item.kind === 'image') {
      const imageButton = document.createElement('button');
      imageButton.dataset.action = 'copyImage';
      imageButton.setAttribute('role', 'menuitem');
      imageButton.textContent = t('copyImage', '复制图片');
      imageButton.setAttribute('aria-label', imageButton.textContent);
      imageButton.onclick = async () => {
        hideContextMenu();
        try {
          await call('copy_image', { path: item.path });
          toast(t('imageCopied', '图片已复制'));
        } catch (error) {
          setStatus(error.message, 'error');
        }
      };
      menu.append(imageButton);
      if (!item.isDir) {
        const editorButton = document.createElement('button');
        editorButton.dataset.action = 'editor';
        editorButton.setAttribute('role', 'menuitem');
        editorButton.textContent = t('openEditor', '在编辑器打开');
        editorButton.setAttribute('aria-label', editorButton.textContent);
        editorButton.onclick = async () => {
          hideContextMenu();
          try {
            await call('editor', { path: item.path });
            toast(t('openedEditor', '已在编辑器打开'));
          } catch (error) {
            setStatus(error.message, 'error');
          }
        };
        const copyFileButton = document.createElement('button');
        copyFileButton.dataset.action = 'copyFile';
        copyFileButton.setAttribute('role', 'menuitem');
        copyFileButton.textContent = t('copyFile', '复制文件');
        copyFileButton.setAttribute('aria-label', copyFileButton.textContent);
        copyFileButton.onclick = async () => {
          hideContextMenu();
          try {
            await call('copy_paths', { paths: [item.path] });
            toast(t('fileCopied', '文件已复制'));
          } catch (error) {
            setStatus(error.message, 'error');
          }
        };
        menu.append(editorButton, copyFileButton);
      }
    }

    menu.hidden = false;
    menu.style.left = `${Math.min(x, innerWidth - 190)}px`;
    menu.style.top = `${Math.min(y, innerHeight - menu.offsetHeight - 10)}px`;
  }

  function hideContextMenu() {
    const menu = $('context-menu');
    if (menu) menu.hidden = true;
  }

  return { openModal, showContextMenu, hideContextMenu };
}
