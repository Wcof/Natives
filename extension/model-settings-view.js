const OAUTH_LABELS = {
  codex: 'OpenAI Codex', claude: 'Anthropic Claude', antigravity: 'Google Gemini', kimi: 'Kimi', xai: 'xAI Grok',
};

export function createModelSettingsView({ t, onAction }) {
  const dialog = document.createElement('dialog');
  dialog.className = 'model-settings-dialog';
  dialog.setAttribute('aria-labelledby', 'model-settings-title');
  dialog.innerHTML = `
    <div class="model-settings-shell">
      <header class="model-settings-header">
        <div><h2 id="model-settings-title"></h2><p class="muted" data-role="subtitle"></p></div>
        <div class="model-settings-header-actions"><button type="button" data-action="refresh" data-role="refresh"></button><button type="button" data-action="close" data-role="close"></button></div>
      </header>
      <div class="model-settings-notice" data-role="notice" role="status" aria-live="polite" hidden></div>
      <section class="model-gateway-card" data-role="gateway"></section>
      <div class="model-settings-layout">
        <aside class="model-provider-pane"><div class="model-provider-scroll" data-role="provider-list"></div><button type="button" class="model-add-provider" data-action="new-provider"></button></aside>
        <main class="model-provider-detail" data-role="detail"></main>
      </div>
      <div class="model-settings-loading" data-role="loading" hidden></div>
      <div class="model-settings-error" data-role="error" hidden><p></p><button type="button" data-action="refresh"></button></div>
    </div>`;
  document.body.append(dialog);

  const roles = Object.fromEntries([...dialog.querySelectorAll('[data-role]')].map((node) => [node.dataset.role, node]));
  roles.refresh.textContent = t('modelRefresh', '刷新');
  roles.close.textContent = t('close', '关闭');
  roles.loading.textContent = t('modelLoading', '正在加载模型设置…');
  roles.error.querySelector('button').textContent = t('retry', '重试');
  dialog.querySelector('.model-add-provider').textContent = `＋ ${t('modelAddProvider', '添加自定义供应商')}`;
  dialog.addEventListener('click', (event) => {
    const action = event.target.closest('[data-action]');
    if (action) onAction(action.dataset.action, action, event);
  });

  return {
    dialog,
    open() { if (!dialog.open) dialog.showModal(); },
    close() { dialog.close(); },
    setLoading(loading) { roles.loading.hidden = !loading; dialog.setAttribute('aria-busy', String(loading)); },
    showError(message) { roles.error.hidden = !message; roles.error.querySelector('p').textContent = message || ''; if (message) roles.notice.hidden = true; },
    showNotice(message) { roles.notice.hidden = !message; roles.notice.textContent = message || ''; },
    render(snapshot, selectedID, pendingOAuth) {
      roles.refresh.textContent = t('modelRefresh', '刷新');
      roles.close.textContent = t('close', '关闭');
      roles.loading.textContent = t('modelLoading', '正在加载模型设置…');
      roles.error.querySelector('button').textContent = t('retry', '重试');
      dialog.querySelector('.model-add-provider').textContent = `＋ ${t('modelAddProvider', '添加自定义供应商')}`;
      dialog.querySelector('#model-settings-title').textContent = t('modelSettings', '模型设置');
      roles.subtitle.textContent = t('modelSettingsDescription', '管理 OAuth 与自定义模型供应商，配置后可通过本地兼容接口使用。');
      renderGateway(roles.gateway, snapshot, t);
      renderProviderList(roles.providerList, snapshot, selectedID, pendingOAuth, t);
      const selected = snapshot.providers.find((provider) => provider.id === selectedID) || snapshot.providers[0];
      if (selected?.kind === 'custom') renderCustomDetail(roles.detail, selected, snapshot, t);
      else renderOAuthDetail(roles.detail, selected, snapshot, pendingOAuth, t);
    },
    destroy() { dialog.remove(); },
  };
}

function renderGateway(container, snapshot, t) {
  const gateway = snapshot.gateway || { state: 'stopped' };
  const running = gateway.state === 'running';
  const busy = ['starting', 'stopping', 'restarting'].includes(gateway.state);
  container.replaceChildren();
  const heading = element('div', 'model-gateway-heading');
  heading.append(element('strong', '', t('modelLocalGateway', '本地模型代理')));
  const state = element('span', `model-status model-status-${gateway.state}`, stateLabel(gateway.state, t));
  state.setAttribute('role', 'status'); state.setAttribute('aria-live', 'polite');
  heading.append(state);
  const actions = element('div', 'model-gateway-actions');
  const lifecycle = button(running ? 'stop-gateway' : 'start-gateway', running ? t('modelStopGateway', '停止') : gateway.state === 'failed' ? t('retry', '重试') : t('modelStartGateway', '启动'), 'primary');
  lifecycle.disabled = busy;
  actions.append(lifecycle);
  const resident = document.createElement('label');
  resident.className = 'model-switch';
  resident.innerHTML = `<input type="checkbox" data-action="resident" ${gateway.resident ? 'checked' : ''}><span></span>`;
  resident.append(document.createTextNode(t('modelResident', '常驻')));
  actions.append(resident, button('reveal-key', t('modelCopyAccessKey', '复制访问密钥')), button('rotate-key', t('modelRotateAccessKey', '重新生成密钥')));
  const endpoints = element('div', 'model-gateway-endpoints');
  if (running) {
    for (const [label, suffix] of [['OpenAI Chat', '/v1'], ['OpenAI Responses', '/v1'], ['Anthropic Messages', '/v1'], ['Gemini', '/v1beta']]) {
      const row = element('div', 'model-gateway-endpoint');
      row.append(element('span', 'muted', label), element('code', '', `${gateway.baseUrl}${suffix}`));
      const copy = button('copy-endpoint', t('copy', '复制')); copy.dataset.endpoint = `${gateway.baseUrl}${suffix}`; row.append(copy); endpoints.append(row);
    }
  } else endpoints.append(element('span', 'muted', t('modelGatewayNotRunning', '代理尚未运行')));
  const activeAccounts = snapshot.accounts.filter((account) => account.enabled && account.status === 'active');
  const activeAccountIDs = new Set(activeAccounts.map((account) => account.id));
  const availableProviders = snapshot.providers.filter((provider) => provider.enabled && (provider.kind === 'custom'
    ? provider.models.some((model) => model.enabled)
    : activeAccounts.some((account) => account.provider === provider.oauthProvider)));
  const enabledModels = snapshot.providers.reduce((count, provider) => count + (!provider.enabled ? 0 : provider.models.filter((model) => model.enabled && (provider.kind === 'custom' || model.accountIds?.some((id) => activeAccountIDs.has(id)))).length), 0);
  const summary = element('div', 'model-gateway-summary', t('modelGatewaySummary', '$1 个供应商 · $2 个账户 · $3 个模型').replace('$1', availableProviders.length).replace('$2', activeAccounts.length).replace('$3', enabledModels));
  container.append(heading, endpoints, actions, summary);
}

function renderProviderList(container, snapshot, selectedID, pendingOAuth, t) {
  container.replaceChildren();
  const oauthTitle = element('h3', '', t('modelOAuthProviders', 'OAuth'));
  container.append(oauthTitle);
  for (const provider of snapshot.providers.filter((item) => item.kind === 'oauth')) {
    const accountCount = snapshot.accounts.filter((account) => account.provider === provider.oauthProvider).length;
    container.append(providerButton(provider, selectedID, pendingOAuth?.provider === provider.oauthProvider ? t('modelAuthorizing', '授权中') : `${accountCount}`, OAUTH_LABELS[provider.oauthProvider] || provider.name));
  }
  container.append(element('h3', '', t('modelCustomProviders', '自定义供应商')));
  const custom = snapshot.providers.filter((item) => item.kind === 'custom');
  if (!custom.length) container.append(element('p', 'model-empty muted', t('modelNoCustomProviders', '尚未添加自定义供应商')));
  for (const provider of custom) container.append(providerButton(provider, selectedID, `${provider.models.length}`, provider.name));
}

function providerButton(provider, selectedID, badge, label) {
  const item = button('select-provider', '', `model-provider-item${provider.id === selectedID ? ' selected' : ''}`);
  item.setAttribute('aria-pressed', String(provider.id === selectedID));
  item.dataset.providerId = provider.id;
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
    row.append(element('div', 'model-account-main', account.label), element('span', `model-status model-status-${account.status}`, accountStatusLabel(account.status, t)));
    const actions = element('div', 'model-row-actions');
    const toggle = button('toggle-account', account.enabled ? t('disable', '禁用') : t('enable', '启用'));
    toggle.dataset.accountId = account.id; toggle.dataset.enabled = String(!account.enabled);
    const reauth = button('reauth-account', t('modelReauthorize', '重新授权')); reauth.dataset.accountId = account.id; reauth.dataset.provider = provider.oauthProvider;
    const remove = button('delete-account', t('delete', '删除'), 'danger'); remove.dataset.accountId = account.id;
    actions.append(toggle, reauth, remove); row.append(actions); container.append(row);
  }
  container.append(element('h3', 'model-list-title', t('modelList', '模型列表')));
  const catalog = element('div', 'model-list');
  for (const model of provider.models) {
    const row = element('div', 'model-row');
    row.append(element('code', '', model.id), element('span', 'muted', model.displayName || model.id), element('span', 'muted', `${model.accountIds?.length || 0} ${t('modelAccounts', '账户')}`));
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
  const list = element('div', 'model-list');
  for (const model of provider.models) {
    const row = element('div', 'model-row');
    const identity = element('div', 'model-row-identity');
    identity.append(element('code', '', model.id), element('span', 'muted', model.displayName || model.id));
    const metadata = element('span', 'muted', [model.alias, model.contextLength ? `${model.contextLength}` : ''].filter(Boolean).join(' · '));
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
function stateLabel(state, t) { return ({ running: t('modelRunning', '运行中'), starting: t('modelStarting', '启动中'), stopping: t('modelStopping', '停止中'), failed: t('modelFailed', '异常'), restarting: t('modelRestarting', '重启中') })[state] || t('modelStopped', '已停止'); }
function accountStatusLabel(status, t) { return ({ active: t('modelAccountActive', '可用'), needs_reauth: t('modelAccountNeedsReauth', '需要重新登录'), failed: t('modelFailed', '异常') })[status] || status; }
function escapeText(value) { return String(value ?? '').replace(/[&<>'"]/g, (char) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', "'": '&#39;', '"': '&quot;' })[char]); }
