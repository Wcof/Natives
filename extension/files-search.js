/**
 * Files search controller.
 * Manages directory search, global multi-root search, quick filtering and debounced cancellation.
 */

export function createFilesSearch({
  $,
  call,
  t,
  session,
  setStatus,
  sortEntries,
  renderEntries,
  renderSelection,
  updatePager,
  getRootPaths,
  PAGE_SIZE = 100,
  loadDirectory,
  iconElement,
}) {
  let searchQuery = '';
  let searchTimer = null;
  let searchToken = 0;
  let activeSearchId = undefined;
  const activeSearchIds = new Set();
  let globalSearchMode = false;
  let recursiveSearch = false;
  let searchTruncated = false;

  function isGlobalMode() {
    return globalSearchMode;
  }

  function setGlobalMode(val) {
    globalSearchMode = Boolean(val);
    updateSearchScopeButton();
  }

  function getSearchQuery() {
    return searchQuery;
  }

  function setSearchQuery(val) {
    searchQuery = String(val || '');
  }

  function updateSearchScopeButton() {
    const button = $('scope-toggle');
    if (!button) return;
    const labelKey = globalSearchMode ? 'globalSearch' : 'currentDirectorySearch';
    const text = globalSearchMode ? t('globalSearch', '全机') : t('currentDirectorySearch', '当前目录');
    button.replaceChildren(iconElement(globalSearchMode ? 'globe' : 'target'), document.createTextNode(` ${text}`));
    button.title = t(labelKey, text);
    button.setAttribute('aria-pressed', String(globalSearchMode));
  }

  function toggleSearchScope() {
    globalSearchMode = !globalSearchMode;
    updateSearchScopeButton();
    const searchInput = $('search');
    if (searchInput) {
      searchInput.focus();
      searchInput.select();
    }
    setStatus(t(globalSearchMode ? 'globalSearch' : 'search', globalSearchMode ? '全机搜索' : '当前目录搜索'));
    if (searchQuery) {
      session.pageOffset = 0;
      search(searchQuery);
    }
  }

  function cancelActiveSearches() {
    searchTruncated = false;
    if (activeSearchId) {
      call('search_cancel', { requestId: activeSearchId }).catch(() => {});
      activeSearchId = undefined;
    }
    activeSearchIds.forEach((requestId) => {
      call('search_cancel', { requestId }).catch(() => {});
    });
    activeSearchIds.clear();
  }

  async function search(query) {
    const token = ++searchToken;
    cancelActiveSearches();
    if (globalSearchMode) return searchGlobal(query, token);

    const content = /^content:\s*/i.test(query);
    const normalizedQuery = query.replace(/^content:\s*/i, '').trim();
    const requestId = crypto.randomUUID();
    activeSearchId = requestId;
    setStatus(t('loading', '加载中…'));

    try {
      const result = await call('search', {
        path: session.currentPath,
        query: normalizedQuery,
        offset: session.pageOffset,
        limit: PAGE_SIZE,
        recursive: recursiveSearch,
        content,
        showHidden: session.showHidden,
      }, requestId);

      if (token !== searchToken) return;
      searchTruncated = Boolean(result?.truncated);
      session.entries = sortEntries(result.entries || []);
      const visiblePaths = new Set(session.entries.map((item) => item.path));
      session.selectedPaths = new Set([...session.selectedPaths].filter((p) => visiblePaths.has(p)));
      session.lastSelectedIndex = session.selectedPaths.size
        ? session.entries.findIndex((item) => session.selectedPaths.has(item.path))
        : -1;
      session.pageHasMore = Boolean(result.hasMore);
      renderEntries();
      renderSelection();
      updatePager();
      const count = session.entries.length;
      const unavail = result?.contentUnavailable ? ` · ${t('contentSearchUnavailable', 'PDF 文本搜索不可用')}` : '';
      setStatus(`${count} ${t('searchResults', '个搜索结果')}${unavail}`);
    } catch (error) {
      if (token === searchToken && error.message !== 'preview cancelled' && error.message !== 'search cancelled') {
        setStatus(error.message, 'error');
      }
    } finally {
      if (activeSearchId === requestId) activeSearchId = undefined;
    }
  }

  async function searchGlobal(query, token) {
    const content = /^content:\s*/i.test(query);
    const normalizedQuery = query.replace(/^content:\s*/i, '').trim();
    const rootPaths = getRootPaths() || [];
    const requests = rootPaths.map((path) => ({ path, requestId: crypto.randomUUID() }));
    requests.forEach(({ requestId }) => activeSearchIds.add(requestId));
    activeSearchId = requests[0]?.requestId;
    setStatus(t('loading', '加载中…'));

    try {
      const results = await Promise.all(
        requests.map(({ path, requestId }) =>
          call('search', {
            path,
            query: normalizedQuery,
            offset: session.pageOffset,
            limit: PAGE_SIZE * 4,
            recursive: true,
            content,
            showHidden: session.showHidden,
          }, requestId),
        ),
      );

      if (token !== searchToken) return;
      searchTruncated = results.some((r) => Boolean(r?.truncated));
      const seen = new Set();
      const merged = sortEntries(
        results
          .flatMap((r) => r.entries || [])
          .filter((item) => !seen.has(item.path) && seen.add(item.path)),
      );
      session.entries = merged.slice(0, PAGE_SIZE);
      session.selectedPaths.clear();
      session.lastSelectedIndex = -1;
      session.pageHasMore = results.some((r) => Boolean(r?.hasMore)) || merged.length > PAGE_SIZE;
      renderEntries();
      renderSelection();
      updatePager();
      const plus = session.pageHasMore ? '+' : '';
      const unavail = results.some((r) => r?.contentUnavailable) ? ` · ${t('contentSearchUnavailable', 'PDF 文本搜索不可用')}` : '';
      setStatus(`${session.entries.length}${plus} ${t('searchResults', '个搜索结果')}${unavail}`);
    } catch (error) {
      if (token === searchToken && error.message !== 'search cancelled') {
        setStatus(error.message, 'error');
      }
    } finally {
      requests.forEach(({ requestId }) => {
        activeSearchIds.delete(requestId);
        call('search_cancel', { requestId }).catch(() => {});
      });
      activeSearchId = undefined;
    }
  }

  return {
    isGlobalMode,
    setGlobalMode,
    getSearchQuery,
    setSearchQuery,
    getRecursive: () => recursiveSearch,
    setRecursive: (v) => { recursiveSearch = Boolean(v); },
    updateSearchScopeButton,
    toggleSearchScope,
    cancelActiveSearches,
    search,
    searchGlobal,
    debounceSearch(text, delay = 180) {
      searchQuery = text.trim();
      session.pageOffset = 0;
      session.selectedPaths.clear();
      session.lastSelectedIndex = -1;
      renderSelection();
      clearTimeout(searchTimer);
      searchTimer = setTimeout(() => {
        if (searchQuery) search(searchQuery); else { searchToken++; cancelActiveSearches(); loadDirectory(session.currentPath); }
      }, delay);
    },
  };
}
