/**
 * Space Workspace Delete Modal controller (<40 lines).
 * Handles workspace deletion confirmation dialog.
 */

export function createSpaceDeleteModal({ $, t, onDeleteWorkspaceConfirmed }) {
  function open(workspace) {
    if (!workspace) return;
    const modal = $('ws-delete-modal');
    const msg = $('ws-delete-message');
    if (msg) {
      msg.textContent = `“${workspace.name}”`;
    }
    if (modal) {
      modal.returnValue = '';
      if (typeof modal.showModal === 'function') {
        modal.showModal();
      }
      modal.onclose = () => {
        if (modal.returnValue !== 'default') return;
        onDeleteWorkspaceConfirmed(workspace);
      };
    }
  }

  return { open };
}
