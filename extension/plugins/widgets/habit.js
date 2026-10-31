/**
 * 习惯打卡：最多 12 个习惯，最近 7 天网格 + 今日打卡。
 * 打卡记录按日累积，只保留最近 30 天，避免 configJson 无限增长。
 */

import {
  cardMeta,
  createCardShell,
  createInlineForm,
  createEmptyState,
  lastNDays,
  dayKey,
  clampInt,
  pruneDayMap,
  createId,
  capText,
  createIcon,
} from '../../space-kit/index.js';

const KEY = 'widget/habit';
const MAX_HABITS = 12;
const KEEP_DAYS = 30;

function readState(data) {
  const habits = (Array.isArray(data.habits) ? data.habits : [])
    .filter((habit) => habit && typeof habit.id === 'string' && habit.name)
    .slice(0, MAX_HABITS)
    .map((habit) => ({
      id: habit.id,
      name: capText(habit.name, 32),
      target: clampInt(habit.target, 1, 10, 1),
    }));
  const log = data.log && typeof data.log === 'object' && !Array.isArray(data.log) ? data.log : {};
  return { habits, log };
}

function pruneLog(log, today) {
  const next = {};
  for (const [habitId, days] of Object.entries(log)) {
    next[habitId] = pruneDayMap(days, KEEP_DAYS, today);
  }
  return next;
}

function countOn(log, habitId, day) {
  const value = Number(log?.[habitId]?.[day]);
  return Number.isFinite(value) && value > 0 ? Math.floor(value) : 0;
}

export const habitWidget = {
  key: KEY,
  name: 'Habits',
  defaultData: { habits: [], log: {} },

  render(container, data = {}, display = {}, { t = (key, fallback) => fallback || key, onDataChange } = {}) {
    const { habits, log } = readState(data);
    const today = dayKey();
    const days = lastNDays(7);
    const base = { ...data, habits, log };
    const doneToday = habits.filter((habit) => countOn(log, habit.id, today) >= habit.target).length;

    const shell = createCardShell({
      title: t('habitTitle', '习惯打卡'),
      icon: cardMeta(KEY)?.icon || 'flame',
      count: habits.length ? `${doneToday}/${habits.length}` : '',
      className: 'space-card-habit',
    });
    container.replaceChildren(shell.root);

    if (habits.length === 0) {
      shell.body.append(createEmptyState(t('habitEmpty', '添加第一个习惯，开始打卡')));
    } else {
      const head = document.createElement('div');
      head.className = 'space-habit-head';
      head.append(document.createElement('span'));
      for (const day of days) {
        const cell = document.createElement('span');
        cell.className = `space-habit-day${day === today ? ' is-today' : ''}`;
        cell.textContent = String(Number(day.slice(8)));
        head.append(cell);
      }
      head.append(document.createElement('span'));
      shell.body.append(head);

      for (const habit of habits) {
        const row = document.createElement('div');
        row.className = 'space-habit-row';

        const name = document.createElement('span');
        name.className = 'space-habit-name';
        name.textContent = habit.name;
        name.title = habit.name;
        row.append(name);

        for (const day of days) {
          const value = countOn(log, habit.id, day);
          const dot = document.createElement('span');
          const state = value >= habit.target ? ' is-full' : value > 0 ? ' is-part' : '';
          dot.className = `space-habit-dot${state}`;
          dot.title = `${day} · ${value}/${habit.target}`;
          row.append(dot);
        }

        const actions = document.createElement('span');
        actions.className = 'space-habit-actions';

        const count = countOn(log, habit.id, today);
        const reached = count >= habit.target;
        const check = document.createElement('button');
        check.type = 'button';
        check.className = `space-habit-check${reached ? ' is-done' : ''}`;
        check.title = t('habitToday', '今日 {count}/{target}')
          .replace('{count}', String(count))
          .replace('{target}', String(habit.target));
        check.setAttribute('aria-label', `${check.title}: ${habit.name}`);
        check.setAttribute('aria-pressed', String(reached));
        check.append(createIcon(reached ? 'check' : 'plus', { size: 10 }));
        check.addEventListener('click', () => {
          const nextLog = { ...log, [habit.id]: { ...(log[habit.id] || {}) } };
          if (reached) delete nextLog[habit.id][today];
          else nextLog[habit.id][today] = count + 1;
          onDataChange?.({ ...base, log: pruneLog(nextLog, today) });
        });

        const remove = document.createElement('button');
        remove.type = 'button';
        remove.className = 'space-habit-remove';
        remove.title = t('habitRemove', '删除习惯');
        remove.setAttribute('aria-label', `${t('habitRemove', '删除习惯')}: ${habit.name}`);
        remove.append(createIcon('close', { size: 10 }));
        remove.addEventListener('click', () => {
          const { [habit.id]: _removed, ...rest } = log;
          onDataChange?.({ ...base, habits: habits.filter((entry) => entry.id !== habit.id), log: rest });
        });

        actions.append(check, remove);
        row.append(actions);
        shell.body.append(row);
      }
    }

    if (habits.length < MAX_HABITS) {
      const form = createInlineForm({
        placeholder: t('habitPlaceholder', '添加一个习惯…'),
        ariaLabel: t('habitAdd', '添加习惯'),
        submitLabel: t('habitAdd', '添加习惯'),
        maxLength: 32,
        onSubmit: (value) => {
          onDataChange?.({
            ...base,
            habits: [...habits, { id: createId('habit'), name: capText(value, 32), target: 1 }],
          });
          return true;
        },
      });
      shell.body.append(form.form);
    }

    shell.setFooter(t('habitHint', '最近 7 天 · 点击方格完成今日打卡'));
    return () => container.replaceChildren();
  },

  renderSettings(container, data = {}, onChange = () => {}, { t = (key, fallback) => fallback || key } = {}) {
    container.replaceChildren();
    const wrap = document.createElement('div');
    wrap.className = 'inspector-field-group';
    wrap.innerHTML = `
      <div class="inspector-notice">${t('habitSettingsHint', '打卡记录只保留最近 30 天；清空记录不会删除习惯。')}</div>
      <button type="button" class="danger" id="hb-clear">${t('habitClearLog', '清空打卡记录')}</button>
    `;
    wrap.querySelector('#hb-clear').onclick = () => {
      onChange({ ...data, log: {} });
    };
    container.append(wrap);
  },

  styles: '',
};
