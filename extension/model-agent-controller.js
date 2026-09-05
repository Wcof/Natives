/**
 * Agent clients controller (智能体配置) actions for ModelSettings.
 * State lives on the controller: statuses, model catalog, per-client selection,
 * mappings, codex sessions, pi status.
 */

export async function loadAgentClients(controller, { force = false } = {}) {
  if (controller.agentStatuses && !force) return;
  controller.agentStatuses = [];
  try {
    const result = await controller.api.listAgentClients();
    controller.agentStatuses = result?.clients || [];
  } catch (error) {
    controller.agentStatuses = [];
    controller.agentLoadError = errorMessage(error);
  }
  controller.agentSelectedId = controller.agentSelectedId || controller.agentStatuses[0]?.id || '';
  await refreshAgentModels(controller, controller.agentSelectedId);
}

export function selectAgentClient(controller, clientId) {
  controller.agentSelectedId = clientId;
  controller.agentActiveTab = 'core';
  controller.render();
  refreshAgentModels(controller, clientId);
}

async function refreshAgentModels(controller, clientId) {
  const client = controller.agentStatuses.find((item) => item.id === clientId);
  if (!client?.installed || !client?.modelPicker) return;
  try {
    const result = await controller.api.getAgentClientModels({ client: clientId });
    controller.agentModels = result?.models || [];
    controller.agentModelsError = '';
  } catch (error) {
    controller.agentModels = [];
    controller.agentModelsError = errorMessage(error);
  }
  if (controller.view.activePage === 'agent') controller.render();
}

async function loadCodexSessions(controller) {
  try {
    const result = await controller.api.listCodexSessions();
    controller.agentSessions = result?.sessions || [];
  } catch (error) {
    controller.agentSessions = [];
    controller.agentModelsError = errorMessage(error);
  }
}

function errorMessage(error) {
  return typeof error === 'string' ? error : error?.message || String(error);
}

export async function handleAgentAction(controller, action, target) {
  switch (action) {
    case 'agent-refresh': {
      controller.view.setLoading(true);
      controller.agentStatuses = null;
      try {
        await loadAgentClients(controller, { force: true });
      } finally {
        controller.view.setLoading(false);
      }
      controller.render();
      return true;
    }
    case 'agent-select': {
      selectAgentClient(controller, target.dataset.client);
      return true;
    }
    case 'agent-tab': {
      controller.agentActiveTab = target.dataset.tab;
      if (controller.agentActiveTab === 'sessions') {
        controller.view.setLoading(true);
        try {
          await loadCodexSessions(controller);
        } finally {
          controller.view.setLoading(false);
        }
      }
      controller.render();
      return true;
    }
    case 'agent-model-change': {
      controller.agentSelections = { ...controller.agentSelections, [target.client]: target.model };
      return true;
    }
    case 'agent-mapping-change': {
      controller.agentSelections = { ...controller.agentSelections, [`${target.client}:mappings`]: target.mappings };
      return true;
    }
    case 'agent-models-refresh': {
      await refreshAgentModels(controller, target.client);
      return true;
    }
    case 'agent-apply':
    case 'agent-default':
    case 'agent-close-config': {
      const clientId = target?.dataset?.client || controller.agentSelectedId;
      const mode = action === 'agent-apply' ? 'apply' : action === 'agent-default' ? 'default' : 'close';
      if (!clientId) return true;
      if (mode === 'close') {
        controller.confirmDelete(controller.t('agentCloseConfirm', '确定关闭配置修改并还原该客户端的原配置吗？'), () =>
          runAgentMutation(controller, clientId, mode));
        return true;
      }
      await runAgentMutation(controller, clientId, mode);
      return true;
    }
    case 'agent-codex-clear': {
      controller.confirmDelete(controller.t('agentCodexClearConfirm', '确定清空 Codex 配置与凭据文件吗？'), async () => {
        try {
          const result = await controller.api.clearCodexConfig();
          await loadAgentClients(controller, { force: true });
          controller.render();
          controller.view.showToast(controller.t('agentCodexCleared', '已清空 $1 个文件').replace('$1', result?.deleted?.length || 0));
        } catch (error) {
          controller.showError(error);
        }
      });
      return true;
    }
    case 'agent-session-delete': {
      const path = target.dataset.path;
      controller.confirmDelete(controller.t('agentSessionDeleteConfirm', '确定删除该会话记录吗？'), async () => {
        try {
          await controller.api.deleteCodexSessions({ paths: [path] });
          await loadCodexSessions(controller);
          controller.render();
          controller.view.showToast(controller.t('agentSessionDeleted', '会话已删除'));
        } catch (error) {
          controller.showError(error);
        }
      });
      return true;
    }
    case 'agent-pi': {
      controller.agentBusy = true;
      controller.render();
      try {
        const model = controller.agentSelections?.[controller.agentSelectedId] || controller.agentModels?.[0]?.name || '';
        const result = await controller.api.piProviderAction({ action: target.action, model });
        await loadAgentClients(controller, { force: true });
        controller.view.showToast((result?.output || controller.t('agentPiDone', 'Pi 插件操作完成')).slice(-200));
      } catch (error) {
        controller.showError(error);
      } finally {
        controller.agentBusy = false;
        controller.render();
      }
      return true;
    }
    case 'agent-launch-prompt': {
      const { client, target: launchTarget } = target;
      const history = JSON.parse(localStorage.getItem('natives-agent-launch-dirs') || '[]');
      const options = history.map((dir) => `<option value="${dir}"></option>`).join('');
      controller.promptInput(controller.t('agentLaunchDirPrompt', '启动工作目录（留空使用主目录）：'), history[0] || '', (dir) => {
        if (dir && !history.includes(dir)) {
          const next = [dir, ...history].slice(0, 8);
          localStorage.setItem('natives-agent-launch-dirs', JSON.stringify(next));
        }
        controller.api.launchAgentClient({ client, target: launchTarget, workingDirectory: dir })
          .then(() => controller.view.showToast(controller.t('agentLaunched', '已启动')))
          .catch((error) => controller.showError(error));
      }, options);
      return true;
    }
    case 'agent-launch': {
      try {
        await controller.api.launchAgentClient({ client: target.dataset.client, target: target.dataset.target });
        controller.view.showToast(controller.t('agentLaunched', '已启动'));
      } catch (error) {
        controller.showError(error);
      }
      return true;
    }
    default:
      return false;
  }
}

async function runAgentMutation(controller, clientId, mode) {
  const mappings = controller.agentSelections?.[`${clientId}:mappings`] || null;
  const model = controller.agentSelections?.[clientId] || controller.agentModels?.[0]?.name || '';
  try {
    const method = mode === 'apply' ? controller.api.applyAgentClientConfig
      : mode === 'default' ? controller.api.defaultAgentClientConfig
      : controller.api.closeAgentClientConfig;
    const result = await method({ client: clientId, model, claudeCodeModelMappings: mappings });
    await loadAgentClients(controller, { force: true });
    controller.render();
    controller.view.showToast(result?.message || controller.t('agentApplyDone', '配置已更新'));
  } catch (error) {
    controller.showError(error);
  }
}
