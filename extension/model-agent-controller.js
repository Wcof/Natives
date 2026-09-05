/**
 * Agent clients controller (智能体配置) actions for ModelSettings.
 * State lives on the controller: statuses, model catalog, per-client selection.
 */

export async function loadAgentClients(controller, { force = false } = {}) {
  if (controller.agentStatuses && !force) return;
  controller.agentStatuses = [];
  try {
    const result = await controller.api.listAgentClients();
    controller.agentStatuses = result?.clients || [];
  } catch (error) {
    controller.agentStatuses = [];
    controller.agentLoadError = localizedError(controller, error);
  }
}

export function selectAgentClient(controller, clientId) {
  controller.agentSelectedId = clientId;
  controller.render();
  if (controller.agentStatuses.some((client) => client.id === clientId && client.installed && client.modelPicker)) {
    loadAgentModels(controller, clientId);
  }
}

export async function loadAgentModels(controller, clientId) {
  try {
    const result = await controller.api.getAgentClientModels({ client: clientId });
    controller.agentModels = result?.models || [];
    controller.agentModelsError = '';
  } catch (error) {
    controller.agentModels = [];
    controller.agentModelsError = localizedError(controller, error);
  }
  if (controller.view.activePage === 'agent') controller.render();
}

function localizedError(controller, error) {
  const message = typeof error === 'string' ? error : error?.message || String(error);
  return message;
}

export async function handleAgentAction(controller, action, target) {
  switch (action) {
    case 'agent-refresh': {
      controller.view.setLoading(true);
      await loadAgentClients(controller, { force: true });
      controller.agentModels = null;
      controller.view.setLoading(false);
      controller.render();
      return true;
    }
    case 'agent-select': {
      selectAgentClient(controller, target.dataset.client);
      return true;
    }
    case 'agent-model-change': {
      controller.agentSelections = { ...controller.agentSelections, [target.client]: target.model };
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
  const model = controller.agentSelections?.[clientId] || controller.agentModels?.[0]?.name || '';
  try {
    const method = mode === 'apply' ? controller.api.applyAgentClientConfig
      : mode === 'default' ? controller.api.defaultAgentClientConfig
      : controller.api.closeAgentClientConfig;
    const result = await method({ client: clientId, model });
    await loadAgentClients(controller, { force: true });
    controller.render();
    controller.view.showToast(result?.message || controller.t('agentApplyDone', '配置已更新'));
  } catch (error) {
    controller.showError(error);
  }
}
