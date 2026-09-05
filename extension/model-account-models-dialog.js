function element(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text) node.textContent = text;
  return node;
}

export async function openAccountModelsDialog(controller, account) {
  const { t } = controller;
  const dialog = element('dialog', 'model-account-models-dialog');
  dialog.setAttribute('aria-labelledby', 'account-models-title');
  const header = element('div', 'model-account-models-header');
  const title = element('h2', '', t('modelAccountModelsTitle', '账号模型'));
  title.id = 'account-models-title';
  const close = element('button', 'icon-button', '×');
  close.type = 'button';
  close.setAttribute('aria-label', t('close', '关闭'));
  header.append(title, close);

  const search = element('input', 'model-account-models-search');
  search.type = 'search';
  search.placeholder = t('modelAccountModelsSearch', '搜索模型');
  const toolbar = element('div', 'model-account-models-toolbar');
  const summary = element('span', 'muted');
  const selectVisible = element('button', '', t('modelAccountModelsSelectVisible', '切换当前结果'));
  const selectAll = element('button', '', t('modelAccountModelsSelectAll', '全部开启'));
  const selectNone = element('button', '', t('modelAccountModelsSelectNone', '全部关闭'));
  for (const button of [selectVisible, selectAll, selectNone]) button.type = 'button';
  toolbar.append(summary, selectVisible, selectAll, selectNone);

  const content = element('div', 'model-account-models-list');
  content.setAttribute('aria-live', 'polite');
  const actions = element('div', 'model-account-models-actions');
  const cancel = element('button', '', t('cancel', '取消'));
  const save = element('button', 'primary', t('save', '保存'));
  cancel.type = save.type = 'button';
  actions.append(cancel, save);
  dialog.append(header, search, toolbar, content, actions);
  document.body.append(dialog);
  dialog.showModal();

  let models = [];
  const enabled = new Set();
  const visibleModels = () => {
    const query = search.value.trim().toLowerCase();
    return query ? models.filter((model) => `${model.id} ${model.displayName || ''}`.toLowerCase().includes(query)) : models;
  };
  const render = () => {
    const visible = visibleModels();
    summary.textContent = t('modelAccountModelsSummary', '$1 个模型，已开启 $2 个').replace('$1', models.length).replace('$2', enabled.size);
    content.replaceChildren();
    if (!visible.length) {
      content.append(element('p', 'model-empty-state', t('modelAccountModelsEmpty', '没有可用模型')));
      return;
    }
    for (const model of visible) {
      const row = element('label', 'model-account-model-row');
      const checkbox = document.createElement('input');
      checkbox.type = 'checkbox';
      checkbox.checked = enabled.has(model.id.toLowerCase());
      checkbox.onchange = () => {
        if (checkbox.checked) enabled.add(model.id.toLowerCase());
        else enabled.delete(model.id.toLowerCase());
        render();
      };
      const label = element('span');
      label.append(element('strong', '', model.id));
      if (model.displayName && model.displayName !== model.id) label.append(element('small', 'muted', model.displayName));
      row.append(checkbox, label);
      content.append(row);
    }
  };
  const dispose = () => dialog.close();
  dialog.addEventListener('close', () => dialog.remove(), { once: true });
  close.onclick = cancel.onclick = dispose;
  search.oninput = render;
  selectAll.onclick = () => { enabled.clear(); for (const model of models) enabled.add(model.id.toLowerCase()); render(); };
  selectNone.onclick = () => { enabled.clear(); render(); };
  selectVisible.onclick = () => {
    const visible = visibleModels();
    const allEnabled = visible.every((model) => enabled.has(model.id.toLowerCase()));
    for (const model of visible) {
      if (allEnabled) enabled.delete(model.id.toLowerCase());
      else enabled.add(model.id.toLowerCase());
    }
    render();
  };
  save.onclick = async () => {
    save.disabled = true;
    try {
      await controller.api.updateAccountModels({
        accountId: account.accountId,
        enabledModelIds: [...enabled],
        expectedRevision: controller.snapshot.revision,
      });
      controller.view.showNotice(t('modelAccountModelsSaved', '账号模型设置已保存'));
      dispose();
    } catch (error) {
      controller.showError(error);
      save.disabled = false;
    }
  };

  content.append(element('p', 'model-empty-state', t('loading', '加载中…')));
  try {
    const result = await controller.api.getAccountModels({ accountId: account.accountId });
    models = result.models || [];
    for (const model of models) if (model.enabled) enabled.add(model.id.toLowerCase());
    render();
    search.focus();
  } catch (error) {
    content.replaceChildren(element('p', 'model-empty-state', t('modelAccountModelsLoadFailed', '模型加载失败')));
    controller.showError(error);
    save.disabled = true;
  }
}
