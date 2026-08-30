/**
 * Widget Catalog Controller (<180 lines).
 * Displays all 29 TablissNG widgets with search filter, instance counts, and add action.
 */

import { WIDGET_KEYS, pluginName, widgetPlugins } from './space-plugins.js';

export function createSpaceCatalog({
  t,
  language = 'zh_CN',
  onAddWidget,
  onBackToOverview,
}) {
  function render(container, snapshot, workspaceId) {
    container.replaceChildren();

    // Catalog Header
    const header = document.createElement('div');
    header.className = 'inspector-heading';
    header.innerHTML = `
      <div style="display:flex;align-items:center;gap:8px;">
        <button class="inspector-back" type="button"><svg class="icon"><use href="#i-chevron-left" /></svg><span>${t('back', '返回')}</span></button>
        <h2>${t('widgetCatalog', '组件目录')}</h2>
      </div>
    `;
    header.querySelector('.inspector-back').onclick = () => onBackToOverview();

    // Search input
    const searchWrap = document.createElement('div');
    searchWrap.className = 'catalog-search-wrap';
    searchWrap.innerHTML = `
      <input type="search" class="catalog-search" placeholder="${t('searchWidgets', '搜索组件...')}" autocomplete="off" />
    `;
    const searchInput = searchWrap.querySelector('input');

    // Widget list container
    const listContainer = document.createElement('div');
    listContainer.className = 'catalog-list';

    // Compute widget instance counts
    const countMap = {};
    for (const w of snapshot?.widgets || []) {
      countMap[w.key] = (countMap[w.key] || 0) + 1;
    }

    // Build catalog items array
    const items = WIDGET_KEYS.map((key) => {
      const plugin = widgetPlugins[key] || {};
      const name = pluginName(key, language, plugin.name || key);
      const desc = t(`${key.replace('widget/', 'desc_')}`, plugin.description || name);
      return {
        key,
        name,
        desc,
        count: countMap[key] || 0,
        plugin,
      };
    });

    // Sort alphabetically by localized name
    items.sort((a, b) => a.name.localeCompare(b.name, language === 'zh_CN' ? 'zh-CN' : 'en'));

    function renderList(query = '') {
      listContainer.replaceChildren();
      const q = query.trim().toLowerCase();
      const filtered = items.filter((item) => !q || item.name.toLowerCase().includes(q) || item.desc.toLowerCase().includes(q) || item.key.toLowerCase().includes(q));

      if (filtered.length === 0) {
        const empty = document.createElement('div');
        empty.className = 'inspector-empty';
        empty.textContent = t('noMatchingWidgets', '没有匹配的组件');
        listContainer.append(empty);
        return;
      }

      for (const item of filtered) {
        const card = document.createElement('div');
        card.className = 'catalog-card';
        card.innerHTML = `
          <div class="catalog-card-info">
            <strong>${item.name}</strong>
            <small>${item.desc}</small>
            ${item.count > 0 ? `<span class="catalog-count-badge">${t('widgetInstanceCount', '已添加')} ${item.count}</span>` : ''}
          </div>
          <button class="catalog-add-btn primary" type="button">
            <svg class="icon"><use href="#i-plus" /></svg>
            <span>${t('addWidget', '添加')}</span>
          </button>
        `;

        const addBtn = card.querySelector('.catalog-add-btn');
        addBtn.onclick = async () => {
          addBtn.disabled = true;
          try {
            await onAddWidget(item.key);
          } finally {
            addBtn.disabled = false;
          }
        };

        listContainer.append(card);
      }
    }

    searchInput.oninput = () => renderList(searchInput.value);

    renderList();
    container.append(header, searchWrap, listContainer);
  }

  return { render };
}
