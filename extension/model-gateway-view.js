export function renderGateway(container, snapshot, t) {
  const gateway = snapshot.gateway || { state: 'stopped' };
  const running = gateway.state === 'running';
  const busy = ['starting', 'stopping', 'restarting'].includes(gateway.state);
  container.replaceChildren();
  const heading = element('div', 'model-gateway-heading');
  heading.append(element('strong', '', t('modelLocalGateway', '本地模型代理')));
  const state = element('span', `model-status model-status-${gateway.state}`, stateLabel(gateway.state, t));
  state.setAttribute('role', 'status'); state.setAttribute('aria-live', 'polite'); heading.append(state);
  const actions = element('div', 'model-gateway-actions');
  const lifecycle = button(running ? 'stop-gateway' : 'start-gateway', running ? t('modelStopGateway', '停止') : gateway.state === 'failed' ? t('retry', '重试') : t('modelStartGateway', '启动'), 'primary');
  lifecycle.disabled = busy; actions.append(lifecycle);
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
  const accounts = snapshot.accounts.filter((account) => account.enabled && account.status === 'active');
  const accountIDs = new Set(accounts.map((account) => account.id));
  const providers = snapshot.providers.filter((provider) => provider.enabled && (provider.kind === 'custom' ? provider.models.some((model) => model.enabled) : accounts.some((account) => account.provider === provider.oauthProvider)));
  const models = snapshot.providers.reduce((count, provider) => count + (!provider.enabled ? 0 : provider.models.filter((model) => model.enabled && (provider.kind === 'custom' || model.accountIds?.some((id) => accountIDs.has(id)))).length), 0);
  container.append(heading, endpoints, actions, element('div', 'model-gateway-summary', t('modelGatewaySummary', '$1 个供应商 · $2 个账户 · $3 个模型').replace('$1', providers.length).replace('$2', accounts.length).replace('$3', models)));
}

function button(action, label, className = '') { const node = document.createElement('button'); node.type = 'button'; node.dataset.action = action; node.className = className; node.textContent = label; return node; }
function element(tag, className = '', text = '') { const node = document.createElement(tag); node.className = className; node.textContent = text; return node; }
function stateLabel(state, t) { return ({ running: t('modelRunning', '运行中'), starting: t('modelStarting', '启动中'), stopping: t('modelStopping', '停止中'), failed: t('modelFailed', '异常'), restarting: t('modelRestarting', '重启中') })[state] || t('modelStopped', '已停止'); }
