/**
 * 今日重点：每天最多三件事（数量可在设置里调整），跨天自动清空。
 * 数据全部落在当前卡片的 configJson，不读全局 store。
 */

import {
  cardMeta,
  normalizeCardLimit,
  createCardShell,
  createInlineForm,
  createListRow,
  createEmptyState,
  dayKey,
  createId,
  capText,
} from '../../space-kit/index.js';

const KEY = 'widget/dailyFocus';

export const dailyFocusWidget = {
  key: KEY,
  name: 'Daily Focus',
  defaultData: { date: '', items: [], limit: 3 },

  render(container, data = {}, display = {}, { t = (key, fallback) => fallback || key, onDataChange } = {}) {
    const limit = normalizeCardLimit(KEY, data.limit);
    const today = dayKey();
    const current = data.date === today && Array.isArray(data.items) ? data.items.slice(0, limit) : [];
    // 跨天时以空列表为基线；第一次写入会同时把 date 更新为今天。
    const base = { ...data, date: today, items: current, limit };
    const done = current.filter((item) => item.done).length;

    const shell = createCardShell({
      title: t('dailyFocusTitle', '今日重点'),
      icon: cardMeta(KEY)?.icon || 'target',
      count: `${done}/${limit}`,
      className: 'space-card-daily-focus',
    });
    container.replaceChildren(shell.root);

    if (current.length === 0) {
      shell.body.append(createEmptyState(t('dailyFocusEmpty', '写下今天最重要的三件事')));
    }

    for (const item of current) {
      shell.body.append(createListRow({
        label: item.text,
        checked: Boolean(item.done),
        toggleLabel: t('dailyFocusToggle', '切换完成状态'),
        removeLabel: t('delete', '删除'),
        onToggle: () => {
          onDataChange?.({
            ...base,
            items: current.map((entry) => (entry.id === item.id ? { ...entry, done: !entry.done } : entry)),
          });
        },
        onRemove: () => {
          onDataChange?.({ ...base, items: current.filter((entry) => entry.id !== item.id) });
        },
      }));
    }

    if (current.length < limit) {
      const form = createInlineForm({
        placeholder: t('dailyFocusPlaceholder', '今天最重要的事…'),
        ariaLabel: t('dailyFocusAdd', '添加今日重点'),
        submitLabel: t('dailyFocusAdd', '添加今日重点'),
        maxLength: 120,
        onSubmit: (value) => {
          onDataChange?.({
            ...base,
            items: [...current, { id: createId('focus'), text: capText(value, 120), done: false }],
          });
          return true;
        },
      });
      shell.body.append(form.form);
    } else {
      shell.body.append(createEmptyState(t('dailyFocusFull', '今日重点已满，完成或删除后再添加')));
    }

    shell.setFooter(t('dailyFocusHint', '每天自动清空，只保留当天重点'));
    return () => container.replaceChildren();
  },

  renderSettings(container, data = {}, onChange = () => {}, { t = (key, fallback) => fallback || key } = {}) {
    container.replaceChildren();
    const wrap = document.createElement('div');
    wrap.className = 'inspector-field-group';
    wrap.innerHTML = `
      <div class="inspector-notice">${t('dailyFocusSettingsHint', '数量上限可在上方「显示数量」调整；这里可以清空今天的重点。')}</div>
      <button type="button" class="danger" id="df-clear">${t('dailyFocusClear', '清空今日重点')}</button>
    `;
    wrap.querySelector('#df-clear').onclick = () => {
      onChange({ ...data, date: dayKey(), items: [] });
    };
    container.append(wrap);
  },

  styles: '',
};
