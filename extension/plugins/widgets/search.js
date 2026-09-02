/**
 * Search Box Widget.
 * Clean TablissNG Search.sass implementation with multiple search engines and clear input styling.
 */

import { escapeHtml } from '../sanitizer.js';

const PROVIDERS = {
  google: { name: 'Google', action: 'https://www.google.com/search', param: 'q' },
  bing: { name: 'Bing', action: 'https://www.bing.com/search', param: 'q' },
  baidu: { name: '百度', action: 'https://www.baidu.com/s', param: 'wd' },
  duckduckgo: { name: 'DuckDuckGo', action: 'https://duckduckgo.com/', param: 'q' },
  github: { name: 'GitHub', action: 'https://github.com/search', param: 'q' },
  ecosia: { name: 'Ecosia', action: 'https://www.ecosia.org/search', param: 'q' },
};

export const searchWidget = {
  key: 'widget/search',
  name: 'Search Box',
  defaultData: {
    provider: 'google',
    placeholder: '',
    newTab: true,
    style: 'default',
  },
  render(container, data = {}, display = {}, { t = (k, f) => f || k } = {}) {
    const providerKey = data.provider || 'google';
    const provider = PROVIDERS[providerKey] || PROVIDERS.google;
    const placeholder = data.placeholder || `${provider.name} 搜索...`;
    const openNewTab = data.newTab !== false;

    const style = ['default', 'transparent-rounded', 'minimal-outlined'].includes(data.style) ? data.style : 'default';
    container.className = `Widget Search style-${style}`;
    container.replaceChildren();

    const form = document.createElement('form');
    form.className = 'search-form';
    form.action = provider.action;
    form.method = 'get';
    if (openNewTab) {
      form.target = '_blank';
    }

    const input = document.createElement('input');
    input.type = 'search';
    input.name = provider.param;
    input.placeholder = placeholder;
    input.autocomplete = 'off';

    const submitBtn = document.createElement('button');
    submitBtn.type = 'submit';
    submitBtn.className = 'search-submit';
    submitBtn.innerHTML = `
      <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <circle cx="11" cy="11" r="8"></circle>
        <line x1="21" y1="21" x2="16.65" y2="16.65"></line>
      </svg>
    `;

    form.append(input, submitBtn);
    container.append(form);

    return () => container.replaceChildren();
  },
  renderSettings(container, data = {}, onChange = () => {}, { t = (k, f) => f || k } = {}) {
    const curProvider = data.provider || 'google';
    container.replaceChildren();

    const wrap = document.createElement('div');
    wrap.className = 'inspector-field-group';
    wrap.innerHTML = `
      <label class="inspector-field">
        <span>${t('searchProvider', '搜索引擎')}</span>
        <select id="s-provider">
          ${Object.entries(PROVIDERS).map(([k, v]) => `<option value="${k}" ${k === curProvider ? 'selected' : ''}>${v.name}</option>`).join('')}
        </select>
      </label>
      <label class="inspector-field">
        <span>${t('placeholder', '占位提示文字')}</span>
        <input type="text" id="s-ph" value="${escapeHtml(data.placeholder || '')}" placeholder="Search..." />
      </label>
      <label class="inspector-checkbox">
        <input type="checkbox" id="s-newtab" ${data.newTab !== false ? 'checked' : ''} />
        <span>${t('openInNewTab', '在新标签页中打开搜索结果')}</span>
      </label>
      <label class="inspector-field"><span>${t('style', '样式')}</span><select id="s-style">
        <option value="default" ${data.style === 'default' || !data.style ? 'selected' : ''}>${t('default', '默认')}</option>
        <option value="transparent-rounded" ${data.style === 'transparent-rounded' ? 'selected' : ''}>${t('rounded', '透明圆角')}</option>
        <option value="minimal-outlined" ${data.style === 'minimal-outlined' ? 'selected' : ''}>${t('outlined', '简洁描边')}</option>
      </select></label>
    `;

    const update = () => onChange({
      ...data,
      provider: wrap.querySelector('#s-provider').value,
      placeholder: wrap.querySelector('#s-ph').value.trim(),
      newTab: wrap.querySelector('#s-newtab').checked,
      style: wrap.querySelector('#s-style').value,
    });

    wrap.querySelector('#s-provider').onchange = update;
    wrap.querySelector('#s-ph').onchange = update;
    wrap.querySelector('#s-newtab').onchange = update;
    wrap.querySelector('#s-style').onchange = update;
    container.append(wrap);
  },
  styles: `
    .Search { min-width:200px; display:block; position:relative; }
    .Search .search-form { display:flex; align-items:center; width:100%; position:relative; }
    .Search input {
      width: 100%;
      background-color:transparent; border:0; border-bottom:2px solid;
      font-family:inherit; font-size:1.1em; outline:none; padding:.15em 0;
      text-align:center; text-shadow:inherit; margin:1rem 0;
    }
    .Search .search-submit { background:none; border:0; outline:none; cursor:pointer; display:flex; align-items:center; justify-content:center; color:inherit; }
    .Search.style-transparent-rounded { display:flex; flex-direction:row; align-items:center; }
    .Search.style-transparent-rounded input { height:2.8rem; background:rgba(245,245,245,.1); border-radius:1.625rem; padding:0 3.5rem 0 1.5rem; font-size:1rem; text-align:left; backdrop-filter:blur(20px); border:1px solid rgba(255,255,255,.2); border-bottom:0; }
    .Search.style-transparent-rounded .search-submit { width:3.5rem; margin-left:-3.5rem; opacity:.7; }
    .Search.style-minimal-outlined { display:flex; flex-direction:row; align-items:center; }
    .Search.style-minimal-outlined input { height:2.2rem; border:1px solid rgba(255,255,255,.7); border-radius:4px; padding:0 2.5rem 0 .75rem; font-size:1rem; text-align:left; }
    .Search.style-minimal-outlined input:focus { border-color:#fff; }
    .Search.style-minimal-outlined .search-submit { width:2.5rem; margin-left:-2.5rem; opacity:.7; }
  `,
};
