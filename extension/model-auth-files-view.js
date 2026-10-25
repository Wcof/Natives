import { formatResetLabel, dedupeQuotaWindows } from './model-quota-view.js';

function escapeHtml(str) {
  return String(str || '').replace(/[&<>"']/g, (m) => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;'
  })[m]);
}

export function renderAuthFilesView(container, { files = [], quotaMap = {}, t, onAction }) {
  container.replaceChildren();

  const disabledCount = files.filter((f) => f.disabled).length;
  const totalCount = files.length;

  const topHeader = document.createElement('div');
  topHeader.className = 'model-af-topbar';
  topHeader.innerHTML = `
    <div class="model-af-stats">
      <span>${totalCount} ${t('filesCount', '个凭据')} · ${disabledCount} ${t('disabledCount', '个停用')}</span>
    </div>
    <div class="model-af-actions">
      <button type="button" class="btn-af-action" data-action="quota-refresh-all">
        <svg class="icon" aria-hidden="true"><use href="#i-refresh" /></svg>
        <span>${t('refreshAll', '刷新全部额度')}</span>
      </button>
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

  const note = document.createElement('p');
  note.className = 'muted';
  note.textContent = t('modelQuotaCredentialNote', '凭据仅用于额度查询，不参与模型推理，也不会被写入第三方工具配置。');
  container.append(note);

  const listContainer = document.createElement('div');
  listContainer.className = 'model-af-list';

  if (!files.length) {
    const empty = document.createElement('div');
    empty.className = 'model-empty-state';
    empty.innerHTML = `<strong>${t('noAuthFiles', '暂无可查询凭据')}</strong><p class="muted">${t('importAuthFilesHint', '导入 .json 凭据文件，或保留既有 OAuth 账户后即可查询额度。')}</p>`;
    listContainer.append(empty);
  } else {
    for (const file of files) {
      const credentialKey = file.accountId || file.name;
      const quota = quotaMap[credentialKey];
      const row = document.createElement('div');
      row.className = `model-af-row${file.disabled ? ' disabled' : ''}`;

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
              ${file.source === 'keychain' ? `<span class="model-af-account">· ${t('modelLegacyReadOnly', '既有账户（只读）')}</span>` : ''}
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
            <button type="button" class="btn-af-icon" data-action="auth-files-copy" data-name="${escapeHtml(file.name)}" title="${t('copyName', '复制文件名')}" aria-label="${t('copyName', '复制文件名')}">
              <svg class="icon" aria-hidden="true"><use href="#i-copy" /></svg>
            </button>
            ${file.source !== 'keychain' ? `<button type="button" class="btn-af-tool" data-action="auth-files-toggle" data-name="${escapeHtml(file.name)}" data-disabled="${String(!file.disabled)}">
              ${file.disabled ? t('enable', '启用') : t('disable', '停用')}
            </button>
            <button type="button" class="btn-af-icon danger" data-action="auth-files-delete" data-name="${escapeHtml(file.name)}" title="${t('delete', '删除')}" aria-label="${t('delete', '删除')}">
              <svg class="icon" aria-hidden="true"><use href="#i-trash" /></svg>
            </button>` : ''}
          </div>
        </div>
      `;
      listContainer.append(row);
    }
  }

  container.append(listContainer);

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
}

function renderQuotaPills(quota, t) {
  if (quota && quota.status === 'error') {
    const message = quota.error || t('quotaQueryFailed', '额度查询失败');
    return `<div class="model-af-quota-pill model-af-quota-error" style="max-width:100%;white-space:normal;line-height:1.5;color:#f8a5a5;border-color:rgba(248,113,113,0.4)">${linkifyUrls(escapeHtml(message))}</div>`;
  }
  if (!quota || !quota.windows || !quota.windows.length) return '';
  const pills = dedupeQuotaWindows(quota.windows, t).slice(0, 3).map((w) => {
    const pct = w.remainingPercent == null ? '—' : `${Math.round(w.remainingPercent)}%`;
    const reset = w.resetTime ? ` · ${formatResetLabel(w.resetTime, t)}` : '';
    return `<span class="model-af-quota-pill">${escapeHtml(w.name)} · ${escapeHtml(pct + reset)}</span>`;
  });
  return `<div class="model-af-quota-pills">${pills.join('')}</div>`;
}

function linkifyUrls(escapedText) {
  return escapedText.replace(/(https?:\/\/[^\s（()<>]+)/g, (url) => `<a href="${url}" target="_blank" rel="noreferrer" style="color:#7dd3fc;text-decoration:underline">${url}</a>`);
}
