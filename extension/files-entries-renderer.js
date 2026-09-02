/**
 * Files list and grid renderer.
 */

export function createFilesEntriesRenderer({
  $,
  t,
  session,
  entryIcon,
  formatSize,
  parentAndName,
  fileUri,
  htmlEscape,
  selectEntry,
  openItemFromDoubleClick,
  showContextMenu,
  guardDirty,
  editorState,
  renderSelection,
  changedPaths,
  searchController,
  navigate,
  importFileList,
  moveDroppedPaths,
  copyDroppedUris,
  setStatus,
  setPendingSelectionPath,
}) {
  function render(renderHomeWelcome) {
    if (!session.currentPath) {
      renderHomeWelcome();
      return;
    }
    const box = $('entries');
    if (!box) return;
    box.replaceChildren();
    box.classList.toggle('grid', session.viewMode === 'grid');
    const emptyEl = $('empty');
    if (emptyEl) {
      emptyEl.hidden = session.entries.length > 0;
      if (!session.entries.length) {
        emptyEl.textContent = searchController.getSearchQuery() ? t('noSearchResults', '没有匹配的文件') : t('emptyFolder', '此文件夹为空');
      }
    }
    if (session.viewMode === 'list' && session.entries.length) {
      const head = document.createElement('div');
      head.className = 'list-head';
      head.setAttribute('aria-hidden', 'true');
      for (const label of ['', t('name', '名称'), t('modified', '修改时间'), t('size', '大小')]) {
        head.append(Object.assign(document.createElement('span'), { textContent: label }));
      }
      box.append(head);
    }
    for (const [index, item] of session.entries.entries()) {
      const row = document.createElement('div');
      row.className = 'entry';
      row.setAttribute('role', 'option');
      row.dataset.path = item.path;
      row.dataset.index = String(index);
      row.tabIndex = 0;
      row.setAttribute('aria-selected', session.selectedPaths.has(item.path));
      if (session.selectedPaths.has(item.path)) row.classList.add('selected');
      const change = changedPaths.get(item.path);
      if (change) {
        row.classList.add('changed');
        row.style.setProperty('--change-heat', String(Math.min(1, 0.35 + change.count * 0.08)));
      }
      const icon = document.createElement('span');
      icon.className = `entry-icon${item.isDir ? '' : ` entry-icon-${item.kind || 'text'}`}`;
      icon.append(entryIcon(item));
      icon.setAttribute('aria-hidden', 'true');
      const name = document.createElement('button');
      name.className = 'entry-name';
      name.textContent = item.name;
      name.title = item.name;
      name.draggable = true;
      if (change) {
        name.dataset.changed = change.count > 1 ? `改·${change.count}` : '改';
        name.title = `${item.name} · ${name.dataset.changed}`;
      }
      const badge = document.createElement('span');
      badge.className = `project-badge${item.projectBadge ? ` proj-${item.projectBadge}` : ''}`;
      badge.textContent = item.projectBadge ? String(item.projectBadge).toUpperCase() : '';
      badge.hidden = !item.projectBadge;
      if (item.projectBadge) name.append(badge);

      const modified = document.createElement('span');
      modified.className = 'entry-meta modified';
      modified.textContent = Number(item.mtime) ? new Date(item.mtime).toLocaleString() : '';

      const size = document.createElement('span');
      size.className = 'entry-meta size';
      size.textContent = item.isDir ? t('folder', '文件夹') : formatSize(item.size);

      const isGlobal = searchController.isGlobalMode();
      const source = document.createElement(isGlobal ? 'button' : 'span');
      source.className = 'entry-source entry-meta';
      source.textContent = isGlobal ? (item.dirHint || parentAndName(item.path).parent) : '';
      source.title = item.path;
      if (isGlobal) {
        source.type = 'button';
        source.setAttribute('aria-label', `${t('path', '路径')}: ${item.path}`);
        source.onclick = (event) => {
          event.stopPropagation();
          setPendingSelectionPath(item.path);
          navigate(parentAndName(item.path).parent);
        };
      } else {
        source.hidden = true;
      }

      row.append(icon, name, modified, size, source);
      row.onclick = (event) => selectEntry(index, event);
      row.ondblclick = () => openItemFromDoubleClick(item);
      row.oncontextmenu = (event) => {
        event.preventDefault();
        const show = () => {
          if (!session.selectedPaths.has(item.path)) {
            session.selectedPaths.clear();
            session.selectedPaths.add(item.path);
            session.lastSelectedIndex = index;
            renderSelection();
          }
          showContextMenu(event.clientX, event.clientY, item);
        };
        if (editorState()?.dirty) guardDirty(show);
        else show();
      };
      row.ondragstart = (event) => {
        const paths = [...session.selectedPaths.size ? session.selectedPaths : [item.path]];
        const uris = paths.map(fileUri);
        event.dataTransfer.setData('text/plain', JSON.stringify(paths));
        event.dataTransfer.setData('text/uri-list', uris.join('\r\n'));
        event.dataTransfer.setData('text/html', uris.map((uri) => `<a href="${htmlEscape(uri)}">${htmlEscape(uri)}</a>`).join('\n'));
        event.dataTransfer.effectAllowed = 'copyMove';
      };
      if (item.isDir) {
        row.ondragover = (event) => {
          event.preventDefault();
          row.classList.add('drop-target');
          event.dataTransfer.dropEffect = event.dataTransfer.types.includes('Files') ? 'copy' : 'move';
        };
        row.ondragleave = () => row.classList.remove('drop-target');
        row.ondrop = async (event) => {
          event.preventDefault();
          row.classList.remove('drop-target');
          const raw = event.dataTransfer.getData('text/plain');
          const urls = event.dataTransfer.getData('text/uri-list');
          try {
            if (event.dataTransfer.files.length) await importFileList(event.dataTransfer.files, item.path);
            else if (raw && raw.startsWith('[')) await moveDroppedPaths(raw, item.path);
            else if (urls) await copyDroppedUris(urls, item.path);
          } catch (error) {
            setStatus(error.message, 'error');
          }
        };
      }
      box.append(row);
    }
  }

  return { render };
}
