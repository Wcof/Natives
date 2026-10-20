import assert from 'node:assert/strict';
import { createSpaceNameModal } from './space-modal-name.js';
import { createSpaceDeleteModal } from './space-modal-delete.js';
import { createSpaceResetModal } from './space-modal-reset.js';

console.log('--- Space Modals Unit Tests ---');

// createSpaceNameModal 直接查 DOM 中的模板选择行（P3：模板只含布局），
// Node 单测环境用最小 mock 顶住：行默认隐藏，仅新建时展示。
globalThis.document = {
  getElementById(id) {
    if (id === 'ws-template-row') return { hidden: true };
    if (id === 'ws-template-select') return { value: 'classic', onchange: null };
    if (id === 'ws-template-investment-desc') return { hidden: true };
    return null;
  },
};

function createMockElement(initial = {}) {
  return {
    textContent: initial.textContent || '',
    value: initial.value || '',
    open: initial.open || false,
    returnValue: initial.returnValue || '',
    onclose: null,
    showModal() { this.open = true; },
    close(value) { this.returnValue = value; this.open = false; this.onclose?.(); },
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
    'ws-name-cancel': createMockElement(),
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

  // 1.2 Confirm with valid name（默认 classic 模板；模板值随保存回调传出）
  inputEl.value = 'My New Workspace';
  modalEl.returnValue = 'default';
  let savedTemplate = null;
  modalEl.onclose();
  assert.equal(savedWs, null);
  assert.equal(savedName, 'My New Workspace');
  assert.equal(savedTemplate, null, '首个实例的回调不感知模板参数');
  console.log('✓ Name modal create confirmed passed');

  // 1.3 Open for rename
  savedWs = null;
  savedName = null;
  const existingWs = { id: 'ws-123', name: 'Work Project', revision: 2 };
  nameModal.open(existingWs);
  assert.equal(titleEl.textContent, '重命名空间');
  assert.equal(inputEl.value, 'Work Project');

  // 1.4 Cancel rename
  elements['ws-name-cancel'].onclick();
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

// ─── Test 1b: createSpaceNameModal 模板选择（P3 / ADR-0036）────────────────

{
  const templateSelect = { value: 'classic', onchange: null };
  globalThis.document = {
    getElementById(id) {
      if (id === 'ws-template-row') return { hidden: true };
      if (id === 'ws-template-select') return templateSelect;
      if (id === 'ws-template-investment-desc') return { hidden: true };
      return null;
    },
  };

  const inputEl = createMockElement();
  const modalEl = createMockElement();
  const elements = { 'ws-name-input': inputEl, 'ws-name-modal': modalEl, 'ws-name-cancel': createMockElement() };
  const $ = (id) => elements[id];
  const t = (k, fallback) => fallback || k;
  let savedTemplate = null;
  const nameModal = createSpaceNameModal({
    $, t,
    onSaveWorkspaceName: (_ws, _name, template) => { savedTemplate = template; },
  });

  // 新建时选择投资概览模板 → 模板值传出，且仅布局、无金额样例（Host 侧断言）
  nameModal.open(null);
  templateSelect.value = 'investment';
  inputEl.value = '投资概览';
  modalEl.returnValue = 'default';
  modalEl.onclose();
  assert.equal(savedTemplate, 'investment');

  // 重命名时模板选择行隐藏；open() 会把 select 重置回 classic，
  // rename 语义（workspace_rename）本身不受模板影响。
  savedTemplate = null;
  nameModal.open({ id: 'ws-1', name: 'Existing', revision: 1 });
  assert.equal(templateSelect.value, 'classic', '重命名时模板选择重置');
  inputEl.value = 'Renamed';
  modalEl.returnValue = 'default';
  modalEl.onclose();
  assert.equal(savedTemplate, 'classic', '重命名走 workspace_rename，模板仅新建时生效');
  console.log('✓ Name modal investment template selection passed');
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
