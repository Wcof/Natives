import { renderUsageView } from './model-usage-view.js';
import { renderAuthFilesView } from './model-auth-files-view.js';

export function createModelSettingsView({ t, onAction }) {
  let activePage = 'custom';
  let activeUsageTab = 'overview';
  let toastTimer = null;
  let noticeTimer = null;
  let loadingFocus = null;

  const dialog = document.createElement('dialog');
  dialog.className = 'model-settings-dialog';
  dialog.setAttribute('aria-labelledby', 'model-settings-title');

  const triggerToast = (message) => {
    if (!message) return;
    try {
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
      clearTimeout(toastTimer);
      toastTimer = setTimeout(() => toast.classList.remove('visible'), 2400);
    } catch (e) {
      console.warn('Toast display error:', e);
    }
  };

  if (typeof globalThis.showToast !== 'function') {
    globalThis.showToast = triggerToast;
  }
  dialog.innerHTML = `
    <div class="model-settings-shell">
      <aside class="model-settings-nav">
        <div class="model-settings-brand"><span aria-hidden="true">N</span><strong>Natives</strong></div>
        <button type="button" class="model-settings-back" data-action="close" data-role="close"></button>
        <div class="model-settings-nav-group">
          <span data-role="appsNavGroup"></span>
          <nav class="model-settings-subnav" aria-label="Apps Center">
            <button type="button" data-action="select-model-page" data-page="apps"><svg class="icon" aria-hidden="true"><use href="#i-grid" /></svg><span data-role="appsListNav"></span></button>
          </nav>
        </div>
        <div class="model-settings-nav-group">
          <span data-role="navGroup"></span>
          <nav class="model-settings-subnav" aria-label="AI settings">
            <button type="button" data-action="select-model-page" data-page="custom"><svg class="icon" aria-hidden="true"><use href="#i-pen" /></svg><span data-role="customNav"></span></button>
            <button type="button" data-action="select-model-page" data-page="usage"><svg class="icon" aria-hidden="true"><use href="#i-list" /></svg><span data-role="usageNav"></span></button>
            <button type="button" data-action="select-model-page" data-page="quota"><svg class="icon" aria-hidden="true"><use href="#i-globe" /></svg><span data-role="quotaNav"></span></button>
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
          <section class="model-settings-page" data-page-panel="apps" hidden>
            <header class="model-page-heading">
              <div>
                <h3 data-role="appsTitle"></h3>
                <p class="muted" data-role="appsDescription"></p>
              </div>
            </header>
            <div class="model-apps-container" data-role="appsContainer">
              <div class="apps-list" data-role="appsList" role="list"></div>
              <div id="app-stage" hidden></div>
            </div>
          </section>
          <section class="model-settings-page" data-page-panel="custom">
            <header class="model-page-heading"><div><h3 data-role="customTitle"></h3><p class="muted" data-role="customDescription"></p></div><button type="button" class="model-add-provider" data-action="new-provider" disabled></button></header>
            <div class="model-retirement-notice" data-role="retirementNotice" role="note"></div>
            <div data-role="defaultModel"></div>
            <label class="model-checkbox model-resident-toggle"><input type="checkbox" data-action="resident" data-role="resident"> <span data-role="residentLabel"></span></label>
            <div class="model-settings-layout"><aside class="model-provider-pane"><div class="model-provider-scroll" data-role="providerList"></div></aside><div class="model-provider-detail" data-role="detail"></div></div>
          </section>
          <section class="model-settings-page" data-page-panel="usage" hidden>
            <header class="model-page-heading"><div><h3 data-role="usageTitle"></h3><p class="muted" data-role="usageDescription"></p></div></header>
            <div class="model-usage-container" data-role="usageContainer"></div>
          </section>
          <section class="model-settings-page" data-page-panel="quota" hidden>
            <header class="model-page-heading"><div><h3 data-role="quotaTitle"></h3><p class="muted" data-role="quotaDescription"></p></div></header>
            <div class="model-auth-files-container" data-role="authFilesContainer"></div>
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
    if (action && !action.matches('select[data-action], input[type="checkbox"][data-action]')) onAction(action.dataset.action, action, event);
  });
  dialog.addEventListener('change', (event) => {
    const action = event.target.matches('select[data-action], input[type="checkbox"][data-action]') ? event.target : null;
    if (action) onAction(action.dataset.action, action, event);
  });

  return {
    dialog,
    open() { if (!dialog.open) dialog.showModal(); },
    close() {
      clearTimeout(noticeTimer);
      clearTimeout(toastTimer);
      dialog.close();
    },
    setPage(page) {
      activePage = ['apps', 'custom', 'usage', 'quota'].includes(page) ? page : 'custom';
      for (const item of dialog.querySelectorAll('[data-page]')) {
        const selected = item.dataset.page === activePage;
        item.classList.toggle('selected', selected);
        item.setAttribute('aria-current', selected ? 'page' : 'false');
      }
      for (const panel of dialog.querySelectorAll('[data-page-panel]')) panel.hidden = panel.dataset.pagePanel !== activePage;
      const titleEl = dialog.querySelector('#model-settings-title');
      if (titleEl) titleEl.textContent = activePage === 'apps' ? t('appsCenter', '应用中心') : t('modelSettings', '模型与服务');
      if (roles.subtitle) {
        roles.subtitle.textContent = activePage === 'apps'
          ? t('appsCenterSub', '管理可选 Natives 应用')
          : t('modelSettingsDescription', '管理内置 AI 调用连接、用量与额度。');
      }
    },
    setUsageTab(tab) { activeUsageTab = tab; },
    get activePage() { return activePage; },
    get activeUsageTab() { return activeUsageTab; },
    setLoading(loading) {
      const pages = dialog.querySelector('.model-settings-pages');
      const controls = () => Array.from(pages.querySelectorAll('button,input,select,textarea'));
      if (loading && pages.contains(document.activeElement)) loadingFocus = { node: document.activeElement, index: controls().indexOf(document.activeElement) };
      roles.loading.hidden = !loading; dialog.setAttribute('aria-busy', String(loading)); pages.inert = loading;
      if (!loading && loadingFocus) {
        const { node, index } = loadingFocus; loadingFocus = null;
        if (document.activeElement === document.body || document.activeElement === dialog) {
          (node.isConnected ? node : controls()[index])?.focus();
        }
      }
    },
    showError(message) {
      clearTimeout(noticeTimer);
      roles.error.hidden = !message;
      roles.error.querySelector('p').textContent = message || '';
      if (message) roles.notice.hidden = true;
    },
    showNotice(message, duration = 3500) {
      clearTimeout(noticeTimer);
      roles.notice.hidden = !message;
      roles.notice.textContent = message || '';
      if (message && duration > 0) {
        noticeTimer = setTimeout(() => {
          roles.notice.hidden = true;
          roles.notice.textContent = '';
        }, duration);
      }
    },
    showToast(message) {
      triggerToast(message);
    },
    render(snapshot, selectedID, usageContext = {}, quotaContext = {}) {
      roles.refresh.textContent = t('modelRefresh', '刷新');
      roles.close.textContent = t('close', '关闭');
      roles.loading.textContent = t('modelLoading', '正在加载模型设置…');
      roles.error.querySelector('button').textContent = t('retry', '重试');
      dialog.querySelector('.model-add-provider').textContent = `＋ ${t('modelAddProvider', '添加 AI 连接')}`;
      dialog.querySelector('.model-add-provider').disabled = false;
      roles.close.textContent = `← ${t('modelBackToWorkspace', '返回工作区')}`;
      roles.appsNavGroup.textContent = t('appsCenter', '应用中心');
      roles.appsListNav.textContent = t('appsList', '应用列表');
      roles.appsTitle.textContent = t('appsList', '应用列表');
      roles.appsDescription.textContent = t('appsCenterSub', '管理可选 Natives 应用');
      roles.navGroup.textContent = t('modelAISettings', 'AI 设置');
      roles.customNav.textContent = t('modelConnections', 'AI 连接');
      roles.usageNav.textContent = t('modelUsageRecords', '使用记录');
      roles.quotaNav.textContent = t('modelQuotaInquiry', '额度与凭据');
      roles.customTitle.textContent = t('modelConnections', 'AI 连接');
      roles.customDescription.textContent = t('modelConnectionsDescription', '统一配置 Base URL、API Key、协议和模型，供各模块调用 AI。');
      roles.retirementNotice.textContent = t('modelProxyRetiredNotice', '本地模型代理与路由已退役；Natives 不再为第三方工具注入代理配置，也不再提供代理访问密钥。');
      roles.residentLabel.textContent = t('modelResidentHost', '关闭页面后保持 AI 服务运行（常驻）');
      roles.resident.checked = Boolean(snapshot.resident);
      roles.defaultModel.replaceChildren(renderDefaultModelSelector(snapshot, t));
      roles.usageTitle.textContent = t('modelUsageRecords', '使用记录');
      roles.usageDescription.textContent = t('modelUsageRecordsDescription', '查看调用指标、Token 消耗、透视分析与费率定价。');
      roles.quotaTitle.textContent = t('modelQuotaInquiry', '额度与凭据');
      roles.quotaDescription.textContent = t('modelQuotaDescription', '额度查询仅读取已导入凭据或既有 OAuth 账户，不参与模型推理。');

      renderCustomProviderList(roles.providerList, snapshot, selectedID, t);
      const selectedCustom = snapshot.providers.find((provider) => provider.id === selectedID && provider.kind === 'custom');
      roles.detail.replaceChildren();
      if (selectedCustom) renderCustomDetail(roles.detail, selectedCustom, snapshot, t);
      else roles.detail.append(emptyState(t('modelNoCustomProviders', '尚未添加 AI 连接'), t('modelNoModelsHint', '添加连接后即可配置和调用模型。')));

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

      renderAuthFilesView(roles.authFilesContainer, {
        files: quotaContext.authFiles || [],
        quotaMap: quotaContext.quotaMap || {},
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
  container.append(element('h3', '', t('modelConnections', 'AI 连接')));
  const providers = snapshot.providers;
  if (!providers.length) container.append(element('p', 'model-empty muted', t('modelNoCustomProviders', '尚未添加 AI 连接')));
  for (const provider of providers) container.append(providerButton(provider, selectedID, `${provider.models.length}`, provider.name));
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

function renderDefaultModelSelector(snapshot, t) {
  const section = element('div', 'model-default-model-section');
  const label = element('h3', 'model-list-title', t('modelDefaultModel', '全局默认模型'));
  const select = document.createElement('select');
  select.name = 'defaultModel';
  select.setAttribute('aria-label', t('modelDefaultModel', '全局默认模型'));
  const current = snapshot.defaultModel || {};
  const placeholder = document.createElement('option');
  placeholder.value = '';
  placeholder.textContent = t('modelDefaultModelNone', '未设置（内置调用将要求显式选择）');
  select.append(placeholder);
  let available = false;
  for (const provider of snapshot.providers) {
    if (!provider.enabled) continue;
    const enabled = provider.models.filter((model) => model.enabled);
    if (!enabled.length) continue;
    const group = document.createElement('optgroup');
    group.label = provider.name;
    for (const model of enabled) {
      const option = document.createElement('option');
      option.value = `${provider.id}::${model.id}`;
      option.textContent = model.displayName || model.id;
      option.selected = current.providerId === provider.id && current.modelId === model.id;
      if (option.selected) available = true;
      group.append(option);
    }
    select.append(group);
  }
  if (current.providerId && current.modelId && !available) {
    const unavailable = document.createElement('option');
    unavailable.textContent = t('modelDefaultModelUnavailable', '原默认模型不可用，请重新选择');
    unavailable.disabled = true;
    unavailable.selected = true;
    select.append(unavailable);
  }
  select.addEventListener('change', () => {
    const separator = select.value.indexOf('::');
    const providerId = separator < 0 ? '' : select.value.slice(0, separator);
    const modelId = separator < 0 ? '' : select.value.slice(separator + 2);
    select.dispatchEvent(new CustomEvent('model-default-model-change', {
      bubbles: true,
      detail: { providerId: providerId || '', modelId: modelId || '' },
    }));
  });
  const hint = element('p', 'muted', t('modelDefaultModelHint', '各模块使用此默认模型调用 AI，无需重复配置地址和 API Key。用量与费用统一记录在 Tokenusage。'));
  section.append(label, select, hint);
  return section;
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
    const metadata = element('span', 'muted model-row-meta', model.contextLength ? `${model.contextLength}` : '');
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

export function renderNewProvider(container, t, snapshot) {
  container.innerHTML = `<form class="model-provider-form" data-new-provider>
    <div class="model-detail-heading"><div>${escapeText(t('modelAddProvider', '添加 AI 连接'))}</div></div>
    ${field(t('name', '名称'), 'name', '', 'text', true)}${field('Base URL', 'baseUrl', '', 'url', true)}
    <label><span>${escapeText(t('modelAPIFormat', 'API 格式'))}</span><select name="protocol">${protocolOptions('openai_chat')}</select></label>
    ${field('API Key', 'apiKey', '', 'password', false)}
    <label><span>${escapeText(t('modelID', '模型 ID'))}</span><input name="modelId" required maxlength="200"></label>
    <label class="model-checkbox"><input name="setAsDefault" type="checkbox" ${snapshot?.defaultModel?.modelId ? '' : 'checked'}> ${escapeText(t('modelSetAsDefault', '设为全局默认模型'))}</label>
    <label class="model-checkbox"><input name="enabled" type="checkbox" checked> ${escapeText(t('enable', '启用'))}</label>
    <label class="model-checkbox"><input name="allowLan" type="checkbox"> ${escapeText(t('modelAllowLAN', '允许局域网地址'))}</label>
    <div class="model-form-actions"><button type="submit" class="primary">${escapeText(t('modelAddProvider', '添加 AI 连接'))}</button><button type="button" data-action="test-new-provider">${escapeText(t('modelTestConnection', '测试连接'))}</button><button type="button" data-action="cancel-new-provider">${escapeText(t('cancel', '取消'))}</button></div>
  </form>`;
  container.querySelector('[name="name"]').focus();
}

function modelForm(t) {
  const form = document.createElement('form'); form.className = 'model-add-form'; form.dataset.modelForm = '';
  form.innerHTML = `<input name="id" required maxlength="200" aria-label="${escapeText(t('modelID', '模型 ID'))}" placeholder="${escapeText(t('modelID', '模型 ID'))}"><input name="displayName" maxlength="200" aria-label="${escapeText(t('modelDisplayName', '显示名称'))}" placeholder="${escapeText(t('modelDisplayName', '显示名称'))}"><input name="contextLength" type="number" min="0" max="100000000" aria-label="${escapeText(t('modelContextLength', '上下文'))}" placeholder="${escapeText(t('modelContextLength', '上下文'))}"><button class="primary" data-role="model-submit">${escapeText(t('add', '添加'))}</button><button type="button" data-action="cancel-model-edit" hidden>${escapeText(t('cancel', '取消'))}</button>`;
  return form;
}

function field(label, name, value, type, required, placeholder = '') { return `<label><span>${escapeText(label)}</span><input name="${name}" type="${type}" value="${escapeText(value || '')}" ${required ? 'required' : ''} placeholder="${escapeText(placeholder)}"></label>`; }
function protocolOptions(selected) { return [['openai_chat', 'OpenAI Chat'], ['openai_responses', 'OpenAI Responses'], ['anthropic_messages', 'Anthropic Messages'], ['gemini', 'Gemini']].map(([value, label]) => `<option value="${value}" ${value === selected ? 'selected' : ''}>${label}</option>`).join(''); }
function button(action, label, className = '') { const node = document.createElement('button'); node.type = 'button'; node.dataset.action = action; node.className = className; node.textContent = label; return node; }
function element(tag, className = '', text = '') { const node = document.createElement(tag); node.className = className; node.textContent = text; return node; }
function emptyState(title, hint) { const node = element('div', 'model-empty-state'); node.append(element('strong', '', title), element('p', 'muted', hint)); return node; }
function escapeText(value) { return String(value ?? '').replace(/[&<>'"]/g, (char) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', "'": '&#39;', '"': '&quot;' })[char]); }
