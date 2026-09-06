

export function createGlobalSearchModal({
  $,
  t,
  nativeCall,
}) {
  const dialog = $('search-dialog');
  const trigger = $('command-search-trigger');
  const input = $('search');
  const resultsBox = $('search-results-box');
  let searchTimer = null;
  let activeSearchId = null;
  let rootPaths = [];
  let currentEntries = [];
  let selectedIndex = 0;

  async function loadRoots() {
    if (rootPaths.length > 0) return rootPaths;
    try {
      const roots = await nativeCall('roots');
      rootPaths = (roots || []).map((r) => r.path);
    } catch {
      rootPaths = [];
    }
    return rootPaths;
  }

  function open() {
    if (!dialog) return;
    currentEntries = [];
    selectedIndex = 0;
    if (resultsBox) resultsBox.replaceChildren();
    if (input) input.value = '';
    if (!dialog.open && typeof dialog.showModal === 'function') {
      dialog.showModal();
    }
    if (input) {
      input.focus();
      input.select();
    }
    loadRoots().catch(() => {});
  }

  function close() {
    if (dialog && dialog.open) {
      dialog.close('cancel');
    }
    if (activeSearchId) {
      nativeCall('search_cancel', { requestId: activeSearchId }).catch(() => {});
      activeSearchId = null;
    }
  }

  async function executeSearch(rawQuery) {
    const query = rawQuery.trim();
    if (!query) {
      currentEntries = [];
      resultsBox?.replaceChildren();
      return;
    }

    const roots = await loadRoots();
    if (!roots.length) return;

    if (resultsBox) {
      resultsBox.innerHTML = `<div style="color:var(--muted);font-size:12px;padding:12px 10px;text-align:center;">${t('loading', '搜索中…')}</div>`;
    }

    const requestId = crypto.randomUUID();
    activeSearchId = requestId;
    const content = /^content:\s*/i.test(query);
    const normalizedQuery = query.replace(/^content:\s*/i, '').trim();

    try {
      const results = await Promise.all(
        roots.map((path) =>
          nativeCall('search', {
            path,
            query: normalizedQuery,
            offset: 0,
            limit: 40,
            recursive: true,
            content,
            showHidden: false,
          }, requestId).catch(() => ({ entries: [] }))
        ),
      );

      if (activeSearchId !== requestId) return;

      const seen = new Set();
      currentEntries = results
        .flatMap((r) => r.entries || [])
        .filter((item) => !seen.has(item.path) && seen.add(item.path))
        .slice(0, 50);

      selectedIndex = 0;
      renderResults(currentEntries, normalizedQuery);
    } catch (err) {
      if (resultsBox && activeSearchId === requestId) {
        resultsBox.innerHTML = `<div style="color:var(--danger);font-size:12px;padding:10px;">${err.message || '搜索失败'}</div>`;
      }
    } finally {
      if (activeSearchId === requestId) activeSearchId = null;
    }
  }

  function renderResults(entries) {
    if (!resultsBox) return;
    resultsBox.replaceChildren();

    if (!entries.length) {
      resultsBox.innerHTML = `<div style="color:var(--muted);font-size:12px;padding:16px 10px;text-align:center;">${t('noSearchResults', '没有匹配的文件')}</div>`;
      return;
    }

    entries.forEach((item, idx) => {
      const row = document.createElement('div');
      row.className = `search-result-row ${idx === selectedIndex ? 'selected' : ''}`;
      row.style.cssText = `display:flex;align-items:center;justify-content:space-between;gap:10px;padding:8px 12px;border-radius:6px;background:${idx === selectedIndex ? 'var(--hover)' : 'var(--surface-3)'};cursor:pointer;border:1px solid ${idx === selectedIndex ? 'var(--accent)' : 'transparent'};transition:all 0.1s;`;
      row.innerHTML = `
        <div style="min-width:0;flex:1;">
          <strong style="font-size:13px;display:block;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;color:var(--text);">${item.name}</strong>
          <small style="color:var(--muted);font-size:11px;display:block;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;">${item.path}</small>
        </div>
        <div style="display:flex;gap:6px;flex-shrink:0;">
          <button type="button" class="open-btn primary" style="font-size:11px;height:24px;min-height:24px;padding:0 8px;border-radius:4px;cursor:pointer;">${t('open', '打开')}</button>
          <button type="button" class="reveal-btn" style="font-size:11px;height:24px;min-height:24px;padding:0 8px;border-radius:4px;border:1px solid var(--control);background:var(--surface-2);color:var(--text);cursor:pointer;">${t('reveal', '显示')}</button>
        </div>
      `;

      row.onmouseenter = () => {
        selectedIndex = idx;
        updateSelectionHighlight();
      };

      row.querySelector('.open-btn').onclick = (e) => {
        e.stopPropagation();
        close();
        nativeCall('open', { path: item.path }).catch(() => {});
      };
      row.querySelector('.reveal-btn').onclick = (e) => {
        e.stopPropagation();
        close();
        nativeCall('reveal', { path: item.path }).catch(() => {});
      };
      row.onclick = () => {
        close();
        nativeCall('open', { path: item.path }).catch(() => {});
      };

      if (typeof resultsBox.append === 'function') {
        resultsBox.append(row);
      } else if (typeof resultsBox.appendChild === 'function') {
        resultsBox.appendChild(row);
      } else if (Array.isArray(resultsBox.children)) {
        resultsBox.children.push(row);
      }
    });
  }

  function updateSelectionHighlight() {
    const rows = typeof resultsBox?.querySelectorAll === 'function' ? resultsBox.querySelectorAll('.search-result-row') : (resultsBox?.children || []);
    rows.forEach((row, idx) => {
      const isSel = idx === selectedIndex;
      if (row.style) {
        row.style.background = isSel ? 'var(--hover)' : 'var(--surface-3)';
        row.style.borderColor = isSel ? 'var(--accent)' : 'transparent';
      }
      if (isSel && typeof row.scrollIntoView === 'function') row.scrollIntoView({ block: 'nearest' });
    });
  }

  if (trigger) {
    trigger.onclick = () => open();
  }

  if (input) {
    input.oninput = (e) => {
      clearTimeout(searchTimer);
      searchTimer = setTimeout(() => executeSearch(e.target.value), 180);
    };
    input.onkeydown = (e) => {
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        if (currentEntries.length > 0) {
          selectedIndex = (selectedIndex + 1) % currentEntries.length;
          updateSelectionHighlight();
        }
      } else if (e.key === 'ArrowUp') {
        e.preventDefault();
        if (currentEntries.length > 0) {
          selectedIndex = (selectedIndex - 1 + currentEntries.length) % currentEntries.length;
          updateSelectionHighlight();
        }
      } else if (e.key === 'Enter') {
        e.preventDefault();
        if (currentEntries[selectedIndex]) {
          const item = currentEntries[selectedIndex];
          close();
          nativeCall('open', { path: item.path }).catch(() => {});
        }
      }
    };
  }

  if (typeof document !== 'undefined' && typeof document.addEventListener === 'function') {
    document.addEventListener('keydown', (e) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        open();
      }
    });
  }

  return { open, close };
}
