

function escapeHtml(str) {
  return String(str || '').replace(/[&<>"']/g, (m) => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;'
  })[m]);
}

export function renderAuthFilesView(container, { files = [], quotaMap = {}, filter = {}, t, onAction }) {
  container.replaceChildren();

  const disabledCount = files.filter((f) => f.disabled).length;
  const totalCount = files.length;

  // 1. Top Header Actions Bar
  const topHeader = document.createElement('div');
  topHeader.className = 'model-af-topbar';
  topHeader.innerHTML = `
    <div class="model-af-stats">
      <span>${totalCount} ${t('filesCount', '个文件')} · ${disabledCount} ${t('disabledCount', '个停用')}</span>
    </div>
    <div class="model-af-actions">
      <button type="button" class="btn-af-action" data-action="auth-files-refresh">
        <svg class="icon" aria-hidden="true"><use href="#i-refresh" /></svg>
        <span>${t('refresh', '刷新')}</span>
      </button>
      <button type="button" class="btn-af-action" data-action="auth-files-open-dir">
        <svg class="icon" aria-hidden="true"><use href="#i-folder" /></svg>
        <span>${t('openFolder', '打开文件夹')}</span>
      </button>
      <label class="btn-af-action primary btn-af-import">
        <svg class="icon" aria-hidden="true"><use href="#i-download" /></svg>
        <span>${t('import', '导入')}</span>
        <input type="file" id="af-file-input" accept=".json,application/json" multiple hidden />
      </label>
    </div>
  `;
  container.append(topHeader);

  // 2. Search and Filters Bar
  const filterBar = document.createElement('div');
  filterBar.className = 'model-af-filterbar';
  filterBar.innerHTML = `
    <div class="model-af-search-wrap">
      <svg class="icon" aria-hidden="true"><use href="#i-search" /></svg>
      <input type="search" id="af-search-input" placeholder="${t('searchAuthFilesPlaceholder', '搜索文件名、账号或提供商')}" value="${escapeHtml(filter.query || '')}" />
    </div>
    <select id="af-provider-filter" class="model-af-select">
      <option value="all">${t('allProviders', '全部提供商')}</option>
      <option value="antigravity" ${filter.provider === 'antigravity' ? 'selected' : ''}>Antigravity</option>
      <option value="claude" ${filter.provider === 'claude' ? 'selected' : ''}>Claude</option>
      <option value="codex" ${filter.provider === 'codex' ? 'selected' : ''}>Codex</option>
      <option value="xai" ${filter.provider === 'xai' ? 'selected' : ''}>xAI</option>
      <option value="kimi" ${filter.provider === 'kimi' ? 'selected' : ''}>Kimi</option>
    </select>
    <select id="af-status-filter" class="model-af-select">
      <option value="all">${t('allStatus', '全部状态')}</option>
      <option value="active" ${filter.status === 'active' ? 'selected' : ''}>${t('modelAccountActive', '可用')}</option>
      <option value="disabled" ${filter.status === 'disabled' ? 'selected' : ''}>${t('disable', '停用')}</option>
    </select>
  `;
  container.append(filterBar);

  // Filter files
  const q = (filter.query || '').trim().toLowerCase();
  const filtered = files.filter((f) => {
    if (filter.provider && filter.provider !== 'all' && f.provider !== filter.provider) return false;
    if (filter.status && filter.status !== 'all') {
      if (filter.status === 'active' && f.disabled) return false;
      if (filter.status === 'disabled' && !f.disabled) return false;
    }
    if (q) {
      const matchName = f.name?.toLowerCase().includes(q);
      const matchAcc = f.account?.toLowerCase().includes(q);
      const matchProv = f.provider?.toLowerCase().includes(q);
      if (!matchName && !matchAcc && !matchProv) return false;
    }
    return true;
  });

  // 3. Files List
  const listContainer = document.createElement('div');
  listContainer.className = 'model-af-list';

  if (!filtered.length) {
    const empty = document.createElement('div');
    empty.className = 'model-empty-state';
    empty.innerHTML = `<strong>${t('noAuthFiles', '暂无认证文件')}</strong><p class="muted">${t('importAuthFilesHint', '点击“导入”上传 .json 凭据文件，或登录后自动保存。')}</p>`;
    listContainer.append(empty);
  } else {
    for (const file of filtered) {
      const credentialKey = file.accountId || file.name;
      const quota = quotaMap[credentialKey];
      const row = document.createElement('div');
      row.className = `model-af-row${file.disabled ? ' disabled' : ''}`;

      // Format date & size
      const dateStr = file.updatedAt ? new Date(file.updatedAt).toLocaleDateString(undefined, { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' }) : '';
      const sizeStr = file.size ? `${Math.round(file.size / 1024) || 1} KB` : '';

      row.innerHTML = `
        <div class="model-af-row-main">
          <div class="model-af-icon">
            <svg class="icon" aria-hidden="true"><use href="#i-box" /></svg>
          </div>
          <div class="model-af-info">
            <div class="model-af-title-line">
              <strong class="model-af-filename">${escapeHtml(file.name)}</strong>
              <span class="model-af-badge ${file.disabled ? 'disabled' : 'active'}">${file.disabled ? t('disable', '停用') : 'active'}</span>
            </div>
            <div class="model-af-meta-line">
              <span class="model-af-provider">${escapeHtml(file.provider)}</span>
              <span class="model-af-account">· ${escapeHtml(file.account)}</span>
            </div>
            ${renderQuotaPills(quota, t)}
          </div>
        </div>
        <div class="model-af-row-side">
          <div class="model-af-filemeta">
            <span>${sizeStr}</span>
            <span>${dateStr}</span>
          </div>
          <div class="model-af-row-actions">
            <button type="button" class="btn-af-tool" data-action="auth-files-quota-one" data-name="${escapeHtml(file.name)}" data-provider="${escapeHtml(file.provider)}" data-account-id="${escapeHtml(file.accountId)}">
              ${t('refreshQuota', '刷新额度')}
            </button>
            <button type="button" class="btn-af-tool" data-action="auth-files-models" data-name="${escapeHtml(file.name)}" data-account-id="${escapeHtml(file.accountId || '')}" data-provider="${escapeHtml(file.provider)}" title="${t('agentAccountModelsTitle', '账号模型')}">
              ${t('models', '模型')}
            </button>
            ${file.source !== 'keychain' ? `<button type="button" class="btn-af-tool" data-action="auth-files-priority" data-name="${escapeHtml(file.name)}" data-priority="${file.priority || 0}">
              <svg class="icon" aria-hidden="true"><use href="#i-pen" /></svg>
              <span>${t('priority', '优先级')} ${file.priority || 0}</span>
            </button>` : ''}
            <button type="button" class="btn-af-icon" data-action="auth-files-copy" data-name="${escapeHtml(file.name)}" title="${t('copyName', '复制文件名')}" aria-label="${t('copyName', '复制文件名')}">
              <svg class="icon" aria-hidden="true"><use href="#i-copy" /></svg>
            </button>
            ${file.source !== 'keychain' ? `<button type="button" class="btn-af-tool" data-action="auth-files-toggle" data-name="${escapeHtml(file.name)}" data-disabled="${String(!file.disabled)}">
              ${file.disabled ? t('enable', '启用') : t('disable', '停用')}
            </button>
            <button type="button" class="btn-af-icon danger" data-action="auth-files-delete" data-name="${escapeHtml(file.name)}" title="${t('delete', '删除')}" aria-label="${t('delete', '删除')}">
              <svg class="icon" aria-hidden="true"><use href="#i-trash" /></svg>
            </button>` : `<button type="button" class="btn-af-tool" data-action="toggle-account" data-account-id="${escapeHtml(file.accountId)}" data-enabled="${String(file.disabled)}">
              ${file.disabled ? t('enable', '启用') : t('disable', '停用')}
            </button>
            <button type="button" class="btn-af-icon danger" data-action="delete-account" data-account-id="${escapeHtml(file.accountId)}" data-provider="${escapeHtml(file.provider)}" title="${t('delete', '删除')}" aria-label="${t('delete', '删除')}">
              <svg class="icon" aria-hidden="true"><use href="#i-trash" /></svg>
            </button>`}
          </div>
        </div>
      `;
      listContainer.append(row);
    }
  }

  container.append(listContainer);

  // Wire search and file input
  const fileInput = topHeader.querySelector('#af-file-input');
  fileInput.onchange = async () => {
    if (!fileInput.files?.length) return;
    for (const file of Array.from(fileInput.files)) {
      try {
        const text = await file.text();
        onAction?.('auth-files-import-file', { name: file.name, content: text });
      } catch (err) {}
    }
    fileInput.value = '';
  };

  const searchInput = filterBar.querySelector('#af-search-input');
  searchInput.oninput = () => {
    onAction?.('auth-files-filter', { query: searchInput.value, provider: filter.provider, status: filter.status });
  };
  filterBar.querySelector('#af-provider-filter').onchange = (e) => {
    onAction?.('auth-files-filter', { query: filter.query, provider: e.target.value, status: filter.status });
  };
  filterBar.querySelector('#af-status-filter').onchange = (e) => {
    onAction?.('auth-files-filter', { query: filter.query, provider: filter.provider, status: e.target.value });
  };
}

function renderQuotaPills(quota, t) {
  if (!quota || !quota.windows || !quota.windows.length) return '';
  const pills = quota.windows.slice(0, 3).map((w) => {
    const pct = w.remainingPercent != null ? `${Math.round(w.remainingPercent)}%` : '--';
    const name = w.name.split('·')[0].trim();
    const reset = w.resetTime ? ` · ${formatResetLabel(w.resetTime, t)}` : '';
    return `<span class="model-af-quota-pill">${escapeHtml(name)} · ${escapeHtml(pct + reset)}</span>`;
  });
  return `<div class="model-af-quota-pills">${pills.join('')}</div>`;
}
import { formatResetLabel } from './model-quota-view.js';
