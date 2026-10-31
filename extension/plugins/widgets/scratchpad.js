/**
 * 便签：一块自动保存的自由文本。
 * 输入时 500ms 防抖写入当前卡片 configJson，失焦/卸载前强制 flush。
 */

import {
  cardMeta,
  createCardShell,
  createTextarea,
  createStatus,
  createDebouncedCommit,
  capText,
} from '../../space-kit/index.js';

const KEY = 'widget/scratchpad';
const MAX_CHARS = 20000;

export const scratchpadWidget = {
  key: KEY,
  name: 'Scratchpad',
  defaultData: { text: '' },

  render(container, data = {}, display = {}, { t = (key, fallback) => fallback || key, onDataChange } = {}) {
    const shell = createCardShell({
      title: t('scratchpadTitle', '便签'),
      icon: cardMeta(KEY)?.icon || 'notebook',
      className: 'space-card-scratchpad',
    });
    container.replaceChildren(shell.root);

    const status = createStatus(t('scratchpadSaved', '已保存'));
    const commit = createDebouncedCommit((next) => {
      onDataChange?.({ ...data, text: next });
      status.set(t('scratchpadSaved', '已保存'));
    }, 500);

    const textarea = createTextarea({
      value: data.text || '',
      placeholder: t('scratchpadPlaceholder', '随手写点什么…'),
      rows: 6,
      maxLength: MAX_CHARS,
      ariaLabel: t('scratchpadTitle', '便签'),
      onInput: (value) => {
        status.set(t('scratchpadSaving', '保存中…'));
        commit.schedule(capText(value, MAX_CHARS));
      },
      onBlur: () => {
        commit.flush();
      },
    });

    shell.body.append(textarea, status.node);

    const count = document.createElement('span');
    count.textContent = `${String(data.text || '').length}/${MAX_CHARS}`;
    shell.setFooter(count);
    textarea.addEventListener('input', () => {
      count.textContent = `${textarea.value.length}/${MAX_CHARS}`;
    });

    const flushOnHide = () => commit.flush();
    const flushWhenHidden = () => {
      if (document.hidden) commit.flush();
    };
    window.addEventListener('pagehide', flushOnHide);
    document.addEventListener('visibilitychange', flushWhenHidden);

    return () => {
      window.removeEventListener('pagehide', flushOnHide);
      document.removeEventListener('visibilitychange', flushWhenHidden);
      commit.flush();
      container.replaceChildren();
    };
  },

  renderSettings(container, data = {}, onChange = () => {}, { t = (key, fallback) => fallback || key } = {}) {
    container.replaceChildren();
    const wrap = document.createElement('div');
    wrap.className = 'inspector-field-group';
    wrap.innerHTML = `
      <div class="inspector-notice">${t('scratchpadSettingsHint', '内容会在输入停止后自动保存，最多 20000 字。')}</div>
      <button type="button" class="danger" id="sp-clear">${t('scratchpadClear', '清空便签')}</button>
    `;
    wrap.querySelector('#sp-clear').onclick = () => {
      onChange({ ...data, text: '' });
    };
    container.append(wrap);
  },

  styles: '',
};
