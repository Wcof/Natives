export async function handleAdvancedAction(controller, action, target, revision) {
  if (action === 'select-advanced-tab') {
    controller.view.setAdvancedTab(target.dataset.tab);
    controller.render();
  } else if (action === 'create-gateway-key') {
    const result = await controller.mutate(() => controller.api.createGatewayKey({ expectedRevision: revision, name: controller.t('modelCreateKeyDefaultName', '新建密钥') }));
    if (result?.accessKey) await copied(controller, result.accessKey, 'modelKeyCreatedAndCopied', '访问密钥已创建并复制到剪贴板');
  } else if (action === 'reveal-key-id') {
    try {
      const result = await controller.api.revealGatewayKey({ keyId: target.dataset.keyId });
      await copied(controller, result.accessKey, 'modelKeyCopied', '密钥已复制到剪贴板');
    } catch (error) { controller.showError(error); }
  } else if (action === 'rotate-key-id') {
    const result = await controller.mutate(() => controller.api.rotateGatewayKey({ keyId: target.dataset.keyId, expectedRevision: revision }));
    if (result?.accessKey) await copied(controller, result.accessKey, 'modelKeyRotatedAndCopied', '密钥已重置并复制到剪贴板');
  } else if (action === 'toggle-key-id') {
    await controller.mutate(() => controller.api.updateGatewayKey({ keyId: target.dataset.keyId, enabled: target.dataset.enabled === 'true', expectedRevision: revision }));
  } else if (action === 'rename-key-id') {
    const current = target.dataset.currentName || '';
    controller.promptInput(controller.t('modelPromptKeyName', '请输入新的密钥名称：'), current, (name) => {
      if (name && name !== current) {
        controller.mutate(() => controller.api.updateGatewayKey({ keyId: target.dataset.keyId, name, expectedRevision: revision }));
      }
    });
  } else if (action === 'delete-key-id') {
    controller.confirmDelete(controller.t('modelDeleteKeyConfirm', '确定删除此访问密钥吗？'), () => controller.api.deleteGatewayKey({ keyId: target.dataset.keyId, expectedRevision: revision }));
  } else return false;
  return true;
}

export async function handleAdvancedSubmit(controller, form) {
  const role = form.dataset.role;
  if (!['basic-settings-form', 'network-settings-form', 'kernel-settings-form'].includes(role)) return false;
  const data = Object.fromEntries(new FormData(form));
  const settings = { ...(controller.snapshot?.gateway?.settings || {}) };
  if (role === 'basic-settings-form') settings.preferredPort = Number(data.preferredPort) || 0;
  if (role === 'network-settings-form') Object.assign(settings, {
    proxyUrl: data.proxyUrl || '', routingStrategy: data.routingStrategy,
    sessionAffinity: form.elements.sessionAffinity.checked, sessionAffinityTtl: Number(data.sessionAffinityTtl) || 0,
  });
  if (role === 'kernel-settings-form') {
    for (const key of ['requestRetry', 'maxRetryCredentials', 'maxRetryIntervalSeconds', 'streamingBootstrapRetries']) settings[key] = Number(data[key]) || 0;
  }
  await controller.mutate(() => controller.api.updateGatewaySettings({ settings, expectedRevision: controller.snapshot.revision }));
  controller.view.showNotice(controller.t('modelSettingsSaved', '设置已保存'));
  return true;
}

async function copied(controller, value, key, fallback) {
  if (await controller.copyText(value)) controller.view.showToast(controller.t(key, fallback));
}
