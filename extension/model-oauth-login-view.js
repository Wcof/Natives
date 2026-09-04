/**
 * Model OAuth Login View (<180 lines)
 * Renders OAuth login provider cards and browser selection bar matching EasyCLIProxyAPI design.
 */

const OAUTH_CARD_PROVIDERS = [
  {
    key: 'codex',
    name: 'Codex OAuth',
    iconColor: '#3b82f6',
    desc: '点击开始登录后，将按上方选择打开浏览器；选择“不自动打开”时仅生成登录链接',
    svgIcon: `<svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="2"><polyline points="4 17 10 11 4 5"></polyline><line x1="12" y1="19" x2="20" y2="19"></line></svg>`,
  },
  {
    key: 'claude',
    name: 'Claude OAuth',
    iconColor: '#d97706',
    desc: '点击开始登录后，将按上方选择打开浏览器；选择“不自动打开”时仅生成登录链接',
    svgIcon: `<svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="4"></circle><path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41"></path></svg>`,
  },
  {
    key: 'antigravity',
    name: 'Antigravity OAuth',
    iconColor: '#2563eb',
    desc: '点击开始登录后，将按上方选择打开浏览器；选择“不自动打开”时仅生成登录链接',
    svgIcon: `<svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 2L2 22h20L12 2z"></path></svg>`,
  },
  {
    key: 'kimi',
    name: 'Kimi OAuth',
    iconColor: '#000000',
    desc: '点击开始登录后，将按上方选择打开浏览器；选择“不自动打开”时仅生成登录链接',
    svgIcon: `<svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="3" width="18" height="18" rx="2"></rect><path d="M9 8v8M15 8l-6 8M15 16l-3.5-4.5"></path></svg>`,
  },
  {
    key: 'xai',
    name: 'xAI OAuth',
    iconColor: '#111827',
    desc: '点击开始登录后，将按上方选择打开浏览器；选择“不自动打开”时仅生成登录链接',
    svgIcon: `<svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="9"></circle><path d="M3.6 9h16.8M3.6 15h16.8M11.5 3a17 17 0 0 0 0 18M12.5 3a17 17 0 0 1 0 18"></path></svg>`,
  },
];

export function renderOAuthLoginView(container, { snapshot, pendingOAuth, t }) {
  container.replaceChildren();

  // 1. Top Bar: Browser choice
  const topBar = document.createElement('div');
  topBar.className = 'model-oauth-browser-bar';
  topBar.innerHTML = `
    <div class="model-oauth-browser-wrap">
      <span class="model-oauth-browser-label">${t('chooseBrowser', '选择浏览器')}</span>
      <select id="oauth-browser-select" class="model-oauth-browser-select">
        <option value="default">${t('systemDefaultBrowser', '系统默认浏览器')}</option>
        <option value="chrome">Google Chrome</option>
        <option value="edge">Microsoft Edge</option>
        <option value="safari">Safari</option>
        <option value="none">${t('doNotAutoOpen', '不自动打开 (仅生成链接)')}</option>
      </select>
    </div>
    <span class="model-oauth-browser-hint">${t('browserSelectionHint', '已自动选择可用浏览器，并会记住你的选择')}</span>
  `;
  container.append(topBar);

  // 2. Card Grid
  const grid = document.createElement('div');
  grid.className = 'model-oauth-grid';

  for (const card of OAUTH_CARD_PROVIDERS) {
    const isPending = pendingOAuth?.provider === card.key;
    const cardEl = document.createElement('div');
    cardEl.className = `model-oauth-card${isPending ? ' pending' : ''}`;
    cardEl.innerHTML = `
      <div class="model-oauth-card-header">
        <div class="model-oauth-card-icon" style="color:${card.iconColor};">
          ${card.svgIcon}
        </div>
        <strong class="model-oauth-card-name">${card.name}</strong>
      </div>
      <p class="model-oauth-card-desc">${card.desc}</p>
      <div class="model-oauth-card-action">
        <button type="button" class="model-oauth-login-btn primary" data-action="oauth-start" data-provider="${card.key}">
          ${isPending ? `⏳ ${t('authorizing', '授权中...')}` : `→] ${t('startLogin', '开始登录')}`}
        </button>
      </div>
    `;
    grid.append(cardEl);
  }

  container.append(grid);
}
