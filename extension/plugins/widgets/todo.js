

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

    let timerInterval = null;
    let timerRemaining = (data.focusDuration || 25) * 60;
    let timerRunning = false;

    
    if (data.showPomodoro) {
      const pomodoroBar = document.createElement('div');
      pomodoroBar.className = 'todo-pomodoro-bar';
      
      const timeDisplay = document.createElement('span');
      timeDisplay.className = 'todo-pomodoro-time';
      const formatTime = (sec) => {
        const m = Math.floor(sec / 60).toString().padStart(2, '0');
        const s = (sec % 60).toString().padStart(2, '0');
        return `${m}:${s}`;
      };
      timeDisplay.textContent = formatTime(timerRemaining);

      const toggleBtn = document.createElement('button');
      toggleBtn.type = 'button';
      toggleBtn.className = 'todo-pomodoro-btn';
      const renderToggleContent = () => {
        toggleBtn.innerHTML = timerRunning
          ? `<svg class="icon" aria-hidden="true" style="width:11px;height:11px;"><use href="#i-pause" /></svg><span>${t('pause', '暂停')}</span>`
          : `<svg class="icon" aria-hidden="true" style="width:11px;height:11px;"><use href="#i-play" /></svg><span>${t('focus', '专注')}</span>`;
      };
      renderToggleContent();

      const resetBtn = document.createElement('button');
      resetBtn.type = 'button';
      resetBtn.className = 'todo-pomodoro-btn reset';
      resetBtn.innerHTML = `<svg class="icon" aria-hidden="true" style="width:11px;height:11px;"><use href="#i-refresh" /></svg>`;
      resetBtn.title = t('reset', '重置');

      toggleBtn.onclick = () => {
        timerRunning = !timerRunning;
        renderToggleContent();
        if (timerRunning) {
          timerInterval = setInterval(() => {
            if (timerRemaining > 0) {
              timerRemaining--;
              timeDisplay.textContent = formatTime(timerRemaining);
            } else {
              clearInterval(timerInterval);
              timerRunning = false;
              renderToggleContent();
              timeDisplay.textContent = t('pomodoroCompleted', '已完成!');
            }
          }, 1000);
        } else {
          clearInterval(timerInterval);
        }
      };

      resetBtn.onclick = () => {
        clearInterval(timerInterval);
        timerRunning = false;
        renderToggleContent();
        timerRemaining = (data.focusDuration || 25) * 60;
        timeDisplay.textContent = formatTime(timerRemaining);
      };

      pomodoroBar.append(timeDisplay, toggleBtn, resetBtn);
      root.append(pomodoroBar);
    }

    const pendingItems = items.filter((it) => !(it.completed ?? it.done));
    const completedCount = items.length - pendingItems.length;
    let currentFilter = 'all';

    if (items.length > 2) {
      const filterBar = document.createElement('div');
      filterBar.className = 'todo-filter-bar';
      filterBar.innerHTML = `
        <div class="todo-filter-tabs">
          <button type="button" class="todo-filter-tab active" data-filter="all">${t('all', '全部')} (${items.length})</button>
          <button type="button" class="todo-filter-tab" data-filter="pending">${t('pending', '待办')} (${pendingItems.length})</button>
          <button type="button" class="todo-filter-tab" data-filter="completed">${t('todoCompleted', '已完成')} (${completedCount})</button>
        </div>
        ${completedCount > 0 ? `<button type="button" class="todo-clear-done" title="${t('clearCompleted', '清除已完成')}">${t('clear', '清完成')}</button>` : ''}
      `;
      filterBar.onclick = (e) => {
        const tab = e.target.closest('.todo-filter-tab');
        if (tab) {
          currentFilter = tab.dataset.filter;
          filterBar.querySelectorAll('.todo-filter-tab').forEach((b) => b.classList.toggle('active', b === tab));
          renderItems();
          return;
        }
        const clearBtn = e.target.closest('.todo-clear-done');
        if (clearBtn) {
          const next = items.filter((it) => !(it.completed ?? it.done));
          onDataChange?.({ ...data, items: next });
        }
      };
      root.append(filterBar);
    }

    const list = document.createElement('div');
    list.className = 'TodoList';

    function renderItems() {
      list.replaceChildren();
      items.forEach((item, index) => {
        const isDone = Boolean(item.completed ?? item.done);
        if (currentFilter === 'pending' && isDone) return;
        if (currentFilter === 'completed' && !isDone) return;

        const row = document.createElement('div');
        row.className = `TodoItem ${isDone ? 'done' : ''}`;

        const pri = item.priority || '';
        const priBtn = document.createElement('button');
        priBtn.type = 'button';
        priBtn.className = `todo-pri-badge ${pri}`;
        priBtn.textContent = pri ? pri.toUpperCase() : '·';
        priBtn.title = t('todoPriority', '切换优先级 (P0 / P1 / P2)');
        priBtn.onclick = (e) => {
          e.stopPropagation();
          const cycle = { '': 'p0', p0: 'p1', p1: 'p2', p2: '' };
          const nextPri = cycle[item.priority || ''] || undefined;
          const next = [...items];
          next[index] = { ...next[index], priority: nextPri };
          onDataChange?.({ ...data, items: next });
        };

        const cb = document.createElement('input');
        cb.type = 'checkbox';
        cb.className = 'todo-checkbox';
        cb.checked = isDone;
        cb.onchange = (e) => {
          const next = [...items];
          next[index] = 'done' in next[index]
            ? { ...next[index], done: e.target.checked }
            : { ...next[index], completed: e.target.checked };
          onDataChange?.({ ...data, items: next });
        };

        const textSpan = document.createElement('span');
        textSpan.className = 'todo-text';
        const originalText = item.contents ?? item.text ?? '';
        textSpan.textContent = originalText;

        const editInput = document.createElement('input');
        editInput.className = 'todo-edit-input';
        editInput.type = 'text';
        editInput.value = originalText;
        editInput.hidden = true;
        let editing = false;
        const finishEdit = (save) => {
          if (!editing) return;
          editing = false;
          const nextText = editInput.value.trim();
          editInput.hidden = true;
          textSpan.hidden = false;
          if (!save || !nextText || nextText === originalText) {
            editInput.value = originalText;
            return;
          }
          textSpan.textContent = nextText;
          const next = [...items];
          next[index] = { ...next[index], ...('contents' in next[index] ? { contents: nextText } : { text: nextText }) };
          onDataChange?.({ ...data, items: next });
        };
        row.ondblclick = (e) => {
          if (e.target?.closest?.('button, input')) return;
          e.stopPropagation();
          if (editing) return;
          editing = true;
          textSpan.hidden = true;
          editInput.hidden = false;
          editInput.focus?.();
          editInput.select?.();
        };
        editInput.onkeydown = (e) => {
          if (e.key === 'Enter') finishEdit(true);
          else if (e.key === 'Escape') finishEdit(false);
        };
        editInput.onblur = () => finishEdit(true);

        const delBtn = document.createElement('button');
        delBtn.className = 'todo-del-btn';
        delBtn.type = 'button';
        delBtn.innerHTML = `<svg class="icon" aria-hidden="true" style="width:11px;height:11px;"><use href="#i-trash" /></svg>`;
        delBtn.title = t('delete', '删除');
        delBtn.setAttribute('aria-label', t('delete', '删除'));
        delBtn.onclick = (e) => {
          e.stopPropagation();
          const next = items.filter((_, i) => i !== index);
          onDataChange?.({ ...data, items: next });
        };

        row.append(priBtn, cb, textSpan, editInput, delBtn);
        list.append(row);
      });
    }

    renderItems();
    root.append(list);

    const addTrigger = document.createElement('button');
    addTrigger.type = 'button';
    addTrigger.className = 'todo-add-trigger';
    addTrigger.textContent = `+ ${t('addTodo', '添加待办事项...')}`;
    root.append(addTrigger);

    const footer = document.createElement('div');
    footer.className = 'todo-footer';
    footer.hidden = true;

    const addInput = document.createElement('input');
    addInput.className = 'todo-add-input';
    addInput.type = 'text';
    addInput.placeholder = `+ ${t('addTodo', '添加待办事项...')}`;

    function expandFooter(focusInput = true) {
      if (!footer.hidden) return;
      footer.hidden = false;
      addTrigger.hidden = true;
      if (focusInput && typeof addInput.focus === 'function') {
        addInput.focus();
      }
    }

    function collapseFooter() {
      if (footer.hidden) return;
      if (addInput.value.trim() || !batchBox.hidden) return;
      footer.hidden = true;
      addTrigger.hidden = false;
    }

    addTrigger.onclick = (e) => {
      e.stopPropagation();
      expandFooter(true);
    };

    container.addEventListener('click', () => {
      expandFooter(false);
    });

    const onWindowPointerDown = (e) => {
      const path = e.composedPath ? e.composedPath() : [];
      const inside = path.includes(container) || (typeof container.contains === 'function' && container.contains(e.target));
      if (!inside) {
        collapseFooter();
      }
    };
    window.addEventListener('pointerdown', onWindowPointerDown);

    addInput.onkeydown = (e) => {
      if (e.key === 'Enter' && addInput.value.trim()) {
        const next = [...items, { id: crypto.randomUUID(), contents: addInput.value.trim(), completed: false }];
        addInput.value = '';
        onDataChange?.({ ...data, items: next });
      } else if (e.key === 'Escape') {
        addInput.value = '';
        collapseFooter();
      }
    };
    footer.append(addInput);

    const batchBox = document.createElement('div');
    batchBox.className = 'todo-batch-box';
    batchBox.hidden = true;
    batchBox.innerHTML = `
      <textarea class="todo-batch-textarea" placeholder="${t('pasteChecklist', '粘贴任务清单（支持 Markdown - [ ] 或 1. 列表）...')}"></textarea>
      <div class="todo-batch-actions">
        <button type="button" class="todo-chip-btn batch-confirm">${t('confirmImport', '确认导入')}</button>
        <button type="button" class="todo-chip-btn batch-cancel">${t('cancel', '取消')}</button>
      </div>
    `;
    batchBox.querySelector('.batch-cancel').onclick = () => {
      batchBox.hidden = true;
    };
    batchBox.querySelector('.batch-confirm').onclick = () => {
      const txt = batchBox.querySelector('.todo-batch-textarea').value.trim();
      if (txt) {
        const parsed = parseBatchTodos(txt);
        if (parsed.length > 0) {
          onDataChange?.({ ...data, items: [...items, ...parsed] });
        }
      }
      batchBox.hidden = true;
    };
    footer.append(batchBox);

    const quickBar = document.createElement('div');
    quickBar.className = 'todo-quick-bar';
    quickBar.innerHTML = `
      <button type="button" class="todo-chip-btn" data-act="batch-import" title="粘贴任务清单批量导入">
        <svg class="icon" aria-hidden="true" style="width:11px;height:11px;"><use href="#i-plus" /></svg>
        <span>批量导入</span>
      </button>
    `;
    quickBar.onclick = (e) => {
      const btn = e.target.closest('.todo-chip-btn');
      if (!btn) return;
      const act = btn.dataset.act;
      if (act === 'batch-import') {
        batchBox.hidden = !batchBox.hidden;
        if (!batchBox.hidden) {
          batchBox.querySelector('.todo-batch-textarea').focus();
        }
      }
    };
    footer.append(quickBar);
    root.append(footer);
    container.append(root);

    return () => {
      if (timerInterval) clearInterval(timerInterval);
      window.removeEventListener('pointerdown', onWindowPointerDown);
      container.replaceChildren();
    };
  },
  renderSettings(container, data = {}, onChange = () => {}, { t = (k, f) => f || k } = {}) {
    container.replaceChildren();
    const wrap = document.createElement('div');
    wrap.className = 'inspector-field-group';
    wrap.innerHTML = `
      <label class="inspector-checkbox">
        <input type="checkbox" id="td-pomodoro" ${data.showPomodoro ? 'checked' : ''} />
        <span>${t('enablePomodoroFocus', '启用番茄钟 / 专注计时器')}</span>
      </label>
      <label class="inspector-field">
        <span>${t('pomodoroDuration', '单轮专注时长（分钟）')}</span>
        <input type="number" id="td-pomodoro-dur" min="5" max="120" value="${data.focusDuration || 25}" />
      </label>
      <div class="inspector-notice">
        ${t('todoNotice', '待办事项支持在 Dashboard 上直接勾选完成、回车新增与悬停删除。')}
      </div>
    `;
    container.append(wrap);
    const update = () => onChange({
      ...data,
      showPomodoro: Boolean(wrap.querySelector('#td-pomodoro')?.checked),
      focusDuration: (() => { const n = Number(wrap.querySelector('#td-pomodoro-dur')?.value); return Number.isFinite(n) ? Math.max(5, Math.min(120, n)) : 25; })(),
    });
    wrap.querySelectorAll('input').forEach((el) => { el.onchange = update; });
  },
  styles: `
    .Todo .todo-content {
      display: inline-flex;
      flex-direction: column;
    }
    .Todo .todo-pomodoro-bar {
      display: flex;
      align-items: center;
      justify-content: center;
      gap: 8px;
      padding: 4px 10px;
      margin-bottom: 8px;
      background: rgba(255,255,255,0.12);
      border-radius: 999px;
      border: 1px solid rgba(255,255,255,0.25);
    }
    .Todo .todo-pomodoro-time {
      font-variant-numeric: tabular-nums;
      font-size: 13px;
      font-weight: 600;
    }
    .Todo .todo-pomodoro-btn {
      min-height: 22px;
      height: 22px;
      padding: 0 8px;
      border-radius: 999px;
      border: 1px solid rgba(255,255,255,0.3);
      background: rgba(255,255,255,0.15);
      color: inherit;
      font-size: 11px;
      cursor: pointer;
      display: inline-flex;
      align-items: center;
      justify-content: center;
      gap: 4px;
    }
    .Todo .todo-pomodoro-btn.reset {
      padding: 0 6px;
    }
    .Todo .todo-pomodoro-btn:hover {
      background: rgba(255,255,255,0.3);
    }
    .Todo .todo-quick-bar {
      display: flex;
      align-items: center;
      gap: 6px;
      margin-top: 6px;
    }
    .Todo .todo-chip-btn {
      display: inline-flex;
      align-items: center;
      gap: 4px;
      padding: 2px 8px;
      border-radius: 999px;
      border: 1px solid rgba(255,255,255,0.22);
      background: rgba(255,255,255,0.08);
      color: inherit;
      font-size: 10.5px;
      cursor: pointer;
      opacity: 0.82;
      transition: opacity .15s, background .15s;
    }
    .Todo .todo-chip-btn:hover {
      opacity: 1;
      background: rgba(255,255,255,0.18);
    }
    .Todo .TodoList { display:inline-block; margin:.25em 0; max-height:35vh; overflow:hidden; }
    .Todo .TodoList:hover { overflow-y:auto; }
    .Todo .TodoItem { white-space:nowrap; border-top:2px solid transparent; border-bottom:2px solid transparent; }
    .Todo .TodoItem > * { display:inline-block; margin:.25em .5em; }
    .Todo .TodoItem > [hidden] { display: none !important; }
    .Todo .todo-checkbox { cursor:pointer; }
    .Todo .todo-text { min-width:8em; text-align:left; }
    .Todo .todo-edit-input {
      min-width: 8em;
      max-width: 18em;
      background: transparent;
      border: 0;
      border-bottom: 1px solid currentColor;
      color: inherit;
      font: inherit;
      outline: none;
    }
    .Todo .todo-edit-input:focus { border-bottom-color: rgba(255,255,255,0.7); }
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
    .Todo .todo-add-trigger {
      display: inline-block;
      width: 100%;
      background: transparent;
      border: 0;
      color: inherit;
      font: inherit;
      font-size: 12px;
      text-align: left;
      opacity: 0.55;
      cursor: pointer;
      padding: 4px 6px;
      border-radius: 4px;
      margin-top: 4px;
      transition: opacity 0.15s, background 0.15s;
    }
    .Todo .todo-add-trigger:hover {
      opacity: 0.9;
      background: rgba(255, 255, 255, 0.08);
    }
    .Todo .todo-footer {
      display: flex;
      flex-direction: column;
      gap: 4px;
      margin-top: 4px;
    }
    .Todo .todo-footer[hidden], .Todo .todo-batch-box[hidden] { display: none; }
    .Todo .todo-add-input {
      width: 100%;
      background:transparent;
      border:0;
      border-bottom:1px solid currentColor;
      color:inherit;
      font:inherit;
    }
    .Todo .todo-filter-bar {
      display: flex;
      align-items: center;
      gap: 6px;
      margin-bottom: 4px;
      font-size: 11px;
      opacity: 0.85;
    }
    .Todo .todo-filter-tabs {
      display: flex;
      gap: 3px;
    }
    .Todo .todo-filter-tab {
      background: transparent;
      border: 0;
      color: inherit;
      font-size: 11px;
      cursor: pointer;
      padding: 2px 6px;
      border-radius: 4px;
      opacity: 0.65;
    }
    .Todo .todo-filter-tab:hover { opacity: 0.9; }
    .Todo .todo-filter-tab.active {
      opacity: 1;
      font-weight: 600;
      background: rgba(255,255,255,0.15);
    }
    .Todo .todo-clear-done {
      background: transparent;
      border: 0;
      color: inherit;
      font-size: 10.5px;
      cursor: pointer;
      opacity: 0.6;
      padding: 1px 4px;
    }
    .Todo .todo-clear-done:hover { opacity: 1; text-decoration: underline; }
    .Todo .todo-pri-badge {
      border: 0;
      border-radius: 3px;
      font-size: 9.5px;
      font-weight: 700;
      padding: 1px 3px;
      cursor: pointer;
      background: rgba(255,255,255,0.1);
      color: inherit;
      line-height: 1;
      vertical-align: middle;
      opacity: 0.65;
      margin-right: -2px;
    }
    .Todo .todo-pri-badge:hover { opacity: 1; }
    .Todo .todo-pri-badge.p0 { background: #ef4444; color: #fff; opacity: 1; }
    .Todo .todo-pri-badge.p1 { background: #f59e0b; color: #000; opacity: 1; }
    .Todo .todo-pri-badge.p2 { background: #3b82f6; color: #fff; opacity: 1; }
    .Todo .todo-batch-box {
      display: flex;
      flex-direction: column;
      gap: 6px;
      margin: 6px 0;
      padding: 8px;
      background: rgba(0,0,0,0.25);
      border: 1px solid rgba(255,255,255,0.2);
      border-radius: 6px;
      text-align: left;
    }
    .Todo .todo-batch-textarea {
      width: 100%;
      min-height: 60px;
      background: transparent;
      border: 1px solid rgba(255,255,255,0.2);
      border-radius: 4px;
      color: inherit;
      font-size: 11.5px;
      font-family: inherit;
      resize: vertical;
      box-sizing: border-box;
      padding: 4px 6px;
    }
    .Todo .todo-batch-actions {
      display: flex;
      gap: 6px;
      justify-content: flex-end;
    }
  `,
};

function parseBatchTodos(text) {
  const lines = text.split('\n');
  const parsed = [];
  for (const raw of lines) {
    let line = raw.trim();
    if (!line) continue;
    let priority = undefined;
    if (/^\[?p0\]?[:\s]/i.test(line)) {
      priority = 'p0';
      line = line.replace(/^\[?p0\]?[:\s]*/i, '');
    } else if (/^\[?p1\]?[:\s]/i.test(line)) {
      priority = 'p1';
      line = line.replace(/^\[?p1\]?[:\s]*/i, '');
    } else if (/^\[?p2\]?[:\s]/i.test(line)) {
      priority = 'p2';
      line = line.replace(/^\[?p2\]?[:\s]*/i, '');
    }
    line = line.replace(/^[-*+]\s+\[[ xX]\]\s*/, '')
               .replace(/^[-*+]\s+/, '')
               .replace(/^\d+[.)]\s+/, '')
               .trim();
    if (line) {
      parsed.push({
        id: crypto.randomUUID(),
        contents: line,
        completed: false,
        ...(priority ? { priority } : {}),
      });
    }
  }
  return parsed;
}
