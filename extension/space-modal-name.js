

export function createSpaceNameModal({ $, t, onSaveWorkspaceName }) {
  function open(workspace = null) {
    const modal = $('ws-name-modal');
    const input = $('ws-name-input');
    const title = $('ws-name-title');
    if (title) {
      title.textContent = workspace
        ? t('renameWorkspace', '重命名空间')
        : t('newWorkspace', '新建 Workspace');
    }
    if (input) {
      input.value = workspace?.name || '';
    }
    // 模板选择仅新建时可见（ADR-0036 / P3：模板只含布局，不覆盖已有空间）。
    const templateRow = document.getElementById('ws-template-row');
    const templateSelect = document.getElementById('ws-template-select');
    const investmentDesc = document.getElementById('ws-template-investment-desc');
    if (templateRow) templateRow.hidden = Boolean(workspace);
    if (templateSelect) templateSelect.value = 'classic';
    if (investmentDesc) investmentDesc.hidden = true;
    if (templateSelect && investmentDesc) {
      templateSelect.onchange = () => { investmentDesc.hidden = templateSelect.value !== 'investment'; };
    }
    if (modal) {
      $('ws-name-cancel').onclick = () => modal.close('cancel');
      modal.returnValue = '';
      if (!modal.open && typeof modal.showModal === 'function') {
        modal.showModal();
      }
      if (input) {
        input.focus();
        input.select();
      }
      modal.onclose = () => {
        $(workspace ? `workspace-menu-${workspace.id}` : 'workspace-create')?.focus();
        const name = input?.value?.trim() || '';
        if (!name || modal.returnValue !== 'default') return;
        const template = templateSelect?.value || 'classic';
        onSaveWorkspaceName(workspace, name, template);
      };
    }
  }

  return { open };
}
