

import { WIDGET_KEYS, pluginName, widgetPlugins } from './space-plugins.js';

export const WIDGET_CATEGORIES = [
  {
    id: 'time',
    nameKey: 'catTimeAndClocks',
    fallback: '时间与时钟',
    keys: [
      'widget/time',
      'widget/binaryTime',
      'widget/since',
      'widget/countdown',
      'widget/workHours',
    ],
  },
  {
    id: 'productivity',
    nameKey: 'catProductivity',
    fallback: '生产力与工具',
    keys: [
      'widget/todo',
      'widget/notes',
      'widget/trello',
      'widget/tallyCounter',
      'widget/search',
      'widget/links',
      'widget/bookmarks',
      'widget/topSites',
    ],
  },
  {
    id: 'information',
    nameKey: 'catInformationFeeds',
    fallback: '资讯与数据',
    keys: [
      'widget/weather',
      'widget/currencyRates',
      'widget/ipInfo',
      'widget/github',
    ],
  },
  {
    id: 'inspiration',
    nameKey: 'catInspirationText',
    fallback: '灵感与问候',
    keys: [
      'widget/greeting',
      'widget/quote',
      'widget/palette',
      'widget/message',
      'widget/customText',
    ],
  },
  {
    id: 'advanced',
    nameKey: 'catCustomAdvanced',
    fallback: '自定义与扩展',
    keys: [
      'widget/html',
      'widget/css',
    ],
  },
];

export function createSpaceCatalog({
  t,
  language = 'zh_CN',
  onAddWidget,
  onBackToOverview,
}) {
  const collapsedCategories = new Set();

  function render(container, snapshot, workspaceId) {
    container.replaceChildren();

    
    const header = document.createElement('div');
    header.className = 'inspector-heading';
    header.innerHTML = `
      <div style="display:flex;align-items:center;gap:8px;">
        <button class="inspector-back" type="button"><svg class="icon"><use href="#i-chevron-left" /></svg><span>${t('back', '返回')}</span></button>
        <h2>${t('widgetCatalog', '卡片目录')}</h2>
      </div>
    `;
    header.querySelector('.inspector-back').onclick = () => onBackToOverview();

    
    const searchWrap = document.createElement('div');
    searchWrap.className = 'catalog-search-wrap';
    searchWrap.innerHTML = `
      <svg class="icon" aria-hidden="true"><use href="#i-search" /></svg>
      <input type="search" class="catalog-search" placeholder="${t('searchWidgets', '搜索卡片...')}" autocomplete="off" />
    `;
    const searchInput = searchWrap.querySelector('input');

    
    const listContainer = document.createElement('div');
    listContainer.className = 'catalog-list';

    
    const countMap = {};
    for (const w of snapshot?.widgets || []) {
      countMap[w.key] = (countMap[w.key] || 0) + 1;
    }

    
    const itemsByKey = {};
    for (const key of WIDGET_KEYS) {
      const plugin = widgetPlugins[key] || {};
      const name = pluginName(key, language, plugin.name || key);
      const desc = t(`${key.replace('widget/', 'desc_')}`, plugin.description || name);
      itemsByKey[key] = {
        key,
        name,
        desc,
        count: countMap[key] || 0,
        plugin,
      };
    }

    function renderList(query = '') {
      listContainer.replaceChildren();
      const q = query.trim().toLowerCase();
      let totalMatching = 0;

      for (const cat of WIDGET_CATEGORIES) {
        const catName = t(cat.nameKey, cat.fallback);
        const catItems = cat.keys
          .map((k) => itemsByKey[k])
          .filter(Boolean)
          .filter((item) => !q || item.name.toLowerCase().includes(q) || item.desc.toLowerCase().includes(q) || item.key.toLowerCase().includes(q));

        if (catItems.length === 0) continue;
        totalMatching += catItems.length;

        const isSearching = Boolean(q);
        const isCollapsed = !isSearching && collapsedCategories.has(cat.id);

        const groupEl = document.createElement('div');
        groupEl.className = 'catalog-category-group';

        const headerBtn = document.createElement('button');
        headerBtn.type = 'button';
        headerBtn.className = `catalog-category-header ${isCollapsed ? 'collapsed' : ''}`;
        headerBtn.setAttribute?.('aria-expanded', String(!isCollapsed));
        headerBtn.innerHTML = `
          <span class="catalog-category-title">${catName}</span>
          <span class="catalog-category-badge">${catItems.length}</span>
          <svg class="icon chev"><use href="#i-chevron-right" /></svg>
        `;

        const bodyEl = document.createElement('div');
        bodyEl.className = 'catalog-category-body';
        if (isCollapsed) bodyEl.hidden = true;

        headerBtn.onclick = () => {
          if (isSearching) return;
          const nextCollapsed = !collapsedCategories.has(cat.id);
          if (nextCollapsed) {
            collapsedCategories.add(cat.id);
          } else {
            collapsedCategories.delete(cat.id);
          }
          headerBtn.classList?.toggle?.('collapsed', nextCollapsed);
          headerBtn.setAttribute?.('aria-expanded', String(!nextCollapsed));
          bodyEl.hidden = nextCollapsed;
        };

        for (const item of catItems) {
          const card = document.createElement('div');
          card.className = 'catalog-card';
          card.innerHTML = `
            <div class="catalog-card-info">
              <strong>${item.name}</strong>
              <small>${item.desc}</small>
              ${item.count > 0 ? `<span class="catalog-count-badge">${t('widgetInstanceCount', '已添加')} ${item.count}</span>` : ''}
            </div>
            <button class="catalog-add-btn" type="button" aria-label="${t('addWidget', '添加卡片')}: ${item.name}">
              <svg class="icon" aria-hidden="true"><use href="#i-plus" /></svg>
              <span>${t('addWidget', '添加卡片')}</span>
            </button>
          `;

          const addBtn = card.querySelector('.catalog-add-btn');
          addBtn.onclick = async (e) => {
            e.stopPropagation();
            addBtn.disabled = true;
            try {
              await onAddWidget(item.key);
            } finally {
              addBtn.disabled = false;
            }
          };

          bodyEl.append(card);
        }

        groupEl.append(headerBtn, bodyEl);
        listContainer.append(groupEl);
      }

      if (totalMatching === 0) {
        const empty = document.createElement('div');
        empty.className = 'inspector-empty';
        empty.textContent = t('noMatchingWidgets', '没有匹配的卡片');
        listContainer.append(empty);
      }
    }

    searchInput.oninput = () => renderList(searchInput.value);

    renderList();
    container.append(header, searchWrap, listContainer);
  }

  return { render };
}
