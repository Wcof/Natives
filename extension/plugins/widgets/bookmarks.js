import { escapeHtml } from '../sanitizer.js';
export const bookmarksWidget = {
  key: 'widget/bookmarks',
  name: 'Bookmarks',
  defaultData: {
    folderId: '1',
    viewMode: 'list',
    showSearch: true,
  },
  render(container, data = {}, display = {}, { t = (k, f) => f || k } = {}) {
    let disposed = false;
    let currentFolderId = data.folderId || '1';
    let folderHistory = [];
    let searchQuery = '';
    container.className = 'Widget Bookmarks';
    container.replaceChildren();
    const root = document.createElement('div');
    root.className = `bookmarks-root mode-${data.viewMode || 'list'}`;
    container.append(root);
    function loadFolder(folderId, permissionChecked = false) {
      if (disposed || (container.isConnected !== undefined && !container.isConnected)) return;
      if (!permissionChecked && globalThis.chrome?.permissions?.contains) {
        chrome.permissions.contains({ permissions: ['bookmarks'] }, (granted) => {
          if (disposed) return;
          if (granted) loadFolder(folderId, true);
          else renderPermissionPrompt();
        });
        return;
      }
      if (!globalThis.chrome?.bookmarks?.getChildren) {
        renderPermissionPrompt();
        return;
      }
      if (searchQuery && typeof chrome.bookmarks.search === 'function') {
        chrome.bookmarks.search(searchQuery, (results) => {
          if (disposed) return;
          renderItems(results || [], true);
        });
      } else {
        chrome.bookmarks.getChildren(folderId, (children) => {
          if (disposed) return;
          renderItems(children || [], false);
        });
      }
    }
    function renderPermissionPrompt() {
      root.replaceChildren();
      const prompt = document.createElement('div');
      prompt.className = 'bookmarks-permission-prompt';
      prompt.innerHTML = `
        <svg viewBox="0 0 24 24" width="22" height="22" stroke="currentColor" stroke-width="2" fill="none"><path d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z"></path></svg>
        <p>${t('bookmarksAuthRequired', '需要书签访问权限以展示收藏夹')}</p>
        <button type="button" class="bookmarks-auth-btn">${t('clickToAuthorizeBookmarks', '授权书签权限')}</button>
      `;
      prompt.querySelector('button').onclick = () => {
        if (globalThis.chrome?.permissions?.request) {
          chrome.permissions.request({ permissions: ['bookmarks'] }, (granted) => {
            if (granted) loadFolder(currentFolderId, true);
          });
        }
      };
      root.append(prompt);
    }
    function renderItems(items, isSearchResult) {
      root.replaceChildren();
      const topBar = document.createElement('div');
      topBar.className = 'bookmarks-top-bar';
      if (!isSearchResult && folderHistory.length > 0) {
        const backBtn = document.createElement('button');
        backBtn.type = 'button';
        backBtn.className = 'bookmarks-back-btn';
        backBtn.innerHTML = '←';
        backBtn.onclick = () => {
          currentFolderId = folderHistory.pop() || '1';
          loadFolder(currentFolderId);
        };
        topBar.append(backBtn);
      }
      if (data.showSearch !== false) {
        const searchInput = document.createElement('input');
        searchInput.type = 'search';
        searchInput.className = 'bookmarks-search-input';
        searchInput.placeholder = t('searchBookmarks', '搜索书签...');
        searchInput.value = searchQuery;
        searchInput.oninput = (e) => {
          searchQuery = e.target.value.trim();
          loadFolder(currentFolderId);
        };
        topBar.append(searchInput);
      }
      if (topBar.childNodes.length > 0) {
        root.append(topBar);
      }
      const listEl = document.createElement('div');
      listEl.className = 'bookmarks-list';
      if (items.length === 0) {
        const empty = document.createElement('div');
        empty.className = 'bookmarks-empty';
        empty.textContent = searchQuery ? t('noMatchingBookmarks', '未找到匹配书签') : t('emptyFolder', '文件夹为空');
        listEl.append(empty);
      } else {
        for (const item of items) {
          const isFolder = !item.url;
          const node = document.createElement(isFolder ? 'button' : 'a');
          node.className = `${isFolder ? 'folder' : 'bookmark'} bookmark-item`;
          if (isFolder) {
            node.type = 'button';
            node.innerHTML = `
              <span class="bm-icon folder-icon"><svg class="icon" aria-hidden="true" style="width:14px;height:14px;"><use href="#i-folder" /></svg></span>
              <span class="bm-title">${escapeHtml(item.title || '文件夹')}</span>
              <span class="bm-arrow">›</span>
            `;
            node.onclick = () => {
              folderHistory.push(currentFolderId);
              currentFolderId = item.id;
              searchQuery = '';
              loadFolder(currentFolderId);
            };
          } else {
            node.href = item.url;
            node.target = '_blank';
            node.rel = 'noopener noreferrer';
            const domain = extractDomain(item.url);
            node.innerHTML = `
              <img class="bm-favicon" src="https://www.google.com/s2/favicons?domain=${encodeURIComponent(domain)}&sz=32" alt="" />
              <span class="bm-title">${escapeHtml(item.title || domain || item.url)}</span>
            `;
          }
          listEl.append(node);
        }
      }
      root.append(listEl);
    }
    loadFolder(currentFolderId);
    return () => {
      disposed = true;
      container.replaceChildren();
    };
  },
  renderSettings(container, data = {}, onChange = () => {}, { t = (k, f) => f || k } = {}) {
    container.replaceChildren();
    const wrap = document.createElement('div');
    wrap.className = 'inspector-field-group';
    wrap.innerHTML = `
      <label class="inspector-field">
        <span>${t('viewMode', '展示模式')}</span>
        <select id="bm-mode">
          <option value="list" ${data.viewMode === 'list' ? 'selected' : ''}>${t('listView', '列表视图')}</option>
          <option value="grid" ${data.viewMode === 'grid' ? 'selected' : ''}>${t('gridView', '图标网格')}</option>
        </select>
      </label>
      <label class="inspector-checkbox">
        <input type="checkbox" id="bm-search" ${data.showSearch !== false ? 'checked' : ''} />
        <span>${t('showSearchBar', '显示书签搜索栏')}</span>
      </label>
    `;
    wrap.querySelector('#bm-mode').onchange = (e) => onChange({ ...data, viewMode: e.target.value });
    wrap.querySelector('#bm-search').onchange = (e) => onChange({ ...data, showSearch: e.target.checked });
    container.append(wrap);
  },
  styles: `
    .Bookmarks:not(.quick-links) { text-align:left; padding-right:10px; }
    .Bookmarks .bookmarks-root {
      display: flex;
      flex-direction: column;
      gap: 8px;
      max-height: 400px;
      text-align: left;
    }
    .Bookmarks .bookmarks-top-bar {
      display: flex;
      align-items: center;
      gap: 6px;
    }
    .Bookmarks .bookmarks-back-btn {
      flex: 0 0 28px;
      height: 28px;
      padding: 0;
      border: 1px solid currentColor;
      background: transparent;
      color: inherit;
      cursor: pointer;
      font-size: 14px;
    }
    .Bookmarks .bookmarks-search-input {
      flex: 1;
      height: 28px;
      padding: 0 8px;
      border: 0;
      border-bottom: 1px solid currentColor;
      background: transparent;
      color: inherit;
      font-size: 12px;
    }
    .Bookmarks .bookmarks-list {
      display: flex;
      flex-direction: column;
      gap: 2px;
      overflow-y: auto;
      max-height: 320px;
      padding-right: 2px;
    }
    .Bookmarks .mode-grid .bookmarks-list {
      display: grid;
      grid-template-columns: repeat(3, 1fr);
      gap: 8px;
    }
    .Bookmarks .folder, .Bookmarks .bookmark {
      display: flex;
      align-items: center;
      overflow:hidden;
      color: inherit;
      text-decoration: none;
      border: 0;
      background: transparent;
      cursor: pointer;
    }
    .Bookmarks .folder { margin-top:.6em; }
    .Bookmarks .bookmark { margin-top:.2em; }
    .Bookmarks .mode-grid .bookmark-item {
      flex-direction: column;
      text-align: center;
      padding: 8px 4px;
      gap: 4px;
    }
    .Bookmarks .folder:hover, .Bookmarks .bookmark:hover { opacity:.8; }
    .Bookmarks .bm-icon, .Bookmarks .bm-favicon {
      width: 16px;
      height: 16px;
      flex: 0 0 auto;
    }
    .Bookmarks .bm-title {
      flex: 1;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    .Bookmarks .bm-arrow {
      opacity: 0.5;
      font-size: 14px;
    }
    .Bookmarks .bookmarks-empty {
      padding: 16px 0;
      text-align: center;
      opacity: 0.6;
      font-size: 12px;
    }
    .Bookmarks .bookmarks-permission-prompt {
      display: flex;
      flex-direction: column;
      align-items: center;
      gap: 8px;
      padding: 16px 8px;
      text-align: center;
    }
    .Bookmarks .bookmarks-auth-btn {
      background-color:var(--bg-secondary, rgba(0,0,0,.5)); color:var(--text-heading, #fff);
      border:2px solid var(--text-heading, #fff); margin:.6em 0; border-radius:8px;
      font-weight:500; padding:10px 12px; text-align:center; transition:all 200ms;
      cursor: pointer;
    }
  `,
};
function extractDomain(url) {
  try {
    return new URL(url).hostname;
  } catch {
    return '';
  }
}
