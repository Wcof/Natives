export function bindFilesEntryEffects({
  $, session, searchController, entryIcon, call, getDirectoryToken, t,
  selectEntry, renderSelection, renderPreviewSelection, editorState,
}) {
  const entries = $('entries');
  if (!entries) return;

  const gridThumbObserver = typeof IntersectionObserver === 'undefined' ? undefined : new IntersectionObserver((observations) => {
    for (const observation of observations) {
      if (!observation.isIntersecting) continue;
      const image = observation.target;
      gridThumbObserver.unobserve(image);
      const row = image.closest('.entry');
      const item = session.entries.find((entry) => entry.path === row?.dataset.path);
      const token = getDirectoryToken();
      if (!item || !row || image.dataset.loading) continue;
      image.dataset.loading = 'true';
      call('image_preview', { path: item.path }, crypto.randomUUID()).then((result) => {
        if (token !== getDirectoryToken() || !row.isConnected || !/^image\//.test(result?.mimeType || '') || typeof result.data !== 'string') {
          throw new Error('thumbnail unavailable');
        }
        image.src = `data:${result.mimeType};base64,${result.data}`;
        image.classList.add('loaded');
      }).catch(() => {
        image.remove();
        row.querySelector('.entry-icon')?.replaceChildren(entryIcon(item));
      });
    }
  });

  function decorateExcerpt(excerpt, item) {
    if (!item?.match) return;
    const first = String(item.matchLines?.[0] || '');
    const separator = first.indexOf(':');
    const prefix = separator >= 0 ? `${first.slice(0, separator + 1)} ` : '';
    const count = Number(item.matchCount) > 1 ? ` · ${item.matchCount} ${t('matches', '处匹配')}` : '';
    const text = `${prefix}${item.match}${count}`;
    excerpt.tabIndex = 0;
    excerpt.setAttribute('role', 'button');
    excerpt.setAttribute('aria-expanded', 'false');
    excerpt.title = Array.isArray(item.matchLines) ? item.matchLines.join(' | ') : item.match;

    const needle = searchController.getSearchQuery().replace(/^content:\s*/i, '').trim();
    if (!needle) {
      excerpt.textContent = text;
      return;
    }
    const fragment = document.createDocumentFragment();
    const lower = text.toLowerCase();
    const query = needle.toLowerCase();
    let cursor = 0;
    while (cursor < text.length) {
      const index = lower.indexOf(query, cursor);
      if (index < 0) {
        fragment.append(document.createTextNode(text.slice(cursor)));
        break;
      }
      fragment.append(document.createTextNode(text.slice(cursor, index)));
      const mark = document.createElement('mark');
      mark.textContent = text.slice(index, index + needle.length);
      fragment.append(mark);
      cursor = index + needle.length;
    }
    excerpt.replaceChildren(fragment);
  }

  function decorateEntry(row) {
    const item = session.entries.find((entry) => entry.path === row.dataset.path);
    if (!item) return;
    const icon = row.querySelector('.entry-icon');
    if (icon) {
      icon.replaceChildren(entryIcon(item));
      icon.classList.add(`entry-icon-${item.isDir ? 'dir' : item.kind || 'other'}`);
    }
    if (item.match) {
      const excerpt = document.createElement('span');
      excerpt.className = 'entry-match';
      decorateExcerpt(excerpt, item);
      row.title = excerpt.title;
      row.setAttribute('aria-label', `${item.name}: ${excerpt.title}`);
      row.append(excerpt);
    }
    if (session.viewMode === 'grid' && !item.isDir && item.kind === 'image' && gridThumbObserver && icon) {
      const image = document.createElement('img');
      image.className = 'grid-thumbnail';
      image.alt = item.name;
      image.loading = 'lazy';
      icon.replaceChildren(image);
      gridThumbObserver.observe(image);
    }
  }

  new MutationObserver((records) => {
    for (const record of records) {
      for (const node of record.addedNodes) {
        if (node instanceof HTMLElement && node.classList.contains('entry')) decorateEntry(node);
      }
    }
  }).observe(entries, { childList: true });

  document.addEventListener('click', (event) => {
    const excerpt = event.target.closest('.entry-match');
    if (!excerpt) return;
    const item = session.entries.find((entry) => entry.path === excerpt.closest('.entry')?.dataset.path);
    if (!item?.matchLines?.length) return;
    event.stopPropagation();
    const expanded = excerpt.dataset.expanded === 'true';
    excerpt.dataset.expanded = String(!expanded);
    excerpt.setAttribute('aria-expanded', String(!expanded));
    excerpt.textContent = expanded ? String(item.match) : item.matchLines.join(' | ');
    excerpt.title = expanded ? String(item.match) : item.matchLines.join(' | ');
  });

  document.addEventListener('keydown', (event) => {
    const excerpt = event.target.closest?.('.entry-match');
    if (!excerpt || !['Enter', ' '].includes(event.key)) return;
    event.preventDefault();
    excerpt.click();
    if (event.key === 'Enter') excerpt.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
  });

  document.addEventListener('dblclick', async (event) => {
    const excerpt = event.target.closest('.entry-match');
    if (!excerpt) return;
    const item = session.entries.find((entry) => entry.path === excerpt.closest('.entry')?.dataset.path);
    const line = Number(String(item?.matchLines?.[0] || '').match(/^\d+/)?.[0]);
    if (!item || !line) return;
    event.preventDefault();
    session.selectedPaths = new Set([item.path]);
    renderSelection();
    await renderPreviewSelection();
    const editor = $('preview-body').querySelector('.file-editor');
    if (!editor) return;
    const offset = editor.value.split(/\n/).slice(0, line - 1).reduce((total, value) => total + value.length + 1, 0);
    editor.focus();
    editor.setSelectionRange(offset, offset);
    const lineHeight = Number.parseFloat(getComputedStyle(editor).lineHeight) || 18;
    editor.scrollTop = Math.max(0, (line - 1) * lineHeight - editor.clientHeight / 2);
  });

  document.addEventListener('keydown', async (event) => {
    const dialog = $('image-lightbox');
    if (!dialog?.open || !['ArrowLeft', 'ArrowRight'].includes(event.key)) return;
    const candidates = session.entries.filter((item) => item.kind === 'image' && !item.isDir);
    const index = candidates.findIndex((item) => item.path === dialog.dataset.path);
    if (index < 0 || !candidates.length) return;
    event.preventDefault();
    const item = candidates[(index + (event.key === 'ArrowRight' ? 1 : -1) + candidates.length) % candidates.length];
    const requestId = crypto.randomUUID();
    dialog.dataset.request = requestId;
    try {
      const result = await call('image_preview', { path: item.path }, requestId);
      if (!dialog.open || dialog.dataset.request !== requestId || !/^image\//.test(result?.mimeType || '') || typeof result.data !== 'string') return;
      const image = dialog.querySelector('img');
      image.src = `data:${result.mimeType};base64,${result.data}`;
      image.alt = item.name;
      dialog.dataset.path = item.path;
    } catch {}
  });

  document.addEventListener('keydown', (event) => {
    if (session.viewMode !== 'grid' || !['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(event.key) || event.target.matches('input,textarea,select')) return;
    const row = event.target.closest('.entry');
    if (!row) return;
    const index = Number(row.dataset.index);
    const columns = Math.max(1, getComputedStyle(entries).gridTemplateColumns.split(' ').length);
    const delta = event.key === 'ArrowLeft' ? -1 : event.key === 'ArrowRight' ? 1 : event.key === 'ArrowUp' ? -columns : columns;
    const next = Math.max(0, Math.min(session.entries.length - 1, index + delta));
    event.preventDefault();
    event.stopImmediatePropagation();
    selectEntry(next, event);
    document.querySelector(`[data-index="${next}"]`)?.focus();
  }, true);

  return () => gridThumbObserver?.disconnect();
}
