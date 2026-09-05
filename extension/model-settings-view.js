import { renderUsageView } from './model-usage-view.js';
import { renderAdvancedView } from './model-advanced-view.js';
import { renderGateway } from './model-gateway-view.js';
import { renderOAuthLoginView } from './model-oauth-login-view.js';
import { renderAuthFilesView } from './model-auth-files-view.js';
import { renderQuotaView } from './model-quota-view.js';
import { renderAgentView } from './model-agent-view.js';

export function createModelSettingsView({ t, onAction }) {
  let activePage = 'oauth';
  let activeOAuthTab = 'login'; // 'login' | 'authFiles' | 'quota'
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
            <button type="button" data-action="select-model-page" data-page="custom"><svg class="icon" aria-hidden="true"><use href="#i-pen" /></svg><span data-role="customNav"></span></button>
            <button type="button" data-action="select-model-page" data-page="oauth"><svg class="icon" aria-hidden="true"><use href="#i-globe" /></svg><span data-role="oauthNav"></span></button>
            <button type="button" data-action="select-model-page" data-page="gateway"><svg class="icon" aria-hidden="true"><use href="#i-bolt" /></svg><span data-role="gatewayNav"></span></button>
            <button type="button" data-action="select-model-page" data-page="usage"><svg class="icon" aria-hidden="true"><use href="#i-list" /></svg><span data-role="usageNav"></span></button>
            <button type="button" data-action="select-model-page" data-page="agent"><svg class="icon" aria-hidden="true"><use href="#i-target" /></svg><span data-role="agentNav"></span></button>
            <button type="button" data-action="select-model-page" data-page="advanced"><svg class="icon" aria-hidden="true"><use href="#i-gear" /></svg><span data-role="advancedNav"></span></button>
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
            <header class="model-page-heading">
              <div>
                <div class="model-oauth-tabs-nav" role="tablist">
                  <button type="button" class="model-oauth-tab-btn selected" data-action="select-oauth-tab" data-tab="login" role="tab" aria-selected="true" data-role="tabLogin">OAuth 登录</button>
                  <button type="button" class="model-oauth-tab-btn" data-action="select-oauth-tab" data-tab="authFiles" role="tab" aria-selected="false" data-role="tabAuthFiles">认证文件</button>
                  <button type="button" class="model-oauth-tab-btn" data-action="select-oauth-tab" data-tab="quota" role="tab" aria-selected="false" data-role="tabQuota">额度查询</button>
                </div>
                <div class="model-oauth-header-text">
                  <span class="model-oauth-section-tag" data-role="oauthSectionTag">OAUTH</span>
                  <h3 data-role="oauthTitle"></h3>
                </div>
              </div>
            </header>
            <div class="model-oauth-panel" data-oauth-panel="login">
              <div class="model-oauth-login-container" data-role="oauthLoginContainer"></div>
            </div>
            <div class="model-oauth-panel" data-oauth-panel="authFiles" hidden>
              <div class="model-auth-files-container" data-role="authFilesContainer"></div>
            </div>
            <div class="model-oauth-panel" data-oauth-panel="quota" hidden>
              <div class="model-quota-container" data-role="quotaContainer"></div>
            </div>
          </section>
          <section class="model-settings-page" data-page-panel="gateway" hidden>
            <header class="model-page-heading"><div><h3 data-role="gatewayTitle"></h3><p class="muted" data-role="gatewayDescription"></p></div></header>
            <div class="model-gateway-view" data-role="gateway"></div>
          </section>
          <section class="model-settings-page" data-page-panel="usage" hidden>
            <header class="model-page-heading"><div><h3 data-role="usageTitle"></h3><p class="muted" data-role="usageDescription"></p></div></header>
            <div class="model-usage-container" data-role="usageContainer"></div>
          </section>
          <section class="model-settings-page" data-page-panel="agent" hidden>
            <header class="model-page-heading"><div><span class="model-oauth-section-tag">AGENT CLIENTS</span><h3 data-role="agentTitle"></h3></div></header>
            <div class="model-agent-container" data-role="agentContainer"></div>
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
    setOAuthTab(tab) {
      activeOAuthTab = tab;
      for (const btn of dialog.querySelectorAll('[data-action="select-oauth-tab"]')) {
        const selected = btn.dataset.tab === activeOAuthTab;
        btn.classList.toggle('selected', selected);
        btn.setAttribute('aria-selected', String(selected));
      }
      for (const panel of dialog.querySelectorAll('[data-oauth-panel]')) {
        panel.hidden = panel.dataset.oauthPanel !== activeOAuthTab;
      }
      // Update section tag & heading
      if (roles.oauthSectionTag) {
        roles.oauthSectionTag.textContent = activeOAuthTab === 'login' ? 'OAUTH' : activeOAuthTab === 'authFiles' ? 'AUTH FILES' : 'QUOTA';
      }
      if (roles.oauthTitle) {
        roles.oauthTitle.textContent = activeOAuthTab === 'login' ? t('modelOAuthLogin', 'OAuth 登录') : activeOAuthTab === 'authFiles' ? t('modelAuthFiles', '认证文件') : t('modelQuotaInquiry', '额度查询');
      }
    },
    get activeOAuthTab() { return activeOAuthTab; },
    setUsageTab(tab) { activeUsageTab = tab; },
    setAdvancedTab(tab) { activeAdvancedTab = tab; },
    get activePage() { return activePage; },
    get activeUsageTab() { return activeUsageTab; },
    get activeAdvancedTab() { return activeAdvancedTab; },
    setLoading(loading) { roles.loading.hidden = !loading; dialog.setAttribute('aria-busy', String(loading)); },
    showError(message) { roles.error.hidden = !message; roles.error.querySelector('p').textContent = message || ''; if (message) roles.notice.hidden = true; },
    showNotice(message) { roles.notice.hidden = !message; roles.notice.textContent = message || ''; },
    showToast(message) {
      if (!message) return;
      let toast = dialog.querySelector('.model-toast');
      if (!toast) {
        toast = document.createElement('div');
        toast.className = 'model-toast';
        toast.setAttribute('role', 'status');
        toast.setAttribute('aria-live', 'polite');
        dialog.append(toast);
      }
      toast.textContent = message;
      toast.classList.add('visible');
      clearTimeout(showToast._timer);
      showToast._timer = setTimeout(() => toast.classList.remove('visible'), 2400);
    },
    render(snapshot, selectedID, pendingOAuth, usageContext = {}, oauthContext = {}) {
      roles.refresh.textContent = t('modelRefresh', '刷新');
      roles.close.textContent = t('close', '关闭');
      roles.loading.textContent = t('modelLoading', '正在加载模型设置…');
      roles.error.querySelector('button').textContent = t('retry', '重试');
      dialog.querySelector('.model-add-provider').textContent = `＋ ${t('modelAddProvider', '添加自定义供应商')}`;
      roles.close.textContent = `← ${t('modelBackToWorkspace', '返回工作区')}`;
      roles.navGroup.textContent = t('modelModelsAndServices', '模型与服务');
      roles.customNav.textContent = t('modelCustomModels', '自定义模型');
      roles.oauthNav.textContent = 'OAuth';
      roles.gatewayNav.textContent = t('modelLocalProxy', '本地代理');
      roles.usageNav.textContent = t('modelUsageRecords', '使用记录');
      roles.agentNav.textContent = t('modelAgentSettings', '智能体配置');
      roles.advancedNav.textContent = t('modelAdvancedSettings', '高级设置');

      dialog.querySelector('#model-settings-title').textContent = t('modelSettings', '模型与服务');
      roles.subtitle.textContent = t('modelSettingsDescription', '管理模型供应商配置，包括自定义模型、OAuth 和本地代理设置。');
      roles.customTitle.textContent = t('modelCustomModels', '自定义模型');
      roles.customDescription.textContent = t('modelCustomModelsDescription', '配置兼容接口、API Key 与可用模型。');
      roles.gatewayTitle.textContent = t('modelLocalProxy', '本地代理');
      roles.gatewayDescription.textContent = t('modelLocalProxyDescription', '管理本地兼容接口、访问密钥与常驻状态。');
      roles.usageTitle.textContent = t('modelUsageRecords', '使用记录');
      roles.usageDescription.textContent = t('modelUsageRecordsDescription', '查看调用指标、Token 消耗、透视分析与费率定价。');
      roles.agentTitle.textContent = t('modelAgentSettings', '智能体配置');
      roles.advancedTitle.textContent = t('modelAdvancedSettings', '高级设置');
      roles.advancedDescription.textContent = t('modelAdvancedSettingsDescription', '管理多访问密钥、调度路由负载、上游网络代理与数据迁移。');

      renderGateway(roles.gateway, snapshot, t);
      renderCustomProviderList(roles.providerList, snapshot, selectedID, t);
      const selectedCustom = snapshot.providers.find((provider) => provider.id === selectedID && provider.kind === 'custom');
      roles.detail.replaceChildren();
      if (selectedCustom) renderCustomDetail(roles.detail, selectedCustom, snapshot, t);
      else roles.detail.append(emptyState(t('modelNoCustomProviders', '尚未添加自定义供应商'), t('modelNoModelsHint', '添加供应商后即可配置和调用模型。')));

      // Render 3 OAuth Sub-views
      renderOAuthLoginView(roles.oauthLoginContainer, { snapshot, pendingOAuth, results: oauthContext.results, t });
      renderAuthFilesView(roles.authFilesContainer, {
        files: oauthContext.authFiles || [],
        quotaMap: oauthContext.quotaMap || {},
        filter: oauthContext.authFileFilter || {},
        t,
        onAction,
      });
      renderQuotaView(roles.quotaContainer, {
        files: oauthContext.authFiles || [],
        quotaMap: oauthContext.quotaMap || {},
        t,
        onAction,
      });

      this.setOAuthTab(activeOAuthTab);

      // Render Usage & Agent & Advanced
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

      renderAgentView(roles.agentContainer, {
        clients: usageContext.agentStatuses || [],
        selectedId: usageContext.agentSelectedId || '',
        models: usageContext.agentModels ?? null,
        modelsError: usageContext.agentModelsError || '',
        selection: usageContext.agentSelections || {},
        busy: usageContext.agentBusy || false,
        activeTab: usageContext.agentActiveTab || 'core',
        sessions: usageContext.agentSessions || [],
        loadError: usageContext.agentLoadError || '',
        detecting: Boolean(usageContext.agentDetecting),
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
function escapeText(value) { return String(value ?? '').replace(/[&<>'"]/g, (char) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', "'": '&#39;', '"': '&quot;' })[char]); }
