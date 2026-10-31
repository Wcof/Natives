/**
 * 卡片内的可复用原语：空态、行内表单、待办行、进度行、文本域、状态与防抖提交。
 * 全部返回原生 DOM 节点/控制器，不绑定任何卡片数据模型。
 */

import { createIcon } from './card-icons.js';

/** 创建元素并写入类名与纯文本。 */
export function el(tag, className = '', text = '') {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== '') node.textContent = text;
  return node;
}

/** 空态文案。 */
export function createEmptyState(text) {
  return el('div', 'space-empty', text);
}

/**
 * 行内新增表单：输入 + 提交按钮，回车提交，提交后清空并保持焦点。
 * onSubmit 返回 false 时保留输入（例如达到数量上限）。
 */
export function createInlineForm({
  placeholder,
  ariaLabel = placeholder,
  submitLabel = 'Add',
  maxLength = 200,
  onSubmit,
} = {}) {
  const form = document.createElement('form');
  form.className = 'space-inline-form';

  const input = document.createElement('input');
  input.type = 'text';
  input.className = 'space-inline-input';
  input.placeholder = placeholder || '';
  input.setAttribute('aria-label', ariaLabel || placeholder || '');
  input.autocomplete = 'off';
  input.maxLength = maxLength;

  const submit = document.createElement('button');
  submit.type = 'submit';
  submit.className = 'space-inline-submit';
  submit.title = submitLabel;
  submit.setAttribute('aria-label', submitLabel);
  submit.append(createIcon('plus', { size: 14 }));

  form.append(input, submit);

  form.addEventListener('submit', (event) => {
    event.preventDefault();
    const value = input.value.trim();
    if (!value) return;
    const accepted = typeof onSubmit === 'function' ? onSubmit(value) : true;
    if (accepted === false) return;
    input.value = '';
    input.focus?.();
  });

  return {
    form,
    input,
    focus: () => input.focus?.(),
  };
}

/**
 * 单行条目：勾选按钮 + 文本 + 可选辅助信息 + 删除按钮。
 * 勾选/删除只回调，不自行改数据。
 */
export function createListRow({
  label,
  checked = false,
  meta = '',
  checkable = true,
  wrap = false,
  onToggle,
  onRemove,
  removeLabel = 'Remove',
  toggleLabel = label,
} = {}) {
  const row = el('div', `space-row${checked ? ' is-done' : ''}${wrap ? ' is-wrap' : ''}`);
  row.dataset.done = String(Boolean(checked));

  const toggle = document.createElement('button');
  toggle.type = 'button';
  toggle.className = 'space-row-check';
  toggle.setAttribute('aria-pressed', String(Boolean(checked)));
  toggle.setAttribute('aria-label', toggleLabel);
  toggle.append(createIcon('check', { size: 11 }));
  if (typeof onToggle === 'function') toggle.addEventListener('click', onToggle);

  const text = el('span', 'space-row-label', String(label ?? ''));
  const metaEl = el('span', 'space-row-meta', String(meta ?? ''));
  metaEl.hidden = !meta;

  const remove = document.createElement('button');
  remove.type = 'button';
  remove.className = 'space-row-remove';
  remove.title = removeLabel;
  remove.setAttribute('aria-label', `${removeLabel}: ${label}`);
  remove.append(createIcon('close', { size: 11 }));
  if (typeof onRemove === 'function') remove.addEventListener('click', onRemove);

  if (checkable) row.append(toggle);
  else {
    toggle.hidden = true;
    row.classList.add('is-static');
  }
  row.append(text, metaEl, remove);
  return row;
}

/** 进度行：标签 + 轨道 + 百分比。value 为 0–1。 */
export function createProgressRow({ label, value = 0, valueLabel = '' } = {}) {
  const ratio = Math.max(0, Math.min(1, Number(value) || 0));
  const row = el('div', 'space-progress');
  const name = el('span', 'space-progress-label', String(label ?? ''));
  const track = el('span', 'space-progress-track');
  const fill = el('span', 'space-progress-fill');
  fill.style.width = `${(ratio * 100).toFixed(1)}%`;
  track.append(fill);
  const amount = el('span', 'space-progress-value', valueLabel || `${Math.floor(ratio * 100)}%`);
  row.append(name, track, amount);
  return row;
}

/** 多行文本域；只负责输入事件，不处理持久化。 */
export function createTextarea({
  value = '',
  placeholder = '',
  rows = 5,
  maxLength = 20000,
  ariaLabel = placeholder,
  onInput,
  onBlur,
} = {}) {
  const textarea = document.createElement('textarea');
  textarea.className = 'space-textarea';
  textarea.rows = rows;
  textarea.value = String(value ?? '');
  textarea.placeholder = placeholder;
  textarea.maxLength = maxLength;
  textarea.setAttribute('aria-label', ariaLabel || placeholder || '');
  if (typeof onInput === 'function') textarea.addEventListener('input', () => onInput(textarea.value));
  if (typeof onBlur === 'function') textarea.addEventListener('blur', () => onBlur(textarea.value));
  return textarea;
}

/** 轻量状态文本（保存中/已保存）。 */
export function createStatus(initial = '') {
  const node = el('span', 'space-status', initial);
  node.setAttribute('aria-live', 'polite');
  return {
    node,
    set(text) {
      node.textContent = text;
    },
  };
}

/** 圆角 chip 按钮。 */
export function createChip({ label, icon = '', onClick, title = label } = {}) {
  const button = document.createElement('button');
  button.type = 'button';
  button.className = 'space-chip';
  button.title = title || label;
  if (icon) button.append(createIcon(icon, { size: 11 }));
  button.append(el('span', '', label));
  if (typeof onClick === 'function') button.addEventListener('click', onClick);
  return button;
}

/**
 * 防抖提交控制器。
 * 输入过程中只保留最后一次 payload；blur / dispose 前调用 flush() 立即落盘。
 */
export function createDebouncedCommit(commit, delay = 600) {
  let timer = 0;
  let pending = null;
  let dirty = false;

  function flush() {
    if (timer) {
      clearTimeout(timer);
      timer = 0;
    }
    if (!dirty) return;
    const payload = pending;
    pending = null;
    dirty = false;
    try {
      commit(payload);
    } catch (error) {
      if (globalThis.reportError) globalThis.reportError(error);
      else console.error(error);
    }
  }

  function schedule(payload) {
    pending = payload;
    dirty = true;
    if (timer) clearTimeout(timer);
    timer = setTimeout(flush, delay);
  }

  function cancel() {
    if (timer) clearTimeout(timer);
    timer = 0;
    pending = null;
    dirty = false;
  }

  return { schedule, flush, cancel, get dirty() { return dirty; } };
}
