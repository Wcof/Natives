/**
 * Claude role mapping editor (claude-code / claude-desktop) and Pi provider
 * section for the agent clients page.
 */

function escapeHtml(value) {
  return String(value ?? '').replace(/[&<>"']/g, (match) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[match]);
}

export function renderClaudeMappings(container, mappings, models, t, onChange) {
  const value = {
    opus: 'claude-opus-5', sonnet: 'claude-sonnet-4-6', haiku: 'claude-haiku-4-5',
    opus1m: false, sonnet1m: false, haiku1m: false,
    maxContextTokens: 200000, autoCompactPct: 90, disableAutoCompact: false,
    ...(mappings || {}),
  };
  const modelOptions = (selected) => {
    const names = new Set(['claude-opus-5', 'claude-sonnet-4-6', 'claude-haiku-4-5', selected].filter(Boolean));
    for (const model of models || []) names.add(model.name);
    return [...names].map((name) => `<option value="${escapeHtml(name)}" ${name === selected ? 'selected' : ''}>${escapeHtml(name)}</option>`).join('');
  };
  container.innerHTML = `
    <div class="model-agent-card-header">
      <div class="model-agent-card-title-group">
        <h4>${t('agentClaudeMappingTitle', 'Claude 模型映射')}</h4>
        <p class="muted">${t('agentClaudeMappingHint', '将 Claude 角色请求路由到网关中的具体模型；开启 1M 后上下文长度按百万级处理。')}</p>
      </div>
    </div>
    <div class="model-agent-mapping-grid">
      <div class="model-agent-role-card">
        <div class="model-agent-role-badge-row">
          <span class="model-role-badge opus">Opus</span>
          <label class="model-checkbox model-agent-1m-check" title="启用 100 万超长上下文"><input type="checkbox" data-role="agent-map-opus-1m" ${value.opus1m ? 'checked' : ''}> 1M</label>
        </div>
        <label class="inspector-field">
          <select class="model-agent-select" data-role="agent-map-opus">${modelOptions(value.opus)}</select>
        </label>
      </div>
      <div class="model-agent-role-card">
        <div class="model-agent-role-badge-row">
          <span class="model-role-badge sonnet">Sonnet</span>
          <label class="model-checkbox model-agent-1m-check" title="启用 100 万超长上下文"><input type="checkbox" data-role="agent-map-sonnet-1m" ${value.sonnet1m ? 'checked' : ''}> 1M</label>
        </div>
        <label class="inspector-field">
          <select class="model-agent-select" data-role="agent-map-sonnet">${modelOptions(value.sonnet)}</select>
        </label>
      </div>
      <div class="model-agent-role-card">
        <div class="model-agent-role-badge-row">
          <span class="model-role-badge haiku">Haiku</span>
          <label class="model-checkbox model-agent-1m-check" title="启用 100 万超长上下文"><input type="checkbox" data-role="agent-map-haiku-1m" ${value.haiku1m ? 'checked' : ''}> 1M</label>
        </div>
        <label class="inspector-field">
          <select class="model-agent-select" data-role="agent-map-haiku">${modelOptions(value.haiku)}</select>
        </label>
      </div>
    </div>
    <div class="model-agent-mapping-grid model-agent-context-grid">
      <label class="inspector-field">
        <span>${t('agentMaxContext', '最大上下文 Token')}</span>
        <input type="number" class="model-agent-num-input" data-role="agent-map-context" min="100000" max="1000000" step="1000" value="${value.maxContextTokens}" />
      </label>
      <label class="inspector-field">
        <span>${t('agentCompactPct', '触发压缩百分比')}</span>
        <input type="number" class="model-agent-num-input" data-role="agent-map-compact" min="1" max="100" value="${value.autoCompactPct}" />
      </label>
    </div>
    <div class="model-agent-checkbox-wrap">
      <label class="model-checkbox"><input type="checkbox" data-role="agent-map-disable-compact" ${value.disableAutoCompact ? 'checked' : ''}> ${t('agentDisableCompact', '禁止自动压缩')}</label>
    </div>
  `;
  const emit = () => onChange({
    opus: container.querySelector('[data-role="agent-map-opus"]').value,
    sonnet: container.querySelector('[data-role="agent-map-sonnet"]').value,
    haiku: container.querySelector('[data-role="agent-map-haiku"]').value,
    opus1m: container.querySelector('[data-role="agent-map-opus-1m"]').checked,
    sonnet1m: container.querySelector('[data-role="agent-map-sonnet-1m"]').checked,
    haiku1m: container.querySelector('[data-role="agent-map-haiku-1m"]').checked,
    maxContextTokens: Number(container.querySelector('[data-role="agent-map-context"]').value) || 200000,
    autoCompactPct: Number(container.querySelector('[data-role="agent-map-compact"]').value) || 90,
    disableAutoCompact: container.querySelector('[data-role="agent-map-disable-compact"]').checked,
  });
  for (const node of container.querySelectorAll('select, input')) node.onchange = emit;
}

export function renderPiSection(container, piStatus, t, onAction) {
  const section = container || document.createElement('div');
  section.className = 'model-section model-agent-pi-card';
  const statusText = piStatus?.installed
    ? `${t('agentPiInstalled', 'Pi provider 插件已安装')}${piStatus.installedVersion ? ` · v${piStatus.installedVersion}` : ''}`
    : t('agentPiMissing', 'Pi provider 插件未安装');
  section.innerHTML = `
    <div class="model-agent-card-header">
      <div class="model-agent-card-title-group">
        <h4>${t('agentPiTitle', 'Pi Provider 插件')}</h4>
        <p class="muted">${statusText}</p>
      </div>
    </div>
    <div class="model-agent-actions">
      <button type="button" class="primary model-agent-primary-btn" data-action="agent-pi" data-pi-action="install">${t('agentPiInstall', '安装插件')}</button>
      <button type="button" class="model-agent-outline-btn" data-action="agent-pi" data-pi-action="update">${t('agentPiUpdate', '更新插件')}</button>
      <button type="button" class="model-agent-outline-btn" data-action="agent-pi" data-pi-action="repair">${t('agentPiRepair', '修复插件')}</button>
      <button type="button" class="danger" data-action="agent-pi" data-pi-action="uninstall">${t('agentPiUninstall', '卸载插件')}</button>
    </div>
  `;
  for (const button of section.querySelectorAll('[data-action="agent-pi"]')) {
    button.onclick = () => onAction('agent-pi', { action: button.dataset.piAction });
  }
  return section;
}

