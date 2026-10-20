

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

    const statsEl = document.createElement('div');
    statsEl.className = 'note-stats-bar';
    statsEl.textContent = computeNoteStats(content, t);

    
    const viewEl = document.createElement('div');
    viewEl.className = 'notes-view';
    viewEl.tabIndex = 0;
    viewEl.setAttribute('role', 'button');
    viewEl.setAttribute('aria-label', t('edit', '编辑'));
    if (content) {
      viewEl.innerHTML = formatNoteContent(content);
    } else {
      viewEl.innerHTML = `<span class="placeholder"><svg class="icon" aria-hidden="true" style="width:13px;height:13px;display:inline-block;vertical-align:-2px;"><use href="#i-pen" /></svg> <span>${escapeHtml(t('clickToWriteNote', '点击此处记录便签...'))}</span></span>`;
    }

    
    const editArea = document.createElement('textarea');
    editArea.className = 'notes-textarea';
    editArea.setAttribute('aria-label', t('notesContent', '便签内容'));
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
      statsEl.textContent = computeNoteStats(nextContent, t);
      if (nextContent) {
        viewEl.innerHTML = formatNoteContent(nextContent);
      } else {
        viewEl.innerHTML = `<span class="placeholder"><svg class="icon" aria-hidden="true" style="width:13px;height:13px;display:inline-block;vertical-align:-2px;"><use href="#i-pen" /></svg> <span>${escapeHtml(t('clickToWriteNote', '点击此处记录便签...'))}</span></span>`;
      }
    }

    viewEl.onclick = (e) => {
      e.stopPropagation();
      enterEdit();
    };
    viewEl.onkeydown = (event) => {
      if (event.key !== 'Enter' && event.key !== ' ') return;
      event.preventDefault(); enterEdit();
    };
    editArea.onkeydown = (event) => {
      if (event.key !== 'Escape') return;
      event.preventDefault(); editArea.blur(); viewEl.focus();
    };

    editArea.onblur = () => exitEdit();
    editArea.oninput = () => {
      editArea.style.height = 'auto';
      editArea.style.height = `${Math.max(80, editArea.scrollHeight)}px`;
      statsEl.textContent = computeNoteStats(editArea.value, t);
      if (onDataChange) {
        onDataChange({ ...data, content: editArea.value });
      }
    };

    root.append(statsEl, viewEl, editArea);

    const actions = createNotesActions({ t, editArea, applyTemplate });

    function applyTemplate(tpl) {
      const nextVal = editArea.value ? `${editArea.value}\n\n${tpl}` : tpl;
      editArea.value = nextVal;
      viewEl.innerHTML = formatNoteContent(nextVal);
      statsEl.textContent = computeNoteStats(nextVal, t);
      onDataChange?.({ ...data, content: nextVal });
    }

    root.append(actions.element);
    container.append(root);

    return () => { actions.dispose(); editArea.onblur = null; container.replaceChildren(); };
  },
  renderSettings(container, data = {}, onChange = () => {}, { t = (k, f) => f || k } = {}) {
    container.replaceChildren();
    const notice = document.createElement('div');
    notice.className = 'inspector-notice';
    notice.textContent = t('notesNotice', '直接编辑便签；输入和插入模板后自动保存。');
    container.append(notice);
  },
  styles: `
    .Notes:not(.markdown-content), .Notes .notes-view { white-space:pre-wrap; }
    .Notes .notes-view { cursor:pointer; overflow-wrap:anywhere; }
    .Notes .placeholder { display:flex; align-items:center; gap:.5em; font-style:italic; }
    .Notes .note-stats-bar {
      font-size: 12px;
      opacity: 0.65;
      text-align: right;
      margin-bottom: 4px;
      font-variant-numeric: tabular-nums;
    }
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
      min-height: 32px;
      height: 32px;
      padding: 0 7px;
      border-radius: 999px;
      border: 1px solid rgba(255,255,255,0.25);
      background: rgba(255,255,255,0.08);
      color: inherit;
      font-size: 12px;
      cursor: pointer;
      opacity: 0.75;
      display: inline-flex;
      align-items: center;
      gap: 4px;
    }
    .Notes .notes-chip-btn:hover {
      opacity: 1;
      background: rgba(255,255,255,0.18);
      border-color: rgba(255,255,255,0.4);
    }
    .Notes .note-code-block {
      background: rgba(0,0,0,0.3);
      padding: 8px 10px;
      border-radius: 6px;
      font-family: monospace;
      font-size: 12px;
      overflow-x: auto;
      margin: 6px 0;
      border: 1px solid rgba(255,255,255,0.15);
      text-align: left;
    }
    .Notes .note-inline-code {
      background: rgba(255,255,255,0.15);
      padding: 1px 4px;
      border-radius: 3px;
      font-family: monospace;
      font-size: 0.9em;
    }
    .Notes .note-task {
      display: flex;
      align-items: center;
      gap: 6px;
      margin: 2px 0;
      text-align: left;
    }
    .Notes .note-task.done {
      text-decoration: line-through;
      opacity: 0.6;
    }
    .Notes .note-bullet {
      display: flex;
      align-items: baseline;
      gap: 6px;
      margin: 2px 0;
      text-align: left;
    }
    .Notes h2, .Notes h3, .Notes h4 {
      margin: 6px 0 2px 0;
      font-weight: 600;
    }
  `,
};

function computeNoteStats(text, t) {
  const clean = (text || '').trim();
  if (!clean) return '';
  const len = clean.length;
  const mins = Math.max(1, Math.ceil(len / 300));
  return t('notesStats', '{count} chars · {minutes} min').replace('{count}', len).replace('{minutes}', mins);
}

function formatNoteContent(text) {
  if (!text) return '';
  const escaped = escapeHtml(text);
  const withCodeBlocks = escaped.replace(/```([a-zA-Z0-9_-]*)\n([\s\S]*?)```/g, (match, lang, code) => {
    return `<pre class="note-code-block"><code>${code.trim()}</code></pre>`;
  });
  const lines = withCodeBlocks.split('\n');
  const formatted = lines.map((line) => {
    if (/^###\s+(.+)$/.test(line)) return line.replace(/^###\s+(.+)$/, '<h4>$1</h4>');
    if (/^##\s+(.+)$/.test(line)) return line.replace(/^##\s+(.+)$/, '<h3>$1</h3>');
    if (/^#\s+(.+)$/.test(line)) return line.replace(/^#\s+(.+)$/, '<h2>$1</h2>');
    if (/^[-*]\s+\[ \]\s+(.+)$/.test(line)) {
      return line.replace(/^[-*]\s+\[ \]\s+(.+)$/, '<div class="note-task"><input type="checkbox" disabled /> <span>$1</span></div>');
    }
    if (/^[-*]\s+\[[xX]\]\s+(.+)$/.test(line)) {
      return line.replace(/^[-*]\s+\[[xX]\]\s+(.+)$/, '<div class="note-task done"><input type="checkbox" checked disabled /> <span>$1</span></div>');
    }
    if (/^[-*]\s+(.+)$/.test(line)) {
      return line.replace(/^[-*]\s+(.+)$/, '<div class="note-bullet">• <span>$1</span></div>');
    }
    return line;
  });
  let html = formatted.join('<br>');
  html = html.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
  html = html.replace(/`([^`]+)`/g, '<code class="note-inline-code">$1</code>');
  return html;
}

function createNotesActions({ t, editArea, applyTemplate }) {
  const element = document.createElement('div');
  element.className = 'notes-quick-bar';
  const actions = [
    ['prompt', 'bolt', 'notesPrompt', 'Prompt'],
    ['arch', 'grid', 'notesArchitecture', 'Design'],
    ['diag', 'alert', 'notesDiagnosis', 'Diagnose'],
    ['review', 'target', 'notesReview', 'Review'],
    ['code', 'code', 'notesCode', 'Code'],
    ['copy', 'copy', 'copy', 'Copy'],
    ['export', 'download', 'notesExport', 'Export .md'],
  ];
  let feedbackTimer;
  let disposed = false;

  function feedback(button, message, label) {
    if (disposed) return;
    clearTimeout(feedbackTimer);
    button.querySelector('span').textContent = message;
    feedbackTimer = setTimeout(() => {
      button.querySelector('span').textContent = label;
    }, 1500);
  }

  for (const [action, icon, key, fallback] of actions) {
    const button = document.createElement('button');
    const label = t(key, fallback);
    button.type = 'button';
    button.className = 'notes-chip-btn';
    button.dataset.act = action;
    button.innerHTML = `<svg class="icon" aria-hidden="true" style="width:12px;height:12px"><use href="#i-${icon}" /></svg><span aria-live="polite">${escapeHtml(label)}</span>`;
    element.append(button);
    button.onclick = async (event) => {
      event.stopPropagation();
      if (action !== 'copy' && action !== 'export') {
        applyTemplate(t(`${key}Template`, `## ${label}\n`));
        return;
      }
      const content = editArea.value;
      if (!content) return;
      if (action === 'copy') {
        button.disabled = true;
        try {
          await navigator.clipboard.writeText(content);
          feedback(button, t('copied', 'Copied'), label);
        } catch {
          feedback(button, t('clipboardUnavailable', 'Clipboard unavailable'), label);
        } finally { if (!disposed) button.disabled = false; }
        return;
      }
      const url = URL.createObjectURL(new Blob([content], { type: 'text/markdown;charset=utf-8' }));
      try {
        const link = document.createElement('a'); link.href = url;
        link.download = `note-${new Date().toISOString().slice(0, 10)}.md`;
        link.click();
      } finally { URL.revokeObjectURL(url); }
    };
  }
  return {
    element,
    dispose() {
      disposed = true; clearTimeout(feedbackTimer);
      element.querySelectorAll('button').forEach(button => { button.onclick = null; });
    },
  };
}
