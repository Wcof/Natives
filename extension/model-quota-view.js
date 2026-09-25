

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

// Google 额度端点返回英文窗口名（Weekly Limit Remaining 等）；
// 通过 i18n 组件按当前语言映射，未识别的名称保留原文
const quotaLabelRules = [
  { match: /weekly/i, key: 'quotaWindowWeekly', fallback: '周额度' },
  { match: /(five hour|5-hour|5 hour)/i, key: 'quotaWindowFiveHour', fallback: '5小时额度' },
  { match: /daily/i, key: 'quotaWindowDaily', fallback: '日额度' },
  { match: /monthly/i, key: 'quotaWindowMonthly', fallback: '月额度' },
];

function localizedQuotaType(name, t) {
  const raw = String(name || '');
  for (const rule of quotaLabelRules) {
    if (rule.match.test(raw)) return t(rule.key, rule.fallback);
  }
  return raw;
}

// Antigravity 的模型组名（Gemini Models / Claude and GPT models 各自独立额度）
function localizedQuotaGroup(group, t) {
  const raw = String(group || '').trim();
  if (!raw) return '';
  const lower = raw.toLowerCase();
  if (lower === 'gemini models') return t('quotaGroupGemini', 'Gemini 模型');
  if (lower.includes('claude') && lower.includes('gpt')) return t('quotaGroupClaudeGPT', 'Claude 与 GPT 模型');
  return raw;
}

// 组装为「模型组 · 窗口类型」标签；两个模型组各自拥有独立的周/5小时窗口，
// 严禁按窗口名去重折叠（那会把两个模型组并成一个额度）
export function localizedQuotaLabel(w, t) {
  const type = localizedQuotaType(w && w.name, t);
  const group = localizedQuotaGroup(w && w.group, t);
  return group ? `${group} · ${type}` : type;
}

// 去重键 = 模型组 + 窗口类型（仅折叠完全相同的行），保留约束最紧（剩余最少）的一行
export function dedupeQuotaWindows(windows, t) {
  const byKey = new Map();
  for (const w of windows || []) {
    const label = localizedQuotaLabel(w, t);
    const prev = byKey.get(label);
    if (!prev || (w.remainingPercent != null && (prev.remainingPercent == null || w.remainingPercent < prev.remainingPercent))) {
      byKey.set(label, { ...w, name: label });
    }
  }
  return [...byKey.values()];
}

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
        <svg class="icon" aria-hidden="true"><use href="#i-refresh" /></svg>
        <span>${t('readList', '读列表')}</span>
      </button>
      <button type="button" class="btn-quota-action" data-action="quota-refresh-all">
        <svg class="icon" aria-hidden="true"><use href="#i-refresh" /></svg>
        <span>${t('refreshAll', '刷新全部')}</span>
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
        const quota = quotaMap[file.accountId || file.name] || {};
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
            <button type="button" class="btn-quota-refresh-card icon-button" data-action="quota-refresh-one" data-name="${escapeHtml(file.name)}" data-provider="${escapeHtml(file.provider)}" data-account-id="${escapeHtml(file.accountId)}" title="${t('refreshQuota', '刷新额度')}" aria-label="${t('refreshQuota', '刷新额度')}">
              <svg class="icon" aria-hidden="true"><use href="#i-refresh" /></svg>
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
    return `<div class="model-quota-loading"><svg class="icon spinning" aria-hidden="true"><use href="#i-refresh" /></svg><span>${t('queryingQuota', '正在查询配额...')}</span></div>`;
  }
  if (quota.status === 'error') {
    return `<div class="model-quota-error"><span>${escapeHtml(quota.error || t('fetchFailed', '获取失败'))}</span></div>`;
  }
  if (!quota.windows || !quota.windows.length) {
    return `
      <div class="model-quota-empty">
        <span class="muted">${t('quotaNotFetched', '尚未获取额度')}</span>
        <button type="button" class="btn-af-tool" data-action="quota-refresh-one" data-name="${escapeHtml(file.name)}" data-provider="${escapeHtml(file.provider)}" data-account-id="${escapeHtml(file.accountId)}">
          ${t('getQuota', '获取额度')}
        </button>
      </div>
    `;
  }

  return dedupeQuotaWindows(quota.windows, t).map((w) => {
    const pct = w.remainingPercent != null ? Math.max(0, Math.min(100, Math.round(w.remainingPercent))) : null;
    const pctStr = pct != null ? `${t('remaining', '剩余')} ${pct}%` : '';
    const resetTime = w.resetTime ? formatResetLabel(w.resetTime, t) : '';
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

export function formatResetLabel(value, t, now = Date.now()) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return String(value);
  const minutes = Math.max(0, Math.ceil((date.getTime() - now) / 60_000));
  const hours = Math.floor(minutes / 60);
  const remainder = minutes % 60;
  const duration = [
    hours ? `${hours} ${t('quotaHours', '小时')}` : '',
    remainder || !hours ? `${remainder} ${t('quotaMinutes', '分钟')}` : '',
  ].filter(Boolean).join(' ');
  const relative = t('quotaResetsIn', '$1 后重置').replace('$1', duration);
  const absolute = date.toLocaleString(undefined, { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' });
  return `${relative} · ${absolute}`;
}
