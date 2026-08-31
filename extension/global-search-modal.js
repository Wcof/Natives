/**
 * Global File Search Modal controller (<150 lines).
 * Handles standalone search dialog in personal space without page navigation.
 */

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

  async function loadRoots() {
    if (rootPaths.length > 0) return rootPaths;
    try {
      const roots = await nativeCall('roots');
      rootPaths = (roots || []).map((r) => r.path);
    } catch (e) {
      rootPaths = [];
    }
    return rootPaths;
  }

  function open() {
    if (!dialog) return;
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

  async function executeSearch(query) {
    if (!query) {
      resultsBox?.replaceChildren();
      return;
    }

    const roots = await loadRoots();
    if (!roots.length) return;

    if (resultsBox) {
      resultsBox.innerHTML = `<div style="color:var(--muted);font-size:12px;padding:8px 10px;">${t('loading', '搜索中...')}</div>`;
    }

    const requestId = crypto.randomUUID();
    activeSearchId = requestId;

    try {
      const results = await Promise.all(
        roots.map((path) => nativeCall('search', {
          path,
          query,
          offset: 0,
          limit: 30,
          recursive: true,
          showHidden: false,
        }, requestId).catch(() => ({ entries: [] }))),
      );

      if (activeSearchId !== requestId) return;

      const seen = new Set();
      const merged = results
        .flatMap((r) => r.entries || [])
        .filter((item) => !seen.has(item.path) && seen.add(item.path))
        .slice(0, 40);

      renderResults(merged, query);
    } catch (err) {
      if (resultsBox) {
        resultsBox.innerHTML = `<div style="color:var(--danger);font-size:12px;padding:8px 10px;">${err.message || '搜索失败'}</div>`;
      }
    } finally {
      if (activeSearchId === requestId) activeSearchId = null;
    }
  }

  function renderResults(entries, query) {
    if (!resultsBox) return;
    resultsBox.replaceChildren();

    if (!entries.length) {
      resultsBox.innerHTML = `<div style="color:var(--muted);font-size:12px;padding:12px 10px;text-align:center;">${t('noSearchResults', '没有匹配的文件')}</div>`;
      return;
    }

    for (const item of entries) {
      const row = document.createElement('div');
      row.className = 'search-result-row';
      row.style.cssText = 'display:flex;align-items:center;justify-content:space-between;gap:8px;padding:8px 10px;border-radius:6px;background:var(--surface-3);cursor:pointer;transition:background 0.15s;';
      row.innerHTML = `
        <div style="min-width:0;flex:1;">
          <strong style="font-size:13px;display:block;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;">${item.name}</strong>
          <small style="color:var(--muted);font-size:11px;display:block;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;">${item.path}</small>
        </div>
        <div style="display:flex;gap:4px;flex:0 0 auto;">
          <button type="button" class="open-btn" style="padding:2px 8px;font-size:11px;border-radius:4px;border:1px solid var(--control);background:var(--surface-2);cursor:pointer;">${t('open', '打开')}</button>
          <button type="button" class="reveal-btn" style="padding:2px 8px;font-size:11px;border-radius:4px;border:1px solid var(--control);background:var(--surface-2);cursor:pointer;">${t('reveal', '显示')}</button>
        </div>
      `;

      row.onmouseenter = () => { row.style.background = 'var(--hover)'; };
      row.onmouseleave = () => { row.style.background = 'var(--surface-3)'; };

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

      resultsBox.append(row);
    }
  }

  if (trigger) {
    trigger.onclick = () => open();
  }

  if (input) {
    input.oninput = (e) => {
      const q = e.target.value.trim();
      clearTimeout(searchTimer);
      searchTimer = setTimeout(() => executeSearch(q), 200);
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
