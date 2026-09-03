import { escapeHtml } from '../sanitizer.js';
const PROVIDERS = {
  google: { name: 'Google', action: 'https://www.google.com/search', param: 'q' },
  bing: { name: 'Bing', action: 'https://www.bing.com/search', param: 'q' },
  baidu: { name: '百度', action: 'https://www.baidu.com/s', param: 'wd' },
  duckduckgo: { name: 'DuckDuckGo', action: 'https://duckduckgo.com/', param: 'q' },
  github: { name: 'GitHub', action: 'https://github.com/search', param: 'q' },
  ecosia: { name: 'Ecosia', action: 'https://www.ecosia.org/search', param: 'q' },
};
const SUGGEST_ENGINES = {
  google: { name: 'Google', url: (q) => `https://www.google.com/complete/search?client=chrome&q=${encodeURIComponent(q)}` },
  duckduckgo: { name: 'DuckDuckGo', url: (q) => `https://duckduckgo.com/ac/?q=${encodeURIComponent(q)}&type=list` },
  wikipedia: { name: 'Wikipedia', url: (q) => `https://en.wikipedia.org/w/rest.php/v1/search/title?q=${encodeURIComponent(q)}&limit=10` },
};
async function fetchSuggestions(engine, query) {
  const res = await fetch(SUGGEST_ENGINES[engine].url(query));
  if (!res.ok) throw new Error('suggestions unavailable');
  if (engine === 'wikipedia') {
    const data = await res.json();
    return (data.pages || []).map((page) => ({
      title: page.title,
      desc: page.description || '',
      thumb: page.thumbnail ? (page.thumbnail.url.startsWith('http') ? page.thumbnail.url : `https:${page.thumbnail.url}`) : '',
    }));
  }
  const data = await res.json();
  const list = Array.isArray(data) && Array.isArray(data[1]) ? data[1] : [];
  return list.map((text) => ({ title: String(text), desc: '', thumb: '' }));
}
export const searchWidget = {
  key: 'widget/search',
  name: 'Search Box',
  defaultData: {
    provider: 'google',
    placeholder: '',
    newTab: true,
    style: 'default',
    suggestions: true,
    suggestionsEngine: 'google',
    suggestionsQuantity: 4,
  },
  render(container, data = {}, display = {}, { t = (k, f) => f || k } = {}) {
    const providerKey = data.provider || 'google';
    const provider = PROVIDERS[providerKey] || PROVIDERS.google;
    const placeholder = data.placeholder || `${provider.name} 搜索...`;
    const openNewTab = data.newTab !== false;
    const quantity = Math.max(1, Math.min(10, Number(data.suggestionsQuantity) || 4));
    const suggestEnabled = data.suggestions !== false && SUGGEST_ENGINES[data.suggestionsEngine || 'google'];
    let disposed = false;
    let fetchToken = 0;
    let activeIndex = -1;
    let suggestions = [];
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
    let panel = null;
    function closeSuggestions() {
      suggestions = [];
      activeIndex = -1;
      panel?.remove();
      panel = null;
    }
    function renderSuggestions() {
      panel?.remove();
      panel = null;
      if (!suggestions.length) return;
      panel = document.createElement('div');
      panel.className = 'Suggestions';
      suggestions.forEach((item, index) => {
        const row = document.createElement('div');
        row.className = `suggestion-item${index === activeIndex ? ' active' : ''}`;
        row.innerHTML = `
          ${item.thumb ? `<img class="suggestion-thumb" src="${escapeHtml(item.thumb)}" alt="" referrerpolicy="no-referrer" />` : ''}
          <span class="suggestion-content">
            <span class="suggestion-title">${escapeHtml(item.title)}</span>
            ${item.desc ? `<span class="suggestion-desc">${escapeHtml(item.desc)}</span>` : ''}
          </span>
        `;
        row.onmousedown = (event) => {
          event.preventDefault();
          selectSuggestion(item.title);
        };
        panel.append(row);
      });
      container.append(panel);
    }
    function selectSuggestion(text) {
      if (!text) return;
      input.value = text;
      closeSuggestions();
      if (typeof form.requestSubmit === 'function') form.requestSubmit();
      else form.submit();
    }
    input.addEventListener('input', () => {
      if (!suggestEnabled) return;
      const query = input.value.trim();
      const token = ++fetchToken;
      if (!query) {
        closeSuggestions();
        return;
      }
      fetchSuggestions(data.suggestionsEngine || 'google', query)
        .then((list) => {
          if (disposed || token !== fetchToken) return;
          suggestions = list.slice(0, quantity);
          activeIndex = -1;
          renderSuggestions();
        })
        .catch(() => {});
    });
    input.addEventListener('keydown', (event) => {
      if (!panel) return;
      if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
        event.preventDefault();
        const step = event.key === 'ArrowDown' ? 1 : -1;
        activeIndex = (activeIndex + step + suggestions.length) % suggestions.length;
        renderSuggestions();
      } else if (event.key === 'Enter' && activeIndex >= 0) {
        event.preventDefault();
        selectSuggestion(suggestions[activeIndex].title);
      } else if (event.key === 'Escape') {
        closeSuggestions();
      }
    });
    input.addEventListener('blur', () => { setTimeout(closeSuggestions, 150); });
    return () => {
      disposed = true;
      container.replaceChildren();
    };
  },
  renderSettings(container, data = {}, onChange = () => {}, { t = (k, f) => f || k } = {}) {
    const curProvider = data.provider || 'google';
    const curSuggestEngine = data.suggestionsEngine || 'google';
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
      <label class="inspector-checkbox">
        <input type="checkbox" id="s-suggest" ${data.suggestions !== false ? 'checked' : ''} />
        <span>${t('searchSuggestions', '输入时显示搜索建议')}</span>
      </label>
      <label class="inspector-field">
        <span>${t('suggestionsEngine', '建议来源')}</span>
        <select id="s-suggest-engine">
          ${Object.entries(SUGGEST_ENGINES).map(([k, v]) => `<option value="${k}" ${k === curSuggestEngine ? 'selected' : ''}>${v.name}</option>`).join('')}
        </select>
      </label>
      <label class="inspector-field">
        <span>${t('suggestionsQuantity', '建议数量')}</span>
        <input type="number" id="s-suggest-count" min="1" max="10" value="${Math.max(1, Math.min(10, Number(data.suggestionsQuantity) || 4))}" />
      </label>
      <label class="inspector-field"><span>${t('style', '样式')}</span><select id="s-style">
        <option value="default" ${data.style === 'default' || !data.style ? 'selected' : ''}>${t('default', '默认')}</option>
        <option value="transparent-rounded" ${data.style === 'transparent-rounded' ? 'selected' : ''}>${t('rounded', '透明圆角')}</option>
        <option value="minimal-outlined" ${data.style === 'minimal-outlined' ? 'selected' : ''}>${t('outlined', '简洁描边')}</option>
      </select></label>
    `;
    const update = () => onChange({
      ...data,
      provider: container.querySelector('#s-provider').value,
      placeholder: container.querySelector('#s-ph').value.trim(),
      newTab: container.querySelector('#s-newtab').checked,
      suggestions: container.querySelector('#s-suggest').checked,
      suggestionsEngine: container.querySelector('#s-suggest-engine').value,
      suggestionsQuantity: Number(container.querySelector('#s-suggest-count').value) || 4,
      style: container.querySelector('#s-style').value,
    });
    container.querySelectorAll('select, input').forEach((el) => { el.onchange = update; });
    container.append(wrap);
  },
  styles: `.Search{min-width:200px;display:block;position:relative;}.Search .search-form{display:flex;align-items:center;width:100%;position:relative;}.Search input{width:100%;background-color:transparent;border:0;border-bottom:2px solid;font-family:inherit;font-size:1.1em;outline:none;padding:.15em 0;text-align:center;text-shadow:inherit;margin:1rem 0;}.Search .search-submit{background:none;border:0;outline:none;cursor:pointer;display:flex;align-items:center;justify-content:center;color:inherit;}.Search.style-transparent-rounded{display:flex;flex-direction:row;align-items:center;}.Search.style-transparent-rounded input{height:2.8rem;background:rgba(245,245,245,.1);border-radius:1.625rem;padding:0 3.5rem 0 1.5rem;font-size:1rem;text-align:left;backdrop-filter:blur(20px);border:1px solid rgba(255,255,255,.2);border-bottom:0;}.Search.style-transparent-rounded .search-submit{width:3.5rem;margin-left:-3.5rem;opacity:.7;}.Search.style-minimal-outlined{display:flex;flex-direction:row;align-items:center;}.Search.style-minimal-outlined input{height:2.2rem;border:1px solid rgba(255,255,255,.7);border-radius:4px;padding:0 2.5rem 0 .75rem;font-size:1rem;text-align:left;}.Search.style-minimal-outlined input:focus{border-color:#fff;}.Search.style-minimal-outlined .search-submit{width:2.5rem;margin-left:-2.5rem;opacity:.7;}.Search .Suggestions{display:grid;position:absolute;top:100%;left:0;right:0;z-index:10;margin-top:-.5rem;width:100%;background:rgba(0,0,0,.5);border-radius:0 0 1rem 1rem;overflow:hidden;}.Search .Suggestions .suggestion-item{text-align:left;display:flex;gap:.75em;padding:.5em 1em;cursor:pointer;width:100%;font-size:1.1rem;color:#fff;}.Search .Suggestions .suggestion-item:hover,.Search .Suggestions .suggestion-item.active{background:rgba(255,255,255,.2);}.Search .Suggestions .suggestion-thumb{width:30px;height:30px;object-fit:cover;border-radius:.25em;align-self:center;}.Search .Suggestions .suggestion-content{display:flex;flex-direction:column;justify-content:space-between;padding:.2em 0;min-width:0;}.Search .Suggestions .suggestion-title{white-space:normal;word-break:break-word;}.Search .Suggestions .suggestion-desc{font-size:.8em;color:#b0b0b0;white-space:normal;word-break:break-word;}.Search.style-transparent-rounded .Suggestions{border-radius:1.5rem;backdrop-filter:blur(10px);}.Search.style-minimal-outlined .Suggestions{border-radius:4px;background:rgba(20,20,20,.9);}`,
};
