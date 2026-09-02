/**
 * Quick Links Widget.
 * Clean multi-column links layout matching TablissNG Links.sass.
 */

import { escapeHtml } from '../sanitizer.js';

export const linksWidget = {
  key: 'widget/links',
  name: 'Quick Links',
  defaultData: {
    columns: 2,
    newTab: true,
    showIcons: true,
    links: [
      { title: 'GitHub', url: 'https://github.com' },
      { title: 'Google', url: 'https://google.com' },
      { title: 'YouTube', url: 'https://youtube.com' },
      { title: 'Wikipedia', url: 'https://wikipedia.org' },
    ],
  },
  render(container, data = {}, display = {}, { t = (k, f) => f || k } = {}) {
    const rawLinks = Array.isArray(data.links) ? data.links : [];
    const cols = Math.max(1, Math.min(6, Number(data.columns) || 2));
    const openNewTab = data.newTab !== false;
    const showIcons = data.showIcons !== false;

    container.className = 'Widget Links';
    container.replaceChildren();
    container.style.gridTemplateColumns = '1fr '.repeat(cols).trim();
    container.style.textAlign = cols > 1 ? 'left' : '';

    for (const link of rawLinks) {
      if (!link.url || !/^https?:\/\//i.test(link.url)) continue;

      const a = document.createElement('a');
      a.className = 'Link';
      a.href = link.url;
      if (openNewTab) {
        a.target = '_blank';
        a.rel = 'noopener noreferrer';
      }

      if (showIcons) {
        const domain = extractDomain(link.url);
        const icon = document.createElement('img');
        icon.className = 'custom-icon';
        icon.src = `https://www.google.com/s2/favicons?domain=${encodeURIComponent(domain)}&sz=32`;
        icon.alt = '';
        a.append(icon);
      }

      const label = document.createElement('span');
      label.className = 'link-text';
      label.textContent = link.title || extractDomain(link.url) || link.url;
      a.append(label);

      container.append(a);
    }

    return () => container.replaceChildren();
  },
  renderSettings(container, data = {}, onChange = () => {}, { t = (k, f) => f || k } = {}) {
    const links = Array.isArray(data.links) ? data.links : [];
    container.replaceChildren();

    const wrap = document.createElement('div');
    wrap.className = 'inspector-field-group';
    wrap.innerHTML = `
      <label class="inspector-field">
        <span>${t('columnsCount', '排列列数 (1-6)')}</span>
        <input type="number" id="l-cols" min="1" max="6" value="${data.columns || 2}" />
      </label>
      <label class="inspector-checkbox">
        <input type="checkbox" id="l-icons" ${data.showIcons !== false ? 'checked' : ''} />
        <span>${t('showFavicons', '显示网站 Favicon 图标')}</span>
      </label>
      <label class="inspector-checkbox">
        <input type="checkbox" id="l-newtab" ${data.newTab !== false ? 'checked' : ''} />
        <span>${t('openInNewTab', '在新标签页中打开')}</span>
      </label>
      <div class="inspector-field">
        <span>${t('linksList', '链接列表')}</span>
        <div class="links-editor-list">
          ${links.map((link, idx) => `
            <div class="link-edit-row">
              <input type="text" class="link-t" data-idx="${idx}" value="${escapeHtml(link.title || '')}" placeholder="标题" />
              <input type="url" class="link-u" data-idx="${idx}" value="${escapeHtml(link.url || '')}" placeholder="https://..." />
              <button type="button" class="del-link-btn danger icon-button" data-idx="${idx}">×</button>
            </div>
          `).join('')}
        </div>
        <button type="button" class="add-link-btn primary" style="margin-top:8px;">+ ${t('addLink', '添加链接')}</button>
      </div>
    `;

    const emitChange = (nextLinks) => onChange({ ...data, links: nextLinks });

    wrap.querySelector('#l-cols').onchange = (e) => onChange({ ...data, columns: Number(e.target.value) || 2 });
    wrap.querySelector('#l-icons').onchange = (e) => onChange({ ...data, showIcons: e.target.checked });
    wrap.querySelector('#l-newtab').onchange = (e) => onChange({ ...data, newTab: e.target.checked });

    wrap.querySelectorAll('.link-t, .link-u').forEach((inp) => {
      inp.onchange = () => {
        const next = [...links];
        const idx = Number(inp.dataset.idx);
        const row = inp.closest('.link-edit-row');
        next[idx] = {
          title: row.querySelector('.link-t').value.trim(),
          url: row.querySelector('.link-u').value.trim(),
        };
        emitChange(next);
      };
    });

    wrap.querySelectorAll('.del-link-btn').forEach((btn) => {
      btn.onclick = () => {
        const idx = Number(btn.dataset.idx);
        const next = links.filter((_, i) => i !== idx);
        emitChange(next);
      };
    });

    wrap.querySelector('.add-link-btn').onclick = () => {
      emitChange([...links, { title: 'New Link', url: 'https://' }]);
    };

    container.append(wrap);
  },
  styles: `
    .Links { column-gap:1em; display:inline-grid; }
    .Links .custom-icon, .Links i { margin-right:5px; margin-left:-2px; }
    .Links a { display:block; margin:.25em; white-space:nowrap; }
    .Links a img { height:1em; width:1em; object-fit:contain; }
    .link-edit-row {
      display: grid;
      grid-template-columns: 1fr 1.5fr auto;
      gap: 6px;
      margin-bottom: 6px;
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
