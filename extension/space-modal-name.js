/**
 * Space Workspace Name Modal controller (<50 lines).
 * Handles workspace create and rename dialog interactions.
 */

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
    if (modal) {
      modal.returnValue = '';
      if (!modal.open && typeof modal.showModal === 'function') {
        modal.showModal();
      }
      if (input) {
        if (typeof input.focus === 'function') input.focus();
        if (typeof input.select === 'function') input.select();
      }
      modal.onclose = () => {
        const name = input?.value?.trim() || '';
        if (!name || modal.returnValue !== 'default') return;
        onSaveWorkspaceName(workspace, name);
      };
    }
  }

  return { open };
}
