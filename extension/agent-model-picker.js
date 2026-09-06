

export function createAgentModelPicker({ value, models, t, onChange, onRefresh }) {
  const root = document.createElement('div');
  root.className = 'agent-model-picker';
  root.innerHTML = `
    <div class="agent-model-picker-input-wrap">
      <div class="agent-model-picker-field">
        <svg class="icon agent-model-search-icon" aria-hidden="true"><use href="#i-target" /></svg>
        <input type="text" class="agent-model-picker-input" placeholder="${t('agentModelSearchPlaceholder', '搜索或选择模型')}" autocomplete="off" />
        <button type="button" class="agent-model-chevron" tabindex="-1" aria-hidden="true">
          <svg class="icon" aria-hidden="true"><use href="#i-chevron-down" /></svg>
        </button>
      </div>
      <button type="button" class="model-icon-button agent-model-refresh" title="${t('modelFetchModels', '自动获取模型')}" aria-label="${t('modelFetchModels', '自动获取模型')}"><svg class="icon" aria-hidden="true"><use href="#i-refresh" /></svg></button>
    </div>
    <div class="agent-model-picker-list" role="listbox" hidden></div>
    <p class="muted agent-model-picker-footer"></p>
  `;
  const input = root.querySelector('.agent-model-picker-input');
  const list = root.querySelector('.agent-model-picker-list');
  const footer = root.querySelector('.agent-model-picker-footer');
  let open = false;
  let activeIndex = -1;
  let filtered = models;

  const syncFooter = () => {
    footer.textContent = `${models.length} ${t('agentModelsHint', '个可用模型，首次默认选择第一项')}`;
  };
  syncFooter();

  const renderList = () => {
    list.replaceChildren();
    for (const model of filtered) {
      const row = document.createElement('button');
      row.type = 'button';
      row.className = `agent-model-option${model.name === value ? ' selected' : ''}`;
      row.setAttribute('role', 'option');
      row.setAttribute('aria-selected', String(model.name === value));
      row.innerHTML = `
        <span class="agent-model-option-left">
          <span class="agent-model-option-dot" aria-hidden="true"></span>
          <span class="agent-model-option-name">${escapeHtml(model.name)}</span>
        </span>
        <span class="agent-model-option-right">
          ${model.alias && model.alias !== model.name ? `<small class="muted">${escapeHtml(model.alias)}</small>` : ''}
          ${model.name === value ? '<svg class="icon icon-check" aria-hidden="true" style="width:13px;height:13px;margin-left:6px;"><use href="#i-check" /></svg>' : ''}
        </span>
      `;
      row.onclick = () => {
        commit(model.name);
      };
      list.append(row);
    }
    if (!filtered.length) {
      list.innerHTML = `<div class="agent-model-picker-empty muted">${t('agentModelNoMatch', '没有匹配的模型')}</div>`;
    }
    activeIndex = filtered.findIndex((model) => model.name === value);
  };


  const commit = (name) => {
    input.value = name;
    close();
    if (name !== value) onChange(name);
  };

  const openList = () => {
    open = true;
    list.hidden = false;
    renderList();
  };
  const close = () => {
    open = false;
    list.hidden = true;
  };

  const filter = (query) => {
    const q = query.trim().toLowerCase();
    filtered = !q ? models : models.filter((model) => model.name.toLowerCase().includes(q) || (model.alias || '').toLowerCase().includes(q));
    renderList();
    if (!open) openList();
  };

  input.onfocus = () => { filter(input.value); };
  input.oninput = () => filter(input.value);
  input.onkeydown = (event) => {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      if (!open) openList();
      const step = event.key === 'ArrowDown' ? 1 : -1;
      activeIndex = (activeIndex + step + filtered.length) % Math.max(1, filtered.length);
      for (const [index, row] of [...list.children].entries()) row.classList.toggle('active', index === activeIndex);
    } else if (event.key === 'Enter') {
      event.preventDefault();
      const picked = filtered[activeIndex >= 0 ? activeIndex : 0];
      if (picked) commit(picked.name);
    } else if (event.key === 'Escape') {
      close();
    }
  };
  input.onblur = () => { setTimeout(close, 150); };
  root.querySelector('.agent-model-refresh').onclick = (event) => {
    event.stopPropagation();
    onRefresh?.();
  };
  root.addEventListener('click', (event) => event.stopPropagation());
  document.addEventListener('click', () => close());

  return root;
}

function escapeHtml(value) {
  return String(value ?? '').replace(/[&<>"']/g, (match) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[match]);
}
