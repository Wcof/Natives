/**
 * Model Agent Clients View.
 * 智能体配置: client list + detail with 核心配置 / 会话管理 (codex) tabs,
 * searchable model picker, Claude role mappings, Pi plugin section, launch bar.
 */

import { createAgentModelPicker } from './agent-model-picker.js';
import { renderClaudeMappings, renderPiSection } from './model-agent-sections.js';

function escapeHtml(value) {
  return String(value ?? '').replace(/[&<>"']/g, (match) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[match]);
}

const AGENT_ICON_MAP = {
  'claude-code': 'icons/agents/claude.svg',
  'claude-desktop': 'icons/agents/claude.svg',
  'codex': 'icons/agents/codex.svg',
  'deepseek-harness': 'icons/agents/deepseek.svg',
  'opencode': 'icons/agents/opencode.svg',
  'openclaw': 'icons/agents/openclaw.svg',
  'hermes': 'icons/agents/hermes.png',
  'zcode': 'icons/agents/zcode.png',
  'kimi-code': 'icons/agents/kimi-light.svg',
  'grok-build': 'icons/agents/grok.svg',
  'pi': 'icons/agents/pi-logo-on-light.svg',
};

function renderAgentMark(client) {
  const iconPath = AGENT_ICON_MAP[client.id];
  if (iconPath) {
    return `<span class="agent-client-icon"><img src="${iconPath}" alt="" class="provider-logo" /></span>`;
  }
  const initial = (client.name || 'A').slice(0, 2).toUpperCase();
  return `<span class="agent-client-icon"><span class="provider-initial">${escapeHtml(initial)}</span></span>`;
}

export function renderAgentView(container, context) {
  const { clients = [], selectedId = '', models = null, modelsError = '', selection = {}, busy = false, activeTab = 'core', sessions = [], loadError = '', detecting = false, t, onAction } = context;
  container.replaceChildren();

  const installedCount = clients.filter((c) => c.installed).length;
  const statsText = detecting
    ? t('agentDetecting', '正在检测本机客户端…')
    : `${clients.length} ${t('agentClientsDetected', '个本机客户端')}`;

  const topbar = document.createElement('div');
  topbar.className = 'model-agent-topbar';
  topbar.innerHTML = `
    <div class="model-agent-stats-wrap">
      <div class="model-agent-stats-badge"><span class="model-agent-pulse-dot" aria-hidden="true"></span>${escapeHtml(statsText)}</div>
      ${!detecting && clients.length ? `<span class="model-agent-stats-sub muted">${installedCount} 个已就绪</span>` : ''}
    </div>
    <button type="button" class="btn-quota-action model-agent-refresh-btn" data-action="agent-refresh" ${detecting ? 'disabled' : ''}>
      <svg class="icon${detecting ? ' spin' : ''}" aria-hidden="true"><use href="#i-refresh" /></svg>
      <span>${t('agentRedetect', '重新检测')}</span>
    </button>
  `;
  container.append(topbar);

  const layout = document.createElement('div');
  layout.className = 'model-agent-layout';
  layout.append(renderClientList(clients, selectedId, { detecting, loadError, t, onAction }));

  const selected = clients.find((client) => client.id === selectedId) || null;
  layout.append(renderDetail(selected, { models, modelsError, selection, busy, activeTab, sessions, t, onAction }));

  container.append(layout);
}

function renderClientList(clients, selectedId, { detecting, loadError, t, onAction }) {
  const pane = document.createElement('aside');
  pane.className = 'model-agent-list';
  pane.innerHTML = `
    <div class="model-agent-list-header">
      <div class="model-agent-list-title-row">
        <strong>${t('agentLocalClients', '本机客户端')}</strong>
        <span class="model-agent-count-pill">${clients.length}</span>
      </div>
      <p class="muted">${t('agentLocalClientsHint', '选择需要管理的智能体')}</p>
    </div>
    <div class="model-agent-client-scroll"></div>
  `;
  const scroll = pane.querySelector('.model-agent-client-scroll');
  for (const client of clients) {
    const item = document.createElement('button');
    item.type = 'button';
    item.className = `model-agent-client${client.id === selectedId ? ' selected' : ''}`;
    item.dataset.action = 'agent-select';
    item.dataset.client = client.id;
    const dotClass = client.modificationState === 'applied' ? ' configured' : client.installed ? ' installed' : '';
    item.innerHTML = `
      ${renderAgentMark(client)}
      <span class="model-agent-client-main">
        <strong>${escapeHtml(client.name)}</strong>
        <small>${escapeHtml(listStatusText(client, t))}</small>
      </span>
      <span class="model-agent-dot${dotClass}" aria-hidden="true"></span>
    `;
    scroll.append(item);
  }
  if (!clients.length) {
    const empty = document.createElement('div');
    empty.className = 'model-empty-state';
    if (detecting) {
      empty.innerHTML = `<strong>${escapeHtml(t('agentDetecting', '正在检测本机客户端…'))}</strong>`;
    } else {
      const message = loadError
        ? `${escapeHtml(t('agentDetectFailed', '客户端检测失败'))}<p class="muted">${escapeHtml(loadError)}</p>`
        : `${escapeHtml(t('agentNoClientsFound', '未检测到任何智能体客户端'))}`;
      empty.innerHTML = `<strong>${message}</strong>`;
      const retry = document.createElement('button');
      retry.type = 'button';
      retry.className = 'btn-quota-action';
      retry.dataset.action = 'agent-refresh';
      retry.innerHTML = `<svg class="icon" aria-hidden="true"><use href="#i-refresh" /></svg><span>${t('agentRedetect', '重新检测')}</span>`;
      empty.append(retry);
    }
    scroll.append(empty);
  }
  return pane;
}

function listStatusText(client, t) {
  if (client.id === 'pi' && client.pluginVersion) return t('agentPiInstalled', 'Pi provider 插件已安装');
  if (!client.installed) return t('agentNotInstalled', '未检测到安装');
  if (client.modificationState === 'applied' && client.appliedModel) {
    return `${t('agentModified', '已修改')} · ${client.appliedModel}`;
  }
  return client.version ? `${t('agentInstalled', '已安装')} · ${client.version}` : t('agentInstalledKeepConfig', '已安装 · 保持原配置');
}

function renderDetail(client, context) {
  const { models, modelsError, selection, busy, activeTab, sessions, t, onAction } = context;
  const panel = document.createElement('section');
  panel.className = 'model-agent-detail';
  if (!client) {
    panel.innerHTML = `<div class="model-empty-state"><strong>${escapeHtml(t('agentSelectHint', '从左侧选择一个智能体客户端'))}</strong><p class="muted">${escapeHtml(t('agentSelectHintDetail', '检测完成后即可查看安装状态并应用网关配置。'))}</p></div>`;
    return panel;
  }

  const heading = document.createElement('header');
  heading.className = 'model-page-heading model-agent-detail-heading';
  heading.innerHTML = `
    <div class="model-agent-header-left">
      <div class="model-agent-header-avatar">${renderAgentMark(client)}</div>
      <div class="model-agent-header-meta">
        <div class="model-agent-header-tags">
          <span class="model-oauth-section-tag">${t('agentCoreConfig', '核心配置')}</span>
          ${client.installed ? '<span class="model-agent-status-pill installed"><span class="pill-dot"></span>已检测到</span>' : '<span class="model-agent-status-pill missing"><span class="pill-dot"></span>未检测到</span>'}
          ${client.modificationState === 'applied' ? '<span class="model-agent-status-pill applied">已应用网关</span>' : ''}
        </div>
        <h3>${escapeHtml(client.name)}</h3>
      </div>
    </div>
  `;
  panel.append(heading);


  if (client.id === 'codex') {
    panel.append(renderDetailTabs(activeTab, t, onAction));
    if (activeTab === 'sessions') {
      panel.append(renderSessions(sessions, busy, t, onAction));
      return panel;
    }
  }

  const body = document.createElement('div');
  body.className = 'model-agent-body';

  body.append(renderStatusGrid(client, t));
  if (client.error) body.append(errorLine(client.error));
  for (const warning of client.warnings || []) body.append(warningLine(warning));

  if (client.installed && client.modelPicker) {
    body.append(renderModelSection(client, models, modelsError, selection, busy, t, onAction));
    if (client.id === 'claude-code' || client.id === 'claude-desktop') {
      const mappingHost = document.createElement('div');
      mappingHost.className = 'model-section';
      body.append(mappingHost);
      renderClaudeMappings(mappingHost, selection[`${client.id}:mappings`], models, t, (mappings) => {
        onAction('agent-mapping-change', { client: client.id, mappings });
      });
    }
    body.append(renderModifySection(client, selection, busy, t, onAction));
    if (client.id === 'codex') body.append(renderCodexDanger(client, busy, t, onAction));
  } else if (client.installed) {
    if (client.id === 'pi') body.append(renderPiSection(null, { installed: true, installedVersion: client.pluginVersion }, t, onAction));
    else body.append(emptyState(t('agentManualClient', '该客户端由本工具自动接管网关参数'), t('agentManualClientHint', '启动后即通过本地代理调用模型。')));
  } else {
    body.append(emptyState(t('agentNotInstalledTitle', '未检测到安装'), t('agentNotInstalledHint', '安装该客户端后点击「重新检测」。')));
  }

  body.append(renderLaunchFooter(client, t, onAction));
  panel.append(body);
  return panel;
}

function renderStatusGrid(client, t) {
  const grid = document.createElement('div');
  grid.className = 'model-agent-status-grid';
  const cards = [
    {
      label: t('agentInstallStatus', '安装状态'),
      value: client.installed ? t('agentClientDetected', '已检测到客户端') : t('agentClientMissing', '未检测到客户端'),
      isStatus: true,
      ok: client.installed,
    },
  ];
  if (client.cliVersion || client.appVersion) {
    cards.push({ label: t('agentCliVersion', 'CLI 版本'), value: client.cliVersion || '—' });
    cards.push({ label: t('agentAppVersion', 'App 版本'), value: client.appVersion || '—' });
  } else if (client.id === 'pi') {
    cards.push({ label: t('agentClientVersion', '客户端版本'), value: client.version || '—' });
    cards.push({ label: t('agentPiPluginVersion', '插件版本'), value: client.pluginVersion || '—' });
  } else {
    cards.push({ label: t('agentClientVersion', '客户端版本'), value: client.version || '—' });
  }

  for (const card of cards) {
    const cardEl = document.createElement('div');
    cardEl.className = 'model-agent-status-card';
    const valClass = card.isStatus ? (card.ok ? 'val-ok' : 'val-dim') : '';
    cardEl.innerHTML = `
      <span class="model-agent-card-label">${escapeHtml(card.label)}</span>
      <strong class="model-agent-card-val ${valClass}">${escapeHtml(card.value)}</strong>
    `;
    grid.append(cardEl);
  }
  return grid;
}

function renderModelSection(client, models, modelsError, selection, busy, t, onAction) {
  const section = document.createElement('div');
  section.className = 'model-section model-agent-model-card';
  const current = selection[client.id] || '';
  section.innerHTML = `
    <div class="model-agent-card-header">
      <div class="model-agent-card-title-group">
        <h4>${t('agentUseModel', '使用模型')}</h4>
        <p class="muted">配置该客户端默认调用的网关模型</p>
      </div>
    </div>
  `;
  if (modelsError) {
    const line = document.createElement('p');
    line.className = 'model-quota-error';
    line.textContent = modelsError;
    section.append(line);
  } else if (!models) {
    const p = document.createElement('p');
    p.className = 'muted model-agent-loading-state';
    p.innerHTML = `<span class="model-agent-spinner" aria-hidden="true"></span>${t('agentModelsLoading', '正在读取可用模型…')}`;
    section.append(p);
  } else if (!models.length) {
    const p = document.createElement('p');
    p.className = 'muted';
    p.textContent = t('agentModelsEmpty', '本地代理暂未发现可用模型，请先在模型设置中添加供应商或账户。');
    section.append(p);
  } else {
    const picker = createAgentModelPicker({
      value: current,
      models,
      t,
      onChange: (model) => onAction('agent-model-change', { client: client.id, model }),
      onRefresh: () => onAction('agent-models-refresh', { client: client.id }),
    });
    section.append(picker);
  }
  return section;
}

function renderModifySection(client, selection, busy, t, onAction) {
  const section = document.createElement('div');
  section.className = 'model-section model-agent-action-card';
  const applied = client.modificationState === 'applied';
  const primaryLabel = !applied
    ? t('agentApply', '应用配置修改')
    : selection[client.id] && selection[client.id] !== client.appliedModel ? t('agentUpdate', '更新配置') : t('agentCloseConfig', '关闭配置修改');
  section.innerHTML = `
    <div class="model-agent-card-header">
      <div class="model-agent-card-title-group">
        <h4>${t('agentModifyTitle', '配置修改')}</h4>
        <p class="muted">${t('agentModifyHint', '应用或关闭配置后，请完全退出并重新启动客户端；默认模型和上下文长度会在新会话中生效。')}</p>
      </div>
    </div>
    <div class="model-agent-actions">
      <button type="button" class="primary model-agent-primary-btn" data-action="agent-apply" data-client="${escapeHtml(client.id)}" ${busy ? 'disabled' : ''}>
        <svg class="icon" aria-hidden="true" style="width:14px;height:14px;margin-right:6px;"><path d="M20 6L9 17l-5-5" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"/></svg>
        <span>${escapeHtml(primaryLabel)}</span>
      </button>
      <button type="button" class="model-agent-outline-btn" data-action="agent-default" data-client="${escapeHtml(client.id)}" ${busy ? 'disabled' : ''}>${t('agentDefault', '默认配置')}</button>
      ${applied && primaryLabel !== t('agentCloseConfig', '关闭配置修改') ? `<button type="button" class="model-agent-outline-btn" data-action="agent-close-config" data-client="${escapeHtml(client.id)}" ${busy ? 'disabled' : ''}>${t('agentCloseConfig', '关闭配置修改')}</button>` : ''}
    </div>
  `;
  return section;
}

function renderCodexDanger(client, busy, t, onAction) {
  const section = document.createElement('div');
  section.className = 'model-section model-agent-danger-card';
  section.innerHTML = `
    <div class="model-agent-card-header">
      <div class="model-agent-card-title-group">
        <h4 class="danger-title">${t('agentCodexDangerTitle', 'Codex 配置清理')}</h4>
        <p class="muted">${t('agentCodexDangerHint', '移除由本工具写入的 Codex 配置与凭据文件，恢复未接管状态。')}</p>
      </div>
    </div>
    <div class="model-agent-actions">
      <button type="button" class="danger" data-action="agent-codex-clear" data-client="${escapeHtml(client.id)}" ${busy ? 'disabled' : ''}>${t('agentCodexClear', '清空配置')}</button>
    </div>
  `;
  return section;
}

function renderDetailTabs(activeTab, t, onAction) {
  const tabs = document.createElement('div');
  tabs.className = 'model-oauth-tabs-nav model-agent-segmented-tabs';
  for (const [tab, label] of [['core', t('agentTabCore', '核心配置')], ['sessions', t('agentTabSessions', '会话管理')]]) {
    const button = document.createElement('button');
    button.type = 'button';
    button.className = `model-oauth-tab-btn${activeTab === tab ? ' selected' : ''}`;
    button.dataset.action = 'agent-tab';
    button.dataset.tab = tab;
    button.textContent = label;
    tabs.append(button);
  }
  return tabs;
}

function renderSessions(sessions, busy, t, onAction) {
  const body = document.createElement('div');
  body.className = 'model-agent-body';
  const wrapper = document.createElement('div');
  wrapper.className = 'model-table-wrapper';
  wrapper.innerHTML = `<table class="model-keys-table"><thead><tr><th>${t('agentSessionName', '会话')}</th><th>${t('size', '大小')}</th><th>${t('modelEventTime', '时间')}</th><th></th></tr></thead><tbody>${sessions.map((session) => `
    <tr><td class="model-col-name"><code title="${escapeHtml(session.path)}">${escapeHtml(session.name)}</code></td><td class="model-col-num">${formatBytes(session.sizeBytes)}</td><td>${escapeHtml(session.updatedAt)}</td><td class="model-col-actions"><button type="button" class="danger btn-sm" data-action="agent-session-delete" data-path="${escapeHtml(session.path)}" ${busy ? 'disabled' : ''}>${t('delete', '删除')}</button></td></tr>`).join('')}</tbody></table>`;
  body.append(wrapper);
  if (!sessions.length) body.append(emptyState(t('agentSessionsEmpty', '没有找到 Codex 会话记录'), ''));
  return body;
}

function renderLaunchFooter(client, t, onAction) {
  const footer = document.createElement('div');
  footer.className = 'model-agent-footer';
  for (const target of client.launchTargets || []) {
    const needsDir = target.id === 'cli';
    const button = document.createElement('button');
    button.type = 'button';
    button.className = 'primary model-agent-launch-btn';
    button.dataset.action = 'agent-launch';
    button.dataset.client = client.id;
    button.dataset.target = target.id;
    button.disabled = !client.installed;
    button.innerHTML = `
      <svg class="icon" aria-hidden="true" style="width:13px;height:13px;margin-right:6px;"><polygon points="5,3 17,10 5,17" fill="currentColor" /></svg>
      <span>${t('agentLaunch', '启动')} ${escapeHtml(target.label)}</span>
    `;
    button.onclick = (event) => {
      if (!needsDir) { onAction('agent-launch', event.currentTarget); return; }
      onAction('agent-launch-prompt', { client: client.id, target: target.id });
    };
    footer.append(button);
  }
  return footer;
}

function formatBytes(bytes) {
  if (!bytes) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB'];
  let index = 0;
  let value = bytes;
  while (value >= 1024 && index < units.length - 1) { value /= 1024; index += 1; }
  return `${value.toFixed(value >= 10 || index === 0 ? 0 : 1)} ${units[index]}`;
}

function errorLine(message) {
  const node = document.createElement('p');
  node.className = 'model-quota-error';
  node.textContent = message;
  return node;
}

function warningLine(message) {
  const node = document.createElement('p');
  node.className = 'muted';
  node.textContent = message;
  return node;
}

function emptyState(title, hint) {
  const node = document.createElement('div');
  node.className = 'model-empty-state';
  node.innerHTML = `<strong>${escapeHtml(title)}</strong>${hint ? `<p class="muted">${escapeHtml(hint)}</p>` : ''}`;
  return node;
}

