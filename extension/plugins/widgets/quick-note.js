/**
 * 快速记录：带时间戳的临时想法，最新在前；可一键复制为 Markdown 列表。
 * 最多保留 100 条，显示条数由卡片元数据的 limit 控制。
 */

import {
  cardMeta,
  normalizeCardLimit,
  createCardShell,
  createInlineForm,
  createListRow,
  createEmptyState,
  formatStamp,
  createId,
  capText,
} from '../../space-kit/index.js';

const KEY = 'widget/quickNote';
const MAX_NOTES = 100;

export const quickNoteWidget = {
  key: KEY,
  name: 'Quick Notes',
  defaultData: { notes: [], limit: 5 },

  render(container, data = {}, display = {}, { t = (key, fallback) => fallback || key, onDataChange } = {}) {
    const limit = normalizeCardLimit(KEY, data.limit);
    const notes = Array.isArray(data.notes) ? data.notes : [];
    const visible = notes.slice(-limit).reverse();
    const base = { ...data, notes, limit };

    const shell = createCardShell({
      title: t('quickNoteTitle', '快速记录'),
      icon: cardMeta(KEY)?.icon || 'pen',
      count: notes.length || '',
      className: 'space-card-quick-note',
    });
    container.replaceChildren(shell.root);

    let copiedTimer = 0;
    if (notes.length > 0) {
      const copyButton = shell.addAction({
        icon: 'copy',
        label: t('quickNoteCopy', '复制全部'),
        onClick: () => {
          const clipboard = globalThis.navigator?.clipboard;
          if (typeof clipboard?.writeText !== 'function') return;
          const markdown = notes.map((note) => `- ${note.text}`).join('\n');
          Promise.resolve(clipboard.writeText(markdown))
            .then(() => {
              copyButton.title = t('quickNoteCopied', '已复制');
              copyButton.setAttribute('aria-label', copyButton.title);
              clearTimeout(copiedTimer);
              copiedTimer = setTimeout(() => {
                copyButton.title = t('quickNoteCopy', '复制全部');
                copyButton.setAttribute('aria-label', copyButton.title);
              }, 1500);
            })
            .catch(() => {});
        },
      });
    }

    const form = createInlineForm({
      placeholder: t('quickNotePlaceholder', '记下一闪而过的想法…'),
      ariaLabel: t('quickNoteAdd', '添加快速记录'),
      submitLabel: t('quickNoteAdd', '添加快速记录'),
      maxLength: 500,
      onSubmit: (value) => {
        const next = [...notes, { id: createId('note'), text: capText(value, 500), time: Date.now() }]
          .slice(-MAX_NOTES);
        onDataChange?.({ ...base, notes: next });
        return true;
      },
    });
    shell.body.append(form.form);

    if (notes.length === 0) {
      shell.body.append(createEmptyState(t('quickNoteEmpty', '还没有记录')));
    } else {
      for (const note of visible) {
        shell.body.append(createListRow({
          checkable: false,
          wrap: true,
          label: note.text,
          meta: formatStamp(Number(note.time) || Date.now()),
          removeLabel: t('delete', '删除'),
          onRemove: () => {
            onDataChange?.({ ...base, notes: notes.filter((entry) => entry.id !== note.id) });
          },
        }));
      }
    }

    if (notes.length > limit) {
      shell.setFooter(t('quickNoteMore', '仅显示最近 {n} 条').replace('{n}', String(limit)));
    } else {
      shell.setFooter(t('quickNoteHint', '记录会保存在当前空间'));
    }

    return () => {
      clearTimeout(copiedTimer);
      container.replaceChildren();
    };
  },

  renderSettings(container, data = {}, onChange = () => {}, { t = (key, fallback) => fallback || key } = {}) {
    container.replaceChildren();
    const wrap = document.createElement('div');
    wrap.className = 'inspector-field-group';
    wrap.innerHTML = `
      <div class="inspector-notice">${t('quickNoteSettingsHint', '显示数量可在上方调整；清空后无法恢复。')}</div>
      <button type="button" class="danger" id="qn-clear">${t('quickNoteClear', '清空全部记录')}</button>
    `;
    wrap.querySelector('#qn-clear').onclick = () => {
      onChange({ ...data, notes: [] });
    };
    container.append(wrap);
  },

  styles: '',
};
