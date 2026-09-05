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
              timeDisplay.textContent = t('completed', '已完成!');
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
      delBtn.innerHTML = `<svg class="icon" aria-hidden="true" style="width:11px;height:11px;"><use href="#i-close" /></svg>`;
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

    if (items.length > 0) {
      const quickBar = document.createElement('div');
      quickBar.className = 'todo-quick-bar';
      quickBar.innerHTML = `
        <button type="button" class="todo-chip-btn" data-act="ai-plan" title="一键将未完成待办生成为 AI 任务规划 Prompt">
          <svg class="icon" aria-hidden="true" style="width:11px;height:11px;"><use href="#i-bolt" /></svg>
          <span>AI 规划 Prompt</span>
        </button>
      `;
      quickBar.onclick = (e) => {
        const btn = e.target.closest('.todo-chip-btn');
        if (!btn) return;
        const pending = items.filter((it) => !(it.completed ?? it.done));
        if (!pending.length) return;
        const promptText = `## 待办任务清单\n${pending.map((it) => `- [ ] ${it.contents ?? it.text ?? ''}`).join('\n')}\n\n请作为我的个人时间效能教练，帮我：\n1. 按照四象限重要度（Eisenhower 矩阵）为以上任务进行优先级排序；\n2. 预估每个任务的合理耗时与执行策略；\n3. 制定高效的心流执行节奏建议。`;
        navigator.clipboard?.writeText(promptText);
        const span = btn.querySelector('span');
        if (span) span.textContent = '已复制 Prompt!';
        setTimeout(() => { if (span) span.textContent = 'AI 规划 Prompt'; }, 1500);
      };
      root.append(list, addInput, quickBar);
    } else {
      root.append(list, addInput);
    }
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
