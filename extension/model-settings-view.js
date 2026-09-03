import { renderUsageView } from './model-usage-view.js';
import { renderAdvancedView } from './model-advanced-view.js';
import { renderGateway } from './model-gateway-view.js';

const OAUTH_LABELS = {
  codex: 'OpenAI Codex', claude: 'Anthropic Claude', antigravity: 'Google Gemini', kimi: 'Kimi', xai: 'xAI Grok',
};

export function createModelSettingsView({ t, onAction }) {
  let activePage = 'oauth';
  let activeUsageTab = 'overview';
  let activeAdvancedTab = 'basic';

  const dialog = document.createElement('dialog');
  dialog.className = 'model-settings-dialog';
  dialog.setAttribute('aria-labelledby', 'model-settings-title');
  dialog.innerHTML = `
    <div class="model-settings-shell">
      <aside class="model-settings-nav">
        <div class="model-settings-brand"><span aria-hidden="true">N</span><strong>Natives</strong></div>
        <button type="button" class="model-settings-back" data-action="close" data-role="close"></button>
        <div class="model-settings-nav-group">
          <span data-role="navGroup"></span>
          <nav class="model-settings-subnav" aria-label="Model settings">
            <button type="button" data-action="select-model-page" data-page="custom" data-role="customNav"></button>
            <button type="button" data-action="select-model-page" data-page="oauth" data-role="oauthNav"></button>
            <button type="button" data-action="select-model-page" data-page="gateway" data-role="gatewayNav"></button>
            <button type="button" data-action="select-model-page" data-page="usage" data-role="usageNav"></button>
            <button type="button" data-action="select-model-page" data-page="advanced" data-role="advancedNav"></button>
          </nav>
        </div>
      </aside>
      <main class="model-settings-workspace">
        <header class="model-settings-header">
          <div><h2 id="model-settings-title"></h2><p class="muted" data-role="subtitle"></p></div>
          <button type="button" data-action="refresh" data-role="refresh"></button>
        </header>
        <div class="model-settings-notice" data-role="notice" role="status" aria-live="polite" hidden></div>
        <div class="model-settings-pages">
          <section class="model-settings-page" data-page-panel="custom">
            <header class="model-page-heading"><div><h3 data-role="customTitle"></h3><p class="muted" data-role="customDescription"></p></div><button type="button" class="model-add-provider" data-action="new-provider"></button></header>
            <div class="model-settings-layout"><aside class="model-provider-pane"><div class="model-provider-scroll" data-role="providerList"></div></aside><div class="model-provider-detail" data-role="detail"></div></div>
          </section>
          <section class="model-settings-page" data-page-panel="oauth" hidden>
            <header class="model-page-heading"><div><h3 data-role="oauthTitle"></h3><p class="muted" data-role="oauthDescription"></p></div></header>
            <div class="model-oauth-table" data-role="oauthList"></div>
            <div class="model-oauth-detail" data-role="oauthDetail" hidden></div>
          </section>
          <section class="model-settings-page" data-page-panel="gateway" hidden>
            <header class="model-page-heading"><div><h3 data-role="gatewayTitle"></h3><p class="muted" data-role="gatewayDescription"></p></div></header>
            <div class="model-gateway-view" data-role="gateway"></div>
          </section>
          <section class="model-settings-page" data-page-panel="usage" hidden>
            <header class="model-page-heading"><div><h3 data-role="usageTitle"></h3><p class="muted" data-role="usageDescription"></p></div></header>
            <div class="model-usage-container" data-role="usageContainer"></div>
          </section>
          <section class="model-settings-page" data-page-panel="advanced" hidden>
            <header class="model-page-heading"><div><h3 data-role="advancedTitle"></h3><p class="muted" data-role="advancedDescription"></p></div></header>
            <div class="model-advanced-container" data-role="advancedContainer"></div>
          </section>
        </div>
      </main>
      <div class="model-settings-loading" data-role="loading" hidden></div>
      <div class="model-settings-error" data-role="error" hidden><p></p><button type="button" data-action="refresh"></button></div>
    </div>`;
  document.body.append(dialog);

  const roles = Object.fromEntries([...dialog.querySelectorAll('[data-role]')].map((node) => [node.dataset.role, node]));
  roles.refresh.textContent = t('modelRefresh', '刷新');
  roles.close.textContent = t('close', '关闭');
  roles.loading.textContent = t('modelLoading', '正在加载模型设置…');
  roles.error.querySelector('button').textContent = t('retry', '重试');

  dialog.addEventListener('click', (event) => {
    const action = event.target.closest('[data-action]');
    if (action) onAction(action.dataset.action, action, event);
  });
  dialog.addEventListener('change', (event) => {
    const action = event.target.matches('select[data-action]') ? event.target : null;
    if (action) onAction(action.dataset.action, action, event);
  });

  return {
    dialog,
    open() { if (!dialog.open) dialog.showModal(); },
    close() { dialog.close(); },
    setPage(page) {
      activePage = page;
      for (const item of dialog.querySelectorAll('[data-page]')) {
        const selected = item.dataset.page === activePage;
        item.classList.toggle('selected', selected);
        item.setAttribute('aria-current', selected ? 'page' : 'false');
      }
      for (const panel of dialog.querySelectorAll('[data-page-panel]')) panel.hidden = panel.dataset.pagePanel !== activePage;
    },
    setUsageTab(tab) { activeUsageTab = tab; },
    setAdvancedTab(tab) { activeAdvancedTab = tab; },
    get activePage() { return activePage; },
    get activeUsageTab() { return activeUsageTab; },
    get activeAdvancedTab() { return activeAdvancedTab; },
    setLoading(loading) { roles.loading.hidden = !loading; dialog.setAttribute('aria-busy', String(loading)); },
    showError(message) { roles.error.hidden = !message; roles.error.querySelector('p').textContent = message || ''; if (message) roles.notice.hidden = true; },
    showNotice(message) { roles.notice.hidden = !message; roles.notice.textContent = message || ''; },
    render(snapshot, selectedID, pendingOAuth, usageContext = {}) {
      roles.refresh.textContent = t('modelRefresh', '刷新');
      roles.close.textContent = t('close', '关闭');
      roles.loading.textContent = t('modelLoading', '正在加载模型设置…');
      roles.error.querySelector('button').textContent = t('retry', '重试');
      dialog.querySelector('.model-add-provider').textContent = `＋ ${t('modelAddProvider', '添加自定义供应商')}`;
      roles.close.textContent = `← ${t('modelBackToWorkspace', '返回工作区')}`;
      roles.navGroup.textContent = t('modelModelsAndServices', '模型与服务');
      roles.customNav.textContent = t('modelCustomModels', '自定义模型');
      roles.oauthNav.textContent = t('modelOAuthModels', 'OAuth 模型');
      roles.gatewayNav.textContent = t('modelLocalProxy', '本地代理');
      roles.usageNav.textContent = t('modelUsageRecords', '使用记录');
      roles.advancedNav.textContent = t('modelAdvancedSettings', '高级设置');

      dialog.querySelector('#model-settings-title').textContent = t('modelSettings', '模型与服务');
      roles.subtitle.textContent = t('modelSettingsDescription', '管理模型供应商配置，包括自定义模型、OAuth 模型和本地代理设置。');
      roles.customTitle.textContent = t('modelCustomModels', '自定义模型');
      roles.customDescription.textContent = t('modelCustomModelsDescription', '配置兼容接口、API Key 与可用模型。');
      roles.oauthTitle.textContent = t('modelOAuthModels', 'OAuth 模型');
      roles.oauthDescription.textContent = t('modelOAuthModelsDescription', '管理通过 OAuth 授权连接的模型供应商账户。');
      roles.gatewayTitle.textContent = t('modelLocalProxy', '本地代理');
      roles.gatewayDescription.textContent = t('modelLocalProxyDescription', '管理本地兼容接口、访问密钥与常驻状态。');
      roles.usageTitle.textContent = t('modelUsageRecords', '使用记录');
      roles.usageDescription.textContent = t('modelUsageRecordsDescription', '查看调用指标、Token 消耗、透视分析与费率定价。');
      roles.advancedTitle.textContent = t('modelAdvancedSettings', '高级设置');
      roles.advancedDescription.textContent = t('modelAdvancedSettingsDescription', '管理多访问密钥、调度路由负载、上游网络代理与数据迁移。');

      renderGateway(roles.gateway, snapshot, t);
      renderCustomProviderList(roles.providerList, snapshot, selectedID, t);
      const selectedCustom = snapshot.providers.find((provider) => provider.id === selectedID && provider.kind === 'custom');
      roles.detail.replaceChildren();
      if (selectedCustom) renderCustomDetail(roles.detail, selectedCustom, snapshot, t);
      else roles.detail.append(emptyState(t('modelNoCustomProviders', '尚未添加自定义供应商'), t('modelNoModelsHint', '添加供应商后即可配置和调用模型。')));

      renderOAuthOverview(roles.oauthList, snapshot, selectedID, pendingOAuth, t);
      const selectedOAuth = snapshot.providers.find((provider) => provider.id === selectedID && provider.kind === 'oauth');
      const hasOAuthDetail = selectedOAuth && (selectedOAuth.models.length || snapshot.accounts.some((account) => account.provider === selectedOAuth.oauthProvider));
      roles.oauthDetail.hidden = !hasOAuthDetail;
      if (hasOAuthDetail) renderOAuthDetail(roles.oauthDetail, selectedOAuth, snapshot, pendingOAuth, t);
      else roles.oauthDetail.replaceChildren();

      // Render Usage & Advanced
      renderUsageView(roles.usageContainer, {
        activeSubTab: activeUsageTab,
        overviewData: usageContext.overviewData,
        analyticsData: usageContext.analyticsData,
        eventsData: usageContext.eventsData,
        pricingData: usageContext.pricingData,
        currentFilter: usageContext.currentFilter,
        filterOptions: usageContext.filterOptions,
        t,
        onAction,
      });

      renderAdvancedView(roles.advancedContainer, {
        activeSubTab: activeAdvancedTab,
        snapshot,
        t,
        onAction,
      });

      this.setPage(activePage);
    },
    destroy() { dialog.remove(); },
  };
}

function renderCustomProviderList(container, snapshot, selectedID, t) {
  container.replaceChildren();
  container.append(element('h3', '', t('modelCustomProviders', '自定义供应商')));
  const custom = snapshot.providers.filter((item) => item.kind === 'custom');
  if (!custom.length) container.append(element('p', 'model-empty muted', t('modelNoCustomProviders', '尚未添加自定义供应商')));
  for (const provider of custom) container.append(providerButton(provider, selectedID, `${provider.models.length}`, provider.name));
}

function renderOAuthOverview(container, snapshot, selectedID, pendingOAuth, t) {
  container.replaceChildren();
  const header = element('div', 'model-oauth-row model-oauth-header');
  for (const [idx, label] of [t('modelProvider', '供应商'), t('modelConnectedAccounts', '已连接账户'), t('modelStatus', '状态'), t('modelActions', '操作')].entries()) {
    const colClasses = ['model-col-provider', 'model-col-accounts', 'model-col-status', 'model-col-actions'];
    header.append(element('span', colClasses[idx] || '', label));
  }
  container.append(header);
  for (const provider of snapshot.providers.filter((item) => item.kind === 'oauth')) {
    const accounts = snapshot.accounts.filter((account) => account.provider === provider.oauthProvider);
    const connected = accounts.filter((account) => account.status === 'active').length;
    const authorizing = pendingOAuth?.provider === provider.oauthProvider;
    const row = element('div', `model-oauth-row${provider.id === selectedID ? ' selected' : ''}`);
    const name = OAUTH_LABELS[provider.oauthProvider] || provider.name;
    const select = button('select-provider', name, 'model-oauth-provider model-col-provider');
    select.title = name;
    select.dataset.providerId = provider.id;
    const icon = element('span', 'model-provider-icon');
    icon.innerHTML = '<svg class="icon" aria-hidden="true"><use href="#i-box" /></svg>';
    select.prepend(icon);
    const accountsSpan = element('span', 'model-col-accounts', `${accounts.length}`);
    const status = element('span', connected ? 'model-oauth-state connected model-col-status' : 'model-oauth-state model-col-status', authorizing ? t('modelAuthorizing', '授权中') : connected ? t('modelAccountActive', '可用') : t('modelNotConnected', '未连接'));
    const action = button('oauth-start', authorizing ? t('modelCancelAuthorization', '取消授权') : t('modelAddAccount', '添加账户'), 'model-col-actions');
    action.dataset.provider = provider.oauthProvider;
    row.append(select, accountsSpan, status, action);
    container.append(row);
  }
}

function providerButton(provider, selectedID, badge, label) {
  const item = button('select-provider', '', `model-provider-item${provider.id === selectedID ? ' selected' : ''}`);
  item.setAttribute('aria-pressed', String(provider.id === selectedID));
  item.dataset.providerId = provider.id;
  item.title = label;
  const icon = element('span', 'model-provider-icon');
  icon.innerHTML = '<svg class="icon" aria-hidden="true"><use href="#i-box" /></svg>';
  item.append(icon, element('span', 'model-provider-name', label), element('span', 'model-provider-badge', badge));
  if (provider.enabled) item.append(element('span', 'model-provider-dot'));
  return item;
}

function renderOAuthDetail(container, provider, snapshot, pendingOAuth, t) {
  container.replaceChildren();
  if (!provider) return;
  const title = element('div', 'model-detail-heading');
  title.append(element('div', '', OAUTH_LABELS[provider.oauthProvider] || provider.name));
  title.append(button('oauth-start', pendingOAuth?.provider === provider.oauthProvider ? t('modelCancelAuthorization', '取消授权') : t('modelAddAccount', '添加账户'), 'primary'));
  title.querySelector('button').dataset.provider = provider.oauthProvider;
  container.append(title);
  const accounts = snapshot.accounts.filter((account) => account.provider === provider.oauthProvider);
  if (!accounts.length) container.append(emptyState(t('modelNoAccounts', '尚未连接账户'), t('modelNoAccountsHint', '点击“添加账户”并在浏览器中完成授权。')));
  for (const account of accounts) {
    const row = element('article', 'model-account-row');
    const main = element('div', 'model-account-main', account.label);
    main.title = account.label || '';
    row.append(main, element('span', `model-status model-status-${account.status}`, accountStatusLabel(account.status, t)));
    const actions = element('div', 'model-row-actions');
    const toggle = button('toggle-account', account.enabled ? t('disable', '禁用') : t('enable', '启用'));
    toggle.dataset.accountId = account.id; toggle.dataset.enabled = String(!account.enabled);
    const reauth = button('reauth-account', t('modelReauthorize', '重新授权')); reauth.dataset.accountId = account.id; reauth.dataset.provider = provider.oauthProvider;
    const remove = button('delete-account', t('delete', '删除'), 'danger'); remove.dataset.accountId = account.id;
    actions.append(toggle, reauth, remove); row.append(actions); container.append(row);
  }
  container.append(element('h3', 'model-list-title', t('modelList', '模型列表')));
  const catalog = element('div', 'model-list model-catalog-list');
  for (const model of provider.models) {
    const row = element('div', 'model-row model-catalog-row');
    const idEl = element('code', 'model-id', model.id);
    idEl.title = model.id;
    const nameEl = element('span', 'muted model-name', model.displayName || model.id);
    nameEl.title = model.displayName || model.id;
    const countEl = element('span', 'muted model-accounts-count', `${model.accountIds?.length || 0} ${t('modelAccounts', '账户')}`);
    row.append(idEl, nameEl, countEl);
    catalog.append(row);
  }
  if (!provider.models.length) catalog.append(emptyState(t('modelNoModels', '暂无模型'), t('modelOAuthModelsHint', '账户授权并启动代理后自动同步模型。')));
  container.append(catalog);
}

function renderCustomDetail(container, provider, snapshot, t) {
  container.replaceChildren();
  const form = document.createElement('form');
  form.className = 'model-provider-form';
  form.dataset.providerId = provider.id;
  form.innerHTML = `
    <div class="model-detail-heading"><div>${escapeText(provider.name)}</div><button type="button" class="danger" data-action="delete-provider">${escapeText(t('delete', '删除'))}</button></div>
    ${field(t('name', '名称'), 'name', provider.name, 'text', true)}
    ${field('Base URL', 'baseUrl', provider.baseUrl, 'url', true)}
    <label><span>${escapeText(t('modelAPIFormat', 'API 格式'))}</span><select name="protocol">${protocolOptions(provider.protocol)}</select></label>
    ${field('API Key', 'apiKey', '', 'password', false, provider.credentialMask || t('modelAPIKeyPlaceholder', '留空则保持现有密钥'))}
    <label class="model-checkbox"><input name="enabled" type="checkbox" ${provider.enabled ? 'checked' : ''}> ${escapeText(t('enable', '启用'))}</label>
    <label class="model-checkbox"><input name="allowLan" type="checkbox" ${provider.allowLan ? 'checked' : ''}> ${escapeText(t('modelAllowLAN', '允许局域网地址'))}</label>
    <div class="model-form-actions"><button type="submit" class="primary">${escapeText(t('save', '保存'))}</button><button type="button" data-action="test-provider">${escapeText(t('modelTestConnection', '测试连接'))}</button><button type="button" data-action="refresh-models">${escapeText(t('modelFetchModels', '自动获取模型'))}</button></div>`;
  form.addEventListener('submit', (event) => { event.preventDefault(); form.dispatchEvent(new CustomEvent('model-provider-submit', { bubbles: true, detail: Object.fromEntries(new FormData(form)) })); });
  container.append(form, element('h3', 'model-list-title', t('modelList', '模型列表')));
  const list = element('div', 'model-list model-custom-list');
  for (const model of provider.models) {
    const row = element('div', 'model-row model-custom-model-row');
    const identity = element('div', 'model-row-identity');
    const idEl = element('code', '', model.id);
    idEl.title = model.id;
    const nameEl = element('span', 'muted', model.displayName || model.id);
    nameEl.title = model.displayName || model.id;
    identity.append(idEl, nameEl);
    const metadata = element('span', 'muted model-row-meta', [model.alias, model.contextLength ? `${model.contextLength}` : ''].filter(Boolean).join(' · '));
    if (metadata.textContent) metadata.title = metadata.textContent;
    row.append(identity, metadata);
    const actions = element('div', 'model-row-actions');
    const edit = button('edit-model', t('edit', '编辑')); edit.dataset.modelId = model.id;
    const toggle = button('toggle-model', model.enabled ? t('disable', '禁用') : t('enable', '启用')); toggle.dataset.modelId = model.id; toggle.dataset.enabled = String(!model.enabled);
    const remove = button('delete-model', t('delete', '删除'), 'danger'); remove.dataset.modelId = model.id;
    actions.append(edit, toggle, remove); row.append(actions); list.append(row);
  }
  if (!provider.models.length) list.append(emptyState(t('modelNoModels', '暂无模型'), t('modelNoModelsHint', '自动获取，或使用下面的表单手动添加。')));
  list.append(modelForm(t)); container.append(list);
}

export function renderNewProvider(container, t) {
  container.innerHTML = `<form class="model-provider-form" data-new-provider>
    <div class="model-detail-heading"><div>${escapeText(t('modelAddProvider', '添加自定义供应商'))}</div></div>
    ${field(t('name', '名称'), 'name', '', 'text', true)}${field('Base URL', 'baseUrl', '', 'url', true)}
    <label><span>${escapeText(t('modelAPIFormat', 'API 格式'))}</span><select name="protocol">${protocolOptions('openai_chat')}</select></label>
    ${field('API Key', 'apiKey', '', 'password', false)}
    <label class="model-checkbox"><input name="enabled" type="checkbox" checked> ${escapeText(t('enable', '启用'))}</label>
    <label class="model-checkbox"><input name="allowLan" type="checkbox"> ${escapeText(t('modelAllowLAN', '允许局域网地址'))}</label>
    <div class="model-form-actions"><button type="submit" class="primary">${escapeText(t('modelAddProvider', '添加自定义供应商'))}</button><button type="button" data-action="test-new-provider">${escapeText(t('modelTestConnection', '测试连接'))}</button><button type="button" data-action="cancel-new-provider">${escapeText(t('cancel', '取消'))}</button></div>
  </form>`;
}

function modelForm(t) {
  const form = document.createElement('form'); form.className = 'model-add-form'; form.dataset.modelForm = '';
  form.innerHTML = `<input name="id" required maxlength="200" aria-label="${escapeText(t('modelID', '模型 ID'))}" placeholder="${escapeText(t('modelID', '模型 ID'))}"><input name="displayName" maxlength="200" aria-label="${escapeText(t('modelDisplayName', '显示名称'))}" placeholder="${escapeText(t('modelDisplayName', '显示名称'))}"><input name="alias" maxlength="200" aria-label="${escapeText(t('modelAlias', '别名'))}" placeholder="${escapeText(t('modelAlias', '别名'))}"><input name="contextLength" type="number" min="0" max="100000000" aria-label="${escapeText(t('modelContextLength', '上下文'))}" placeholder="${escapeText(t('modelContextLength', '上下文'))}"><button class="primary" data-role="model-submit">${escapeText(t('add', '添加'))}</button><button type="button" data-action="cancel-model-edit" hidden>${escapeText(t('cancel', '取消'))}</button>`;
  return form;
}

function field(label, name, value, type, required, placeholder = '') { return `<label><span>${escapeText(label)}</span><input name="${name}" type="${type}" value="${escapeText(value || '')}" ${required ? 'required' : ''} placeholder="${escapeText(placeholder)}"></label>`; }
function protocolOptions(selected) { return [['openai_chat', 'OpenAI Chat'], ['openai_responses', 'OpenAI Responses'], ['anthropic_messages', 'Anthropic Messages'], ['gemini', 'Gemini']].map(([value, label]) => `<option value="${value}" ${value === selected ? 'selected' : ''}>${label}</option>`).join(''); }
function button(action, label, className = '') { const node = document.createElement('button'); node.type = 'button'; node.dataset.action = action; node.className = className; node.textContent = label; return node; }
function element(tag, className = '', text = '') { const node = document.createElement(tag); node.className = className; node.textContent = text; return node; }
function emptyState(title, hint) { const node = element('div', 'model-empty-state'); node.append(element('strong', '', title), element('p', 'muted', hint)); return node; }
function accountStatusLabel(status, t) { return ({ active: t('modelAccountActive', '可用'), needs_reauth: t('modelAccountNeedsReauth', '需要重新登录'), failed: t('modelFailed', '异常') })[status] || status; }
function escapeText(value) { return String(value ?? '').replace(/[&<>'"]/g, (char) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', "'": '&#39;', '"': '&quot;' })[char]); }
