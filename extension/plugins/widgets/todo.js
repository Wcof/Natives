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

    let timerInterval = null;
    let timerRemaining = (data.focusDuration || 25) * 60;
    let timerRunning = false;

    // Optional Pomodoro Focus bar
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
      toggleBtn.textContent = '▶ 专注';

      const resetBtn = document.createElement('button');
      resetBtn.type = 'button';
      resetBtn.className = 'todo-pomodoro-btn reset';
      resetBtn.textContent = '↺';
      resetBtn.title = t('reset', '重置');

      toggleBtn.onclick = () => {
        timerRunning = !timerRunning;
        toggleBtn.textContent = timerRunning ? '⏸ 暂停' : '▶ 专注';
        if (timerRunning) {
          timerInterval = setInterval(() => {
            if (timerRemaining > 0) {
              timerRemaining--;
              timeDisplay.textContent = formatTime(timerRemaining);
            } else {
              clearInterval(timerInterval);
              timerRunning = false;
              toggleBtn.textContent = '▶ 专注';
              timeDisplay.textContent = '🎉 完成!';
            }
          }, 1000);
        } else {
          clearInterval(timerInterval);
        }
      };

      resetBtn.onclick = () => {
        clearInterval(timerInterval);
        timerRunning = false;
        toggleBtn.textContent = '▶ 专注';
        timerRemaining = (data.focusDuration || 25) * 60;
        timeDisplay.textContent = formatTime(timerRemaining);
      };

      pomodoroBar.append(timeDisplay, toggleBtn, resetBtn);
      root.append(pomodoroBar);
    }

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

    return () => {
      if (timerInterval) clearInterval(timerInterval);
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
    const update = () => onChange({
      ...data,
      showPomodoro: container.querySelector('#td-pomodoro').checked,
      focusDuration: Number(container.querySelector('#td-pomodoro-dur').value) || 25,
    });
    container.querySelectorAll('input').forEach((el) => { el.onchange = update; });
    container.append(wrap);
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
    }
    .Todo .todo-pomodoro-btn.reset {
      padding: 0 6px;
    }
    .Todo .todo-pomodoro-btn:hover {
      background: rgba(255,255,255,0.3);
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
