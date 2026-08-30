import assert from 'node:assert/strict';
import { createSpaceNameModal } from './space-modal-name.js';
import { createSpaceDeleteModal } from './space-modal-delete.js';
import { createSpaceResetModal } from './space-modal-reset.js';

console.log('--- Space Modals Unit Tests ---');

function createMockElement(initial = {}) {
  return {
    textContent: initial.textContent || '',
    value: initial.value || '',
    open: initial.open || false,
    returnValue: initial.returnValue || '',
    onclose: null,
    showModal() { this.open = true; },
    focus() { this.focused = true; },
    select() { this.selected = true; },
    ...initial,
  };
}

// ─── Test 1: createSpaceNameModal ──────────────────────────────────────────

{
  let titleEl = createMockElement();
  let inputEl = createMockElement();
  let modalEl = createMockElement();
  let savedWs = null;
  let savedName = null;

  const elements = {
    'ws-name-title': titleEl,
    'ws-name-input': inputEl,
    'ws-name-modal': modalEl,
  };
  const $ = (id) => elements[id];
  const t = (k, fallback) => fallback || k;

  const nameModal = createSpaceNameModal({
    $,
    t,
    onSaveWorkspaceName: (ws, name) => {
      savedWs = ws;
      savedName = name;
    },
  });

  // 1.1 Open for create
  nameModal.open(null);
  assert.equal(titleEl.textContent, '新建 Workspace');
  assert.equal(inputEl.value, '');
  assert.equal(modalEl.open, true);
  assert.equal(inputEl.focused, true);
  assert.equal(inputEl.selected, true);

  // 1.2 Confirm with valid name
  inputEl.value = 'My New Workspace';
  modalEl.returnValue = 'default';
  modalEl.onclose();
  assert.equal(savedWs, null);
  assert.equal(savedName, 'My New Workspace');
  console.log('✓ Name modal create confirmed passed');

  // 1.3 Open for rename
  savedWs = null;
  savedName = null;
  const existingWs = { id: 'ws-123', name: 'Work Project', revision: 2 };
  nameModal.open(existingWs);
  assert.equal(titleEl.textContent, '重命名空间');
  assert.equal(inputEl.value, 'Work Project');

  // 1.4 Cancel rename
  modalEl.returnValue = 'cancel';
  modalEl.onclose();
  assert.equal(savedWs, null);
  assert.equal(savedName, null);
  console.log('✓ Name modal rename cancelled passed');

  // 1.5 Empty name confirmation does not trigger callback
  savedWs = null;
  savedName = null;
  nameModal.open(existingWs);
  inputEl.value = '   ';
  modalEl.returnValue = 'default';
  modalEl.onclose();
  assert.equal(savedWs, null);
  assert.equal(savedName, null);
  console.log('✓ Name modal empty name rejection passed');
}

// ─── Test 2: createSpaceDeleteModal ────────────────────────────────────────

{
  let messageEl = createMockElement();
  let modalEl = createMockElement();
  let deletedWs = null;

  const elements = {
    'ws-delete-message': messageEl,
    'ws-delete-modal': modalEl,
  };
  const $ = (id) => elements[id];
  const t = (k, fallback) => fallback || k;

  const deleteModal = createSpaceDeleteModal({
    $,
    t,
    onDeleteWorkspaceConfirmed: (ws) => {
      deletedWs = ws;
    },
  });

  // 2.1 Open with null is no-op
  deleteModal.open(null);
  assert.equal(modalEl.open, false);

  // 2.2 Open for delete
  const targetWs = { id: 'ws-delete-1', name: 'Temporary Space', revision: 1 };
  deleteModal.open(targetWs);
  assert.equal(messageEl.textContent, '“Temporary Space”');
  assert.equal(modalEl.open, true);

  // 2.3 Cancel delete
  modalEl.returnValue = 'cancel';
  modalEl.onclose();
  assert.equal(deletedWs, null);
  console.log('✓ Delete modal cancel passed');

  // 2.4 Confirm delete
  deleteModal.open(targetWs);
  modalEl.returnValue = 'default';
  modalEl.onclose();
  assert.deepEqual(deletedWs, targetWs);
  console.log('✓ Delete modal confirm passed');
}

// ─── Test 3: createSpaceResetModal ─────────────────────────────────────────

{
  const options = ['classic', 'focus', 'blank'].map((template) => createMockElement({
    dataset: { resetTemplate: template },
    setAttribute(name, value) { this[name] = value; },
  }));
  const modalEl = createMockElement({
    querySelectorAll() { return options; },
  });
  let resetRequest = null;
  const resetModal = createSpaceResetModal({
    $: (id) => id === 'ws-reset-modal' ? modalEl : undefined,
    onResetWorkspace: (context, template) => { resetRequest = { context, template }; },
  });

  const context = { workspaceId: 'ws-reset-1', expectedRevision: 4 };
  resetModal.open(context);
  assert.equal(modalEl.open, true);
  assert.equal(options[0]['aria-pressed'], 'true');

  options[1].onclick();
  assert.equal(options[1]['aria-pressed'], 'true');
  assert.equal(options[0]['aria-pressed'], 'false');
  modalEl.returnValue = 'default';
  modalEl.onclose();
  assert.deepEqual(resetRequest, { context, template: 'focus' });
  console.log('✓ Reset modal template selection passed');
}

console.log('All space-modals tests passed!\n');
