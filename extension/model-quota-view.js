/**
 * Model Quota View (<260 lines)
 * Grouped quota inquiry cards with double-cycle progress tracks matching EasyCLIProxyAPI design.
 */

function escapeHtml(str) {
  return String(str || '').replace(/[&<>"']/g, (m) => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;'
  })[m]);
}

const PROVIDER_NAMES = {
  antigravity: 'Antigravity',
  claude: 'Claude',
  codex: 'Codex',
  xai: 'xAI',
  kimi: 'Kimi',
};

export function renderQuotaView(container, { files = [], quotaMap = {}, t, onAction }) {
  container.replaceChildren();

  // Filter only active files
  const activeFiles = files.filter((f) => !f.disabled);
  const totalCount = activeFiles.length;

  // 1. Top Bar
  const topBar = document.createElement('div');
  topBar.className = 'model-quota-topbar';
  topBar.innerHTML = `
    <div class="model-quota-stats">
      <span>${totalCount} ${t('queryableCredentials', '个可查询凭据')}</span>
    </div>
    <div class="model-quota-actions">
      <button type="button" class="btn-quota-action" data-action="quota-read-list">
        <span>🔄 ${t('readList', '读列表')}</span>
      </button>
      <button type="button" class="btn-quota-action" data-action="quota-refresh-all">
        <span>🔄 ${t('refreshAll', '刷新全部')}</span>
      </button>
    </div>
  `;
  container.append(topBar);

  // Group files by provider
  const groups = {};
  for (const f of activeFiles) {
    const prov = f.provider || 'other';
    if (!groups[prov]) groups[prov] = [];
    groups[prov].push(f);
  }

  // 2. Provider Sections
  const content = document.createElement('div');
  content.className = 'model-quota-content';

  if (!activeFiles.length) {
    const empty = document.createElement('div');
    empty.className = 'model-empty-state';
    empty.innerHTML = `<strong>${t('noActiveCredentials', '暂无可查询凭据')}</strong><p class="muted">${t('quotaNoCredsHint', '在“认证文件”中导入凭据并保持启用后即可在此查询额度。')}</p>`;
    content.append(empty);
  } else {
    for (const [provider, fileList] of Object.entries(groups)) {
      const groupEl = document.createElement('div');
      groupEl.className = 'model-quota-group';

      const displayName = PROVIDER_NAMES[provider] || provider;

      const groupHeader = document.createElement('div');
      groupHeader.className = 'model-quota-group-header';
      groupHeader.innerHTML = `
        <div class="model-quota-group-title">
          <svg class="icon" aria-hidden="true"><use href="#i-box" /></svg>
          <strong>${escapeHtml(displayName)}</strong>
        </div>
        <span class="muted">${fileList.length} ${t('credentialsCount', '个凭据')}</span>
      `;
      groupEl.append(groupHeader);

      for (const file of fileList) {
        const quota = quotaMap[file.name] || {};
        const card = document.createElement('div');
        card.className = 'model-quota-card';

        card.innerHTML = `
          <div class="model-quota-card-header">
            <div class="model-quota-card-title">
              <strong>${escapeHtml(file.name)}</strong>
              <div class="model-quota-card-sub">
                <span>${escapeHtml(displayName)}</span>
                ${quota.plan ? `<span>· ${escapeHtml(quota.plan)}</span>` : ''}
              </div>
            </div>
            <button type="button" class="btn-quota-refresh-card icon-button" data-action="quota-refresh-one" data-name="${escapeHtml(file.name)}" data-provider="${escapeHtml(file.provider)}" title="${t('refreshQuota', '刷新额度')}">
              🔄
            </button>
          </div>
          <div class="model-quota-card-body">
            ${renderQuotaWindows(quota, file, t)}
          </div>
        `;
        groupEl.append(card);
      }

      content.append(groupEl);
    }
  }

  container.append(content);
}

function renderQuotaWindows(quota, file, t) {
  if (quota.status === 'loading') {
    return `<div class="model-quota-loading"><span>⏳ ${t('queryingQuota', '正在查询配额...')}</span></div>`;
  }
  if (quota.status === 'error') {
    return `<div class="model-quota-error"><span>❌ ${escapeHtml(quota.error || t('fetchFailed', '获取失败'))}</span></div>`;
  }
  if (!quota.windows || !quota.windows.length) {
    return `
      <div class="model-quota-empty">
        <span class="muted">${t('quotaNotFetched', '尚未获取额度')}</span>
        <button type="button" class="btn-af-tool" data-action="quota-refresh-one" data-name="${escapeHtml(file.name)}" data-provider="${escapeHtml(file.provider)}">
          ${t('getQuota', '获取额度')}
        </button>
      </div>
    `;
  }

  return quota.windows.map((w) => {
    const pct = w.remainingPercent != null ? Math.max(0, Math.min(100, Math.round(w.remainingPercent))) : null;
    const pctStr = pct != null ? `${t('remaining', '剩余')} ${pct}%` : '';
    const resetTime = w.resetTime ? formatResetTime(w.resetTime) : '';
    const modelsStr = (w.models || []).length ? `${t('modelsWithinGroup', 'Models within this group')}: ${w.models.join(', ')}` : '';

    return `
      <div class="model-quota-window">
        <div class="model-quota-window-header">
          <span class="model-quota-window-name">${escapeHtml(w.name)}</span>
          <strong class="model-quota-window-pct">${pctStr}</strong>
        </div>
        <div class="model-quota-progress-track">
          <div class="model-quota-progress-fill" style="width: ${pct != null ? pct : 0}%"></div>
        </div>
        <div class="model-quota-window-sub">
          ${modelsStr ? `<span class="muted">${escapeHtml(modelsStr)}</span>` : ''}
          ${resetTime ? `<span class="muted">· ${escapeHtml(resetTime)}</span>` : ''}
        </div>
      </div>
    `;
  }).join('');
}

function formatResetTime(isoStr) {
  try {
    const d = new Date(isoStr);
    if (isNaN(d.getTime())) return isoStr;
    return d.toLocaleDateString(undefined, { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' });
  } catch {
    return isoStr;
  }
}
