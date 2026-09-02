/**
 * To Do List Widget.
 * Clean TablissNG TodoList.sass and TodoItem.sass implementation.
 */

import { escapeHtml } from '../sanitizer.js';

export const todoWidget = {
  key: 'widget/todo',
  name: 'To Do',
  defaultData: {
    items: [],
  },
  render(container, data = {}, display = {}, { t = (k, f) => f || k, onDataChange } = {}) {
    const items = Array.isArray(data.items) ? data.items : [];

    container.className = 'Widget Todo';
    container.replaceChildren();

    const root = document.createElement('div');
    root.className = 'todo-content';

    const list = document.createElement('div');
    list.className = 'TodoList';

    items.forEach((item, index) => {
      const row = document.createElement('div');
      row.className = `TodoItem ${item.done ? 'done' : ''}`;

      const cb = document.createElement('input');
      cb.type = 'checkbox';
      cb.className = 'todo-checkbox';
      cb.checked = Boolean(item.completed ?? item.done);
      cb.onchange = (e) => {
        const next = [...items];
        next[index] = 'done' in next[index]
          ? { ...next[index], done: e.target.checked }
          : { ...next[index], completed: e.target.checked };
        onDataChange?.({ ...data, items: next });
      };

      const textSpan = document.createElement('span');
      textSpan.className = 'todo-text';
      textSpan.textContent = item.contents ?? item.text ?? '';

      const delBtn = document.createElement('button');
      delBtn.className = 'todo-del-btn';
      delBtn.type = 'button';
      delBtn.innerHTML = '×';
      delBtn.title = t('delete', '删除');
      delBtn.onclick = (e) => {
        e.stopPropagation();
        const next = items.filter((_, i) => i !== index);
        onDataChange?.({ ...data, items: next });
      };

      row.append(cb, textSpan, delBtn);
      list.append(row);
    });

    const addInput = document.createElement('input');
    addInput.className = 'todo-add-input';
    addInput.type = 'text';
    addInput.placeholder = `+ ${t('addTodo', '添加待办事项...')}`;
    addInput.onkeydown = (e) => {
      if (e.key === 'Enter' && addInput.value.trim()) {
        const next = [...items, { id: crypto.randomUUID(), contents: addInput.value.trim(), completed: false }];
        addInput.value = '';
        onDataChange?.({ ...data, items: next });
      }
    };

    root.append(list, addInput);
    container.append(root);

    return () => container.replaceChildren();
  },
  renderSettings(container, data = {}, onChange = () => {}, { t = (k, f) => f || k } = {}) {
    container.replaceChildren();
    const notice = document.createElement('div');
    notice.className = 'inspector-notice';
    notice.textContent = t('todoNotice', '待办事项支持在 Dashboard 上直接勾选完成、回车新增与悬停删除。');
    container.append(notice);
  },
  styles: `
    .Todo .todo-content {
      display: inline-flex;
      flex-direction: column;
    }
    .Todo .TodoList { display:inline-block; margin:.25em 0; max-height:35vh; overflow:hidden; }
    .Todo .TodoList:hover { overflow-y:auto; }
    .Todo .TodoItem { white-space:nowrap; border-top:2px solid transparent; border-bottom:2px solid transparent; }
    .Todo .TodoItem > * { display:inline-block; margin:.25em .5em; }
    .Todo .todo-checkbox { cursor:pointer; }
    .Todo .todo-text { min-width:8em; text-align:left; }
    .Todo .TodoItem.done .todo-text, .Todo .TodoItem:has(.todo-checkbox:checked) .todo-text {
      text-decoration: line-through;
      opacity: 0.55;
    }
    .Todo .todo-del-btn {
      visibility:hidden;
      background:transparent;
      border:0;
      color:inherit;
      cursor: pointer;
    }
    .Todo .TodoItem:hover .todo-del-btn { visibility:visible; }
    .Todo .todo-add-input {
      width: 100%;
      background:transparent;
      border:0;
      border-bottom:1px solid currentColor;
      color:inherit;
      font:inherit;
    }
  `,
};
