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
        icon.onerror = () => {
          icon.style.display = 'none';
          const fallback = document.createElement('span');
          fallback.className = 'custom-icon-fallback';
          const initial = (domain.replace(/^www\./, '')[0] || '✦').toUpperCase();
          fallback.textContent = initial;
          icon.after(fallback);
        };
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
        <div class="link-actions-group">
          <button type="button" class="add-link-btn primary">
            <svg class="icon" aria-hidden="true" style="width:13px;height:13px;"><use href="#i-plus" /></svg>
            <span>${t('addLink', '添加链接')}</span>
          </button>
          <button type="button" class="preset-ai-btn">
            <svg class="icon" aria-hidden="true" style="width:13px;height:13px;"><use href="#i-bolt" /></svg>
            <span>导入 AI 工具预设</span>
          </button>
        </div>
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

    wrap.querySelector('.preset-ai-btn').onclick = () => {
      const aiPresets = [
        { title: 'ChatGPT', url: 'https://chatgpt.com' },
        { title: 'Claude', url: 'https://claude.ai' },
        { title: 'DeepSeek', url: 'https://chat.deepseek.com' },
        { title: 'Kimi', url: 'https://kimi.moonshot.cn' },
        { title: 'Perplexity', url: 'https://www.perplexity.ai' },
        { title: 'GitHub', url: 'https://github.com' },
        { title: 'Hugging Face', url: 'https://huggingface.co' },
        { title: 'v0.dev', url: 'https://v0.dev' },
      ];
      emitChange(aiPresets);
    };

    container.append(wrap);
  },
  styles: `
    .Links { column-gap:1em; display:inline-grid; }
    .Links .custom-icon, .Links i { margin-right:5px; margin-left:-2px; }
    .Links a { display:block; margin:.25em; white-space:nowrap; }
    .Links a img { height:1em; width:1em; object-fit:contain; }
    .Links .custom-icon-fallback {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      width: 1em;
      height: 1em;
      border-radius: 3px;
      background: rgba(255,255,255,0.18);
      font-size: 0.75em;
      font-weight: 700;
      margin-right: 5px;
      margin-left: -2px;
      vertical-align: middle;
      line-height: 1;
    }
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
