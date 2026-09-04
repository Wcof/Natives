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

    const toolbar = document.createElement('div');
    toolbar.className = 'notes-quick-bar';
    toolbar.innerHTML = `
      <button type="button" class="notes-chip-btn" data-act="prompt" title="快速插入 AI 提示词框架">
        <svg class="icon" aria-hidden="true" style="width:12px;height:12px;"><use href="#i-bolt" /></svg>
        <span>Prompt 模板</span>
      </button>
      <button type="button" class="notes-chip-btn" data-act="code" title="快速插入代码块草稿">
        <svg class="icon" aria-hidden="true" style="width:12px;height:12px;"><use href="#i-code" /></svg>
        <span>代码草稿</span>
      </button>
      <button type="button" class="notes-chip-btn" data-act="copy" title="一键复制全部便签内容">
        <svg class="icon" aria-hidden="true" style="width:12px;height:12px;"><use href="#i-copy" /></svg>
        <span>复制</span>
      </button>
    `;

    toolbar.onclick = (e) => {
      const btn = e.target.closest('.notes-chip-btn');
      if (!btn) return;
      e.stopPropagation();
      const act = btn.dataset.act;
      if (act === 'copy') {
        const curText = editArea.value || data.content || '';
        if (curText) {
          navigator.clipboard?.writeText(curText);
          const span = btn.querySelector('span');
          if (span) span.textContent = '已复制';
          setTimeout(() => { if (span) span.textContent = '复制'; }, 1500);
        }
      } else if (act === 'prompt') {
        const tpl = `## 角色设定\n你是一名资深的工程师，擅长系统架构与代码审计。\n\n## 背景与目标\n\n## 约束规范\n- 符合生产环境安全标准\n- 严禁硬编码敏感凭证\n`;
        const nextVal = editArea.value ? `${editArea.value}\n\n${tpl}` : tpl;
        editArea.value = nextVal;
        viewEl.innerHTML = formatNoteContent(nextVal);
        onDataChange?.({ ...data, content: nextVal });
      } else if (act === 'code') {
        const tpl = `\`\`\`typescript\n// 快速测试脚本 / AI 生成代码片段\nfunction executeTask() {\n  \n}\n\`\`\``;
        const nextVal = editArea.value ? `${editArea.value}\n\n${tpl}` : tpl;
        editArea.value = nextVal;
        viewEl.innerHTML = formatNoteContent(nextVal);
        onDataChange?.({ ...data, content: nextVal });
      }
    };

    root.append(toolbar);
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
    .Notes .notes-quick-bar {
      display: flex;
      gap: 6px;
      margin-top: 8px;
      flex-wrap: wrap;
    }
    .Notes .notes-chip-btn {
      min-height: 22px;
      height: 22px;
      padding: 0 7px;
      border-radius: 999px;
      border: 1px solid rgba(255,255,255,0.25);
      background: rgba(255,255,255,0.08);
      color: inherit;
      font-size: 11px;
      cursor: pointer;
      opacity: 0.75;
      transition: all .15s ease;
    }
    .Notes .notes-chip-btn:hover {
      opacity: 1;
      background: rgba(255,255,255,0.18);
      border-color: rgba(255,255,255,0.4);
    }
  `,
};

function formatNoteContent(text) {
  return escapeHtml(text).replace(/\n/g, '<br>');
}
