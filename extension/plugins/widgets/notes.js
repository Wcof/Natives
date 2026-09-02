/**
 * Notes Widget.
 * Clean direct Markdown/plain text preview with click-to-edit mode matching TablissNG Notes.sass.
 */

import { escapeHtml } from '../sanitizer.js';

export const notesWidget = {
  key: 'widget/notes',
  name: 'Notes',
  defaultData: {
    content: '',
  },
  render(container, data = {}, display = {}, { t = (k, f) => f || k, onDataChange } = {}) {
    container.className = 'Widget Notes';
    container.replaceChildren();

    const root = document.createElement('div');
    root.className = 'notes-content';

    const content = (data.content || '').trim();

    // View element (Rendered view)
    const viewEl = document.createElement('div');
    viewEl.className = 'notes-view';
    if (content) {
      viewEl.innerHTML = formatNoteContent(content);
    } else {
      viewEl.innerHTML = `<span class="placeholder">✎ <span>${escapeHtml(t('clickToWriteNote', '点击此处记录便签...'))}</span></span>`;
    }

    // Edit textarea (hidden by default)
    const editArea = document.createElement('textarea');
    editArea.className = 'notes-textarea';
    editArea.placeholder = t('clickToWriteNote', '点击此处记录便签...');
    editArea.value = data.content || '';
    editArea.hidden = true;

    function enterEdit() {
      viewEl.hidden = true;
      editArea.hidden = false;
      editArea.style.height = `${Math.max(80, viewEl.offsetHeight + 10)}px`;
      editArea.focus();
    }

    function exitEdit() {
      editArea.hidden = true;
      viewEl.hidden = false;
      const nextContent = editArea.value.trim();
      if (nextContent) {
        viewEl.innerHTML = formatNoteContent(nextContent);
      } else {
        viewEl.innerHTML = `<span class="placeholder">✎ <span>${escapeHtml(t('clickToWriteNote', '点击此处记录便签...'))}</span></span>`;
      }
      if (nextContent !== (data.content || '').trim() && onDataChange) {
        onDataChange({ ...data, content: editArea.value });
      }
    }

    viewEl.onclick = (e) => {
      e.stopPropagation();
      enterEdit();
    };

    editArea.onblur = () => exitEdit();
    editArea.oninput = () => {
      editArea.style.height = 'auto';
      editArea.style.height = `${Math.max(80, editArea.scrollHeight)}px`;
      if (onDataChange) {
        onDataChange({ ...data, content: editArea.value });
      }
    };

    root.append(viewEl, editArea);
    container.append(root);

    return () => container.replaceChildren();
  },
  renderSettings(container, data = {}, onChange = () => {}, { t = (k, f) => f || k } = {}) {
    container.replaceChildren();
    const notice = document.createElement('div');
    notice.className = 'inspector-notice';
    notice.textContent = t('notesNotice', '便签支持在 Dashboard 直接点击浏览与实时编辑，失去焦点自动保存。');
    container.append(notice);
  },
  styles: `
    .Notes:not(.markdown-content), .Notes .notes-view { white-space:pre-wrap; }
    .Notes .notes-view { cursor:pointer; overflow-wrap:anywhere; }
    .Notes .placeholder { display:flex; align-items:center; gap:.5em; font-style:italic; }
    .Notes .notes-textarea {
      width: 100%;
      min-height: 80px;
      background:transparent;
      border:1px solid currentColor;
      color:inherit;
      font-family: inherit;
      font-size:inherit;
      resize: vertical;
      box-sizing: border-box;
    }
  `,
};

function formatNoteContent(text) {
  return escapeHtml(text).replace(/\n/g, '<br>');
}
